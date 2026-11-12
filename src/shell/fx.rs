//! CSS-style transitions for GPUI, which has none: easing curves, colour mixing, and hover fades
//! keyed by element id (each with its own measured duration / easing, like the client's CSS).

use std::{cell::RefCell, collections::HashMap, time::Instant};

use gpui::SharedString;

/// cubic-bezier(x1, y1, x2, y2) at progress `x` (0..1).
pub fn bezier(x1: f32, y1: f32, x2: f32, y2: f32, x: f32) -> f32 {
    if x <= 0. {
        return 0.;
    }
    if x >= 1. {
        return 1.;
    }
    let b = |t: f32, p1: f32, p2: f32| 3. * (1. - t) * (1. - t) * t * p1 + 3. * (1. - t) * t * t * p2 + t * t * t;
    let (mut lo, mut hi) = (0f32, 1f32);
    for _ in 0..24 {
        let m = (lo + hi) / 2.;
        if b(m, x1, x2) < x {
            lo = m
        } else {
            hi = m
        }
    }
    b((lo + hi) / 2., y1, y2)
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Ease {
    Linear,
    /// CSS `ease`.
    Ease,
    In,
    Out,
    InOut,
    /// Encore's "productive", cubic-bezier(.3, 0, 0, 1).
    Productive,
}

impl Ease {
    pub fn at(self, x: f32) -> f32 {
        let x = x.clamp(0., 1.);
        match self {
            Ease::Linear => x,
            Ease::Ease => bezier(0.25, 0.1, 0.25, 1., x),
            Ease::In => bezier(0.42, 0., 1., 1., x),
            Ease::Out => bezier(0., 0., 0.58, 1., x),
            Ease::InOut => bezier(0.42, 0., 0.58, 1., x),
            Ease::Productive => bezier(0.3, 0., 0., 1., x),
        }
    }
}

/// 0xRRGGBBAA blend.
pub fn mix(a: u32, b: u32, t: f32) -> gpui::Rgba {
    let ch = |v: u32, s: u32| ((v >> s) & 0xFF) as f32 / 255.;
    let l = |s| ch(a, s) + (ch(b, s) - ch(a, s)) * t;
    gpui::Rgba { r: l(24), g: l(16), b: l(8), a: l(0) }
}

/// The same blend, back as 0xRRGGBBAA.
pub fn mix_u32(a: u32, b: u32, t: f32) -> u32 {
    let m = mix(a, b, t);
    let q = |v: f32| ((v * 255.).round().clamp(0., 255.)) as u32;
    q(m.r) << 24 | q(m.g) << 16 | q(m.b) << 8 | q(m.a)
}

/// A value easing between two targets the way a CSS transition does: a change starts from wherever
/// the value is at that moment.
#[derive(Clone, Copy, Debug)]
pub struct Tween {
    pub from: f32,
    pub to: f32,
    pub at: Instant,
    pub secs: f32,
    pub ease: Ease,
}

impl Tween {
    pub fn new(v: f32) -> Self {
        Tween { from: v, to: v, at: Instant::now(), secs: 0., ease: Ease::Linear }
    }

    pub fn value(&self) -> f32 {
        if self.secs <= 0. {
            return self.to;
        }
        let p = self.at.elapsed().as_secs_f32() / self.secs;
        self.from + (self.to - self.from) * self.ease.at(p)
    }

    pub fn running(&self) -> bool {
        self.secs > 0. && self.at.elapsed().as_secs_f32() < self.secs && self.from != self.to
    }

    /// Heads for `to` over `secs`; no-op when already heading there.
    pub fn set(&mut self, to: f32, secs: f32, ease: Ease) {
        if self.to == to {
            return;
        }
        let now = self.value();
        *self = Tween { from: now, to, at: Instant::now(), secs, ease };
    }
}

/// Hover progress (0..1) of many elements, by id. Held in a RefCell so render code (`&self`) can
/// read it and listeners can write it.
#[derive(Default)]
pub struct Fades(RefCell<HashMap<SharedString, Tween>>);

impl Fades {
    /// Current progress of `id` (0 when never hovered).
    pub fn get(&self, id: &str) -> f32 {
        self.0.borrow().get(id).map(|t| t.value()).unwrap_or(0.)
    }

    /// Hover in / out of `id`, transitioning over `secs` with `ease`.
    pub fn hover(&self, id: SharedString, on: bool, secs: f32, ease: Ease) {
        let mut m = self.0.borrow_mut();
        let t = m.entry(id).or_insert_with(|| Tween::new(0.));
        t.set(if on { 1. } else { 0. }, secs, ease);
    }

    /// Whether any fade is still running (the root then asks for another frame). Drops finished
    /// fades at rest.
    pub fn animating(&self) -> bool {
        let mut m = self.0.borrow_mut();
        m.retain(|_, t| t.running() || t.to != 0.);
        m.values().any(|t| t.running())
    }
}
