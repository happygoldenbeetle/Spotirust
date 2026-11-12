//! Your Library's filter field ("Search in Your Library"), measured in the reference:
//!   closed: a 32px round button with the 16px search icon (rgba(255,255,255,.7));
//!   open: the field grows to 188 wide (width, opacity .3s cubic-bezier(.3,0,.4,1)), background
//!   rgba(255,255,255,.1), radius 4 (at once on open; back to round only after the .3s on close),
//!   text 13.33px rgba(255,255,255,.7) from x 32, a 32px clear button at its right end while it
//!   has text, and the sort button drops its label. Losing focus with no text closes it again.
//!   Typing filters the rows by name (and creator).

use super::playlist_view::text_width;
use super::*;

const W: f32 = 188.;
const SECS: f32 = 0.3;
const TEXT: f32 = 13.333;

pub(super) struct LibFind {
    pub text: String,
    pub open: bool,
    pub focused: bool,
    /// 0 closed .. 1 open.
    pub t: fx::Tween,
    pub edit_at: Instant,
    pub seq: u64,
}

impl Default for LibFind {
    fn default() -> Self {
        LibFind { text: String::new(), open: false, focused: false, t: fx::Tween::new(0.), edit_at: Instant::now(), seq: 0 }
    }
}

fn curve() -> fx::Ease {
    fx::Ease::Standard
}

impl Shell {
    pub(super) fn lib_find_focus(&mut self, cx: &mut Context<Self>) {
        let f = &mut self.lib_find;
        f.edit_at = Instant::now();
        if !f.open {
            f.open = true;
            f.t.set(1., SECS, curve());
        }
        if !f.focused {
            f.focused = true;
            f.seq += 1;
            let seq = f.seq;
            // caret blink, as in the top search field
            cx.spawn(async move |this, cx| loop {
                cx.background_executor().timer(Duration::from_millis(265)).await;
                let on = this.update(cx, |this, cx| {
                    let on = this.lib_find.focused && this.lib_find.seq == seq;
                    if on {
                        cx.notify();
                    }
                    on
                });
                if !on.unwrap_or(false) {
                    break;
                }
            })
            .detach();
        }
        cx.notify();
    }

    pub(super) fn lib_find_blur(&mut self, cx: &mut Context<Self>) {
        let f = &mut self.lib_find;
        f.focused = false;
        f.seq += 1;
        if f.text.is_empty() && f.open {
            f.open = false;
            f.t.set(0., SECS, curve());
        }
        cx.notify();
    }

    pub(super) fn lib_find_animating(&self) -> bool {
        self.lib_find.t.running()
    }

    /// Whether a library row passes the filter.
    pub(super) fn lib_find_matches(&self, item: &LibItem) -> bool {
        let q = self.lib_find.text.trim().to_lowercase();
        q.is_empty() || item.name.to_lowercase().contains(&q) || item.owner.to_lowercase().contains(&q)
    }

    /// Keys while the field has the caret.
    pub(super) fn lib_find_key(&mut self, ev: &gpui::KeyDownEvent, cx: &mut Context<Self>) -> bool {
        if !self.lib_find.focused || self.ctx_menu.is_some() {
            return false;
        }
        let k = &ev.keystroke;
        let f = &mut self.lib_find;
        match k.key.as_str() {
            "escape" => {
                f.text.clear();
                self.lib_find_blur(cx);
                return true;
            }
            "backspace" => {
                if k.modifiers.control {
                    let keep = f.text.trim_end().rfind(' ').map_or(0, |i| i + 1);
                    f.text.truncate(keep);
                } else {
                    f.text.pop();
                }
            }
            "tab" => {
                self.lib_find_blur(cx);
                return true;
            }
            "enter" => {}
            _ if k.modifiers.control || k.modifiers.alt => return false,
            _ => match k.key_char.as_deref() {
                Some(ch) if !ch.chars().any(char::is_control) => f.text.push_str(ch),
                _ => return false,
            },
        }
        f.edit_at = Instant::now();
        cx.notify();
        true
    }

    pub(super) fn lib_find_field(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let f = &self.lib_find;
        let t = f.t.value();
        let w = 32. + (W - 32.) * t;
        let has_text = !f.text.is_empty();
        let font = gpui::font(ui_font());
        let room = (w - 32. - if has_text { 32. } else { 8. }).max(0.);
        let text_w = text_width(window, &f.text, TEXT, font);
        let scroll = (text_w - room).max(0.);
        let caret_on = f.focused && (f.edit_at.elapsed().as_millis() / 530) % 2 == 0;
        let icon_t = self.fades.get("lib-search");
        div()
            .id("lib-search")
            .relative()
            .flex_none()
            .w(px(w))
            .h(px(32.))
            .when(t > 0., |d| d.rounded(px(4.)))
            .when(t <= 0., |d| d.rounded_full().hover(|s| s.bg(rgba(0xFFFFFF1A))))
            .cursor(if f.open { CursorStyle::IBeam } else { CursorStyle::PointingHand })
            .on_hover(cx.listener(|this, h: &bool, _, cx| {
                this.fades.hover("lib-search".into(), *h, 0., fx::Ease::Linear);
                cx.notify();
            }))
            .on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, cx| {
                cx.stop_propagation();
                this.lib_find_focus(cx);
            }))
            .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                if this.lib_find.focused {
                    this.lib_find_blur(cx);
                }
            }))
            .child(div().absolute().inset_0().rounded(px(4.)).bg(rgba(0xFFFFFF1A)).opacity(t))
            .child(
                svg()
                    .path("icons/lib_search.svg")
                    .absolute()
                    .left(px(8.))
                    .top(px(8.))
                    .size(px(16.))
                    .text_color(if !f.open && icon_t > 0. { c(WHITE) } else { rgba(0xFFFFFFB3) }),
            )
            .child(
                div()
                    .absolute()
                    .left(px(32.))
                    .top_0()
                    .w(px(room))
                    .h(px(32.))
                    .overflow_hidden()
                    .opacity(t)
                    .child(
                        div()
                            .absolute()
                            .left(px(-scroll))
                            .top_0()
                            .h(px(32.))
                            .flex()
                            .items_center()
                            .whitespace_nowrap()
                            .text_size(px(TEXT))
                            .text_color(rgba(0xFFFFFFB3))
                            .child(if has_text { SharedString::from(f.text.clone()) } else { "Search in Your Library".into() }),
                    )
                    .when(caret_on, |d| d.child(div().absolute().left(px(text_w - scroll)).top(px(8.)).w(px(1.)).h(px(16.)).bg(c(WHITE)))),
            )
            .when(has_text && t > 0.5, |d| {
                d.child(
                    div()
                        .id("lib-search-clear")
                        .absolute()
                        .right_0()
                        .top_0()
                        .size(px(32.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .cursor_pointer()
                        .group("lib-search-clear")
                        .on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, cx| {
                            cx.stop_propagation();
                            this.lib_find.text.clear();
                            this.lib_find_focus(cx);
                        }))
                        .child(
                            svg()
                                .path("icons/close16.svg")
                                .size(px(16.))
                                .text_color(rgba(0xFFFFFFB3))
                                .group_hover("lib-search-clear", |s| s.text_color(c(WHITE))),
                        ),
                )
            })
            .into_any_element()
    }
}
