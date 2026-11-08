//! Renders the login page's "fake web player" backdrop exactly like the CSS does:
//!
//! .fakeWebPlayer            fixed, inset -3px, bg #000, filter: blur(3px)
//!   __sidebar               200 x (H+6-90), bg #121212, side.png @ 0 60px no-repeat
//!   __content-wrapper       x 197, linear-gradient(-180deg, #414141 0%, #181818 20%)
//!     __content             top +20, content.avif, size 90%, pos 50% 0%
//!   __footer                height 90, bg #282828; left/center/right images centered in boxes
//!   ::before (z 1)          linear-gradient(-180deg, rgba(0,0,0,.3), rgba(0,0,0,.8)),
//!                           linear-gradient(-225deg, rgba(29,49,102,.4), rgba(159,195,211,.4))
//!
//! The result is composed at physical resolution and blurred once, then shown as one image.

use image::{Rgba, RgbaImage, imageops};

use crate::spotify_assets::Spa;

pub struct BackdropAssets {
    side: RgbaImage,
    content: RgbaImage,
    /// `@media (width<=1500px)` swaps in this smaller artwork.
    content_small: RgbaImage,
    controls: RgbaImage,
    footer_left: RgbaImage,
    footer_right: RgbaImage,
}

impl BackdropAssets {
    pub fn load() -> anyhow::Result<Self> {
        let mut spa = Spa::open("login.spa")?;
        let dir = "images/fake-web-player/";
        Ok(Self {
            side: spa.image(&format!("{dir}side.png"))?,
            content: spa.image(&format!("{dir}content.avif"))?,
            content_small: spa.image(&format!("{dir}content-small.avif"))?,
            controls: spa.image(&format!("{dir}controls.png"))?,
            footer_left: spa.image(&format!("{dir}footer-left.png"))?,
            footer_right: spa.image(&format!("{dir}footer-right.png"))?,
        })
    }
}

/// Canvas in physical pixels with a margin so the blur has real content past the viewport edge.
struct Canvas {
    img: RgbaImage,
    s: f32,
    margin: f32, // css px
}

impl Canvas {
    fn px(&self, v: f32) -> i64 {
        ((v + self.margin) * self.s).round() as i64
    }

    fn rect_px(&self, x: f32, y: f32, w: f32, h: f32) -> (i64, i64, i64, i64) {
        let (x0, y0) = (self.px(x), self.px(y));
        (x0, y0, self.px(x + w) - x0, self.px(y + h) - y0)
    }

    fn for_each(&mut self, x: f32, y: f32, w: f32, h: f32, mut f: impl FnMut(&mut Rgba<u8>, f32, f32)) {
        let (x0, y0, pw, ph) = self.rect_px(x, y, w, h);
        let (iw, ih) = (self.img.width() as i64, self.img.height() as i64);
        for py in y0.max(0)..(y0 + ph).min(ih) {
            let ty = (py - y0) as f32 / ph.max(1) as f32;
            for px in x0.max(0)..(x0 + pw).min(iw) {
                let tx = (px - x0) as f32 / pw.max(1) as f32;
                f(self.img.get_pixel_mut(px as u32, py as u32), tx, ty);
            }
        }
    }

    fn fill(&mut self, x: f32, y: f32, w: f32, h: f32, c: [u8; 3]) {
        self.for_each(x, y, w, h, |p, _, _| *p = Rgba([c[0], c[1], c[2], 255]));
    }

    /// Draw `src` scaled to css size (w, h) at (x, y), clipped to `clip` (css rect).
    fn image(&mut self, src: &RgbaImage, x: f32, y: f32, w: f32, h: f32, clip: (f32, f32, f32, f32)) {
        let (x0, y0, pw, ph) = self.rect_px(x, y, w, h);
        if pw <= 0 || ph <= 0 {
            return;
        }
        let scaled = imageops::resize(src, pw as u32, ph as u32, imageops::FilterType::Triangle);
        let (cx0, cy0, cw, ch) = self.rect_px(clip.0, clip.1, clip.2, clip.3);
        let (iw, ih) = (self.img.width() as i64, self.img.height() as i64);
        for (sx, sy, sp) in scaled.enumerate_pixels() {
            let (dx, dy) = (x0 + sx as i64, y0 + sy as i64);
            if dx < cx0.max(0) || dy < cy0.max(0) || dx >= (cx0 + cw).min(iw) || dy >= (cy0 + ch).min(ih) {
                continue;
            }
            blend(self.img.get_pixel_mut(dx as u32, dy as u32), [sp[0], sp[1], sp[2]], sp[3] as f32 / 255.);
        }
    }
}

