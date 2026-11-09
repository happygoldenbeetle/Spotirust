//! Audio output for the built-in player, made for instant controls.
//!
//! librespot's stock rodio sink keeps ~0.5 s queued, applies volume to samples *before* that
//! queue (so a volume change is heard half a second late) and drains the queue before it pauses.
//! Here the decoded samples go into a short queue that the output stream reads directly, and
//! volume / pause are applied as samples leave for the device, so they take effect within one
//! device period (~10 ms). Seeks and skips flush the queue so stale audio isn't heard either.

use std::{
    collections::VecDeque,
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, AtomicU16, AtomicU64, Ordering},
    },
    time::Duration,
};

use librespot_core::Error;
use librespot_playback::{
    audio_backend::{Sink, SinkResult},
    config::VolumeCtrl,
    convert::Converter,
    decoder::AudioPacket,
    mixer::{Mixer, MixerConfig, mappings::MappedCtrl},
};

const RATE: u32 = 44_100;
const CHANNELS: u16 = 2;
/// How far decoding may run ahead of the speakers (samples, both channels): ~200 ms.
const AHEAD: usize = (RATE as usize * CHANNELS as usize) / 5;

/// State shared by the decoder side (librespot's player thread), the output stream and the UI.
pub struct Output {
    queue: Mutex<VecDeque<f32>>,
    space: Condvar,
    paused: AtomicBool,
    /// Gain as f64 bits (already mapped through the volume curve).
    gain: AtomicU64,
    /// Bumped by `flush`, so the stream also drops the samples it has taken but not played.
    generation: AtomicU64,
}

impl Output {
    /// Opens the default output device and starts streaming (silence until samples arrive).
    pub fn start() -> anyhow::Result<Arc<Self>> {
        let out = Arc::new(Self {
            queue: Mutex::new(VecDeque::with_capacity(AHEAD * 2)),
            space: Condvar::new(),
            paused: AtomicBool::new(true),
            gain: AtomicU64::new(0.5f64.to_bits()),
            generation: AtomicU64::new(0),
        });
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let source = Stream { out: out.clone(), chunk: VecDeque::new(), generation: 0, n: 0, gain: 0., silent: true };
        // The output stream isn't Send; it lives on its own thread for the life of the app.
        std::thread::Builder::new().name("audio-out".into()).spawn(move || {
            let opened = rodio::OutputStreamBuilder::from_default_device()
                .and_then(|b| b.with_sample_rate(RATE).open_stream_or_fallback());
            match opened {
                Ok(mut stream) => {
                    stream.log_on_drop(false);
                    stream.mixer().add(source);
                    let _ = ready_tx.send(Ok(()));
                    loop {
                        std::thread::park();
                    }
                }
                Err(e) => {
                    let _ = ready_tx.send(Err(anyhow::anyhow!("audio output: {e}")));
                }
            }
        })?;
        ready_rx.recv()??;
        Ok(out)
    }

    /// Silences output now (queued audio is kept, so resuming is instant too).
    pub fn pause(&self) {
        self.paused.store(true, Ordering::SeqCst);
    }

    pub fn resume(&self) {
        self.paused.store(false, Ordering::SeqCst);
        self.space.notify_all();
    }

    /// Drops everything queued (after a seek / skip, so the old position isn't heard).
    pub fn flush(&self) {
        self.queue.lock().unwrap().clear();
        self.generation.fetch_add(1, Ordering::SeqCst);
        self.space.notify_all();
    }

    fn set_gain(&self, g: f64) {
        self.gain.store(g.to_bits(), Ordering::Relaxed);
    }
}

/// The source the output device pulls from: interleaved stereo f32 at 44.1 kHz.
struct Stream {
    out: Arc<Output>,
    /// Samples taken from the queue in one lock (keeps the lock out of the per-sample path).
    chunk: VecDeque<f32>,
    generation: u64,
    /// Samples emitted, so pause / flush only take effect on a frame boundary (keeps L/R aligned).
    n: u64,
    /// Smoothed gain (no zipper noise while the volume bar is dragged).
    gain: f32,
    silent: bool,
}

impl Iterator for Stream {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        if self.n % CHANNELS as u64 == 0 {
            let generation = self.out.generation.load(Ordering::SeqCst);
            if generation != self.generation {
                self.generation = generation;
                self.chunk.clear();
            }
            self.silent = self.out.paused.load(Ordering::SeqCst);
            if !self.silent && self.chunk.is_empty() {
                let mut q = self.out.queue.lock().unwrap();
                let take = q.len().min(1024) & !1;
                self.chunk.extend(q.drain(..take));
                drop(q);
                self.out.space.notify_all();
            }
            // Nothing decoded yet (start / underrun): output a silent frame.
            if self.chunk.is_empty() {
                self.silent = true;
            }
        }
        self.n += 1;
        if self.silent {
            return Some(0.);
        }
        let target = f64::from_bits(self.out.gain.load(Ordering::Relaxed)) as f32;
        self.gain += (target - self.gain) * 0.005;
        Some(self.chunk.pop_front().unwrap_or(0.) * self.gain)
    }
}

impl rodio::Source for Stream {
    fn current_span_len(&self) -> Option<usize> {
        None
    }
    fn channels(&self) -> u16 {
        CHANNELS
    }
    fn sample_rate(&self) -> u32 {
        RATE
    }
    fn total_duration(&self) -> Option<Duration> {
        None
    }
}

/// librespot's sink, writing into the shared queue.
pub struct OutSink(pub Arc<Output>);

impl Sink for OutSink {
    fn start(&mut self) -> SinkResult<()> {
        self.0.resume();
        Ok(())
    }

    /// Pauses at once: the stock sink drains its queue first, which is the pause delay.
    fn stop(&mut self) -> SinkResult<()> {
        self.0.pause();
        Ok(())
    }

    fn write(&mut self, packet: AudioPacket, converter: &mut Converter) -> SinkResult<()> {
        let Ok(samples) = packet.samples() else { return Ok(()) };
        let samples = converter.f64_to_f32(samples);
        let out = &self.0;
        let mut q = out.queue.lock().unwrap();
        q.extend(samples.iter().copied());
        // Keep at most ~200 ms ahead. While paused the stream doesn't drain, so don't block the
        // player thread then (it has to be free to handle play / seek / load).
        while q.len() > AHEAD && !out.paused.load(Ordering::SeqCst) {
            q = out.space.wait_timeout(q, Duration::from_millis(20)).unwrap().0;
        }
        Ok(())
    }
}

/// Volume control applied at the output (instant), with librespot's default volume curve.
pub struct OutMixer {
    out: Arc<Output>,
    ctrl: VolumeCtrl,
    volume: AtomicU16,
}

impl OutMixer {
    pub fn new(out: Arc<Output>, volume: u16) -> Self {
        let m = Self { out, ctrl: VolumeCtrl::default(), volume: AtomicU16::new(volume) };
        m.set_volume(volume);
        m
    }
}

impl Mixer for OutMixer {
    fn open(_: MixerConfig) -> Result<Self, Error> {
        Err(Error::unimplemented("OutMixer is built with OutMixer::new"))
    }

    fn volume(&self) -> u16 {
        self.volume.load(Ordering::Relaxed)
    }

    fn set_volume(&self, volume: u16) {
        self.volume.store(volume, Ordering::Relaxed);
        self.out.set_gain(self.ctrl.to_mapped(volume));
    }
}
