//! Spotify Web API sign-in: Authorization Code + PKCE (no client secret needed).
//! The browser redirects to a tiny local server on 127.0.0.1:8888, we swap the code for tokens
//! and persist them in %APPDATA%\Spotirust\token.json so the user only signs in once.

use std::{
    path::PathBuf,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use anyhow::{Context as _, Result, anyhow, bail};
use base64::Engine as _;
use rand::Rng as _;
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

/// The user's own app from developer.spotify.com (a client ID is public, not a secret).
pub const CLIENT_ID: &str = "535bd2560d1a446b9176f1c77d4f273c";
pub const REDIRECT_URI: &str = "http://127.0.0.1:8888/callback";
const LISTEN_ADDR: &str = "127.0.0.1:8888";
const SCOPES: &str = "user-read-private user-read-email user-library-read user-library-modify \
    playlist-read-private playlist-read-collaborative playlist-modify-private playlist-modify-public \
    user-read-playback-state user-modify-playback-state user-read-currently-playing \
    user-read-recently-played user-top-read user-follow-read user-follow-modify streaming";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Token {
    pub access_token: String,
    pub refresh_token: String,
    /// Unix seconds.
    pub expires_at: u64,
}

impl Token {
    pub fn is_expired(&self) -> bool {
        now() + 60 >= self.expires_at
    }
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: Option<String>,
    expires_in: u64,
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn data_dir() -> PathBuf {
    let base = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    base.join("Spotirust")
}

fn token_path() -> PathBuf {
    data_dir().join("token.json")
}

pub fn load_token() -> Option<Token> {
    let bytes = std::fs::read(token_path()).ok()?;
    serde_json::from_slice(&bytes).ok()
}

pub fn save_token(token: &Token) -> Result<()> {
    std::fs::create_dir_all(data_dir())?;
    std::fs::write(token_path(), serde_json::to_vec_pretty(token)?)?;
    Ok(())
}

pub fn clear_token() {
    let _ = std::fs::remove_file(token_path());
}

fn random_string(len: usize) -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    let mut rng = rand::thread_rng();
    (0..len).map(|_| CHARS[rng.gen_range(0..CHARS.len())] as char).collect()
}

fn url_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn query_param(query: &str, key: &str) -> Option<String> {
    query.split('&').find_map(|kv| {
        let (k, v) = kv.split_once('=')?;
        (k == key).then(|| percent_decode(v))
    })
}

pub(crate) fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("");
                if let Ok(v) = u8::from_str_radix(hex, 16) {
                    out.push(v);
                    i += 3;
                    continue;
                }
                out.push(b'%');
            }
            b'+' => out.push(b' '),
            b => out.push(b),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// A sign-in in progress: the local callback server is already listening.
pub struct PendingLogin {
    pub url: String,
    server: tiny_http::Server,
    verifier: String,
    state: String,
}

pub fn begin_login() -> Result<PendingLogin> {
    let verifier = random_string(64);
    let challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    let state = random_string(16);
    let server = tiny_http::Server::http(LISTEN_ADDR)
        .map_err(|e| anyhow!("could not listen on {LISTEN_ADDR}: {e}"))?;
    let url = format!(
        "https://accounts.spotify.com/authorize?client_id={CLIENT_ID}&response_type=code&redirect_uri={}\
         &code_challenge_method=S256&code_challenge={challenge}&state={state}&scope={}",
        url_encode(REDIRECT_URI),
        url_encode(SCOPES),
    );
    Ok(PendingLogin { url, server, verifier, state })
}

const DONE_PAGE: &str = "<!doctype html><meta charset=utf-8><title>Spotirust</title>\
<body style=\"background:#121212;color:#fff;font:16px system-ui;display:grid;place-items:center;height:100vh;margin:0\">\
<p>You're logged in. You can close this tab and return to Spotirust.</p>";

impl PendingLogin {
    /// Blocks until the browser hits the callback (or 5 minutes pass), then exchanges the code.
    pub fn wait(self) -> Result<Token> {
        loop {
            let req = self
                .server
                .recv_timeout(Duration::from_secs(300))?
                .ok_or_else(|| anyhow!("login timed out"))?;
            let url = req.url().to_string();
            let Some(query) = url.strip_prefix("/callback?") else {
                let _ = req.respond(tiny_http::Response::empty(404));
                continue;
            };
            let html = tiny_http::Header::from_bytes(&b"Content-Type"[..], &b"text/html; charset=utf-8"[..]).unwrap();
            let _ = req.respond(tiny_http::Response::from_string(DONE_PAGE).with_header(html));
            if let Some(err) = query_param(query, "error") {
                bail!("Spotify returned an error: {err}");
            }
            if query_param(query, "state").as_deref() != Some(self.state.as_str()) {
                bail!("login state mismatch");
            }
            let code = query_param(query, "code").context("no code in callback")?;
            return exchange(&code, &self.verifier);
        }
    }
}

fn token_request(form: &[(&str, &str)], old_refresh: Option<&str>) -> Result<Token> {
    let resp: TokenResponse = crate::http::agent().post("https://accounts.spotify.com/api/token")
        .send_form(form)
        .map_err(|e| match e {
            ureq::Error::Status(code, r) => anyhow!("token request failed ({code}): {}", r.into_string().unwrap_or_default()),
            e => anyhow!(e),
        })?
        .into_json()?;
    let refresh_token = resp
        .refresh_token
        .or_else(|| old_refresh.map(str::to_owned))
        .context("no refresh token returned")?;
    let token = Token { access_token: resp.access_token, refresh_token, expires_at: now() + resp.expires_in };
    save_token(&token)?;
    Ok(token)
}

fn exchange(code: &str, verifier: &str) -> Result<Token> {
    token_request(
        &[
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", REDIRECT_URI),
            ("client_id", CLIENT_ID),
            ("code_verifier", verifier),
        ],
        None,
    )
}

pub fn refresh(token: &Token) -> Result<Token> {
    token_request(
        &[
            ("grant_type", "refresh_token"),
            ("refresh_token", &token.refresh_token),
            ("client_id", CLIENT_ID),
        ],
        Some(&token.refresh_token),
    )
}
