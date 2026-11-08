//! Minimal Spotify Web API client (blocking; call from the background executor).

use std::sync::{Arc, Mutex};

use anyhow::{Result, anyhow};
use serde::{Deserialize, de::DeserializeOwned};

use crate::auth::{self, Token};

pub struct Api {
    token: Mutex<Token>,
}

impl Api {
    pub fn new(token: Token) -> Arc<Self> {
        Arc::new(Self { token: Mutex::new(token) })
    }

    pub fn token(&self) -> Token {
        self.token.lock().unwrap().clone()
    }

    fn access_token(&self) -> Result<String> {
        let mut t = self.token.lock().unwrap();
        if t.is_expired() {
            *t = auth::refresh(&t)?;
        }
        Ok(t.access_token.clone())
    }

    fn get_opt<T: DeserializeOwned>(&self, path: &str) -> Result<Option<T>> {
        let url = if path.starts_with("https://") { path.to_string() } else { format!("https://api.spotify.com/v1{path}") };
        let resp = crate::http::agent().get(&url)
            .set("Authorization", &format!("Bearer {}", self.access_token()?))
            .call()
            .map_err(|e| match e {
                ureq::Error::Status(code, r) => anyhow!("GET {path} -> {code}: {}", r.into_string().unwrap_or_default()),
                e => anyhow!(e),
            })?;
        if resp.status() == 204 {
            return Ok(None);
        }
        Ok(Some(resp.into_json()?))
    }

    pub fn get<T: DeserializeOwned>(&self, path: &str) -> Result<T> {
        self.get_opt(path)?.ok_or_else(|| anyhow!("GET {path}: empty response"))
    }

    /// PUT/POST a player command; 200/202/204 all mean accepted.
    fn send(&self, method: &str, path: &str, body: Option<serde_json::Value>) -> Result<()> {
        let url = format!("https://api.spotify.com/v1{path}");
        let req = crate::http::agent()
            .request(method, &url)
            .set("Authorization", &format!("Bearer {}", self.access_token()?));
        let res = match body {
            Some(b) => req.send_json(b),
            None => req.set("Content-Length", "0").call(),
        };
        res.map(|_| ()).map_err(|e| match e {
            ureq::Error::Status(code, r) => anyhow!("{method} {path} -> {code}: {}", r.into_string().unwrap_or_default()),
            e => anyhow!(e),
        })
    }

    /// Creates a private playlist owned by `user_id` and returns its URI.
    pub fn create_playlist(&self, user_id: &str, name: &str) -> Result<String> {
        let path = format!("/users/{user_id}/playlists");
        let v: serde_json::Value = crate::http::agent()
            .post(&format!("https://api.spotify.com/v1{path}"))
            .set("Authorization", &format!("Bearer {}", self.access_token()?))
            .send_json(serde_json::json!({ "name": name, "public": false }))
            .map_err(|e| match e {
                ureq::Error::Status(code, r) => anyhow!("POST {path} -> {code}: {}", r.into_string().unwrap_or_default()),
                e => anyhow!(e),
            })?
            .into_json()?;
        v.get("uri").and_then(|u| u.as_str()).map(str::to_string).ok_or_else(|| anyhow!("POST {path}: no uri"))
    }

    /// Adds a track or episode to the end of the play queue.
    pub fn add_to_queue(&self, uri: &str) -> Result<()> {
        self.send("POST", &format!("/me/player/queue?uri={}", enc(uri)), None)
    }

    /// Track URIs of an album or playlist (for "Add to queue" on a whole context).
    pub fn context_tracks(&self, uri: &str) -> Result<Vec<String>> {
        let id = uri.rsplit(':').next().unwrap_or_default();
        let mut out = Vec::new();
        let mut next = Some(match uri.split(':').nth(1) {
            Some("album") => format!("/albums/{id}/tracks?limit=50"),
            Some("playlist") => format!("/playlists/{id}/items?limit=100&fields=next,items(item(uri),track(uri))"),
            _ => return Ok(out),
        });
        while let Some(path) = next.take() {
            let v: serde_json::Value = self.get(&path)?;
            for it in v.get("items").and_then(|i| i.as_array()).into_iter().flatten() {
                let u = it.get("uri").or_else(|| it.pointer("/item/uri")).or_else(|| it.pointer("/track/uri"));
                if let Some(u) = u.and_then(|u| u.as_str()) {
                    out.push(u.to_string());
                }
            }
            next = v.get("next").and_then(|n| n.as_str()).map(str::to_string);
        }
        Ok(out)
    }

    /// Appends tracks to a playlist.
    pub fn add_to_playlist(&self, playlist_uri: &str, uris: &[String]) -> Result<()> {
        let id = playlist_uri.rsplit(':').next().unwrap_or_default();
        self.send("POST", &format!("/playlists/{id}/items"), Some(serde_json::json!({ "uris": uris })))
    }

    /// Removes every occurrence of a track from a playlist.
    pub fn remove_from_playlist(&self, playlist_uri: &str, uri: &str) -> Result<()> {
        let id = playlist_uri.rsplit(':').next().unwrap_or_default();
        self.send("DELETE", &format!("/playlists/{id}/items"), Some(serde_json::json!({ "items": [{ "uri": uri }], "tracks": [{ "uri": uri }] })))
    }

