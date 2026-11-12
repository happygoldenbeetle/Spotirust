//! Artist page (main view). Measured on the reference at main width 506, viewport height 764:
//!   header 40vh (305.6): wide image (cover, position 50% 15%, scale 1.1) over the artist colour;
//!   title 56px/800 (line 1.2), "Verified by Spotify" row 24 tall, listeners line 32, 16px padding.
//!   Action bar 16 below: play 48, explore 38x48, shuffle 24, Follow (32 tall, 0.8px #7C7C7C
//!   border), more 24, cs apart; 16 below it. Sections: "Popular" (5 rows, "See more"),
//!   "Artist pick", Discography (filter chips + one row of cards), Featuring, On Tour, About,
//!   then card shelves. Section titles are 24px bold, 40px apart.

use super::playlist_view::{cover_rect, fluid, fr_columns, text_width};
use super::*;
use crate::artist;

const DISCO_FILTERS: [&str; 4] = ["Popular releases", "Albums", "Singles and EPs", "Compilations"];

fn h2(text: impl Into<SharedString>) -> Div {
    one_line(text)
        .font_family(title_font())
        .font_weight(FontWeight::BOLD)
        .text_size(px(24.))
        .line_height(px(28.8))
        .text_color(c(WHITE))
}

impl Shell {
    pub(super) fn artist_view(&self, uri: &str, main_w: f32, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let panel = div().flex_1().min_w_0().h_full().rounded(px(8.)).bg(c(SURFACE)).overflow_hidden().relative();
        let Some(page) = self.artist_pages.get(uri) else {
            return panel.into_any_element();
        };
        let w = main_w;
        if let Some(y) = std::env::var("SPOTIRUST_SCROLL").ok().and_then(|v| v.parse::<f32>().ok()) {
            self.page_scroll.set_offset(gpui::point(px(0.), px(-y)));
        }
        let vh = f32::from(window.viewport_size().height);
        let cs = fluid(w, 16., 24.);
        let hdr_h = (vh * 0.4).round_to_tenth();

        // ---- header ----
        let mut title_font = gpui::font(title_font());
        title_font.weight = FontWeight(800.);
        // measured: 56px at W 506; wider views take the largest step that fits on one line
        let title_size = if w < 600. { 56. } else { [96., 72., 56.]
            .into_iter()
            .find(|&s| text_width(window, &page.name, s, title_font.clone()) <= w - 32.)
            .unwrap_or(48.) };
        let banner = page.header_image.as_ref().and_then(|u| {
            let url = format!("crop:{w:.1}x{hdr_h:.1}@0.5,0.15:{u}");
            images::cache(cx).update(cx, |c, cx| c.get_sized(&url, w * 1.1, cx))
        });
        let header = div()
            .relative()
            .flex_none()
            .w_full()
            .h(px(hdr_h))
            .overflow_hidden()
            .bg(c(page.color))
            .when_some(banner, |d, b| {
                d.child(
                    img(ImageSource::Render(b))
                        .absolute()
                        .left(px(-w * 0.05))
                        .top(px(-hdr_h * 0.05))
                        .w(px(w * 1.1))
                        .h(px(hdr_h * 1.1)),
                )
            })
            // measured: linear-gradient(transparent, rgba(0,0,0,.6)) over the image
            .child(div().absolute().inset_0().bg(linear_gradient(
                180.,
                linear_color_stop(rgba(0x00000000), 0.),
                linear_color_stop(rgba(0x00000099), 1.),
            )))
            .child(
                div()
                    .absolute()
                    .left(px(16.))
                    .right(px(16.))
                    .bottom(px(16.))
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .font_family(title_font.family.clone())
                            .font_weight(FontWeight(800.))
                            .text_size(px(title_size))
                            .line_height(px(title_size * 1.2))
                            .text_color(c(WHITE))
                            .whitespace_nowrap()
                            .overflow_hidden()
                            .text_ellipsis()
                            .child(page.name.clone()),
                    )
                    .when(page.verified, |d| {
                        d.child(
                            div()
                                .h(px(24.))
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap(px(8.))
                                .child(svg().path("icons/verified24.svg").size(px(24.)).text_color(c(0xC5F0C9)))
                                .child(
                                    div()
                                        .text_size(px(14.))
                                        .line_height(px(19.2))
                                        .font_weight(FontWeight::BOLD)
                                        .text_color(c(WHITE))
                                        .child("Verified by Spotify"),
                                ),
                        )
                    })
                    .child(
                        div()
                            .mt(px(4.))
                            .h(px(32.))
                            .flex()
                            .items_center()
                            .text_size(px(16.))
                            .line_height(px(22.4))
                            .text_color(c(WHITE))
                            .child(format!("{} monthly listeners", artist::compact(page.monthly_listeners))),
                    ),
            );

