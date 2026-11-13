//! Reads the official client's image cache, so covers it already downloaded load from disk here.
//!
//! The client is Chromium (CEF), and its HTTP cache is Chromium's "blockfile" disk cache:
//! `Browser\Cache\Cache_Data\` with `index`, `data_0`..`data_3` (block files) and `f_XXXXXX`
//! (external files for big bodies). Each entry is a 256-byte `EntryStore` in a block file whose
//! entry size is 256; its key ends with the request URL, stream 0 holds the response headers and
//! stream 1 the body (here JPEG / WebP / PNG covers from Spotify's image hosts).
//!
//! Where it looks (the first that exists wins for each image, all read-only; nothing is ever written
//! to, moved or deleted in these folders):
//! - `%APPDATA%\Spotirust\cache\Browser\Cache\Cache_Data`: a copy of the client's
//!   `%LOCALAPPDATA%\Spotify\Browser` folder pasted into Spotirust's `cache` folder;
//! - `%LOCALAPPDATA%\Spotify\Browser\Cache\Cache_Data`: the client's own cache, in place.
//!
//! The audio cache (`%LOCALAPPDATA%\Spotify\Data\*.file`) is not used: the client keeps those
//! files encrypted with its own local protection (they match neither the CDN's bytes nor its file
//! ids), and getting around that is off limits.

use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

const BLOCK_HEADER: u64 = 8192;
const BLOCK_MAGIC: u32 = 0xC104_CAC3;
const ENTRY_SIZE: usize = 256;

/// Where an entry's body lives.
#[derive(Clone, Debug)]
struct Body {
    dir: PathBuf,
    addr: u32,
    len: u32,
}

/// Image key (see `image_key`) -> body, over every cache folder found.
fn index() -> &'static HashMap<String, Body> {
    static INDEX: OnceLock<HashMap<String, Body>> = OnceLock::new();
    INDEX.get_or_init(|| {
        let t = std::time::Instant::now();
        let mut map = HashMap::new();
        // Later folders don't override earlier ones: the pasted copy wins over the live cache.
        for dir in cache_dirs() {
            if let Err(e) = scan(&dir, &mut map) {
                eprintln!("spotify cache {}: {e:#}", dir.display());
            }
        }
        if !map.is_empty() {
            eprintln!("spotify cache: {} images indexed in {:?}", map.len(), t.elapsed());
        }
        map
    })
}

fn cache_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(appdata) = std::env::var_os("APPDATA") {
        dirs.push(PathBuf::from(appdata).join("Spotirust").join("cache").join("Browser").join("Cache").join("Cache_Data"));
    }
    if std::env::var_os("SPOTIRUST_NO_CLIENT_CACHE").is_none() {
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            dirs.push(PathBuf::from(local).join("Spotify").join("Browser").join("Cache").join("Cache_Data"));
        }
    }
    dirs.into_iter().filter(|d| d.join("index").is_file()).collect()
}

fn u32_at(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes(b[o..o + 4].try_into().unwrap())
}

/// Walks every 256-byte block of each block file that stores entries.
fn scan(dir: &Path, map: &mut HashMap<String, Body>) -> anyhow::Result<()> {
    for n in 0..64 {
        let path = dir.join(format!("data_{n}"));
        let Ok(bytes) = std::fs::read(&path) else {
            if n >= 4 {
                break;
            }
            continue;
        };
        if bytes.len() < BLOCK_HEADER as usize || u32_at(&bytes, 0) != BLOCK_MAGIC || u32_at(&bytes, 12) as usize != ENTRY_SIZE {
            continue;
        }
        for e in bytes[BLOCK_HEADER as usize..].chunks_exact(ENTRY_SIZE) {
            let key_len = u32_at(e, 32) as usize;
            // Long keys (stored elsewhere) are never image URLs.
            if key_len == 0 || key_len > ENTRY_SIZE - 96 - 1 {
                continue;
            }
            let Ok(key) = std::str::from_utf8(&e[96..96 + key_len]) else { continue };
            let url = key.rsplit(' ').next().unwrap_or(key);
            let Some(img) = image_key(url) else { continue };
            let (len, addr) = (u32_at(e, 44), u32_at(e, 60));
            if len == 0 || addr & 0x8000_0000 == 0 {
                continue;
            }
            map.entry(img).or_insert(Body { dir: dir.to_path_buf(), addr, len });
        }
    }
    Ok(())
}

/// The lookup key for an image URL on Spotify's image hosts: `<40 hex>` for `…/image/<40 hex>` (the
/// same file is served from three hosts), else the URL itself.
fn image_key(url: &str) -> Option<String> {
    let rest = url.strip_prefix("https://")?;
    let (host, path) = rest.split_once('/')?;
    if !(host.ends_with(".scdn.co") || host.ends_with(".spotifycdn.com")) || host.starts_with("audio") {
        return None;
    }
    if let Some(key) = path.strip_prefix("image/") {
        if key.len() == 40 && key.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Some(key.to_ascii_lowercase());
        }
    }
    Some(url.to_string())
}

/// Reads a body by its cache address: an external `f_XXXXXX` file or blocks in `data_N`.
fn read_body(b: &Body) -> anyhow::Result<Vec<u8>> {
    let kind = (b.addr >> 28) & 7;
    let len = b.len as usize;
    if kind == 0 {
        let mut f = std::fs::File::open(b.dir.join(format!("f_{:06x}", b.addr & 0x0FFF_FFFF)))?;
        let mut buf = vec![0; len];
        f.read_exact(&mut buf)?;
        return Ok(buf);
    }
    let block = match kind {
        2 => 256,
        3 => 1024,
        4 => 4096,
        _ => anyhow::bail!("unexpected block type {kind}"),
    };
    let blocks = ((b.addr >> 24) & 3) as usize + 1;
    anyhow::ensure!(len <= blocks * block, "body larger than its blocks");
    let mut f = std::fs::File::open(b.dir.join(format!("data_{}", (b.addr >> 16) & 0xff)))?;
    f.seek(SeekFrom::Start(BLOCK_HEADER + (b.addr & 0xffff) as u64 * block as u64))?;
    let mut buf = vec![0; len];
    f.read_exact(&mut buf)?;
    Ok(buf)
}

/// The cover the official client cached for this URL, if it has it. Only whole, decodable images
/// count (a half-written entry falls through to the network).
pub fn image(url: &str) -> Option<Vec<u8>> {
    let body = index().get(&image_key(url)?)?;
    let bytes = read_body(body).ok()?;
    complete(&bytes).then_some(bytes)
}

/// Whether the bytes are a whole JPEG / WebP / PNG (an entry can be cut short).
fn complete(b: &[u8]) -> bool {
    match b {
        [0xFF, 0xD8, ..] => b.ends_with(&[0xFF, 0xD9]) || b[b.len().saturating_sub(64)..].windows(2).any(|w| w == [0xFF, 0xD9]),
        [b'R', b'I', b'F', b'F', a, b2, c, d, b'W', b'E', b'B', b'P', ..] => u32::from_le_bytes([*a, *b2, *c, *d]) as usize + 8 <= b.len(),
        [0x89, b'P', b'N', b'G', ..] => b.windows(4).rev().take(64).any(|w| w == b"IEND"),
        _ => false,
    }
}

/// Builds the index off the UI thread at startup, so the first cover doesn't wait for it.
pub fn warm_up() {
    std::thread::spawn(|| {
        index();
    });
}
