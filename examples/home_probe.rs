//! Proves the "real client" data path before building UI on it:
//! librespot login -> login5 token + client token -> pathfinder `home` query.
//! Saves the raw response to temp/home.json.
//!
//! cargo run --example home_probe

use std::path::PathBuf;

use anyhow::{Context, Result, anyhow};
use librespot_core::{SessionConfig, authentication::Credentials, cache::Cache, session::Session};

/// librespot's own desktop client id (what every librespot-based client uses).
const KEYMASTER: &str = "65b708073fc0480ea92a077233ca87bd";
const REDIRECT: &str = "http://127.0.0.1:5588/login";
/// Persisted query captured from the real client's Home load (2026-11-07).
const HOME_HASH: &str = "5fb7da7a03ee2776c0670856609cb85a189a3cecc36cc160b91cde79063b9cf1";
/// `homeSection` persisted query, as used by Spotifast's Made for you / top mixes shelves.
const HOME_SECTION_HASH: &str = "eb3fba2d388cf4fc4d696b1757a58584e9538a3b515ea742e9cc9465807340be";

/// The user's own developer app (developer.spotify.com), as Spotifast requires.
const OWN_CLIENT_ID: &str = "535bd2560d1a446b9176f1c77d4f273c";
const OWN_REDIRECT: &str = "http://127.0.0.1:8888/callback";

fn use_own_app() -> bool {
    std::env::var_os("PROBE_OWN_APP").is_some()
}

/// Session/login5 always use librespot's identity (login5 rejects third-party client ids).
fn cache_dir() -> PathBuf {
    PathBuf::from(std::env::var_os("APPDATA").expect("APPDATA")).join("Spotirust").join("librespot")
}

/// Web API token from the user's own app, used only as `sp_t` (Spotifast's setup).
async fn own_app_token() -> Result<String> {
    println!("getting a Web API token from your own app (browser)...");
    let token = librespot_oauth::OAuthClientBuilder::new(
        OWN_CLIENT_ID,
        OWN_REDIRECT,
        vec!["user-read-private", "user-read-email", "user-library-read", "user-top-read"],
    )
    .open_in_browser()
    .build()
    .map_err(|e| anyhow!("{e}"))?
    .get_access_token_async()
    .await
    .map_err(|e| anyhow!("{e}"))?;
    Ok(token.access_token)
}

#[tokio::main]
async fn main() -> Result<()> {
    let cache = Cache::new(Some(cache_dir()), None, None, None)?;
    let mut sp_t = String::new();
    let credentials = match cache.credentials() {
        Some(c) => {
            println!("using cached credentials");
            c
        }
        None => {
            println!("opening browser for Spotify login...");
            let scopes = vec![
                "streaming", "user-read-private", "user-read-email", "user-library-read",
                "user-library-modify", "playlist-read-private", "playlist-read-collaborative",
                "playlist-modify-private", "playlist-modify-public", "user-read-playback-state",
                "user-modify-playback-state", "user-read-currently-playing", "user-read-recently-played",
                "user-top-read", "user-follow-read", "user-follow-modify",
            ];
            let token = librespot_oauth::OAuthClientBuilder::new(KEYMASTER, REDIRECT, scopes)
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
    session.connect(credentials, true).await.context("session connect")?;
    println!("connected as {}", session.username());

    if use_own_app() {
        sp_t = own_app_token().await?;
    }

    let auth = session.login5().auth_token().await.context("login5 token")?;
    let client_token = session.spclient().client_token().await.context("client token")?;

    let country = session.country();
    let full_home = serde_json::json!({
        "variables": {
            "includeHomeChapterVideoCards": false,
            "homeEndUserIntegration": "INTEGRATION_DESKTOP",
            "timeZone": "Asia/Karachi",
            "sp_t": sp_t,
            "facet": "",
            "sectionItemsLimit": 10,
            "includeEpisodeContentRatingsV2": true
        },
        "operationName": "home",
        "extensions": { "persistedQuery": { "version": 1, "sha256Hash": HOME_HASH } }
    });
    // Same request shape Spotifast (MIT, github.com/crmne/spotifast) uses: one Home section at a time.
    let section = |uri: &str| {
        serde_json::json!({
            "variables": {
                "uri": uri, "country": country, "timeZone": "Asia/Karachi", "sp_t": sp_t,
                "sectionItemsLimit": 20, "sectionItemsOffset": 0
            },
            "operationName": "homeSection",
            "extensions": { "persistedQuery": { "version": 1, "sha256Hash": HOME_SECTION_HASH } }
        })
    };
    let attempts = vec![
        ("home", full_home),
        ("made_for_you", section("spotify:section:0JQ5DAUnp4wcj0bCb3wh3S")),
        ("top_mixes", section("spotify:section:0JQ5DAnM3wGh0gz1MXnu89")),
    ];

    for (name, body) in attempts {
        // Only the session's own credentials: no headers that claim to be Spotify's web player.
        let (bearer, ct) = (auth.access_token.clone(), client_token.clone());
        let resp = tokio::task::spawn_blocking(move || {
            ureq::post("https://api-partner.spotify.com/pathfinder/v2/query")
                .set("authorization", &format!("Bearer {bearer}"))
                .set("client-token", &ct)
                .set("user-agent", concat!("Spotirust/", env!("CARGO_PKG_VERSION")))
                .send_json(body)
        })
        .await?;
        match resp {
            Ok(r) => {
                let text = r.into_string()?;
                let out = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("temp").join(format!("{name}.json"));
                std::fs::write(&out, &text)?;
                println!("{name}: OK {} bytes -> {}", text.len(), out.display());
            }
            Err(ureq::Error::Status(code, r)) => {
                let msg: String = r.into_string().unwrap_or_default().chars().take(200).collect();
                println!("{name}: FAILED {code}: {}", msg.replace('\n', " "));
            }
            Err(e) => println!("{name}: transport error {e}"),
        }
    }
    Ok(())
}