        // ---- action bar ----
        let playing_here = self.playback.as_ref().and_then(|p| p.context.as_ref()).is_some_and(|ctx| ctx.uri == page.uri);
        let is_playing = playing_here && self.playback.as_ref().is_some_and(|p| p.is_playing);
        let page_uri = page.uri.clone();
        let explore = page.top_tracks.first().and_then(|t| t.image.as_deref()).map(|u| images::variant(u, images::Variant::Small));
        // Encore tertiary: subdued -> white and 1.04 (.05s in / .15s out); `on`: green + a dot.
        let fades = &self.fades;
        let bar_icon = move |id: &'static str, path: &'static str, on: bool| {
            let t = fades.get(id);
            let color = if on { fx::mix(0x1ED760FF, 0x3BE477FF, t) } else { fx::mix(0xB3B3B3FF, 0xFFFFFFFF, t) };
            div()
                .id(id)
                .relative()
                .w(px(24.))
                .h(px(48.))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .child(fx::scaled_icon(path, 24., 1. + 0.04 * t).text_color(color))
                .when(on, |d| d.child(div().absolute().bottom(px(4.)).left(px(10.)).size(px(4.)).rounded_full().bg(color)))
        };
        let shuffle_on = playing_here && self.playback.as_ref().is_some_and(|p| p.shuffle_state);
        let (follow_uri, followed) = (page.uri.clone(), page.saved);
        let (menu_uri, menu_name) = (page.uri.clone(), page.name.clone());
        let action_bar = div()
            .flex_none()
            .h(px(48.))
            .mt(px(16.))
            .mb(px(16.))
            .px(px(cs))
            .flex()
            .flex_row()
            .items_center()
            .gap(px(cs))
            .child(
                div()
                    .id("ar-play")
                    .size(px(48.))
                    .flex_none()
                    .rounded_full()
                    .bg(c(GREEN))
                    .hover(|s| s.bg(c(GREEN_HOVER)))
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .justify_center()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if playing_here {
                            this.toggle_play(cx);
                        } else {
                            let uri = page_uri.clone();
                            this.player(Play::Context(uri.clone()), cx);
                        }
                    }))
                    .child(icon(if is_playing { "icons/pause24.svg" } else { "icons/play24.svg" }, 24., 0x000000)),
            )
            .when_some(explore, |d, url| {
                d.child(
                    div()
                        .id("ar-explore")
                        .relative()
                        .p(px(4.))
                        .flex_none()
                        .cursor_pointer()
                        .child(cover_rect(Some(&url), 30., 40., cx))
                        .child(svg().path("icons/pl_explore_frame.svg").absolute().top_0().left_0().w(px(38.)).h(px(48.)).text_color(c(0xA7A7A7))),
                )
            })
            .child(self.on_fade(
                bar_icon("ar-shuffle", "icons/pl_shuffle24.svg", shuffle_on).on_click({
                    let uri = page.uri.clone();
                    cx.listener(move |this, _, _, cx| {
                        if playing_here {
                            this.player(Play::Shuffle(!shuffle_on), cx);
                        } else {
                            this.player(Play::Shuffle(true), cx);
                            this.player(Play::Context(uri.clone()), cx);
                        }
                    })
                }),
                "ar-shuffle",
                fx::TERTIARY,
                cx,
            ))
            .child(
                div()
                    .id("ar-follow")
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(p) = this.artist_pages.get_mut(&follow_uri) {
                            p.saved = !followed;
                        }
                        this.set_hit_saved(follow_uri.clone(), !followed, cx);
                        cx.notify();
                    }))
                    .h(px(32.))
                    .px(px(15.2))
                    .flex_none()
                    .flex()
                    .items_center()
                    .rounded_full()
                    .border(px(0.8))
                    .border_color(c(0x7C7C7C))
                    .hover(|s| s.border_color(c(WHITE)))
                    .cursor_pointer()
                    .text_size(px(14.))
                    .line_height(px(19.2))
                    .font_weight(FontWeight::BOLD)
                    .text_color(c(WHITE))
                    .child(if page.saved { "Following" } else { "Follow" }),
            )
            .child(self.on_fade(
                bar_icon("ar-more", "icons/pl_more24.svg", false).on_click(cx.listener(move |this, ev: &gpui::ClickEvent, _, cx| {
                    if let Some(t) = this.target_for(&menu_uri, &menu_name) {
                        this.open_ctx_menu(t, ev.position(), false, cx);
                    }
                })),
                "ar-more",
                fx::TERTIARY,
                cx,
            ));

        // ---- Popular ----
        let list_w = w - 2. * cs;
        let row_w = list_w - 1.6;
        let widths = fr_columns(row_w - 32. - 16. - 32., &[(180., 4.), (120., 1.)]);
        let playing_track = self
            .playback
            .as_ref()
            .filter(|_| playing_here)
            .and_then(|p| p.item.as_ref()?.id.clone())
            .map(|id| format!("spotify:track:{id}"));
        let shown = if self.artist_more { 10 } else { 5 };
        let now = crate::playlist::now_unix();
        let rows: Vec<AnyElement> = page
            .top_tracks
            .iter()
            .take(shown)
            .enumerate()
            .map(|(i, t)| {
                self.track_row(i, t, &page.uri, widths[0], &[], widths[1], playing_track.as_deref() == Some(t.uri.as_str()), now, window, cx)
            })
            .collect();
        let popular = div()
            .mt(px(8.))
            .flex()
            .flex_col()
            .child(h2("Popular").px(px(16.)))
            .child(div().mt(px(16.8)).mx(px(cs + 0.8)).w(px(row_w)).flex().flex_col().children(rows))
            .when(page.top_tracks.len() > 5, |d| {
                d.child(
                    div().px(px(16.)).pt(px(0.8)).flex().child(
                        div()
                            .id("ar-see-more")
                            .p(px(16.))
                            .cursor_pointer()
                            .text_size(px(14.))
                            .line_height(px(19.2))
                            .font_weight(FontWeight::BOLD)
                            .text_color(rgba(0xFFFFFFB3))
                            .hover(|s| s.text_color(c(WHITE)))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.artist_more = !this.artist_more;
                                cx.notify();
                            }))
                            .child(if self.artist_more { "Show less" } else { "See more" }),
                    ),
                )
            });

        // ---- You Liked (from the Liked Songs page, once it has been loaded) ----
        let liked_count = self
            .playlist_pages
            .get(LIKED_URI)
            .map(|p| p.tracks.iter().filter(|t| t.artists.iter().any(|a| a.1 == page.uri)).count())
            .unwrap_or(0);
        let you_liked = (liked_count > 0).then(|| {
            let avatar = page.avatar.as_deref().and_then(|u| {
                let url = format!("crop:1x1:{u}");
                images::cache(cx).update(cx, |c, cx| c.get_sized(&url, 88., cx))
            });
            div()
                .mt(px(24.))
                .px(px(16.))
                .flex()
                .flex_col()
                .child(h2("You Liked"))
                .child(
                    div()
                        .mt(px(16.))
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(16.))
                        .child(
                            div()
                                .relative()
                                .size(px(88.))
                                .flex_none()
                                .child(match avatar {
                                    Some(a) => img(ImageSource::Render(a)).size(px(88.)).rounded_full().into_any_element(),
                                    None => div().size(px(88.)).rounded_full().bg(c(PLACEHOLDER)).into_any_element(),
                                })
                                .child(
                                    div()
                                        .absolute()
                                        .left(px(64.))
                                        .top(px(62.))
                                        .size(px(24.))
                                        .rounded_full()
                                        .bg(c(SURFACE))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .child(icon("icons/heart.svg", 16., GREEN)),
                                ),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap(px(4.))
                                .child(
                                    self.link("ar-liked".into(), format!("{liked_count} song{}", if liked_count == 1 { "" } else { "s" }), "spotify:collection:tracks".into(), cx)
                                        .text_size(px(16.))
                                        .line_height(px(22.4))
                                        .font_weight(FontWeight::BOLD)
                                        .text_color(c(WHITE)),
                                )
                                .child(
                                    div()
                                        .text_size(px(14.))
                                        .line_height(px(19.2))
                                        .text_color(c(TEXT_SUBDUED))
                                        .child(format!("By {}", page.name)),
                                ),
                        ),
                )
        });

        // ---- Artist pick ----
        let pick = page.pinned.as_ref().map(|p| {
            let image = p.image.as_deref().and_then(|u| images::cache(cx).update(cx, |c, cx| c.get_sized(u, 88., cx)));
            let avatar = page.avatar.as_deref().and_then(|u| images::cache(cx).update(cx, |c, cx| c.get_sized(u, 24., cx)));
            div()
                .mt(px(24.))
                .px(px(16.))
                .flex()
                .flex_col()
                .child(h2("Artist pick"))
                .child(
                    div()
                        .mt(px(16.))
                        .flex()
                        .flex_row()
                        .gap(px(16.))
                        .child(match image {
                            Some(i) => img(ImageSource::Render(i)).size(px(88.)).rounded(px(4.)).flex_none().into_any_element(),
                            None => div().size(px(88.)).rounded(px(4.)).bg(c(PLACEHOLDER)).flex_none().into_any_element(),
                        })
                        .child(
                            div()
                                .min_w_0()
                                .w(px((w - 32. - 104.) * 303. / 370.))
                                .flex()
                                .flex_col()
                                .items_start()
                                .pt(px(2.))
                                .when(!p.comment.is_empty(), |d| {
                                    d.child(
                                        div()
                                            .w_full()
                                            .h(px(28.))
                                            .pl(px(2.))
                                            .pr(px(12.))
                                            .flex()
                                            .flex_row()
                                            .items_center()
                                            .gap(px(8.))
                                            .rounded_full()
                                            .bg(c(WHITE))
                                            .child(match avatar {
                                                Some(a) => img(ImageSource::Render(a)).size(px(24.)).rounded_full().into_any_element(),
                                                None => div().size(px(24.)).rounded_full().bg(c(PLACEHOLDER)).into_any_element(),
                                            })
                                            .child(div().text_size(px(14.)).line_height(px(19.2)).text_color(c(0x000000)).child(p.comment.clone())),
                                    )
                                })
                                .child(
                                    self.link("ar-pick".into(), p.title.clone(), p.uri.clone(), cx)
                                        .mt(px(6.))
                                        .text_size(px(16.))
                                        .line_height(px(22.4))
                                        .font_weight(FontWeight::BOLD)
                                        .text_color(c(WHITE)),
                                )
                                .child(div().mt(px(4.)).text_size(px(14.)).line_height(px(19.2)).text_color(c(TEXT_SUBDUED)).child(p.subtitle.clone())),
                        ),
                )
        });

        // ---- card shelves ----
        let cols = (((w - 8.) / 180.).floor() as usize).max(2);
        let card_w = (w - 8.) / cols as f32;
        let shelf_head = |id: &str, title: String, right: Option<String>| {
            div()
                .h(px(48.))
                .pl(px(16.))
                .pr(px(24.))
                .flex()
                .flex_row()
                .items_end()
                .child(h2(title).flex_1().min_w_0())
                .when_some(right, |d, r| {
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
                            .child(r),
                    )
                })
        };
        let cards = |this: &Self, id: &str, items: &[home::Item], cx: &mut Context<Self>| {
            let cards: Vec<AnyElement> = items
                .iter()
                .take(cols)
                .enumerate()
                .map(|(i, it)| this.sized_card(format!("{id}-{i}").into(), it, card_w, 4., cx))
                .collect();
            div().px(px(4.)).flex().flex_row().children(cards)
        };
        let shelf = |this: &Self, id: &str, title: &str, items: &[home::Item], cx: &mut Context<Self>| {
            (!items.is_empty()).then(|| {
                div()
                    .mt(px(40.))
                    .flex()
                    .flex_col()
                    .child(shelf_head(id, title.to_string(), Some("Show all".into())))
                    .child(cards(this, id, items, cx).mt(px(8.)))
            })
        };

        let filter = self.disco_filter.min(3);
        let disco_items = &page.discography[filter];
        let discography = page.discography.iter().any(|d| !d.is_empty()).then(|| {
            let chips = DISCO_FILTERS.iter().enumerate().filter(|(i, _)| !page.discography[*i].is_empty()).map(|(i, label)| {
                let on = i == filter;
                div()
                    .id(gpui::ElementId::Name(format!("disco-chip-{i}").into()))
                    .h(px(32.))
                    .px(px(12.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .rounded_full()
                    .bg(if on { rgba(0xFFFFFFFF) } else { rgba(0xFFFFFF1A) })
                    .when(!on, |d| d.hover(|s| s.bg(rgba(0xFFFFFF24))))
                    .cursor_pointer()
                    .text_size(px(14.))
                    .line_height(px(14.))
                    .text_color(c(if on { 0x000000 } else { WHITE }))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.disco_filter = i;
                        cx.notify();
                    }))
                    .child(*label)
            });
            div()
                .mt(px(40.))
                .flex()
                .flex_col()
                .child({
                    let page = Page::Discography(uri.to_string());
                    div()
                        .id("disco-head")
                        .on_click(cx.listener(move |this, _, _, cx| this.open(page.clone(), cx)))
                        .child(shelf_head("disco", "Discography".into(), Some("Show all".into())))
                })
                .child(div().id("disco-chips").mt(px(16.)).px(px(16.)).flex().flex_row().gap(px(8.)).overflow_x_scroll().children(chips))
                .child(cards(self, "disco", disco_items, cx).mt(px(16.)))
        });
        let featuring = shelf(self, "featuring", &format!("Featuring {}", page.name), &page.featuring, cx);

        // ---- On Tour ----
        let tour = (!page.concerts.is_empty()).then(|| {
            // `.onTourShelfGrid`: 2 columns, 1 when the window is <= 992 wide, 3 when >= 1280; gap 8;
            // the first 9 concerts. Items: `margin-left: -8px; padding: 8px`, grid 72px auto.
            let vw = f32::from(window.viewport_size().width);
            let tcols: usize = if vw <= 992. { 1 } else if vw >= 1280. { 3 } else { 2 };
            let grid_w = w - 32.;
            let cell_w = (grid_w - 8. * (tcols as f32 - 1.)) / tcols as f32;
            let tile_w = cell_w + 8. - 0.01; // a hair under so rounding never wraps a row early
            let info_w = (tile_w - 16. - 80.).max(0.); // padding 8+8, date column 72 + gap 8
            let tiles = page.concerts.iter().take(9).enumerate().map(|(i, k)| {
                let (month, day, time) = concert_date(&k.start);
                div()
                    .id(gpui::ElementId::Name(format!("concert-{i}").into()))
                    .w(px(tile_w))
                    .when(i % tcols == 0, |d| d.ml(px(-8.)))
                    .h(px(80.8))
                    .p(px(8.))
                    .flex_none()
                    .flex()
                    .flex_row()
                    .rounded(px(4.))
                    .overflow_hidden()
                    .hover(|s| s.bg(c(0x1F1F1F)))
                    .cursor_pointer()
                    .child(
                        div()
                            .size(px(64.))
                            .flex_none()
                            .rounded(px(4.))
                            .bg(rgba(0xFFFFFF1A))
                            .flex()
                            .flex_col()
                            .items_center()
                            .justify_center()
                            .font_weight(FontWeight::BOLD)
                            .text_color(c(WHITE))
                            .child(div().text_size(px(14.)).line_height(px(19.2)).child(month))
                            .child(div().mt(px(2.)).text_size(px(24.)).line_height(px(28.8)).font_family(title_font.family.clone()).child(day)),
                    )
                    .child(
                        div()
                            .ml(px(16.))
                            .min_w_0()
                            .flex_1()
                            .flex()
                            .flex_col()
                            .child(one_line(k.city.clone()).w(px(info_w)).text_size(px(16.)).line_height(px(22.4)).font_weight(FontWeight::BOLD).text_color(c(WHITE)))
                            .child(one_line(k.title.clone()).w(px(info_w)).mt(px(4.)).text_size(px(14.)).line_height(px(19.2)).text_color(c(TEXT_SUBDUED)))
                            .child(one_line(format!("{time} \u{2022} {}", k.venue)).w(px(info_w)).text_size(px(14.)).line_height(px(19.2)).text_color(c(TEXT_SUBDUED))),
                    )
            });
            div()
                .mt(px(40.))
                .flex()
                .flex_col()
                .child(shelf_head("tour", "On Tour".into(), Some(format!("View all upcoming concerts ({})", page.concerts_total.max(page.concerts.len())))))
                .child(div().mt(px(8.)).px(px(16.)).w(px(w)).flex().flex_row().flex_wrap().gap_y(px(8.)).children(tiles))
        });

        // ---- About ----
        // `.qLRCOUHINaVfhHXdVtNi.rYVDSuYxVcgAONzJGphz`: full width, 516 tall, background-size cover.
        let about_w = w - 32.;
        let about_h = 516.;
        let about_img = page.about_image.as_ref().and_then(|u| {
            let url = format!("crop:{about_w:.1}x{about_h:.1}:{u}");
            images::cache(cx).update(cx, |c, cx| c.get_sized(&url, about_w, cx))
        });
        let about = div()
            .mt(px(40.))
            .px(px(16.))
            .flex()
            .flex_col()
            .child(h2("About"))
            .child(
                div()
                    .id("ar-about")
                    .on_click({
                        let uri = page.uri.clone();
                        cx.listener(move |this, _, _, cx| this.open_about_for(&uri, cx))
                    })
                    .mt(px(16.))
                    .relative()
                    .w(px(about_w))
                    .h(px(about_h))
                    .rounded(px(8.))
                    .overflow_hidden()
                    .bg(c(0x282828))
                    .cursor_pointer()
                    .when_some(about_img, |d, i| d.child(img(ImageSource::Render(i)).absolute().inset_0().size_full().rounded(px(8.))))
                    .child(div().absolute().inset_0().bg(linear_gradient(
                        180.,
                        linear_color_stop(rgba(0x00000000), 0.),
                        linear_color_stop(rgba(0x000000B3), 1.),
                    )))
                    .when(page.world_rank > 0, |d| {
                        // 80px #0D72EA circle, top 40 / end 40; "#N" 24/700 at 25%, "in the world" 12 at 55%.
                        d.child(
                            div()
                                .absolute()
                                .top(px(40.))
                                .right(px(40.))
                                .size(px(80.))
                                .rounded_full()
                                .bg(c(0x0D72EA))
                                .text_color(c(WHITE))
                                .child(
                                    div()
                                        .absolute()
                                        .top(px(20.))
                                        .w_full()
                                        .flex()
                                        .justify_center()
                                        .text_size(px(24.))
                                        .line_height(px(28.8))
                                        .font_weight(FontWeight::BOLD)
                                        .font_family(title_font.family.clone())
                                        .child(format!("#{}", page.world_rank)),
                                )
                                .child(
                                    div()
                                        .absolute()
                                        .top(px(44.))
                                        .w_full()
                                        .flex()
                                        .justify_center()
                                        .text_size(px(12.))
                                        .line_height(px(16.8))
                                        .child("in the world"),
                                ),
                        )
                    })
                    .child(
                        div()
                            .absolute()
                            .left_0()
                            .bottom_0()
                            .w(px(about_w.min(672.)))
                            .p(px(40.))
                            .flex()
                            .flex_col()
                            .gap(px(8.))
                            .text_color(c(WHITE))
                            .child(
                                div()
                                    .text_size(px(16.))
                                    .line_height(px(22.4))
                                    .font_weight(FontWeight::BOLD)
                                    .child(format!("{} monthly listeners", artist::grouped(page.monthly_listeners))),
                            )
                            .when(!page.biography.is_empty(), |d| {
                                d.child(div().text_size(px(16.)).line_height(px(22.4)).line_clamp(3).text_ellipsis().child(page.biography.clone()))
                            }),
                    ),
            );

        let discovered = shelf(self, "discovered", "Discovered on", &page.discovered_on, cx).map(|d| d.mt(px(5.6)));
        let related = shelf(self, "related", "Fans also like", &page.related, cx);
        let appears = shelf(self, "appears", "Appears On", &page.appears_on, cx);

        let body = div()
            .relative()
            .flex()
            .flex_col()
            .pb(px(32.))
            .child(div().absolute().top_0().left_0().right_0().h(px(173.)).bg(linear_gradient(
                180.,
                linear_color_stop(rgba(page.color << 8 | 0xB3), 0.),
                linear_color_stop(rgba(page.color << 8), 1.),
            )))
            .child(action_bar)
            .child(popular)
            .children(you_liked)
            .children(pick)
            .children(discography)
            .children(featuring)
            .children(tour)
            .child(about)
            .children(discovered)
            .children(related)
            .children(appears);

        let top_bar = self.sticky_bar(&page.name, &page.uri, page.color, hdr_h, cx);
        panel
            .child(
                div()
                    .id(gpui::ElementId::Name(format!("ar-scroll-{uri}").into()))
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

/// `2026-11-22T18:00+05:30` -> ("Nov", "22", "Sun 6:00 PM"), in the concert's own local time.
pub(super) fn concert_date(iso: &str) -> (String, String, String) {
    const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    const DAYS: [&str; 7] = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"];
    let n = |r: std::ops::Range<usize>| iso.get(r).and_then(|x| x.parse::<i64>().ok());
    let (Some(y), Some(m), Some(d)) = (n(0..4), n(5..7), n(8..10)) else {
        return Default::default();
    };
    let (h, mi) = (n(11..13).unwrap_or(0), n(14..16).unwrap_or(0));
    // days since 1970-01-01 (a Thursday)
    let yy = if m <= 2 { y - 1 } else { y };
    let era = yy.div_euclid(400);
    let yoe = yy - era * 400;
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + d - 1;
    let days = era * 146_097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719_468;
    let wd = DAYS[days.rem_euclid(7) as usize];
    let (h12, ampm) = match h {
        0 => (12, "AM"),
        1..=11 => (h, "AM"),
        12 => (12, "PM"),
        _ => (h - 12, "PM"),
    };
    (MONTHS[(m - 1) as usize].to_string(), d.to_string(), format!("{wd} {h12}:{mi:02} {ampm}"))
}

trait RoundTenth {
    fn round_to_tenth(self) -> f32;
}
impl RoundTenth for f32 {
    fn round_to_tenth(self) -> f32 {
        (self * 10.).round() / 10.
    }
}
