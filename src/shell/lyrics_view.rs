//! The cinema view (the client's expanded Now Playing view), measured from the reference. It takes
//! Your Library's and the right panel's place: one rounded #121212 panel under the top bar.
//!
//! Toolbar (top 16, right 16): Show artwork / Show Canvas / Show artist image / Show lyrics, a
//! 1.6px rgba(255,255,255,.1) divider (8 | 17.6 | 8), then More / Enter Full screen / Minimize;
//! 32px round buttons 8 apart, white 16px icons; unavailable modes at opacity .3; the open mode on
//! #121212 with a 4px white dot (top 26, centred).
//!
//! Artwork: `linear-gradient(--cinema-mode-bg-color-from, -to)` (the cover's colour set:
//! highContrast then higherContrast backgroundBase) running 108px past the bottom; the cover is
//! 58vh square, centred (10px above the window's middle), radius 16, shadow
//! `0 52px 52px -20px rgba(0,0,0,.3), 0 4px 20px rgba(0,0,0,.1)`.
//!
//! Lyrics: the lyrics' card colours (background, subdued text, white for the playing line, passed
//! lines the subdued colour at 50%). Lines: title font 48/700, line height 57.6, 28.8 apart, in a
//! centred column up to 1024 wide, starting 204.6 down; the playing line is kept at the vertical
//! centre and scrolls smoothly. Clicking a line seeks to it.

use std::cell::RefCell;

use gpui::{ScrollDelta, ScrollWheelEvent};

use super::*;

/// Where the first line sits before any scrolling.
const LINES_TOP: f32 = 204.6;

/// What the cinema view shows.
#[derive(Clone, Copy, PartialEq, Default)]
pub enum CinemaMode {
    #[default]
    Artwork,
    Lyrics,
}

/// Per-frame state of the lyrics scroller.
#[derive(Default)]
pub struct LyricsScroll {
    /// Current scroll offset (px) and where it's heading.
    pub offset: f32,
    pub target: f32,
    /// Line tops / heights from the last paint (relative to the column).
    pub lines: Rc<RefCell<Vec<(f32, f32)>>>,
    /// Viewport height from the last paint.
    pub view_h: Rc<Cell<f32>>,
    /// The user scrolled: stop following until the next line starts.
    pub manual_until_line: Option<usize>,
    pub last_frame: Option<Instant>,
    /// Wheel scrolling glides like the client's (Chromium) scrollers.
    pub glide: gpui::ScrollGlide,
}

impl Shell {
    /// Player bar mic: opens the cinema view on the lyrics, or closes it when they're showing.
    pub(super) fn toggle_lyrics(&mut self, cx: &mut Context<Self>) {
        if self.page == Page::Lyrics && self.cinema_mode == CinemaMode::Lyrics {
            self.go_back(cx);
        } else {
            self.open_cinema(CinemaMode::Lyrics, cx);
        }
    }

    pub(super) fn open_cinema(&mut self, mode: CinemaMode, cx: &mut Context<Self>) {
        if mode == CinemaMode::Lyrics && self.cinema_mode != CinemaMode::Lyrics {
            self.lyrics_scroll = LyricsScroll::default();
        }
        self.cinema_mode = mode;
        if self.page != Page::Lyrics {
            self.open(Page::Lyrics, cx);
        }
        cx.notify();
    }

    /// Index of the line playing at `ms` (synced lyrics).
    pub(super) fn active_line(timed: &[(u64, String)], ms: u64) -> Option<usize> {
        let i = timed.partition_point(|(t, _)| *t <= ms);
        i.checked_sub(1)
    }

