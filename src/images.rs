//! Remote image cache: downloads cover art once, decodes off the main thread,
//! and hands GPUI a ready `RenderImage`.

use std::{collections::HashMap, sync::Arc};

use gpui::{App, AppContext, Context, Entity, Global, RenderImage};

#[derive(Default)]
pub struct ImageCache {
    entries: HashMap<String, Entry>,
}

enum Entry {
    Loading,
    Ready(Arc<RenderImage>),
    Failed,
}

struct GlobalImages(Entity<ImageCache>);
impl Global for GlobalImages {}

pub fn init(cx: &mut App) {
    let cache = cx.new(|_| ImageCache::default());
    cx.set_global(GlobalImages(cache));
}

pub fn cache(cx: &App) -> Entity<ImageCache> {
    cx.global::<GlobalImages>().0.clone()
}

/// Which rendition of a cover the real client loads for a slot (it rewrites the size code in
/// `…/image/<prefix 12 hex><code 4 hex><id 24 hex>` and asks for WebP).
#[derive(Clone, Copy)]
pub enum Variant {
    /// Library rows, player bar: 64px (artists 160px).
    Small,
    /// Home shortcuts: 300px (artists 160px).
    Medium,
    /// Now Playing cover: 640px.
    Large,
}

/// Measured from the reference (2026-11-07): album `ab67616d0000`, artist `ab6761610000`,
/// user playlist `ab67706c0000`. Unknown prefixes and non-cover URLs are returned unchanged.
pub fn variant(url: &str, v: Variant) -> String {
    let Some(pos) = url.find("/image/") else { return url.to_string() };
    let key = &url[pos + 7..];
    if key.len() != 40 || !key.bytes().all(|b| b.is_ascii_hexdigit()) {
        return url.to_string();
    }
    let (prefix, id) = (&key[..12], &key[16..]);
    let code = match (prefix, v) {
        ("ab67616d0000", Variant::Small) => "11eb",
        ("ab67616d0000", Variant::Medium) => "e1a3",
        ("ab67616d0000", Variant::Large) => "aa54",
        ("ab6761610000", Variant::Small | Variant::Medium) => "101f",
        ("ab6761610000", Variant::Large) => "e5eb",
        ("ab67706c0000", Variant::Small) => "dec5",
        ("ab67706c0000", Variant::Medium | Variant::Large) => "d72c",
        _ => return url.to_string(),
    };
    format!("{}{prefix}{code}{id}", &url[..pos + 7])
}

/// Downloads with retries: the CDNs sometimes reset TLS handshakes when many covers load at once.
/// Spotify serves the same `/image/<id>` files from these hosts (the client itself uses all three);
/// a connection failure on one moves on to the next.
const IMAGE_HOSTS: [&str; 3] = ["i.scdn.co", "image-cdn-fa.spotifycdn.com", "image-cdn-ak.spotifycdn.com"];

fn download(url: &str) -> anyhow::Result<Vec<u8>> {
    let mut last = None;
    let host = IMAGE_HOSTS.iter().find(|h| url.starts_with(&format!("https://{h}/image/")));
    for attempt in 0..9u64 {
        if attempt > 0 {
            std::thread::sleep(std::time::Duration::from_millis((150 * attempt).min(1000)));
        }
        let url = match host {
            Some(h) => url.replacen(h, IMAGE_HOSTS[attempt as usize % IMAGE_HOSTS.len()], 1),
            None => url.to_string(),
        };
        match crate::http::agent().get(&url).call() {
            Ok(resp) => {
                let mut bytes = Vec::new();
                resp.into_reader().read_to_end(&mut bytes)?;
                return Ok(bytes);
            }
            // A real HTTP status (404 etc.) won't change on retry.
            Err(e @ ureq::Error::Status(..)) => return Err(e.into()),
            Err(e) => last = Some(e),
        }
    }
    Err(last.map(Into::into).unwrap_or_else(|| anyhow::anyhow!("download failed")))
}

/// On-disk cache (`%APPDATA%\Spotirust\cache\images`), like the real client's, so restarts
/// don't re-download every cover.
fn cache_path(url: &str) -> Option<std::path::PathBuf> {
    use sha2::Digest as _;
    let base = std::env::var_os("APPDATA")?;
    let name: String = sha2::Sha256::digest(url.as_bytes()).iter().map(|b| format!("{b:02x}")).collect();
    Some(std::path::PathBuf::from(base).join("Spotirust").join("cache").join("images").join(name))
}

fn cached_download(url: &str) -> anyhow::Result<Vec<u8>> {
    let path = cache_path(url);
    if let Some(bytes) = path.as_ref().and_then(|p| std::fs::read(p).ok()) {
        return Ok(bytes);
    }
    let bytes = download(url)?;
    if let Some(p) = path {
        if let Some(dir) = p.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(p, &bytes);
    }
    Ok(bytes)
}

