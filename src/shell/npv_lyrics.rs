//! The Lyrics card in Now Playing, measured from the reference.
//!
//! Collapsed ("Lyrics preview"): 177 tall, radius 8, colour-lyrics background, header 16/700 at
//! padding 16, then the lines container (padding 16) under a mask:
//! `linear-gradient(transparent 0 16px, black 24px, black calc(100% - 64px), transparent
//! calc(100% - 16px))`, so the third line fades out. Hovering the card slides a white "Show more"
//! pill (14/700, padding 4/16, h32, 16 from the bottom right) up 25.6px while fading it in
//! (`opacity .2s, transform .2s`); the pill itself turns #f0f0f0 and scales 1.04.
//!
//! Expanded ("Lyrics", `max-height .1s linear` to 440): the header gains Enter full screen and
//! Open in cinema mode (32px tertiary buttons, 8 apart, shown while the card is hovered), every
//! line of the song is listed and the playing one is kept centred (overflow hidden, scrolled by
//! the client). Passed lines are the text colour at 50%, the playing one white, upcoming ones the
//! text colour; a hovered line turns white and underlined and seeks there on click. The pill reads
//! "Show less". The last line is followed by "Lyrics provided by …" (14px).

use std::cell::RefCell;

use super::playlist_view::text_width;
use super::*;

const COLLAPSED_H: f32 = 177.;
const EXPANDED_H: f32 = 440.;

/// Scroll state of the expanded card (kept across frames; written at paint time).
#[derive(Default)]
pub struct NpvLyr {
    pub expanded: bool,
    /// Lyrics (track id) the measurements belong to.
    track: RefCell<String>,
    offset: Cell<f32>,
    last_frame: Cell<Option<Instant>>,
    /// Window-space top / height of every line, and of the column, from the last paint.
    lines: Rc<RefCell<Vec<(f32, f32)>>>,
    column: Rc<Cell<(f32, f32)>>,
}

impl Shell {
    fn toggle_npv_lyrics(&mut self, cx: &mut Context<Self>) {
        let st = &mut self.npv_lyr;
        st.expanded = !st.expanded;
        st.offset.set(0.);
        st.last_frame.set(None);
        self.fades.hover("npv-lyr-exp".into(), self.npv_lyr.expanded, 0.1, fx::Ease::Linear);
        cx.notify();
    }