    /// Saves / removes an album or playlist in Your Library (follow / unfollow for playlists).
    pub fn set_in_library(&self, uri: &str, saved: bool) -> Result<()> {
        let id = uri.rsplit(':').next().unwrap_or_default();
        let method = if saved { "PUT" } else { "DELETE" };
        match uri.split(':').nth(1) {
            Some("album") => self.send(method, &format!("/me/albums?ids={id}"), None),
            Some("playlist") => self.send(method, &format!("/playlists/{id}/followers"), None),
            Some("artist") => self.send(method, &format!("/me/following?type=artist&ids={id}"), None),
            _ => Err(anyhow!("can't save {uri}")),
        }
    }

    /// Makes one of your playlists public or private.
    pub fn set_playlist_public(&self, uri: &str, public: bool) -> Result<()> {
        let id = uri.rsplit(':').next().unwrap_or_default();
        self.send("PUT", &format!("/playlists/{id}"), Some(serde_json::json!({ "public": public })))
    }

    pub fn resume(&self) -> Result<()> {
        self.send("PUT", "/me/player/play", None)
    }
    pub fn pause(&self) -> Result<()> {
        self.send("PUT", "/me/player/pause", None)
    }
    pub fn next(&self) -> Result<()> {
        self.send("POST", "/me/player/next", None)
    }
    pub fn previous(&self) -> Result<()> {
        self.send("POST", "/me/player/previous", None)
    }
    pub fn seek(&self, ms: u64) -> Result<()> {
        self.send("PUT", &format!("/me/player/seek?position_ms={ms}"), None)
    }
    pub fn set_shuffle(&self, on: bool) -> Result<()> {
        self.send("PUT", &format!("/me/player/shuffle?state={on}"), None)
    }
    /// `off` | `context` | `track`
    pub fn set_repeat(&self, state: &str) -> Result<()> {
        self.send("PUT", &format!("/me/player/repeat?state={state}"), None)
    }
    pub fn set_volume(&self, percent: u32) -> Result<()> {
        self.send("PUT", &format!("/me/player/volume?volume_percent={}", percent.min(100)), None)
    }
    /// Whether a track is in Liked Songs.
    pub fn is_saved(&self, track_id: &str) -> Result<bool> {
        let v: Vec<bool> = self.get(&format!("/me/tracks/contains?ids={track_id}"))?;
        Ok(v.first().copied().unwrap_or(false))
    }
    /// Adds a track to / removes it from Liked Songs.
    pub fn set_saved(&self, track_id: &str, saved: bool) -> Result<()> {
        self.send(if saved { "PUT" } else { "DELETE" }, &format!("/me/tracks?ids={track_id}"), None)
    }
    /// Play a playlist / album / artist (or Liked Songs) on the active device.
    pub fn play_context(&self, context_uri: &str) -> Result<()> {
        self.send("PUT", "/me/player/play", Some(serde_json::json!({ "context_uri": context_uri })))
    }

    /// Play `context_uri` starting at the track `track_uri` (a tracklist row double-click).
    pub fn play_context_at(&self, context_uri: &str, track_uri: &str) -> Result<()> {
        self.send(
            "PUT",
            "/me/player/play",
            Some(serde_json::json!({ "context_uri": context_uri, "offset": { "uri": track_uri } })),
        )
    }

    /// Display name of a playback context, as the Now Playing view's header shows it.
    pub fn context_name(&self, uri: &str) -> Result<String> {
        #[derive(Deserialize)]
        struct Named {
            name: String,
        }
        let mut parts = uri.split(':');
        let kind = parts.nth(1).unwrap_or_default();
        let id = uri.rsplit(':').next().unwrap_or_default();
        Ok(match kind {
            "album" => self.get::<Named>(&format!("/albums/{id}"))?.name,
            "playlist" => self.get::<Named>(&format!("/playlists/{id}?fields=name"))?.name,
            "artist" => self.get::<Named>(&format!("/artists/{id}"))?.name,
            "user" if uri.ends_with(":collection") => "Liked Songs".into(),
            "collection" => "Liked Songs".into(),
            _ => String::new(),
        })
    }

    /// The client's own mosaic for a playlist: `spotify:mosaic:<id>:<id>:<id>:<id>`, the first four
    /// distinct album covers in track order (the `mosaic.scdn.co` URL sorts them, so its layout differs).
    /// `None` when the playlist has fewer than four albums.
    pub fn mosaic_uri(&self, playlist_id: &str) -> Result<Option<String>> {
        let v: serde_json::Value =
            self.get(&format!("/playlists/{playlist_id}/tracks?limit=50&fields=items(track(album(images)))"))?;
        let mut ids: Vec<String> = Vec::new();
        for item in v.get("items").and_then(|i| i.as_array()).into_iter().flatten() {
            let images = item.pointer("/track/album/images").and_then(|i| i.as_array());
            let url = images
                .and_then(|imgs| imgs.iter().find(|i| i.get("width").and_then(|w| w.as_u64()) == Some(300)).or(imgs.first()))
                .and_then(|i| i.get("url")?.as_str());
            if let Some(id) = url.and_then(|u| u.rsplit('/').next()) {
                if !ids.iter().any(|x| x == id) {
                    ids.push(id.to_string());
                }
            }
            if ids.len() == 4 {
                return Ok(Some(format!("spotify:mosaic:{}", ids.join(":"))));
            }
        }
        Ok(None)
    }