fn blend(dst: &mut Rgba<u8>, c: [u8; 3], a: f32) {
    for i in 0..3 {
        dst[i] = (dst[i] as f32 * (1. - a) + c[i] as f32 * a).round() as u8;
    }
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// Compose + blur the backdrop for a viewport of `w x h` css px at `scale`. Returns RGBA.
pub fn compose(a: &BackdropAssets, w: f32, h: f32, scale: f32) -> RgbaImage {
    let margin = 16.;
    let (cw, chh) = (((w + 2. * margin) * scale).round() as u32, ((h + 2. * margin) * scale).round() as u32);
    let mut cv = Canvas { img: RgbaImage::from_pixel(cw, chh, Rgba([0, 0, 0, 255])), s: scale, margin };

    // .fakeWebPlayer box
    let (lx, ly, lw, lh) = (-3., -3., w + 6., h + 6.);
    let body_h = lh - 90.;

    // sidebar
    cv.fill(lx, ly, 200., body_h, [18, 18, 18]);
    let (sw, sh) = (a.side.width() as f32, a.side.height() as f32);
    cv.image(&a.side, lx, ly + 60., sw, sh, (lx, ly, 200., body_h));

    // content wrapper gradient: #414141 0% -> #181818 20%
    let (wx, ww) = (lx + 200., lw - 200.);
    cv.for_each(wx, ly, ww, body_h, |p, _, t| {
        let k = (t / 0.2).min(1.);
        let v = lerp(65., 24., k).round() as u8;
        *p = Rgba([v, v, v, 255]);
    });
    // content image: 90% of content width, centered, top aligned, 20px below wrapper top
    let (cx, cy, cwid, chei) = (wx, ly + 20., ww, body_h - 20.);
    // login.css: `@media (width<=1500px)` uses content-small.avif; both at background-size 90%.
    let content = if w <= 1500. { &a.content_small } else { &a.content };
    let iw = cwid * 0.9;
    let ih = iw * content.height() as f32 / content.width() as f32;
    cv.image(content, cx + (cwid - iw) / 2., cy, iw, ih, (cx, cy, cwid, chei));

    // footer
    let fy = ly + body_h;
    cv.fill(lx, fy, lw, 90., [40, 40, 40]);
    let footer_clip = (lx, fy, lw, 90.);
    let fl = &a.footer_left;
    cv.image(fl, 17., fy + 30.5, fl.width() as f32, fl.height() as f32, footer_clip);
    let ct = &a.controls;
    cv.image(ct, lx + (lw - ct.width() as f32) / 2., fy + 18.5, ct.width() as f32, ct.height() as f32, footer_clip);
    let fr = &a.footer_right;
    cv.image(fr, w - 17. - fr.width() as f32, fy + 38., fr.width() as f32, fr.height() as f32, footer_clip);

    // ::before overlay (bottom layer first: the -225deg tint, then the -180deg darkening)
    let diag = (lw + lh) * std::f32::consts::FRAC_1_SQRT_2; // gradient line length for 135deg
    let (ccx, ccy) = (lx + lw / 2., ly + lh / 2.);
    let s = cv.s;
    let m = cv.margin;
    cv.for_each(lx, ly, lw, lh, |p, tx, ty| {
        // css position of this pixel
        let x = lx + tx * lw;
        let y = ly + ty * lh;
        let _ = (s, m);
        let t = (((x - ccx) + (y - ccy)) * std::f32::consts::FRAC_1_SQRT_2 / diag + 0.5).clamp(0., 1.);
        let c = [lerp(29., 159., t), lerp(49., 195., t), lerp(102., 211., t)];
        blend(p, [c[0] as u8, c[1] as u8, c[2] as u8], 0.4);
        blend(p, [0, 0, 0], lerp(0.3, 0.8, ty));
    });

    // filter: blur(3px)
    let blurred = imageops::fast_blur(&cv.img, 3. * scale);

    // crop to the viewport
    let off = (margin * scale).round() as u32;
    let (vw, vh) = ((w * scale).round() as u32, (h * scale).round() as u32);
    imageops::crop_imm(&blurred, off, off, vw, vh).to_image()
}