/// `spotify:mosaic:a:b:c:d` as the client draws it: the four covers in a 2x2 grid
/// (a b / c d), composed at full tile size and scaled afterwards.
fn mosaic(ids: &str) -> anyhow::Result<image::RgbaImage> {
    let tiles = ids
        .split(':')
        .map(|id| Ok(image::load_from_memory(&cached_download(&format!("https://i.scdn.co/image/{id}"))?)?.to_rgba8()))
        .collect::<anyhow::Result<Vec<_>>>()?;
    anyhow::ensure!(tiles.len() == 4, "mosaic needs 4 tiles, got {}", tiles.len());
    let t = tiles.iter().map(|i| i.width().min(i.height())).min().unwrap_or(1);
    let mut out = image::RgbaImage::new(t * 2, t * 2);
    for (k, tile) in tiles.iter().enumerate() {
        let tile = if tile.width() == t && tile.height() == t {
            tile.clone()
        } else {
            image::imageops::resize(tile, t, t, image::imageops::FilterType::Triangle)
        };
        image::imageops::replace(&mut out, &tile, (k as i64 % 2) * t as i64, (k as i64 / 2) * t as i64);
    }
    Ok(out)
}

/// Window scale factor (f32 bits), so covers can be pre-sized to device pixels.
static SCALE: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0x3F80_0000); // 1.0

pub fn set_scale(scale: f32) {
    SCALE.store(scale.to_bits(), std::sync::atomic::Ordering::Relaxed);
}

/// Device-pixel width for a slot `css_px` wide.
fn device_px(css_px: f32) -> u32 {
    (css_px * f32::from_bits(SCALE.load(std::sync::atomic::Ordering::Relaxed))).round() as u32
}

/// `width` 0 keeps the source size. Otherwise the image is downscaled to `width` device pixels with
/// an antialiased triangle filter: GPUI samples big textures without mipmaps (aliasing), and this
/// filter is the closest match to Chromium's cover rendering (measured: mean error 2.3 vs 10 on a
/// 300px cover drawn at 60px).
fn fetch(url: &str, width: u32) -> anyhow::Result<Arc<RenderImage>> {
    // `crop:<w>x<h>:<url>`: centre crop to that aspect ratio first (CSS object-fit: cover; GPUI's
    // cover fit draws outside the element instead of clipping).
    // `crop:<w>x<h>@<fx>,<fy>:<url>` anchors the crop like `background-position: fx fy` (fractions).
    let (aspect, anchor, url) = match url.strip_prefix("crop:").and_then(|r| r.split_once(':')) {
        Some((dims, rest)) => {
            let (dims, anchor) = match dims.split_once('@') {
                Some((d, a)) => {
                    let (fx, fy) = a.split_once(',').unwrap_or(("0.5", "0.5"));
                    (d, (fx.parse::<f32>().unwrap_or(0.5), fy.parse::<f32>().unwrap_or(0.5)))
                }
                None => (dims, (0.5, 0.5)),
            };
            let (w, h) = dims.split_once('x').unwrap_or(("1", "1"));
            (Some(w.parse::<f32>().unwrap_or(1.) / h.parse::<f32>().unwrap_or(1.)), anchor, rest)
        }
        None => (None, (0.5, 0.5), url),
    };
    let mut rgba = match url.strip_prefix("spotify:mosaic:") {
        Some(ids) => mosaic(ids)?,
        None => image::load_from_memory(&cached_download(url)?)?.to_rgba8(),
    };
    if let Some(a) = aspect {
        let (w, h) = (rgba.width() as f32, rgba.height() as f32);
        let (cw, ch) = if w / h > a { (h * a, h) } else { (w, w / a) };
        let (cw, ch) = (cw.round().max(1.) as u32, ch.round().max(1.) as u32);
        let x = ((rgba.width() - cw) as f32 * anchor.0).round() as u32;
        let y = ((rgba.height() - ch) as f32 * anchor.1).round() as u32;
        rgba = image::imageops::crop_imm(&rgba, x, y, cw, ch).to_image();
    }
    if width > 0 && rgba.width() > width {
        let height = ((rgba.height() as f32 * width as f32 / rgba.width() as f32).round() as u32).max(1);
        rgba = image::imageops::resize(&rgba, width, height, image::imageops::FilterType::Triangle);
    }
    for p in rgba.pixels_mut() {
        p.0.swap(0, 2); // GPUI wants BGRA
    }
    Ok(Arc::new(RenderImage::new(vec![image::Frame::new(rgba)])))
}

use std::io::Read as _;

impl ImageCache {
    /// Returns the image if loaded; otherwise starts loading it and notifies observers when done.
    pub fn get(&mut self, url: &str, cx: &mut Context<Self>) -> Option<Arc<RenderImage>> {
        self.load(url, 0, cx)
    }

    /// Like [`Self::get`], pre-sized for a slot `css_px` wide.
    pub fn get_sized(&mut self, url: &str, css_px: f32, cx: &mut Context<Self>) -> Option<Arc<RenderImage>> {
        self.load(url, device_px(css_px), cx)
    }

    fn load(&mut self, url: &str, width: u32, cx: &mut Context<Self>) -> Option<Arc<RenderImage>> {
        let key = if width > 0 { format!("{url}#{width}") } else { url.to_string() };
        match self.entries.get(&key) {
            Some(Entry::Ready(img)) => return Some(img.clone()),
            Some(_) => return None,
            None => {}
        }
        self.entries.insert(key.clone(), Entry::Loading);
        let url = url.to_string();
        cx.spawn(async move |this, cx| {
            let u = url.clone();
            let result = cx.background_executor().spawn(async move { fetch(&u, width) }).await;
            let _ = this.update(cx, |this, cx| {
                let entry = match result {
                    Ok(img) => Entry::Ready(img),
                    Err(e) => {
                        eprintln!("image {url}: {e:#}");
                        Entry::Failed
                    }
                };
                this.entries.insert(key, entry);
                cx.notify();
            });
        })
        .detach();
        None
    }
}
