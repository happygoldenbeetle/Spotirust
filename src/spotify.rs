//! The librespot session (playback identity) plus private reads Spotify's own client uses.
//!
//! Two grants, the same split Spotifast uses:
//! - librespot session under librespot's client id: login5 bearer + client token for pathfinder
//!   (login5 rejects third-party client ids).
//! - the user's own developer app (`auth`): Web API, and its token as the Home `sp_t` value.

use std::{path::PathBuf, sync::Mutex};

use anyhow::{Context as _, Result, anyhow};
use librespot_core::{SessionConfig, authentication::Credentials, cache::Cache, session::Session};

use crate::auth::{self, Token};

const LIBRESPOT_CLIENT_ID: &str = "65b708073fc0480ea92a077233ca87bd";
const LIBRESPOT_REDIRECT: &str = "http://127.0.0.1:5588/login";
const PATHFINDER: &str = "https://api-partner.spotify.com/pathfinder/v2/query";
/// Persisted query captured from the real client's Home load.
const HOME_HASH: &str = "5fb7da7a03ee2776c0670856609cb85a189a3cecc36cc160b91cde79063b9cf1";
/// Persisted query captured from the real client's Now Playing view.
const DYNAMIC_COLORS_HASH: &str = "f0f112945d6d745bd8ff790317bbf8d310036da75df33130490e9d6dc96c59d9";
/// Persisted query from the web player bundle (open.spotify.com, 2026-11-07): playlist page.
const PLAYLIST_HASH: &str = "21ea11a6ac75a5f3e01fd365d29c18e0a4a8b7dc09f89bbb5d73910190c6acaf";
/// Persisted query from the web player bundle (open.spotify.com, 2026-11-07).
const LIBRARY_HASH: &str = "390c78e5b951029bad359785e69b07b536a509c581cbcd0aded5e5067f187455";
/// Persisted query captured from the real client's album page (2026-11-07).
/// Persisted query captured from the real client's artist page (2026-11-08).
const ARTIST_HASH: &str = "9f8134ef565e78621f1e1793555bd6633c5ac144ae0f89604ed3ae3f80b3c8e6";
const BROWSE_ALL_HASH: &str = "dbd8b55e09a58afc52eab438bc228ba28fd72ac2f2148c6c26354980e4579001";
const USER_TOP_HASH: &str = "61b7353d3625780022dbc63d459eb64cd609e7e5e2a820f9160d65030efe2e15";
const EXTRACTED_COLORS_HASH: &str = "36e90fcaea00d47c695fce31874efeb2519b97d4cd0ee1abfb4f8dc9348596ea";
const ALBUM_HASH: &str = "0b6ce3511e1b02e4a40fa95440f992acbe85080eb308af9194322d708fc1914f";

pub struct Spotify {
    rt: tokio::runtime::Runtime,
    session: Session,
    web: Mutex<Token>,
}

fn cache_dir() -> PathBuf {
    let base = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    base.join("Spotirust").join("librespot")
}

impl Spotify {
    /// Blocking. Uses cached librespot credentials, or opens the browser once to get them.
    pub fn connect(web: Token) -> Result<Self> {
        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build()?;
        let session = rt.block_on(async {
            let cache = Cache::new(Some(cache_dir()), None, None, None)?;
            let credentials = match cache.credentials() {
                Some(c) => c,
                None => {
                    let token = librespot_oauth::OAuthClientBuilder::new(
                        LIBRESPOT_CLIENT_ID,
                        LIBRESPOT_REDIRECT,
                        vec!["streaming"],
                    )
                    .open_in_browser()
                    .build()
                    .map_err(|e| anyhow!("{e}"))?
                    .get_access_token_async()
                    .await
                    .map_err(|e| anyhow!("{e}"))?;
                    Credentials::with_access_token(token.access_token)
                }
            };
            let session = Session::new(SessionConfig::default(), Some(cache));
            session.connect(credentials, true).await.context("librespot connect")?;
            anyhow::Ok(session)
        })?;
        Ok(Self { rt, session, web: Mutex::new(web) })
    }

    fn web_token(&self) -> Result<String> {
        let mut t = self.web.lock().unwrap();
        if t.is_expired() {
            *t = auth::refresh(&t)?;
        }
        Ok(t.access_token.clone())
    }

    fn partner_tokens(&self) -> Result<(String, String)> {
        self.rt.block_on(async {
            let bearer = self.session.login5().auth_token().await.context("login5")?;
            let client_token = self.session.spclient().client_token().await.context("client token")?;
            anyhow::Ok((bearer.access_token, client_token))
        })
    }

