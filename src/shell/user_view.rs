//! User profile page (main view). Measured on the reference at main width 506:
//!   header like a playlist (186 at W 506): round 128 avatar at the bottom left (16 padding),
//!   "Profile" 14px, name (largest of 96/72/48/32 that fits), meta row 8 below
//!   ("N Public Playlists" rgba(255,255,255,.7) • "N Followers" • "N Following" white links).
//!   Header: base colour + linear-gradient(transparent, rgba(0,0,0,.5)); body: base colour +
//!   rgba(0,0,0,.6) -> #121212 over 232px.
//!   Action bar 16 below, 48 tall: settings (own profile) and more, 24px icons 16 apart.
//!   Shelves: "Top artists this month" 16 below (header 50 with "Only visible to you"), then
//!   "Public Playlists", "Followers", "Following" 40 apart (header 48); cards 8 below the header,
//!   (W-8)/cols wide. "Show all" only when the shelf has more items than columns. 32 bottom padding.

use super::playlist_view::{fluid, text_width};
use super::*;

impl Shell {
    pub(super) fn user_view(&self, uri: &str, main_w: f32, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let panel = div().flex_1().min_w_0().h_full().rounded(px(8.)).bg(c(SURFACE)).overflow_hidden().relative();
        let Some(page) = self.user_pages.get(uri) else {
            return panel.into_any_element();
        };
        let w = main_w;
        if let Some(y) = std::env::var("SPOTIRUST_SCROLL").ok().and_then(|v| v.parse::<f32>().ok()) {
            self.page_scroll.set_offset(gpui::point(px(0.), px(-y)));
        }
        let vh = f32::from(window.viewport_size().height);
        let cs = fluid(w, 16., 24.);
        let hdr_h = fluid(w, 186., 276.).max(fluid(w, 186., 336.).min(vh * 0.3).min(336.));
        let avatar_size = fluid(w, 128., 232.);
        let text_x = cs + avatar_size + cs;
        let text_w = (w - text_x - cs).max(0.);

        let mut title_font = gpui::font(title_font());
        title_font.weight = FontWeight(800.);
        let title_size = [96., 72., 48.]
            .into_iter()
            .find(|&s| text_width(window, &page.name, s, title_font.clone()) <= text_w)
            .unwrap_or(32.);

        let avatar = page.image.as_deref().and_then(|u| {
            let url = format!("crop:1x1:{u}");
            images::cache(cx).update(cx, |c, cx| c.get_sized(&url, avatar_size, cx))
        });
        let sub = rgba(0xFFFFFFB3);
        let bullet = || div().mx(px(4.)).flex_none().text_color(sub).child("\u{2022}");
        let plural = |n: u64, one: &str, many: &str| format!("{n} {}", if n == 1 { one } else { many });
        let meta = div()
            .mt(px(8.))
            .h(px(22.4))
            .flex()
            .flex_row()
            .items_center()
            .text_size(px(14.))
            .line_height(px(19.2))
            .whitespace_nowrap()
            .when(page.public_playlists > 0, |d| {
                d.child(div().flex_none().text_color(sub).child(plural(page.public_playlists, "Public Playlist", "Public Playlists")))
                    .child(bullet())
            })
            .child(
                div()
                    .id("user-followers")
                    .flex_none()
                    .text_color(c(WHITE))
                    .cursor_pointer()
                    .hover(|s| s.underline())
                    .child(plural(page.followers, "Follower", "Followers")),
            )
            .child(bullet())
            .child(
                div()
                    .id("user-following")
                    .flex_none()
                    .text_color(c(WHITE))
                    .cursor_pointer()
                    .hover(|s| s.underline())
                    .child(format!("{} Following", page.following)),
            );

        let header = div()
            .relative()
            .flex_none()
            .w_full()
            .h(px(hdr_h))
            .bg(c(page.color))
            .child(div().absolute().inset_0().bg(linear_gradient(
                180.,
                linear_color_stop(rgba(0x00000000), 0.),
                linear_color_stop(rgba(0x00000080), 1.),
            )))
            .child(
                div()
                    .absolute()
                    .left(px(cs))
                    .top(px(hdr_h - cs - avatar_size))
                    .size(px(avatar_size))
                    .rounded_full()
                    .shadow(vec![gpui::BoxShadow {
                        color: gpui::hsla(0., 0., 0., 0.5),
                        offset: gpui::point(px(0.), px(4.)),
                        blur_radius: px(60.),
                        spread_radius: px(0.),
                    }])
                    .child(match avatar {
                        Some(a) => img(ImageSource::Render(a)).size(px(avatar_size)).rounded_full().into_any_element(),
                        None => div().size_full().rounded_full().bg(c(0x282828)).into_any_element(),
                    }),
            )
            .child(
                div()
                    .absolute()
                    .left(px(text_x))
                    .w(px(text_w))
                    .bottom(px(cs))
                    .flex()
                    .flex_col()
                    .child(div().text_size(px(14.)).line_height(px(19.2)).text_color(c(WHITE)).child("Profile"))
                    .child(
                        div()
                            .font_family(title_font.family.clone())
                            .font_weight(FontWeight(800.))
                            .text_size(px(title_size))
                            .line_height(px(title_size * 1.2))
                            .text_color(c(WHITE))
                            .when(title_size == 32., |d| d.line_clamp(3).text_ellipsis())
                            .when(title_size > 32., |d| d.whitespace_nowrap())
                            .child(page.name.clone()),
                    )
                    .child(meta),
            );

        // ---- action bar ----
        let bar_icon = |id: &'static str, path: &'static str| {
            div()
                .id(id)
                .size(px(24.))
                .flex_none()
                .cursor_pointer()
                .child(svg().path(path).size(px(24.)).text_color(c(TEXT_SUBDUED)).hover(|s| s.text_color(c(WHITE))))
        };
        let action_bar = div()
            .mt(px(cs))
            .px(px(cs))
            .h(px(48.))
            .flex()
            .flex_row()
            .items_center()
            .gap(px(16.))
            .when(page.is_current_user, |d| d.child(bar_icon("user-settings", "icons/gear24.svg")))
            .child(bar_icon("user-more", "icons/pl_more24.svg"));

        // ---- shelves ----
        let cols = (((w - 8.) / 180.).floor() as usize).max(2);
        let card_w = (w - 8.) / cols as f32;
        let shelf = |this: &Self, id: &str, title: &str, subtitle: Option<&str>, items: &[home::Item], first: bool, cx: &mut Context<Self>| {
            (!items.is_empty()).then(|| {
                let show_all = items.len() > cols;
                let cards: Vec<AnyElement> = items
                    .iter()
                    .take(cols)
                    .enumerate()
                    .map(|(i, it)| this.sized_card(format!("{id}-{i}").into(), it, card_w, 4., cx))
                    .collect();
                let titles = div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(
                        one_line(title.to_string())
                            .font_family(title_font.family.clone())
                            .font_weight(FontWeight::BOLD)
                            .text_size(px(24.))
                            .line_height(px(28.8))
                            .text_color(c(WHITE)),
                    )
                    .when_some(subtitle, |d, s| {
                        d.child(
                            one_line(s.to_string())
                                .mt(px(2.))
                                .text_size(px(14.))
                                .line_height(px(19.2))
                                .text_color(c(TEXT_SUBDUED)),
                        )
                    });
                div()
                    .mt(px(if first { 16. } else { 40. }))
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .min_h(px(48.))
                            .pl(px(16.))
                            .pr(px(24.))
                            .flex()
                            .flex_row()
                            .items_end()
                            .child(titles)
                            .when(show_all, |d| {
                                d.child(
                                    div()
                                        .id(gpui::ElementId::Name(format!("{id}-all").into()))
                                        .pb(px(1.))
                                        .flex_none()
                                        .text_size(px(14.))
                                        .line_height(px(19.2))
                                        .font_weight(FontWeight::BOLD)
                                        .text_color(c(TEXT_SUBDUED))
                                        .cursor_pointer()
                                        .hover(|s| s.underline())
                                        .child("Show all"),
                                )
                            }),
                    )
                    .child(div().mt(px(8.)).px(px(4.)).flex().flex_row().children(cards))
            })
        };
        let mut sections = Vec::new();
        let mut first = true;
        if let Some(s) = shelf(self, "u-top", "Top artists this month", Some("Only visible to you"), &page.top_artists, first, cx) {
            sections.push(s);
            first = false;
        }
        for (id, title, items) in [
            ("u-pl", "Public Playlists", &page.playlists),
            ("u-fers", "Followers", &page.follower_list),
            ("u-fing", "Following", &page.following_list),
        ] {
            if let Some(s) = shelf(self, id, title, None, items, first, cx) {
                sections.push(s);
                first = false;
            }
        }

