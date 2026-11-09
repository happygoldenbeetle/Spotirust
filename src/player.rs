//! Built-in Spotify Connect device ("Spotirust"): librespot's player + Spirc in this process.
//!
//! While it is the active device, play/pause/skip/seek/volume go straight to the local player
//! (no round trip through Spotify's servers to another app), so they take effect instantly.
//! It runs on its own librespot session: `Spirc::new` connects the session itself.

use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Instant,
};

use anyhow::{Context as _, Result, anyhow};
use librespot_connect::{ConnectConfig, LoadRequest, LoadRequestOptions, PlayingTrack, Spirc};
use librespot_core::{SessionConfig, cache::Cache, config::DeviceType, session::Session};
use librespot_playback::{
    audio_backend,
    config::{AudioFormat, PlayerConfig},
    mixer::{self, MixerConfig},
    player::{Player, PlayerEvent},
};

/// What the local player is doing, from its own events (instant, unlike Web API polling).
#[derive(Clone, Debug, Default)]
pub struct LocalState {
    pub track_uri: String,
    pub playing: bool,
    pub position_ms: u32,
    /// When `position_ms` was reported.
    pub at: Option<Instant>,
    /// A track is loaded here (set on load/play/pause, cleared on stop: another device took over).
    pub loaded: bool,
    /// Bumped on every event, so the UI can tell something changed.
    pub seq: u64,
}

pub struct LocalPlayer {
    spirc: Arc<Spirc>,
    pub device_id: String,
    state: Arc<Mutex<LocalState>>,
    rt: tokio::runtime::Handle,
}

fn data_dir() -> PathBuf {
    let base = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    base.join("Spotirust")
}

/// A stable device id, so Spotify sees one "Spotirust" device rather than a new one per launch.
fn device_id() -> String {
    let path = data_dir().join("device_id");
    if let Ok(id) = std::fs::read_to_string(&path) {
        let id = id.trim().to_string();
        if id.len() >= 16 {
            return id;
        }
    }
    let id: String = (0..40).map(|_| format!("{:x}", rand::random::<u8>() & 0xF)).collect();
    let _ = std::fs::create_dir_all(data_dir());
    let _ = std::fs::write(&path, &id);
    id
}

impl LocalPlayer {
    /// Starts the device on `rt` (blocking until it's registered with Spotify).
    pub fn start(rt: &tokio::runtime::Runtime) -> Result<Self> {
        let state = Arc::new(Mutex::new(LocalState::default()));
        let st = state.clone();
        let (spirc, device_id) = rt.block_on(async move {
            let cache = Cache::new(Some(data_dir().join("librespot")), None, None, None)?;
            let credentials = cache.credentials().context("no cached Spotify credentials")?;
            let device_id = device_id();
            let session = Session::new(SessionConfig { device_id: device_id.clone(), ..SessionConfig::default() }, Some(cache));

            let mixer_fn = mixer::find(None).ok_or_else(|| anyhow!("no mixer"))?;
            let mixer = mixer_fn(MixerConfig::default())?;
            let backend = audio_backend::find(None).ok_or_else(|| anyhow!("no audio backend"))?;
            let player = Player::new(PlayerConfig::default(), session.clone(), mixer.get_soft_volume(), move || {
                backend(None, AudioFormat::default())
            });

            let mut events = player.get_player_event_channel();
            tokio::spawn(async move {
                while let Some(ev) = events.recv().await {
                    let mut s = st.lock().unwrap();
                    let set = |s: &mut LocalState, uri: String, playing: bool, pos: u32| {
                        s.track_uri = uri;
                        s.playing = playing;
                        s.position_ms = pos;
                        s.at = Some(Instant::now());
                        s.loaded = true;
                    };
                    match ev {
                        PlayerEvent::Playing { track_id, position_ms, .. } => set(&mut s, track_id.to_uri().unwrap_or_default(), true, position_ms),
                        PlayerEvent::Paused { track_id, position_ms, .. } => set(&mut s, track_id.to_uri().unwrap_or_default(), false, position_ms),
                        PlayerEvent::Loading { track_id, position_ms, .. } => set(&mut s, track_id.to_uri().unwrap_or_default(), false, position_ms),
                        PlayerEvent::Seeked { track_id, position_ms, .. }
                        | PlayerEvent::PositionCorrection { track_id, position_ms, .. }
                        | PlayerEvent::PositionChanged { track_id, position_ms, .. } => {
                            let playing = s.playing;
                            set(&mut s, track_id.to_uri().unwrap_or_default(), playing, position_ms)
                        }
                        PlayerEvent::Stopped { .. } => {
                            s.playing = false;
                            s.loaded = false;
                        }
                        _ => {}
                    }
                    s.seq += 1;
                }
            });

            let config = ConnectConfig {
                name: "Spotirust".into(),
                device_type: DeviceType::Computer,
                initial_volume: u16::MAX / 2,
                ..ConnectConfig::default()
            };
            let (spirc, task) = Spirc::new(config, session, credentials, player, mixer).await?;
            tokio::spawn(task);
            anyhow::Ok((spirc, device_id))
        })?;
        Ok(Self { spirc: Arc::new(spirc), device_id, state, rt: rt.handle().clone() })
    }