    pub(super) fn lyrics_view(&mut self, w: f32, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let lyr = self.lyrics.as_ref().and_then(|l| l.1.clone());
        // The client's --lyrics-color-*: the card's background and subdued text, active white.
        let (bg, inactive, active) = lyr.as_ref().map(|l| (l.background, l.text, 0xFFFFFF)).unwrap_or((0x535353, 0xFFFFFF, 0xFFFFFF));
        let mode = self.cinema_mode;
        let toolbar = self.cinema_toolbar(mode, lyr.is_some(), cx);

        if mode == CinemaMode::Artwork {
            return self.cinema_artwork(w, window, cx).child(toolbar).into_any_element();
        }

        let Some(lyr) = lyr else {
            let msg = if self.lyrics.as_ref().is_some_and(|l| l.0.is_empty()) || self.lyrics.is_none() { "" } else { "Looks like we don\u{2019}t have the lyrics for this song." };
            return div()
                .relative()
                .size_full()
                .rounded(px(8.))
                .bg(c(bg))
                .child(
                    div()
                        .size_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(px(24.))
                        .font_weight(FontWeight::BOLD)
                        .text_color(c(WHITE))
                        .child(msg),
                )
                .child(toolbar)
                .into_any_element();
        };

        let now = self.progress_ms();
        let cur = if lyr.synced { Self::active_line(&lyr.timed, now) } else { None };

        // Follow the active line (centre it), easing toward it each frame.
        let st = &mut self.lyrics_scroll;
        // Line tops were recorded in window space at the last paint: make them column-relative.
        let measured: Vec<(f32, f32)> = {
            let v = st.lines.borrow();
            let base = v.first().map(|f| f.0).unwrap_or(0.);
            v.iter().map(|(t, h)| (t - base, *h)).collect()
        };
        let view_h = st.view_h.get();
        if st.manual_until_line.is_some_and(|l| Some(l) != cur) {
            st.manual_until_line = None;
        }
        if st.manual_until_line.is_none() {
            if let Some((top, h)) = cur.and_then(|i| measured.get(i).copied()) {
                st.target = (LINES_TOP + top + h / 2. - view_h / 2.).max(0.);
            } else if cur.is_none() {
                st.target = 0.;
            }
        }
        let dt = st.last_frame.map(|t| t.elapsed().as_secs_f32()).unwrap_or(0.016).min(0.1);
        st.last_frame = Some(Instant::now());
        let k = 1. - (-dt * 10.).exp();
        if st.manual_until_line.is_some() && st.glide.running() {
            st.offset = st.glide.value();
            st.target = st.glide.target();
            window.request_animation_frame();
        } else {
            st.glide.stop();
            st.offset += (st.target - st.offset) * k;
        }
        if (st.target - st.offset).abs() < 0.5 {
            st.offset = st.target;
        } else {
            window.request_animation_frame();
        }
        if self.playback.as_ref().is_some_and(|p| p.is_playing) {
            window.request_animation_frame();
        }
        let offset = st.offset;
        let rec = st.lines.clone();
        let view_rec = st.view_h.clone();

        let col_w = (w - 128.).min(1024.).max(200.);
        let mut column = div().w(px(col_w)).flex().flex_col().gap(px(28.8));
        for (i, (ms, words)) in lyr.timed.iter().enumerate() {
            let words = if words.is_empty() { "\u{266a}".to_string() } else { balance(window, words, 48., col_w) };
            let (color, opacity) = match cur {
                _ if !lyr.synced => (active, 1.),
                // passed lines: the inactive colour at 50% (measured)
                Some(c) if i < c => (inactive, 0.5),
                Some(c) if i == c => (active, 1.),
                _ => (inactive, 1.),
            };
            let rec = rec.clone();
            let seek_to = *ms;
            column = column.child(
                div()
                    .id(gpui::ElementId::Name(format!("lyric-{i}").into()))
                    .relative()
                    .font_family(title_font())
                    .text_size(px(48.))
                    .line_height(px(57.6))
                    .font_weight(FontWeight::BOLD)
                    .text_color(c(color))
                    .opacity(opacity)
                    .when(lyr.synced, |d| d.cursor_pointer().hover(|s| s.underline()))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if lyr_synced_guard(this) {
                            this.player(Play::Seek(seek_to), cx);
                        }
                    }))
                    .child(
                        canvas(
                            move |b, _, _| {
                                let mut v = rec.borrow_mut();
                                if v.len() <= i {
                                    v.resize(i + 1, (0., 0.));
                                }
                                v[i] = (f32::from(b.origin.y), f32::from(b.size.height));
                            },
                            |_, _, _, _| {},
                        )
                        .absolute()
                        .inset_0(),
                    )
                    .child(words),
            );
        }
        if !lyr.synced {
            column = column.child(
                div()
                    .mt(px(16.))
                    .text_size(px(16.))
                    .text_color(c(inactive))
                    .child("These lyrics aren\u{2019}t synced to the song yet."),
            );
        }
        div()
            .id("lyrics-view")
            .relative()
            .size_full()
            .rounded(px(8.))
            .overflow_hidden()
            .bg(c(bg))
            .on_scroll_wheel(cx.listener(|this, ev: &ScrollWheelEvent, _, cx| {
                let dy = match ev.delta {
                    ScrollDelta::Lines(p) => p.y * gpui::ScrollGlide::PX_PER_LINE,
                    ScrollDelta::Pixels(p) => f32::from(p.y),
                };
                let st = &mut this.lyrics_scroll;
                if matches!(ev.delta, ScrollDelta::Lines(_)) {
                    let off = st.offset;
                    st.glide.push(off, -dy, 0., f32::MAX);
                    st.target = st.glide.target();
                } else {
                    st.glide.stop();
                    st.target = (st.target - dy).max(0.);
                    st.offset = st.target;
                }
                let cur = this.lyrics.as_ref().and_then(|l| l.1.as_ref()).and_then(|l| Self::active_line(&l.timed, this.progress_ms()));
                this.lyrics_scroll.manual_until_line = cur.or(Some(usize::MAX));
                cx.notify();
            }))
            .child(
                canvas(move |b, _, _| view_rec.set(f32::from(b.size.height)), |_, _, _, _| {})
                    .absolute()
                    .inset_0(),
            )
            .child(
                div()
                    .absolute()
                    .left(px((w - col_w) / 2.))
                    .top(px(LINES_TOP - offset))
                    .pb(px(240.))
                    .child(column),
            )
            .child(top_shade())
            .child(toolbar)
            .into_any_element()
    }
}

