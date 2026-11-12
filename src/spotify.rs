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
// Search (the top bar's dropdown, `/search/<query>` and its filter pages), from the 1.2.x client.
const SEARCH_SUGGESTIONS_HASH: &str = "f244254b94c0e824d458ac216e0f8406a73f2f69a9ff52831a2f78fa774ff6ce";
const SEARCH_TOP_HASH: &str = "dd78eaff943eba629ed70ee25517b9cea0dcaa41193e2592ae2727660b21892c";
const SEARCH_TRACKS_HASH: &str = "b02683192a98dde7966b5e6655a79eeb62713eab703eda9902c932818dd52751";
const SEARCH_ALBUMS_HASH: &str = "202cb3305e31e5a0767ba7925f28bd728cf8f8b0217e6da43909056071cd70e9";
const SEARCH_ARTISTS_HASH: &str = "7bf95d754fdbe32c8b161fbbe54d1ae50974900df4dce4c8f1afcbcad153224d";
const SEARCH_PLAYLISTS_HASH: &str = "d520014e748f9ea44f7707d8df1819867ac1205e8b7f3e28f22fe5fc858921b1";
const SEARCH_USERS_HASH: &str = "8f358dd82e62f61dd4ceaa9f8cd0889e644c9b707f1b724fbfb356a757cb7e5a";
const SEARCH_AUDIOBOOKS_HASH: &str = "e05ac765d02c084f8783d3c1572b23d57761c43f47eb8b87ce2f9ccced3fa068";
const SEARCH_AUTHORS_HASH: &str = "4a9d403a7cbc7e19da5520d619a865472b35382b043bfa458154e73a5c6f46bd";
/// One document holds `recentSearches`, `saveRecentSearches` and `removeRecentSearches`.
const RECENT_SEARCHES_HASH: &str = "ece96d7b82c820fb9938dd1df50dee3e53737d808c3883e3d01c467c423601b0";
const BROWSE_ALL_HASH: &str = "dbd8b55e09a58afc52eab438bc228ba28fd72ac2f2148c6c26354980e4579001";
const BROWSE_PAGE_HASH: &str = "f5c4e6d668f5716464a231c1cc8b22c1cbf6ad68b09929fd7de813a30581298b";
const DISCOGRAPHY_HASH: &str = "5e07d323febb57b4a56a42abbf781490e58764aa45feb6e3dc0591564fc56599";
const ALBUM_TRACKS_HASH: &str = "0b6ce3511e1b02e4a40fa95440f992acbe85080eb308af9194322d708fc1914f";
const WHATS_NEW_HASH: &str = "d889c8c936ab192af8ced595427f5ba2acdf63478fdc0a181c8d477f8322630e";
const BROWSE_SECTION_HASH: &str = "b13c1cccbfcb6947753c2613411b3566485c21fd5f36d80a80bb64be61ba2d51";
const USER_TOP_HASH: &str = "61b7353d3625780022dbc63d459eb64cd609e7e5e2a820f9160d65030efe2e15";
const EXTRACTED_COLORS_HASH: &str = "36e90fcaea00d47c695fce31874efeb2519b97d4cd0ee1abfb4f8dc9348596ea";
const ALBUM_HASH: &str = "0b6ce3511e1b02e4a40fa95440f992acbe85080eb308af9194322d708fc1914f";

pub struct Spotify {
    rt: tokio::runtime::Runtime,
    session: Session,
    web: Mutex<Token>,
    /// The dealer connection's current id (it changes when the connection is re-made).
    connection: std::sync::Arc<Mutex<Option<String>>>,
    dealer_start: Mutex<()>,
    /// Friends' activity pushed since the last `take_listening_pushes`.
    listening_pushes: std::sync::Arc<Mutex<Vec<serde_json::Value>>>,
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
        Ok(Self { rt, session, web: Mutex::new(web), connection: Default::default(), dealer_start: Mutex::new(()), listening_pushes: Default::default() })
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
        .inspect(|v| dump("library", v))
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