    fn pathfinder(&self, operation: &str, hash: &str, variables: serde_json::Value) -> Result<serde_json::Value> {
        let (bearer, client_token) = self.partner_tokens()?;
        let body = serde_json::json!({
            "variables": variables,
            "operationName": operation,
            "extensions": { "persistedQuery": { "version": 1, "sha256Hash": hash } }
        });
        crate::http::agent()
            .post(PATHFINDER)
            .set("authorization", &format!("Bearer {bearer}"))
            .set("client-token", &client_token)
            .send_json(body)
            .map_err(|e| match e {
                ureq::Error::Status(code, r) => {
                    anyhow!("{operation} -> {code}: {}", r.into_string().unwrap_or_default())
                }
                e => anyhow!(e),
            })?
            .into_json()
            .map_err(Into::into)
    }

    /// Blocking: Your Library as the client shows it (Recents order, pins, folders, item types).
    /// `libraryV3` is the web player's query; the desktop client reads the same data from its core.
    pub fn library(&self, filters: &[&str]) -> Result<serde_json::Value> {
        self.pathfinder(
            "libraryV3",
            LIBRARY_HASH,
            serde_json::json!({
                "filters": filters,
                "order": "Recents",
                "textFilter": "",
                "features": ["LIKED_SONGS", "YOUR_EPISODES_V2", "PRERELEASES", "PRERELEASES_V2", "EVENTS"],
                "limit": 200,
                "offset": 0,
                "flatten": false,
                "expandedFolders": [],
                "folderUri": null,
                "includeFoldersWhenFlattening": true
            }),
        )
    }

    /// Blocking: a playlist page as the client loads it (`fetchPlaylist`): header, colours, owner,
    /// and `limit` items from `offset`.
    pub fn playlist(&self, uri: &str, offset: u32, limit: u32) -> Result<serde_json::Value> {
        self.pathfinder(
            "fetchPlaylist",
            PLAYLIST_HASH,
            serde_json::json!({
                "uri": uri,
                "offset": offset,
                "limit": limit,
                "enableWatchFeedEntrypoint": false,
                "includeEpisodeContentRatingsV2": false
            }),
        )
    }

    /// Blocking: an album page as the client loads it (`getAlbum`): header, artists, colours,
    /// `limit` tracks from `offset`, copyright and "More by" releases.
    pub fn album(&self, uri: &str, offset: u32, limit: u32) -> Result<serde_json::Value> {
        let v = self.pathfinder(
            "getAlbum",
            ALBUM_HASH,
            serde_json::json!({ "uri": uri, "locale": "", "offset": offset, "limit": limit }),
        )?;
        dump("album", &v);
        Ok(v)
    }

    /// Blocking: an artist page as the client loads it (`queryArtistOverview`).
    pub fn artist(&self, uri: &str) -> Result<serde_json::Value> {
        let v = self.pathfinder(
            "queryArtistOverview",
            ARTIST_HASH,
            serde_json::json!({ "uri": uri, "locale": "", "preReleaseV2": true }),
        )?;
        dump("artist", &v);
        Ok(v)
    }

    /// Blocking: GET on spclient (`spclient.wg.spotify.com<path>`) with the session's tokens, as JSON.
    fn spclient_json(&self, path: &str) -> Result<serde_json::Value> {
        let (bearer, client_token) = self.partner_tokens()?;
        crate::http::agent()
            .get(&format!("https://spclient.wg.spotify.com{path}"))
            .set("authorization", &format!("Bearer {bearer}"))
            .set("client-token", &client_token)
            .set("accept", "application/json")
            .set("accept-language", "en")
            .call()
            .map_err(|e| match e {
                ureq::Error::Status(code, r) => anyhow!("{path} -> {code}: {}", r.into_string().unwrap_or_default()),
                e => anyhow!(e),
            })?
            .into_json()
            .map_err(Into::into)
    }

    /// Blocking: a user profile as the client loads it: `user-profile-view/v3` profile plus the
    /// followers and following lists.
    pub fn user_profile(&self, id: &str) -> Result<(serde_json::Value, serde_json::Value, serde_json::Value)> {
        let base = format!("/user-profile-view/v3/profile/{id}");
        let p = self.spclient_json(&format!("{base}?playlist_limit=10&artist_limit=10&episode_limit=10&market=from_token"))?;
        let followers = self.spclient_json(&format!("{base}/followers?market=from_token")).unwrap_or_default();
        let following = self.spclient_json(&format!("{base}/following?market=from_token")).unwrap_or_default();
        dump("profile", &p);
        dump("followers", &followers);
        dump("following", &following);
        Ok((p, followers, following))
    }