/// Lyrics mode: the toolbar's 120px `::before` shade, `linear-gradient(rgba(0,0,0,.85) 0%, .75 15%,
/// .64 25%, .51 35%, .38 45%, .26 55%, .16 65%, .09 75%, .04 85%, .01 93%, 0 100%)`, as two-stop
/// segments (GPUI gradients take two stops).
fn top_shade() -> Div {
    const STOPS: [(f32, f32); 11] = [
        (0., 0.85), (0.15, 0.75), (0.25, 0.64), (0.35, 0.51), (0.45, 0.38), (0.55, 0.26),
        (0.65, 0.16), (0.75, 0.09), (0.85, 0.04), (0.93, 0.01), (1., 0.),
    ];
    let mut d = div().absolute().top_0().left_0().right_0().h(px(120.));
    for w in STOPS.windows(2) {
        let ((p0, a0), (p1, a1)) = (w[0], w[1]);
        d = d.child(div().absolute().left_0().right_0().top(px(120. * p0)).h(px(120. * (p1 - p0))).bg(linear_gradient(
            180.,
            linear_color_stop(gpui::hsla(0., 0., 0., a0), 0.),
            linear_color_stop(gpui::hsla(0., 0., 0., a1), 1.),
        )));
    }
    d
}

impl Shell {
    /// The cinema view's toolbar (see the module docs).
    fn cinema_toolbar(&self, mode: CinemaMode, has_lyrics: bool, cx: &mut Context<Self>) -> Div {
        let button = |id: &'static str, path: &'static str, on: bool, enabled: bool, cx: &mut Context<Self>| {
            let t = self.fades.get(id);
            let s = if self.fades.on(id) && enabled { 1.04 } else { 1. };
            let el = round_button(id, 32.)
                .relative()
                .opacity(if enabled { 1. } else { 0.3 })
                .when(on, |d| d.bg(c(0x121212)))
                .child(fx::scaled_icon(path, 16., s).text_color(fx::mix(0xFFFFFFFF, 0xFFFFFFFF, t)))
                .when(on, |d| d.child(div().absolute().top(px(26.)).left(px(14.)).size(px(4.)).rounded_full().bg(c(WHITE))));
            if enabled { self.on_fade(el, id, fx::TERTIARY, cx) } else { el.cursor_default() }
        };
        let media = div()
            .flex()
            .flex_row()
            .gap(px(8.))
            .child(
                button("cinema-artwork", "icons/npv_artwork.svg", mode == CinemaMode::Artwork, true, cx)
                    .on_click(cx.listener(|this, _, _, cx| this.open_cinema(CinemaMode::Artwork, cx))),
            )
            .child(button("cinema-canvas", "icons/npv_canvas.svg", false, false, cx))
            .child(button("cinema-artist", "icons/npv_artist_image.svg", false, false, cx))
            .child({
                let b = button("cinema-lyrics", "icons/npv_lyrics.svg", mode == CinemaMode::Lyrics, has_lyrics, cx);
                if has_lyrics { b.on_click(cx.listener(|this, _, _, cx| this.open_cinema(CinemaMode::Lyrics, cx))) } else { b }
            });
        let rest = div()
            .flex()
            .flex_row()
            .gap(px(8.))
            .child(button("cinema-more", "icons/more.svg", false, true, cx))
            .child(button("cinema-full", "icons/fullscreen.svg", false, true, cx).on_click(cx.listener(|this, _, window, cx| this.toggle_fullscreen(window, cx))))
            .child(button("cinema-min", "icons/npv_minimize.svg", false, true, cx).on_click(cx.listener(|this, _, _, cx| this.go_back(cx))));
        div()
            .absolute()
            .top(px(16.))
            .right(px(16.))
            .flex()
            .flex_row()
            .items_center()
            .child(media)
            .child(div().mx(px(8.)).w(px(17.6)).h(px(24.)).flex().justify_center().child(div().w(px(1.6)).h_full().bg(rgba(0xFFFFFF1A))))
            .child(rest)
    }