    /// The dealer (push) connection's id, starting the connection on first use. Subscriptions to
    /// friends' activity are keyed on it; their changes then arrive on `hm://listening-activity/`
    /// and queue for `take_listening_pushes`. The id changes when the connection is re-made.
    pub fn connection_id(&self) -> Result<String> {
        if let Some(id) = self.connection.lock().unwrap().clone() {
            return Ok(id);
        }
        let _starting = self.dealer_start.lock().unwrap();
        if let Some(id) = self.connection.lock().unwrap().clone() {
            return Ok(id);
        }
        let conn = self.connection.clone();
        let pushes = self.listening_pushes.clone();
        self.rt.block_on(async {
            use futures_util::StreamExt;
            use librespot_core::dealer::protocol::PayloadValue;
            let dealer = self.session.dealer();
            let mut ids = dealer.listen_for("hm://pusher/v1/connections/", |m| {
                Ok(m.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case("Spotify-Connection-Id")).map(|(_, v)| v.clone()))
            })?;
            // `{username, activity: {uri, contextUri, isPlaying, timestamp}}`
            let mut activity = dealer.listen_for("hm://listening-activity/", |m| {
                let text = match &m.payload {
                    PayloadValue::Raw(b) => String::from_utf8_lossy(b).into_owned(),
                    PayloadValue::Json(j) => j.clone(),
                    PayloadValue::Empty => String::new(),
                };
                let mut v: serde_json::Value = serde_json::from_str(&text).unwrap_or_default();
                if let Some(inner) = v.as_str().and_then(|s| serde_json::from_str(s).ok()) {
                    v = inner;
                }
                Ok(v)
            })?;
            dealer.start().await?;
            tokio::spawn(async move {
                while let Some(m) = activity.next().await {
                    if let Ok(v) = m {
                        if v.get("username").is_some() {
                            pushes.lock().unwrap().push(v);
                        }
                    }
                }
            });
            let (first, mut got) = tokio::sync::oneshot::channel();
            tokio::spawn(async move {
                let mut first = Some(first);
                while let Some(m) = ids.next().await {
                    if let Ok(Some(id)) = m {
                        *conn.lock().unwrap() = Some(id);
                        if let Some(tx) = first.take() {
                            let _ = tx.send(());
                        }
                    }
                }
            });
            tokio::time::timeout(std::time::Duration::from_secs(10), &mut got).await.map_err(|_| anyhow!("no dealer connection id"))?.ok();
            anyhow::Ok(())
        })?;
        self.connection.lock().unwrap().clone().ok_or_else(|| anyhow!("no dealer connection id"))
    }

    /// Blocking: starts the built-in Connect device on this runtime.
    pub fn start_player(&self) -> Result<crate::player::LocalPlayer> {
        crate::player::LocalPlayer::start(&self.rt)
    }

    /// Friends' activity pushed since the last call:
    /// `{username, activity: {uri, contextUri, isPlaying, timestamp}}`.
    pub fn take_listening_pushes(&self) -> Vec<serde_json::Value> {
        std::mem::take(&mut *self.listening_pushes.lock().unwrap())
    }

    /// Blocking: what the people you follow last played, for the Listening activity panel
    /// (`listening-activity/v1/feed`, as the client asks):
    /// `entities[].userEntity | followEntity {uri, activity {entityUri, contextUri, isPlaying, timestamp}}`.
    pub fn listening_feed(&self) -> Result<serde_json::Value> {
        let v = self.spclient_post("/listening-activity/v1/feed", &serde_json::json!({ "unused": true, "resultLimit": 100 }))?;
        dump("listening_feed", &v);
        Ok(v)
    }

    /// Blocking: subscribes this push connection to those friends' activity
    /// (`listening-activity/v1/subscribe`). The reply holds `subscriptionExpirationTimestamp`.
    pub fn subscribe_listening(&self, usernames: &[String], connection_id: &str) -> Result<serde_json::Value> {
        let subs: Vec<serde_json::Value> =
            usernames.iter().map(|u| serde_json::json!({ "username": u, "pusher": { "connectionId": connection_id } })).collect();
        let v = self.spclient_post("/listening-activity/v1/subscribe", &serde_json::json!({ "subscriptions": subs }))?;
        dump("listening_subscribe", &v);
        Ok(v)
    }

    /// Blocking: a user's display name and picture (`user-profile-view/v3/profile/<username>`).
    pub fn user_brief(&self, username: &str) -> Result<(String, Option<String>)> {
        let v = self.spclient_json(&format!("/user-profile-view/v3/profile/{}?market=from_token", percent_encode(username)))?;
        let name = v.get("name").and_then(|n| n.as_str()).unwrap_or(username).to_string();
        Ok((name, v.get("image_url").and_then(|u| u.as_str()).map(str::to_string)))
    }

    /// Blocking: POST on spclient with a JSON body; the reply as JSON (null when it's empty).
    fn spclient_post(&self, path: &str, body: &serde_json::Value) -> Result<serde_json::Value> {
        let (bearer, client_token) = self.partner_tokens()?;
        let text = crate::http::agent()
            .post(&format!("https://spclient.wg.spotify.com{path}"))
            .set("authorization", &format!("Bearer {bearer}"))
            .set("client-token", &client_token)
            .set("accept", "application/json")
            .set("content-type", "application/json")
            .send_string(&body.to_string())
            .map_err(|e| match e {
                ureq::Error::Status(code, r) => anyhow!("{path} -> {code}: {}", r.into_string().unwrap_or_default()),
                e => anyhow!(e),
            })?
            .into_string()?;
        Ok(serde_json::from_str(&text).unwrap_or_default())
    }

    /// Blocking: the Browse page ("Browse all" category cards), `browseAll`.
    /// The search box's dropdown while typing: completed queries and matching things.
    pub fn search_suggestions(&self, query: &str) -> Result<serde_json::Value> {
        self.pathfinder(
            "searchSuggestions",
            SEARCH_SUGGESTIONS_HASH,
            serde_json::json!({
                "query": query,
                "limit": 30,
                "numberOfTopResults": 30,
                "offset": 0,
                "includeAuthors": true,
                "includeAlbumPreReleases": false,
                "includeEpisodeContentRatingsV2": true
            }),
        )
        .inspect(|v| dump("search_suggestions", v))
    }

    /// `/search/<query>`: the top result, its "Featuring" shelf and the mixed list under it.
    pub fn search_top(&self, query: &str, offset: u32) -> Result<serde_json::Value> {
        self.pathfinder(
            "searchTopResultsList",
            SEARCH_TOP_HASH,
            serde_json::json!({
                "query": query,
                "limit": 50,
                "offset": offset,
                "numberOfTopResults": 50,
                "includeArtistHasConcertsField": false,
                "includeAudiobooks": true,
                "includeAuthors": true,
                "includePreReleases": true,
                "includeAlbumPreReleases": false,
                "includeEpisodeContentRatingsV2": true,
                "isPrefix": null,
                "sectionFilters": ["GENERIC", "VIDEO_CONTENT"]
            }),
        )
        .inspect(|v| dump("search_top", v))
    }

    /// A filter page of `/search/<query>/<kind>` (`tracks`, `albums`, `artists`, `playlists`,
    /// `users`, `audiobooks`, `authors`), paged like the client (20 songs, 30 of the rest).
    pub fn search_kind(&self, kind: &str, query: &str, offset: u32) -> Result<serde_json::Value> {
        let (operation, hash) = match kind {
            "tracks" => ("searchTracks", SEARCH_TRACKS_HASH),
            "albums" => ("searchAlbums", SEARCH_ALBUMS_HASH),
            "artists" => ("searchArtists", SEARCH_ARTISTS_HASH),
            "playlists" => ("searchPlaylists", SEARCH_PLAYLISTS_HASH),
            "users" => ("searchUsers", SEARCH_USERS_HASH),
            "audiobooks" => ("searchAudiobooks", SEARCH_AUDIOBOOKS_HASH),
            "authors" => ("searchAuthors", SEARCH_AUTHORS_HASH),
            _ => return Err(anyhow!("no search page for {kind}")),
        };
        self.pathfinder(
            operation,
            hash,
            serde_json::json!({
                "includePreReleases": kind == "audiobooks",
                "includeAlbumPreReleases": false,
                "numberOfTopResults": 20,
                "searchTerm": query,
                "offset": offset,
                "limit": if kind == "tracks" { 20 } else { 30 },
                "includeAudiobooks": true,
                "includeAuthors": true,
                "includeEpisodeContentRatingsV2": true
            }),
        )
        .inspect(|v| dump(&format!("search_{kind}"), v))
    }

    /// Recent searches (kept by the service, so every client shows the same list).
    pub fn recent_searches(&self) -> Result<serde_json::Value> {
        self.pathfinder(
            "recentSearches",
            RECENT_SEARCHES_HASH,
            serde_json::json!({ "limit": 50, "includeAuthors": true, "includeEpisodeContentRatingsV2": true }),
        )
        .inspect(|v| dump("recent_searches", v))
    }

    /// Adds to (`save`) or removes from the recent searches.
    pub fn change_recent_searches(&self, save: bool, uris: &[String]) -> Result<()> {
        let operation = if save { "saveRecentSearches" } else { "removeRecentSearches" };
        let v = self.pathfinder(operation, RECENT_SEARCHES_HASH, serde_json::json!({ "uris": uris }))?;
        let result = v.pointer(if save { "/data/saveToRecentSearches" } else { "/data/removeFromRecentSearches" });
        match result.and_then(|r| r.get("__typename")).and_then(|t| t.as_str()) {
            Some("GenericError") | None => Err(anyhow!("{operation}: {v}")),
            _ => Ok(()),
        }
    }

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

    /// Blocking: a Browse category page (`/genre/<id>`, `spotify:page:<id>`), as the client loads it.
    pub fn browse_page(&self, uri: &str) -> Result<serde_json::Value> {
        let v = self.pathfinder(
            "browsePage",
            BROWSE_PAGE_HASH,
            serde_json::json!({
                "pagePagination": { "offset": 0, "limit": 10 },
                "sectionPagination": { "offset": 0, "limit": 10 },
                "uri": uri,
                "browseEndUserIntegration": "INTEGRATION_DESKTOP",
                "includeEpisodeContentRatingsV2": true
            }),
        )?;
        dump("browse_page", &v);
        Ok(v)
    }

    /// Blocking: an artist's discography totals and name (`queryArtistDiscographyOverview`).
    pub fn discography_overview(&self, artist: &str) -> Result<serde_json::Value> {
        self.pathfinder("queryArtistDiscographyOverview", DISCOGRAPHY_HASH, serde_json::json!({ "uri": artist }))
    }

    /// Blocking: one page of an artist's releases, newest first (`queryArtistDiscographyAll`).
    /// `which` is All / Albums / Singles / Compilations (the response is under
    /// `/data/artistUnion/discography/<which lowercased>`), `order` DATE_DESC / DATE_ASC /
    /// NAME_DESC / NAME_ASC.
    pub fn discography(&self, artist: &str, which: &str, order: &str, offset: usize, limit: usize) -> Result<serde_json::Value> {
        let v = self.pathfinder(
            &format!("queryArtistDiscography{which}"),
            DISCOGRAPHY_HASH,
            serde_json::json!({ "uri": artist, "offset": offset, "limit": limit, "order": order }),
        )?;
        dump("discography_all", &v);
        Ok(v)
    }

    /// Blocking: an album's tracks with play counts (`queryAlbumTracks`).
    pub fn album_tracks(&self, album: &str) -> Result<serde_json::Value> {
        self.pathfinder("queryAlbumTracks", ALBUM_TRACKS_HASH, serde_json::json!({ "uri": album, "offset": 0, "limit": 300 }))
    }

    /// Blocking: What's New (the bell): 50 newest releases, optionally only ALBUM or EPISODE.
    pub fn whats_new(&self, content_type: Option<&str>) -> Result<serde_json::Value> {
        let types: Vec<&str> = content_type.into_iter().collect();
        let v = self.pathfinder(
            "queryWhatsNewFeed",
            WHATS_NEW_HASH,
            serde_json::json!({
                "offset": 0,
                "limit": 50,
                "onlyUnPlayedItems": false,
                "includedContentTypes": types,
                "includeEpisodeContentRatingsV2": true
            }),
        )?;
        dump("whats_new", &v);
        Ok(v)
    }

    /// Blocking: one Browse category shelf with up to 20 items ("Show all", `browseSection`).
    pub fn browse_section(&self, uri: &str) -> Result<serde_json::Value> {
        let v = self.pathfinder(
            "browseSection",
            BROWSE_SECTION_HASH,
            serde_json::json!({
                "pagination": { "offset": 0, "limit": 20 },
                "uri": uri,
                "browseEndUserIntegration": "INTEGRATION_DESKTOP",
                "includeEpisodeContentRatingsV2": true
            }),
        )?;
        dump("browse_section", &v);
        Ok(v)
    }

    /// Blocking: the radio playlist seeded by a track or artist ("Go to song radio" /
    /// "Go to artist radio"), `inspiredby-mix/v2/seed_to_playlist`.
    pub fn radio_for(&self, seed_uri: &str) -> Result<String> {
        let v = self.spclient_json(&format!("/inspiredby-mix/v2/seed_to_playlist/{seed_uri}?response-format=json"))?;
        dump("radio", &v);
        v.pointer("/mediaItems/0/uri")
            .and_then(|u| u.as_str())
            .map(str::to_string)
            .ok_or_else(|| anyhow!("no radio for {seed_uri}"))
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

    /// Blocking: one Home section with up to 20 items, as the client's "Show all" page loads it
    /// (`homeSection`, same persisted query document as `home`).
    pub fn home_section(&self, uri: &str) -> Result<serde_json::Value> {
        let sp_t = self.web_token().unwrap_or_default();
        self.pathfinder(
            "homeSection",
            HOME_HASH,
            serde_json::json!({
                "includeHomeChapterVideoCards": false,
                "uri": uri,
                "homeEndUserIntegration": "INTEGRATION_DESKTOP",
                "timeZone": local_time_zone(),
                "sp_t": sp_t,
                "sectionItemsOffset": 0,
                "sectionItemsLimit": 20,
                "includeEpisodeContentRatingsV2": true
            }),
        )
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


fn percent_encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}