    /// Blocking: "Your Updates" in the profile menu (`gander/v2/GetNotifications`, 20 per page).
    pub fn notifications(&self) -> Result<serde_json::Value> {
        let v = self.spclient_json("/gander/v2/GetNotifications?locale=&limit=20")?;
        dump("notifications", &v);
        Ok(v)
    }

    /// Blocking: what friends are listening to (`presence-view/v1/buddylist`), for the
    /// Listening activity panel.
    pub fn buddylist(&self) -> Result<serde_json::Value> {
        // The client also opens `v2/init-friend-feed/{dealer connection}`, but that only
        // subscribes to live updates (its first response is empty); v1 returns the list.
        let v = self.spclient_json("/presence-view/v1/buddylist")?;
        dump("buddylist", &v);
        Ok(v)
    }

    /// Blocking: the Browse page ("Browse all" category cards), `browseAll`.
    pub fn browse_all(&self) -> Result<serde_json::Value> {
        let v = self.pathfinder(
            "browseAll",
            BROWSE_ALL_HASH,
            serde_json::json!({
                "pagePagination": { "offset": 0, "limit": 10 },
                "sectionPagination": { "offset": 0, "limit": 99 },
                "browseEndUserIntegration": "INTEGRATION_DESKTOP"
            }),
        )?;
        dump("browse_all", &v);
        Ok(v)
    }

    /// Blocking: "Top artists this month" for the signed-in user (`userTopContent`).
    pub fn user_top_artists(&self, limit: u32) -> Result<serde_json::Value> {
        let input = serde_json::json!({ "offset": 0, "limit": limit, "sortBy": "AFFINITY", "timeRange": "SHORT_TERM" });
        let v = self.pathfinder(
            "userTopContent",
            USER_TOP_HASH,
            serde_json::json!({
                "includeTopArtists": true, "topArtistsInput": input,
                "includeTopTracks": false, "topTracksInput": { "offset": 0, "limit": 4, "sortBy": "AFFINITY", "timeRange": "SHORT_TERM" }
            }),
        )?;
        dump("user_top", &v);
        Ok(v)
    }

    /// Blocking: colours Spotify extracted from images (`fetchExtractedColors`).
    pub fn extracted_colors(&self, image_uris: &[String]) -> Result<serde_json::Value> {
        let v = self.pathfinder("fetchExtractedColors", EXTRACTED_COLORS_HASH, serde_json::json!({ "imageUris": image_uris }))?;
        dump("extracted_colors", &v);
        Ok(v)
    }

    /// Blocking: the Liked Songs filter chips (Calm, Moody, Pop, ...), as the client requests them.
    pub fn liked_filters(&self) -> Result<serde_json::Value> {
        let v = self.spclient_json("/content-filter/v1/liked-songs?subjective=true&market=from_token")?;
        dump("liked_filters", &v);
        Ok(v)
    }

    /// Blocking: Spotify's own lyrics with the colours picked for this cover art
    /// (`color-lyrics/v2`, the request the client makes for the Now Playing view).
    /// `image_url` is an `i.scdn.co/image/<40 hex>` cover URL. `Ok(None)` when the track has no lyrics.
    pub fn lyrics(&self, track_id: &str, image_url: &str) -> Result<Option<serde_json::Value>> {
        use librespot_core::{FileId, SpotifyId};
        let track = SpotifyId::from_base62(track_id).map_err(|e| anyhow!("{e}"))?;
        let hex = image_url.rsplit('/').next().unwrap_or_default();
        let raw: Vec<u8> = (0..hex.len() / 2)
            .filter_map(|i| u8::from_str_radix(hex.get(i * 2..i * 2 + 2)?, 16).ok())
            .collect();
        let spclient = self.session.spclient();
        let res = if raw.len() == 20 {
            self.rt.block_on(spclient.get_lyrics_for_image(&track, &FileId::from_raw(&raw)))
        } else {
            self.rt.block_on(spclient.get_lyrics(&track))
        };
        match res {
            Ok(bytes) if bytes.is_empty() => Ok(None),
            Ok(bytes) => Ok(Some(serde_json::from_slice(&bytes)?)),
            // 404: no lyrics for this track.
            Err(e) if format!("{e}").contains("404") || format!("{e:?}").contains("NotFound") => Ok(None),
            Err(e) => Err(anyhow!("{e}")),
        }
    }

