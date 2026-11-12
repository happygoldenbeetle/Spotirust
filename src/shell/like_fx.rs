//! The burst a like / unlike plays, the client's own Lottie files
//! (`animations/plus-selected-confetti-white.json`, 60 frames, and
//! `plus-deselected-confetti-white.json`, frames 24..60), both at 60 fps. The client plays them in a
//! 16px box scaled 2.448x, so the confetti spills past the icon. The frames were rendered to SVG
//! with the client's lottie-web (`temp/pull_lottie.mjs`); here they're rasterised with resvg at the
//! exact device size, once per frame.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex, OnceLock},
    time::Instant,
};

use gpui::RenderImage;

use super::*;

const ON: &str = include_str!("../../assets/anim/like_on.svgs");
const OFF: &str = include_str!("../../assets/anim/like_off.svgs");
const FPS: f32 = 60.;
/// The player's box is the icon's size, transform scale(2.448), centred on the icon.
const SCALE: f32 = 2.448;

fn frames(on: bool) -> &'static [&'static str] {
    static F: OnceLock<[Vec<&'static str>; 2]> = OnceLock::new();
    &F.get_or_init(|| [OFF.lines().collect(), ON.lines().collect()])[on as usize]
}

/// How long the burst runs.
fn length(on: bool) -> f32 {
    frames(on).len() as f32 / FPS
}

fn raster(on: bool, i: usize, px: u32) -> Option<Arc<RenderImage>> {
    static CACHE: OnceLock<Mutex<HashMap<(bool, usize, u32), Arc<RenderImage>>>> = OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    if let Some(img) = cache.lock().ok()?.get(&(on, i, px)) {
        return Some(img.clone());
    }
    let svg = frames(on).get(i)?;
    let tree = resvg::usvg::Tree::from_str(svg, &resvg::usvg::Options::default()).ok()?;
    let mut pm = resvg::tiny_skia::Pixmap::new(px, px)?;
    let s = px as f32 / tree.size().width();
    resvg::render(&tree, resvg::tiny_skia::Transform::from_scale(s, s), &mut pm.as_mut());
    // tiny-skia is premultiplied RGBA; GPUI wants straight BGRA.
    let mut data = pm.take();
    for p in data.chunks_exact_mut(4) {
        let a = p[3] as u32;
        if a > 0 && a < 255 {
            for ch in &mut p[..3] {
                *ch = ((*ch as u32 * 255 + a / 2) / a).min(255) as u8;
            }
        }
        p.swap(0, 2);
    }
    let buf = image::ImageBuffer::from_raw(px, px, data)?;
    let img = Arc::new(RenderImage::new(vec![image::Frame::new(buf)]));
    cache.lock().ok()?.insert((on, i, px), img.clone());
    Some(img)
}

impl Shell {
    /// Starts the burst for `track` (called whenever its liked state changes here).
    pub(super) fn like_burst_start(&mut self, track: &str, on: bool) {
        self.like_fx.retain(|_, (o, at)| at.elapsed().as_secs_f32() < length(*o));
        self.like_fx.insert(track.to_string(), (on, Instant::now()));
    }

    /// Whether a burst is still running (the root then asks for another frame).
    pub(super) fn like_burst_animating(&self) -> bool {
        self.like_fx.values().any(|(on, at)| at.elapsed().as_secs_f32() < length(*on))
    }

    /// The running burst for `track`, to show in place of its `size` px icon.
    pub(super) fn like_burst(&self, track: &str, size: f32) -> Option<AnyElement> {
        // Test hook: SPOTIRUST_BURST="on|<frame>" or "off|<frame>" freezes that frame on every track.
        static FROZEN: OnceLock<Option<(bool, usize)>> = OnceLock::new();
        let frozen = *FROZEN.get_or_init(|| {
            let v = std::env::var("SPOTIRUST_BURST").ok()?;
            let (on, f) = v.split_once('|')?;
            Some((on == "on", f.parse().ok()?))
        });
        let (on, i) = match frozen {
            Some(f) => f,
            None => {
                let (on, at) = *self.like_fx.get(track)?;
                (on, (at.elapsed().as_secs_f32() * FPS) as usize)
            }
        };
        if i >= frames(on).len() {
            return None;
        }
        let boxed = size * SCALE;
        let dev = (boxed * images::scale()).round() as u32;
        let img = raster(on, i, dev)?;
        let off = (size - boxed) / 2.;
        Some(
            div()
                .size(px(size))
                .flex_none()
                .relative()
                .child(gpui::img(gpui::ImageSource::Render(img)).absolute().left(px(off)).top(px(off)).size(px(boxed)))
                .into_any_element(),
        )
    }
}
