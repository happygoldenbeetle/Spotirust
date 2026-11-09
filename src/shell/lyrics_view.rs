//! Lyrics (player bar mic button): the client's expanded Now Playing view in lyrics mode.
//! Measured from the reference: Your Library slides away and the view spans library + main;
//! background = the lyrics' background colour (radius 8). Header 64px: the context name 16/700
//! at x24, toolbar at the right (32px buttons, 16 from the edge). Lines: title font 48/700,
//! line height 57.6, 28.8 apart, in a 740px column 64px in; the active line (white) is kept at
//! the vertical centre and scrolls smoothly; passed lines are the active colour at 70%,
//! upcoming ones the inactive colour; colour/opacity ease over 0.1s. Clicking a line seeks to it.

use std::cell::RefCell;

use gpui::{ScrollDelta, ScrollWheelEvent};

use super::*;

/// Where the first line sits before any scrolling.
const LINES_TOP: f32 = 191.;

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
}

impl Shell {
    pub(super) fn toggle_lyrics(&mut self, cx: &mut Context<Self>) {
        if self.page == Page::Lyrics {
            self.go_back(cx);
        } else {
            self.lyrics_scroll = LyricsScroll::default();
            self.open(Page::Lyrics, cx);
        }
    }

    /// Index of the line playing at `ms` (synced lyrics).
    fn active_line(timed: &[(u64, String)], ms: u64) -> Option<usize> {
        let i = timed.partition_point(|(t, _)| *t <= ms);
        i.checked_sub(1)
    }

    pub(super) fn lyrics_view(&mut self, w: f32, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let lyr = self.lyrics.as_ref().and_then(|l| l.1.clone());
        let ctx_name = self.context_name.as_ref().map(|c| c.1.clone()).unwrap_or_default();
        let ctx_page = self.playback.as_ref().and_then(|p| p.context.as_ref()).and_then(|c| Page::for_uri(&c.uri));
        let (bg, inactive, active) = lyr.as_ref().map(|l| (l.view_bg, l.view_text, l.view_active)).unwrap_or((0x535353, 0xFFFFFF, 0xFFFFFF));

        let tool = |id: &'static str, path: &'static str, cx: &mut Context<Self>| {
            round_button(id, 32.).group(id).child(
                svg().path(path).size(px(16.)).text_color(rgba(0xFFFFFFB3)).group_hover(id, |s| s.text_color(c(WHITE))),
            )
            .on_hover(cx.listener(move |this, h: &bool, _, cx| this.bar_hovered(id, *h, cx)))
        };
        let header = div()
            .absolute()
            .top_0()
            .left_0()
            .right_0()
            .h(px(64.))
            .px(px(24.))
            .rounded_t(px(8.))
            .bg(c(bg))
            .flex()
            .flex_row()
            .items_center()
            .child(
                div()
                    .id("lyrics-ctx")
                    .flex_1()
                    .min_w_0()
                    .text_size(px(16.))
                    .font_weight(FontWeight::BOLD)
                    .text_color(c(WHITE))
                    .cursor_pointer()
                    .hover(|s| s.underline())
                    .child(ctx_name)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(p) = ctx_page.clone() {
                            this.open(p, cx);
                        }
                    })),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap(px(8.))
                    .mr(px(-8.))
                    .child(tool("lyrics-fullscreen", "icons/fullscreen.svg", cx).on_click(cx.listener(|this, _, window, cx| this.toggle_fullscreen(window, cx))))
                    .child(tool("lyrics-minimize", "icons/lib_collapse16.svg", cx).on_click(cx.listener(|this, _, _, cx| this.toggle_lyrics(cx)))),
            );

        let Some(lyr) = lyr else {
            let msg = if self.lyrics.as_ref().is_some_and(|l| l.0.is_empty()) || self.lyrics.is_none() { "" } else { "Looks like we don\u{2019}t have the lyrics for this song." };
            return div()
                .relative()
                .size_full()
                .rounded(px(8.))
                .bg(c(bg))
                .child(header)
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
        st.offset += (st.target - st.offset) * k;
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

        let col_w = (w - 128.).min(740.).max(200.);
        let mut column = div().w(px(col_w)).flex().flex_col().gap(px(28.8));
        for (i, (ms, words)) in lyr.timed.iter().enumerate() {
            let words = if words.is_empty() { "\u{266a}".to_string() } else { words.clone() };
            let (color, opacity) = match cur {
                _ if !lyr.synced => (active, 1.),
                // --lyrics-color-passed: the active text colour at 70%.
                Some(c) if i < c => (active, 0.7),
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
                    ScrollDelta::Lines(p) => p.y * 40.,
                    ScrollDelta::Pixels(p) => f32::from(p.y),
                };
                let st = &mut this.lyrics_scroll;
                st.target = (st.target - dy).max(0.);
                st.offset = st.target;
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
                    .left(px(64.))
                    .top(px(LINES_TOP - offset))
                    .pb(px(240.))
                    .child(column),
            )
            .child(header)
            .into_any_element()
    }
}

/// Clicking a line seeks only while it's synced lyrics on the playing track.
fn lyr_synced_guard(this: &Shell) -> bool {
    this.lyrics.as_ref().and_then(|l| l.1.as_ref()).is_some_and(|l| l.synced)
}