    /// Blocking: the colour set the client derives from a cover (`getDynamicColorsByUris`),
    /// used for the Now Playing view's lyrics card. `image_url` is an `i.scdn.co/image/<hex>` URL.
    pub fn dynamic_colors(&self, image_url: &str) -> Result<serde_json::Value> {
        let hex = image_url.rsplit('/').next().unwrap_or_default();
        self.pathfinder(
            "getDynamicColorsByUris",
            DYNAMIC_COLORS_HASH,
            serde_json::json!({ "imageUris": [format!("spotify:image:{hex}")] }),
        )
    }

    /// Blocking: which of `uris` are "Mixed playlists" (the MIX_STATE metadata extension, kind 225).
    pub fn mixed_playlists(&self, uris: &[String]) -> Result<std::collections::HashSet<String>> {
        use librespot_protocol::extended_metadata::{BatchedEntityRequest, EntityRequest, ExtensionQuery};
        const MIX_STATE: i32 = 225;
        let req = BatchedEntityRequest {
            entity_request: uris
                .iter()
                .map(|u| EntityRequest {
                    entity_uri: u.clone(),
                    query: vec![ExtensionQuery {
                        extension_kind: protobuf::EnumOrUnknown::from_i32(MIX_STATE),
                        ..Default::default()
                    }],
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        };
        let res = self.rt.block_on(self.session.spclient().get_extended_metadata(req)).map_err(|e| anyhow!("{e}"))?;
        let mut out = std::collections::HashSet::new();
        for arr in res.extended_metadata {
            for d in arr.extension_data {
                // MixState { bool is_mixed = 1; } -> bytes 08 01 when set.
                if d.extension_data.as_ref().is_some_and(|any| any.value.as_slice() == [8, 1]) {
                    out.insert(d.entity_uri.clone());
                }
            }
        }
        Ok(out)
    }

    /// Blocking: the full Home feed (all sections), like the real client's first Home load.
    pub fn home(&self, facet: &str) -> Result<serde_json::Value> {
        let sp_t = self.web_token().unwrap_or_default();
        self.pathfinder(
            "home",
            HOME_HASH,
            serde_json::json!({
                "includeHomeChapterVideoCards": false,
                "homeEndUserIntegration": "INTEGRATION_DESKTOP",
                "timeZone": local_time_zone(),
                "sp_t": sp_t,
                "facet": facet,
                "sectionItemsLimit": 10,
                "includeEpisodeContentRatingsV2": true
            }),
        )
    }
}

/// IANA zone name, from `TZ` if set, else mapped from the Windows zone (falls back to UTC).
fn local_time_zone() -> String {
    if let Ok(tz) = std::env::var("TZ") {
        return tz;
    }
    windows_zone().unwrap_or_else(|| "UTC".into())
}

fn windows_zone() -> Option<String> {
    let out = std::process::Command::new("tzutil").arg("/g").output().ok()?;
    let name = String::from_utf8_lossy(&out.stdout).trim().to_string();
    // Common Windows zones -> IANA. Extend as needed.
    let iana = match name.as_str() {
        "Pakistan Standard Time" => "Asia/Karachi",
        "India Standard Time" => "Asia/Kolkata",
        "GMT Standard Time" => "Europe/London",
        "W. Europe Standard Time" => "Europe/Berlin",
        "Central European Standard Time" => "Europe/Warsaw",
        "Eastern Standard Time" => "America/New_York",
        "Central Standard Time" => "America/Chicago",
        "Mountain Standard Time" => "America/Denver",
        "Pacific Standard Time" => "America/Los_Angeles",
        "Arabian Standard Time" => "Asia/Dubai",
        "Arab Standard Time" => "Asia/Riyadh",
        "Tokyo Standard Time" => "Asia/Tokyo",
        "China Standard Time" => "Asia/Shanghai",
        "AUS Eastern Standard Time" => "Australia/Sydney",
        "UTC" => "UTC",
        _ => return None,
    };
    Some(iana.to_string())
}

/// Debug: with `SPOTIRUST_DUMP=<dir>`, raw pathfinder responses are written there as `<name>.json`.
fn dump(name: &str, v: &serde_json::Value) {
    if let Some(dir) = std::env::var_os("SPOTIRUST_DUMP") {
        let _ = std::fs::write(std::path::Path::new(&dir).join(format!("{name}.json")), v.to_string());
    }
}

