//! Create (+) menu in Your Library. Measured on the reference:
//!   bottom-start under the button, 8 below; #282828, radius 8, padding 4, sort-menu shadow,
//!   width = content. Items 64 tall (radius 6, padding 8, 16 more on the right): 48px circle
//!   rgba(255,255,255,.1) with a 24px white icon; text 12 to the right (title 16/700 white,
//!   description 14 #B3B3B3). Divider after Blend: 1px rgba(255,255,255,.1), 4 above and below,
//!   inset 16. Hover: item rgba(255,255,255,.1); icon fill -> green, rotate(5deg) scale(1.05),
//!   .2s ease-in-out both ways. "Mixed Playlist" has a "Beta" badge (12/700 black on green,
//!   radius 4, padding 2 4, 8 after the title).

use super::library_view::menu_list;
use super::*;
use crate::library::LibKind;

const ITEMS: [(&str, &str, &str, bool); 5] = [
    ("icons/create_playlist24.svg", "Playlist", "Create a playlist with songs or episodes", false),
    ("icons/create_mixed24.svg", "Mixed Playlist", "Mix songs with smooth transitions", true),
    ("icons/create_blend.svg", "Blend", "Combine your friends\u{2019} tastes into a playlist", false),
    ("icons/create_folder24.svg", "Folder", "Organize your playlists", false),
    ("icons/create_jam24.svg", "Jam", "Listen together from anywhere", false),
];

impl Shell {
    /// Left edge of the Create button in window coordinates (pill from a 332px panel, else round).
    fn create_button_left(&self) -> f32 {
        let w = if self.left_w >= 332. { 99.6 } else { 35. };
        8. + self.left_w - 16. - 35. - 8. - w
    }

    /// Hover progress (0..1) of item `i`: .2s ease-in-out in and out.
    fn create_item_progress(&self, i: usize, animating: &mut bool) -> f32 {
        let ease = crate::anim::cubic_bezier(0.42, 0., 0.58, 1.);
        let since = |at: std::time::Instant| at.elapsed().as_secs_f32() / 0.2;
        match (self.create_hover, self.create_unhover) {
            (Some((h, at)), _) if h == i => {
                let t = since(at);
                *animating |= t < 1.;
                ease(t.min(1.))
            }
            (_, Some((h, at))) if h == i => {
                let t = since(at);
                *animating |= t < 1.;
                1. - ease(t.min(1.))
            }
            _ => 0.,
        }
    }

    pub(super) fn create_menu(&self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.create_open || self.left_collapsed {
            return None;
        }
        let mut animating = false;
        let mut items: Vec<AnyElement> = Vec::new();
        for (i, (icon_path, title, desc, beta)) in ITEMS.iter().enumerate() {
            let p = self.create_item_progress(i, &mut animating);
            let color = gpui::Rgba {
                r: 1. + (0x1E as f32 / 255. - 1.) * p,
                g: 1. + (0xD7 as f32 / 255. - 1.) * p,
                b: 1. + (0x60 as f32 / 255. - 1.) * p,
                a: 1.,
            };
            let blend = *icon_path == "icons/create_blend.svg";
            let transform = gpui::Transformation::rotate(gpui::radians(5f32.to_radians() * p))
                .with_scaling(gpui::size(1. + 0.05 * p, 1. + 0.05 * p));
            items.push(
                div()
                    .id(gpui::ElementId::Name(format!("create-{i}").into()))
                    .h(px(64.))
                    .p(px(8.))
                    .pr(px(24.))
                    .rounded(px(6.))
                    .flex()
                    .flex_row()
                    .items_center()
                    .cursor_pointer()
                    .hover(|s| s.bg(rgba(0xFFFFFF1A)))
                    .on_hover(cx.listener(move |this, h: &bool, _, cx| {
                        let now = std::time::Instant::now();
                        if *h {
                            this.create_hover = Some((i, now));
                        } else if this.create_hover.map(|x| x.0) == Some(i) {
                            this.create_hover = None;
                            this.create_unhover = Some((i, now));
                        }
                        cx.notify();
                    }))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.create_open = false;
                        this.create_toggled_at = std::time::Instant::now();
                        if i == 0 {
                            this.create_playlist(cx);
                        }
                        cx.notify();
                    }))
                    .child(
                        div()
                            .size(px(48.))
                            .flex_none()
                            .rounded_full()
                            .bg(rgba(0xFFFFFF1A))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(
                                svg()
                                    .path(*icon_path)
                                    .w(px(if blend { 27. } else { 24. }))
                                    .h(px(24.))
                                    .with_transformation(transform)
                                    .text_color(if blend { gpui::Rgba { r: 1., g: 1., b: 1., a: 1. } } else { color }),
                            ),
                    )
                    .child(
                        div()
                            .ml(px(12.))
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .items_center()
                                    .text_size(px(16.))
                                    .line_height(px(22.4))
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(c(WHITE))
                                    .whitespace_nowrap()
                                    .child(*title)
                                    .when(*beta, |d| {
                                        d.child(
                                            div()
                                                .ml(px(8.))
                                                .h(px(16.))
                                                .px(px(4.))
                                                .rounded(px(4.))
                                                .bg(c(GREEN))
                                                .flex()
                                                .items_center()
                                                .text_size(px(12.))
                                                .line_height(px(16.8))
                                                .text_color(c(0x000000))
                                                .child("Beta"),
                                        )
                                    }),
                            )
                            .child(
                                div()
                                    .text_size(px(14.))
                                    .line_height(px(19.2))
                                    .text_color(c(TEXT_SUBDUED))
                                    .whitespace_nowrap()
                                    .child(*desc),
                            ),
                    )
                    .into_any_element(),
            );
            if i == 2 {
                items.push(div().mx(px(16.)).my(px(4.)).h(px(1.)).bg(rgba(0xFFFFFF1A)).into_any_element());
            }
        }
        if animating {
            window.request_animation_frame();
        }
        Some(
            div()
                .id("create-menu")
                .absolute()
                .top(px(64. + 16. + 35.2 + 8.))
                .left(px(self.create_button_left()))
                .occlude()
                .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                    if this.create_open {
                        this.create_open = false;
                        this.menu_closed_at = std::time::Instant::now();
                        this.create_toggled_at = std::time::Instant::now();
                    }
                    cx.notify();
                }))
                .child(menu_list(true).rounded(px(8.)).w_auto().children(items))
                .into_any_element(),
        )
    }

    /// "Playlist": a new empty playlist named "My Playlist #N" (`playlist.new-default-name`), opened.
    fn create_playlist(&mut self, cx: &mut Context<Self>) {
        let (Some(api), Some(me)) = (self.api.clone(), self.me.clone()) else { return };
        let me_name = me.display_name.clone().unwrap_or_default();
        let n = self.library_items().iter().filter(|i| i.kind == LibKind::Playlist && i.owner == me_name).count() + 1;
        let name = format!("My Playlist #{n}");
        cx.spawn(async move |this, cx| {
            let result = cx.background_executor().spawn(async move { api.create_playlist(&me.id, &name) }).await;
            let _ = this.update(cx, |this, cx| match result {
                Ok(uri) => {
                    this.reload_library(cx);
                    if let Some(p) = Page::for_uri(&uri) {
                        this.open(p, cx);
                    }
                }
                Err(e) => eprintln!("create playlist: {e:#}"),
            });
        })
        .detach();
    }
}