    pub(super) fn npv_lyrics_card(&self, l: Lyrics, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let track_id = self.lyrics.as_ref().map(|l| l.0.clone()).unwrap_or_default();
        let st = &self.npv_lyr;
        if *st.track.borrow() != track_id {
            *st.track.borrow_mut() = track_id;
            st.lines.borrow_mut().clear();
            st.offset.set(0.);
        }
        let expanded = st.expanded;
        let grow = self.fades.get("npv-lyr-exp");
        let card_h = COLLAPSED_H + (EXPANDED_H - COLLAPSED_H) * grow;
        let hover = self.fades.get("npv-lyr-card");
        let hovered = self.fades.on("npv-lyr-card");
        let hdr_h = if expanded { 25. } else { 22.4 };
        let lines_top = 16. + hdr_h;
        let lines_h = card_h - lines_top;
        let bg = l.background;

        // Header: title, and in the expanded card the two view buttons (only while hovered).
        let tool = |id: &'static str, path: &'static str, cx: &mut Context<Self>| {
            let t = self.fades.get(id);
            let s = if self.fades.on(id) { 1.04 } else { 1. };
            self.on_fade(
                round_button(id, 32.).child(fx::scaled_icon(path, 16., s).text_color(fx::mix(0xB3B3B3FF, 0xFFFFFFFF, t))),
                id,
                fx::TERTIARY,
                cx,
            )
        };
        let mut header = div()
            .h(px(hdr_h))
            .flex_none()
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .child(
                div()
                    .text_size(px(16.))
                    .line_height(px(22.4))
                    .font_weight(FontWeight::BOLD)
                    .text_color(c(WHITE))
                    .child(if expanded { "Lyrics" } else { "Lyrics preview" }),
            );
        if expanded {
            header = header.child(
                div()
                    .flex()
                    .flex_row()
                    .gap(px(8.))
                    .mr(px(-4.))
                    .my(px(-3.5))
                    .opacity(if hovered { 1. } else { 0. })
                    .child(
                        tool("npv-lyr-full", "icons/fullscreen.svg", cx)
                            .on_click(cx.listener(|this, _, window, cx| this.toggle_fullscreen(window, cx))),
                    )
                    .child(tool("npv-lyr-cinema", "icons/expand.svg", cx).on_click(cx.listener(|this, _, _, cx| {
                        if this.page != Page::Lyrics {
                            this.toggle_lyrics(cx);
                        }
                    }))),
            );
        }

        // The lines.
        let line_div = |text: String| {
            div()
                .font_family(title_font())
                .text_size(px(24.))
                .line_height(px(28.8))
                .font_weight(FontWeight::BOLD)
                .child(text)
        };
        let mut column = div().flex().flex_col();
        let mut offset = 0.;
        if expanded {
            let now = self.progress_ms();
            let cur = if l.synced { Self::active_line(&l.timed, now) } else { None };
            // Follow the playing line: centre it, easing toward it each frame.
            let measured: Vec<(f32, f32)> = st.lines.borrow().clone();
            let (col_top, col_h) = st.column.get();
            let max = (col_h + 32. - lines_h).max(0.);
            let target = match cur.and_then(|i| measured.get(i)) {
                Some(&(top, h)) => (16. + top - col_top + h / 2. - lines_h / 2.).clamp(0., max),
                None => 0.,
            };
            let dt = st.last_frame.get().map(|t| t.elapsed().as_secs_f32()).unwrap_or(0.016).min(0.1);
            st.last_frame.set(Some(Instant::now()));
            let mut o = st.offset.get();
            o += (target - o) * (1. - (-dt * 10.).exp());
            if (target - o).abs() < 0.5 {
                o = target;
            } else {
                window.request_animation_frame();
            }
            st.offset.set(o);
            offset = o;
            if self.playback.as_ref().is_some_and(|p| p.is_playing) {
                window.request_animation_frame();
            }

            let mut seen = false;
            for (i, (ms, words)) in l.timed.iter().enumerate() {
                let rec = st.lines.clone();
                let probe = canvas(
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
                .inset_0();
                if words.is_empty() || words == "\u{266a}" {
                    column = column.child(div().relative().h(px(0.)).child(probe));
                    continue;
                }
                let (color, opacity) = match cur {
                    Some(c) if i < c => (l.text, 0.5),
                    Some(c) if i == c => (0xFFFFFF, 1.),
                    _ => (l.text, 1.),
                };
                let seek_to = *ms;
                let id = SharedString::from(format!("npv-lyr-line-{i}"));
                column = column.child(
                    line_div(words.clone())
                        .id(gpui::ElementId::Name(id))
                        .relative()
                        .when(seen, |d| d.mt(px(14.4)))
                        .text_color(c(color))
                        .opacity(opacity)
                        .when(l.synced, |d| {
                            d.cursor_pointer().hover(|s| s.text_color(c(WHITE)).opacity(1.).underline()).on_click(cx.listener(
                                move |this, _, _, cx| {
                                    if this.lyrics.as_ref().and_then(|l| l.1.as_ref()).is_some_and(|l| l.synced) {
                                        this.player(Play::Seek(seek_to), cx);
                                    }
                                },
                            ))
                        })
                        .child(probe),
                );
                seen = true;
            }
            if !l.provider.is_empty() {
                column = column.child(
                    div()
                        .mt(px(33.6))
                        .py(px(20.))
                        .text_size(px(14.))
                        .line_height(px(19.2))
                        .text_color(c(l.text))
                        .child(format!("Lyrics provided by {}", l.provider)),
                );
            }
        } else {
            for (i, line) in l.lines.iter().take(6).enumerate() {
                column = column.child(line_div(line.clone()).when(i > 0, |d| d.mt(px(14.4))).text_color(c(l.text)));
            }
        }
        let col_rec = st.column.clone();
        let column = column.child(
            canvas(move |b, _, _| col_rec.set((f32::from(b.origin.y), f32::from(b.size.height))), |_, _, _, _| {})
                .absolute()
                .inset_0(),
        );

        // The mask, drawn as the card colour over the lines (the card is a flat colour).
        let a = |alpha: u32| rgba(bg << 8 | alpha);
        let fade = |top: f32, h: f32, from: u32, to: u32| {
            div().absolute().left_0().right_0().top(px(top)).h(px(h)).bg(linear_gradient(
                180.,
                linear_color_stop(a(from), 0.),
                linear_color_stop(a(to), 1.),
            ))
        };
        let lines = div()
            .absolute()
            .left_0()
            .right_0()
            .top(px(lines_top))
            .h(px(lines_h))
            .overflow_hidden()
            .child(div().absolute().left(px(16.)).right(px(16.)).top(px(16. - offset)).child(column))
            .child(fade(0., 16., 0xFF, 0xFF))
            .child(fade(16., 8., 0xFF, 0x00))
            .child(fade(lines_h - 64., 48., 0x00, 0xFF))
            .child(fade(lines_h - 16., 16., 0xFF, 0xFF));

        // Show more / Show less: slides up and fades in while the card is hovered.
        let label = if expanded { "Show less" } else { "Show more" };
        let pill_w = text_width(window, label, 14., gpui::font(ui_font()).bold()) + 32.;
        let ph = self.fades.get("npv-lyr-pill");
        let s = if self.fades.on("npv-lyr-pill") { 1.04 } else { 1. };
        let pill = self.on_fade(
            div()
                .id("npv-lyr-pill")
                .absolute()
                .right(px(16.))
                .top(px(card_h - 48. + 25.6 * (1. - hover)))
                .w(px(pill_w))
                .h(px(32.))
                .opacity(hover)
                .cursor_pointer()
                .on_click(cx.listener(|this, _, _, cx| this.toggle_npv_lyrics(cx)))
                .child(
                    div()
                        .absolute()
                        .left(px(-pill_w * (s - 1.) / 2.))
                        .top(px(-32. * (s - 1.) / 2.))
                        .w(px(pill_w * s))
                        .h(px(32. * s))
                        .rounded_full()
                        .bg(fx::mix(0xFFFFFFFF, 0xF0F0F0FF, ph))
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(px(14. * s))
                        .font_weight(FontWeight::BOLD)
                        .text_color(c(0x000000))
                        .child(label),
                ),
            "npv-lyr-pill",
            fx::Trans { enter: 0.15, leave: 0.15, ease: fx::Ease::Productive },
            cx,
        );

        let card = div()
            .id("npv-lyrics")
            .relative()
            .mt(px(28.8))
            .h(px(card_h))
            .flex_none()
            .rounded(px(8.))
            .overflow_hidden()
            .bg(c(bg))
            .child(div().absolute().left(px(16.)).right(px(16.)).top(px(16.)).child(header))
            .child(lines)
            .when(hover > 0., |d| d.child(pill));
        self.on_fade(card, "npv-lyr-card", fx::Trans { enter: 0.2, leave: 0.2, ease: fx::Ease::Ease }, cx).into_any_element()
    }
}