    pub fn state(&self) -> LocalState {
        self.state.lock().unwrap().clone()
    }

    pub fn play(&self) -> Result<()> {
        Ok(self.spirc.play()?)
    }
    pub fn pause(&self) -> Result<()> {
        Ok(self.spirc.pause()?)
    }
    pub fn next(&self) -> Result<()> {
        Ok(self.spirc.next()?)
    }
    pub fn prev(&self) -> Result<()> {
        Ok(self.spirc.prev()?)
    }
    pub fn seek(&self, ms: u64) -> Result<()> {
        Ok(self.spirc.set_position_ms(ms.min(u32::MAX as u64) as u32)?)
    }
    /// 0..=100
    pub fn set_volume(&self, percent: u32) -> Result<()> {
        Ok(self.spirc.set_volume((percent.min(100) as f32 / 100. * u16::MAX as f32) as u16)?)
    }
    pub fn shuffle(&self, on: bool) -> Result<()> {
        Ok(self.spirc.shuffle(on)?)
    }
    /// `off` | `context` | `track`
    pub fn repeat(&self, state: &str) -> Result<()> {
        self.spirc.repeat(state != "off")?;
        Ok(self.spirc.repeat_track(state == "track")?)
    }

    /// Makes this the active device and plays `context` (from `track`, if given). Load requests
    /// are ignored by an inactive device, so activate first (commands are handled in order).
    pub fn play_context(&self, context: &str, track: Option<&str>) -> Result<()> {
        self.spirc.activate()?;
        let options = LoadRequestOptions {
            start_playing: true,
            seek_to: 0,
            context_options: None,
            playing_track: track.map(|t| PlayingTrack::Uri(t.to_string())),
        };
        Ok(self.spirc.load(LoadRequest::from_context_uri(context.to_string(), options))?)
    }

    /// Moves the current session (from whichever device has it) to this device. `then_play`
    /// resumes once the transfer has landed, if it arrived paused.
    pub fn take_over(&self, then_play: bool) -> Result<()> {
        self.spirc.transfer(None)?;
        if then_play {
            let state = self.state.clone();
            let spirc = self.spirc.clone();
            // Spirc ignores `play` until the transfer makes us active; retry briefly.
            self.rt.spawn(async move {
                for _ in 0..20 {
                    tokio::time::sleep(std::time::Duration::from_millis(150)).await;
                    if state.lock().unwrap().playing {
                        return;
                    }
                    let _ = spirc.play();
                }
            });
        }
        Ok(())
    }
}