        let body = div()
            .relative()
            .flex()
            .flex_col()
            .pb(px(32.))
            .child(
                div().absolute().top_0().left_0().right_0().h(px(232.)).bg(c(page.color)).child(
                    div().size_full().bg(linear_gradient(180., linear_color_stop(rgba(0x00000099), 0.), linear_color_stop(c(SURFACE), 1.))),
                ),
            )
            .child(action_bar)
            .children(sections);

        // sticky bar: header colour + name (no play button on profiles)
        let scroll = -f32::from(self.page_scroll.offset().y);
        let bg_op = ((scroll - (hdr_h - 110.)) / 111.).clamp(0., 1.);
        let content_op = ((scroll - (hdr_h - 88.6)) / 85.2).clamp(0., 1.);
        let top_bar = (bg_op > 0.).then(|| {
            div()
                .absolute()
                .top_0()
                .left_0()
                .right_0()
                .h(px(64.))
                .child(div().absolute().inset_0().bg(c(page.color)).opacity(bg_op))
                .child(
                    one_line(page.name.clone())
                        .absolute()
                        .top(px(17.6))
                        .left(px(16.))
                        .right(px(16.))
                        .opacity(content_op)
                        .font_family(title_font.family.clone())
                        .font_weight(FontWeight::BOLD)
                        .text_size(px(24.))
                        .line_height(px(28.8))
                        .text_color(c(WHITE)),
                )
        });

        panel
            .child(
                div()
                    .id(gpui::ElementId::Name(format!("user-scroll-{uri}").into()))
                    .track_scroll(&self.page_scroll)
                    .absolute()
                    .inset_0()
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .child(header)
                    .child(body),
            )
            .children(top_bar)
            .into_any_element()
    }
}
