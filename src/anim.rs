//! CSS timing functions, so transitions measured on the reference carry over exactly.

/// `cubic-bezier(x1, y1, x2, y2)` as an easing function for `t` in 0..=1.
pub fn cubic_bezier(x1: f32, y1: f32, x2: f32, y2: f32) -> impl Fn(f32) -> f32 + Copy {
    move |t: f32| {
        if t <= 0. {
            return 0.;
        }
        if t >= 1. {
            return 1.;
        }
        let bx = |s: f32| 3. * (1. - s).powi(2) * s * x1 + 3. * (1. - s) * s * s * x2 + s.powi(3);
        let by = |s: f32| 3. * (1. - s).powi(2) * s * y1 + 3. * (1. - s) * s * s * y2 + s.powi(3);
        // solve bx(s) = t by bisection (monotonic for x1, x2 in 0..=1)
        let (mut lo, mut hi) = (0f32, 1f32);
        for _ in 0..24 {
            let mid = (lo + hi) / 2.;
            if bx(mid) < t {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        by((lo + hi) / 2.)
    }
}

/// CSS `ease-out`.
pub fn ease_out(t: f32) -> f32 {
    cubic_bezier(0., 0., 0.58, 1.)(t)
}

/// Progress of a CSS transition `duration` ms long with `delay` ms, `elapsed` ms after it started.
pub fn progress(elapsed: f32, delay: f32, duration: f32) -> f32 {
    ((elapsed - delay) / duration).clamp(0., 1.)
}