    pub fn me(&self) -> Result<Me> {
        self.get("/me")
    }

    /// All of the user's playlists (follows `next` links).
    pub fn my_playlists(&self) -> Result<Vec<Playlist>> {
        let mut out = Vec::new();
        let mut next = Some("/me/playlists?limit=50".to_string());
        while let Some(url) = next {
            let page: Paging<Playlist> = self.get(&url)?;
            out.extend(page.items.into_iter().flatten());
            next = page.next;
        }
        Ok(out)
    }

    pub fn liked_count(&self) -> Result<u32> {
        let page: Paging<serde_json::Value> = self.get("/me/tracks?limit=1")?;
        Ok(page.total.unwrap_or(0))
    }

    /// The current player state; each real response is also saved, see [`Self::playback_or_last`].
    pub fn playback(&self) -> Result<Option<Playback>> {
        let Some(raw) = self.get_opt::<serde_json::Value>("/me/player?additional_types=track,episode")? else {
            return Ok(None);
        };
        if let Some(path) = last_playback_path() {
            if let Some(dir) = path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let _ = std::fs::write(path, raw.to_string());
        }
        Ok(Some(serde_json::from_value(raw)?))
    }

    /// Like the real client: with no active device it keeps showing the last Connect state
    /// (track, position, "Playing on" device), paused. The Web API forgets an idle device after a
    /// while (204), so the last real response is kept on disk; recently-played is the fallback
    /// when there is none (it has no position, so that starts at 0:00).
    pub fn playback_or_last(&self) -> Result<Option<Playback>> {
        if let Some(pb) = self.playback()? {
            return Ok(Some(pb));
        }
        let saved = last_playback_path()
            .and_then(|p| std::fs::read(p).ok())
            .and_then(|b| serde_json::from_slice::<Playback>(&b).ok());
        if let Some(mut pb) = saved {
            pb.is_playing = false;
            return Ok(Some(pb));
        }
        #[derive(Deserialize)]
        struct Played {
            track: Track,
            context: Option<PlayContext>,
        }
        let page: Paging<Played> = self.get("/me/player/recently-played?limit=1")?;
        Ok(page.items.into_iter().flatten().next().map(|p| Playback {
            is_playing: false,
            progress_ms: Some(0),
            shuffle_state: false,
            repeat_state: "off".into(),
            device: None,
            item: Some(p.track),
            context: p.context,
        }))
    }
}

fn last_playback_path() -> Option<std::path::PathBuf> {
    Some(std::path::PathBuf::from(std::env::var_os("APPDATA")?).join("Spotirust").join("last_playback.json"))
}

#[derive(Clone, Debug, Deserialize)]
pub struct Image {
    pub url: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
}

/// Smallest image that is still at least `min` px wide (Spotify lists largest first).
pub fn pick_image(images: &[Image], min: u32) -> Option<&Image> {
    images
        .iter()
        .filter(|i| i.width.unwrap_or(u32::MAX) >= min)
        .min_by_key(|i| i.width.unwrap_or(u32::MAX))
        .or_else(|| images.first())
}

#[derive(Clone, Debug, Deserialize)]
pub struct Me {
    pub id: String,
    pub display_name: Option<String>,
    #[serde(default)]
    pub images: Vec<Image>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Paging<T> {
    pub items: Vec<Option<T>>,
    pub next: Option<String>,
    pub total: Option<u32>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Owner {
    pub id: String,
    pub display_name: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Playlist {
    pub id: String,
    pub name: String,
    pub owner: Owner,
    #[serde(default, deserialize_with = "null_as_default")]
    pub images: Vec<Image>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Artist {
    pub name: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Album {
    pub name: String,
    #[serde(default)]
    pub images: Vec<Image>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Track {
    pub id: Option<String>,
    pub name: String,
    pub duration_ms: u64,
    #[serde(default)]
    pub artists: Vec<Artist>,
    pub album: Option<Album>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Device {
    pub name: String,
    pub volume_percent: Option<u32>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Playback {
    pub is_playing: bool,
    pub progress_ms: Option<u64>,
    pub shuffle_state: bool,
    pub repeat_state: String,
    pub device: Option<Device>,
    pub item: Option<Track>,
    #[serde(default)]
    pub context: Option<PlayContext>,
}

/// What's playing from (album / playlist / artist / collection).
#[derive(Clone, Debug, Deserialize)]
pub struct PlayContext {
    pub uri: String,
}

fn null_as_default<'de, D, T>(d: D) -> std::result::Result<T, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Default + Deserialize<'de>,
{
    Ok(Option::<T>::deserialize(d)?.unwrap_or_default())
}

/// Percent-encodes a query value.
fn enc(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}