    /// Artwork mode: the cover on the cover's colour gradient.
    fn cinema_artwork(&self, w: f32, window: &Window, cx: &mut Context<Self>) -> Div {
        let id = self.playback.as_ref().and_then(|p| p.item.as_ref()).and_then(|t| t.id.clone()).unwrap_or_default();
        let (from, to) = self.cinema_colors.as_ref().filter(|c| c.0 == id).map(|c| (c.1, c.2)).unwrap_or((0x535353, 0x353535));
        let art = self
            .playback
            .as_ref()
            .and_then(|p| p.item.as_ref())
            .and_then(|t| t.album.as_ref())
            .and_then(|a| api::pick_image(&a.images, 640).map(|i| images::variant(&i.url, images::Variant::Large)));
        let win_h = f32::from(window.viewport_size().height);
        let size = (win_h * 0.58).min(w - 32.).max(0.);
        // centred in the window, 10px up; the view starts 64px down
        let top = win_h / 2. - 10. - size / 2. - 64.;
        div()
            .relative()
            .size_full()
            .rounded(px(8.))
            .overflow_hidden()
            .bg(c(0x121212))
            .child(div().absolute().top_0().left_0().right_0().bottom(px(-108.)).bg(linear_gradient(
                180.,
                linear_color_stop(c(from), 0.),
                linear_color_stop(c(to), 1.),
            )))
            .child(
                div()
                    .absolute()
                    .left(px((w - size) / 2.))
                    .top(px(top))
                    .size(px(size))
                    .rounded(px(16.))
                    .shadow(vec![
                        gpui::BoxShadow { color: gpui::hsla(0., 0., 0., 0.3), offset: gpui::point(px(0.), px(52.)), blur_radius: px(52.), spread_radius: px(-20.) },
                        gpui::BoxShadow { color: gpui::hsla(0., 0., 0., 0.1), offset: gpui::point(px(0.), px(4.)), blur_radius: px(20.), spread_radius: px(0.) },
                    ])
                    .child(cover(art.as_deref(), size, 16., cx)),
            )
    }
}

/// Clicking a line seeks only while it's synced lyrics on the playing track.
fn lyr_synced_guard(this: &Shell) -> bool {
    this.lyrics.as_ref().and_then(|l| l.1.as_ref()).is_some_and(|l| l.synced)
}

/// CSS `text-wrap: balance`: when `text` needs more than one line at `max_w`, the narrowest width
/// that keeps the same number of lines, returned as the text with its line breaks put in.
fn balance(window: &Window, text: &str, size: f32, max_w: f32) -> String {
    let font = gpui::font(title_font()).bold();
    let words: Vec<&str> = text.split(' ').filter(|w| !w.is_empty()).collect();
    if words.len() < 2 {
        return text.to_string();
    }
    let widths: Vec<f32> = words.iter().map(|w| super::playlist_view::text_width(window, w, size, font.clone())).collect();
    let space = super::playlist_view::text_width(window, "a b", size, font.clone())
        - super::playlist_view::text_width(window, "ab", size, font.clone());
    // Greedy wrap at `w`: the index each line starts at.
    let wrap = |w: f32| {
        let mut starts = vec![0];
        let mut x = widths[0];
        for i in 1..words.len() {
            if x + space + widths[i] > w {
                starts.push(i);
                x = widths[i];
            } else {
                x += space + widths[i];
            }
        }
        starts
    };
    let lines = wrap(max_w).len();
    if lines < 2 {
        return text.to_string();
    }
    let (mut lo, mut hi) = (widths.iter().cloned().fold(0., f32::max), max_w);
    for _ in 0..24 {
        let mid = (lo + hi) / 2.;
        if wrap(mid).len() > lines { lo = mid } else { hi = mid }
    }
    let starts = wrap(hi);
    let mut out = String::new();
    for (k, &s) in starts.iter().enumerate() {
        let e = starts.get(k + 1).copied().unwrap_or(words.len());
        if k > 0 {
            out.push('\n');
        }
        out.push_str(&words[s..e].join(" "));
    }
    out
}
