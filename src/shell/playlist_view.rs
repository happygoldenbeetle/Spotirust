//! Playlist page (main view). Geometry from the reference client's CSS, with W the main view width:
//!   s(a, b) = clamp(a, a + (W - 600) / 424 * (b - a), b)
//!   content spacing cs = s(16, 24); header height = max(s(186, 276), min(30vh, s(186, 336), 336)),
//!   padding-bottom cs; cover s(128, 232) at the bottom left (radius 4, shadow 0 4 60 #00000080).
//!   Text column cs right of the cover, bottom aligned: label 14px (line 19.2), title, 8px gap,
//!   owner line 24 tall. Title: 32px below W 600, else the largest of 96/72/48/32 that fits on one line.
//!   Header background: higherContrast base + linear gradient minContrast base -> transparent.
//!   Body: #121212 + 173px overlay rgba(base, .7) -> transparent.
//!   Action bar cs below the header: play 48 (56 from W 600), explore 30x40, 24px icons cs apart;
//!   search 32 + "Custom order" at the right. Pills 22 below; tracklist 6 below the pills.
//!   Tracklist: [16 | first 4fr (>=180) | var1 2fr | var2 2fr (>=120) | last 1fr (>=120)], gap 16,
//!   padding 0 16; Album from W 600, Date added from W 776. Header row 36 (+1 border), rows 56.

use super::*;
use crate::playlist::{self, PageKind, PlaylistPage, Track};
use gpui::{BoxShadow, Font, TextRun, hsla as gpui_hsla, point};

pub(super) fn fluid(w: f32, a: f32, b: f32) -> f32 {
    (a + (w - 600.) / 424. * (b - a)).clamp(a.min(b), a.max(b))
}

pub(super) fn text_width(window: &Window, text: &str, size: f32, font: Font) -> f32 {
    let run = TextRun { len: text.len(), font, color: hsla(WHITE), background_color: None, underline: None, strikethrough: None };
    f32::from(window.text_system().shape_line(text.to_string().into(), px(size), &[run], None).width)
}

/// CSS grid track sizing for `minmax(min, Nfr)` columns: fr shares, with columns below their
/// minimum frozen at it and the rest redistributed.
pub(super) fn fr_columns(space: f32, cols: &[(f32, f32)]) -> Vec<f32> {
    let mut frozen = vec![false; cols.len()];
    loop {
        let rest: f32 = space - cols.iter().zip(&frozen).filter(|(_, f)| **f).map(|(c, _)| c.0).sum::<f32>();
        let frs: f32 = cols.iter().zip(&frozen).filter(|(_, f)| !**f).map(|(c, _)| c.1).sum();
        let out: Vec<f32> =
            cols.iter().zip(&frozen).map(|(c, f)| if *f { c.0 } else { rest * c.1 / frs.max(0.0001) }).collect();
        let mut changed = false;
        for i in 0..cols.len() {
            if !frozen[i] && out[i] < cols[i].0 {
                frozen[i] = true;
                changed = true;
            }
        }
        if !changed {
            return out;
        }
    }
}

const ROW_H: f32 = 56.;

impl Shell {
    pub(super) fn playlist_view(&self, uri: &str, main_w: f32, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let panel = div().flex_1().min_w_0().h_full().rounded(px(8.)).bg(c(SURFACE)).overflow_hidden().relative();
        let Some(page) = self.playlist_pages.get(uri) else {
            return panel.into_any_element();
        };
        let w = main_w;
        // Debug: SPOTIRUST_SCROLL=<px> holds the page at that scroll position (for captures).
        if let Some(y) = std::env::var("SPOTIRUST_SCROLL").ok().and_then(|v| v.parse::<f32>().ok()) {
            self.page_scroll.set_offset(point(px(0.), px(-y)));
        }
        let vh = f32::from(window.viewport_size().height);
        let cs = fluid(w, 16., 24.);
        let hdr_h = fluid(w, 186., 276.).max(fluid(w, 186., 336.).min(vh * 0.3).min(336.));
        let cover_size = fluid(w, 128., 232.);
        let text_x = cs + cover_size + cs;
        let text_w = (w - text_x - cs).max(0.);

        // Title size: fit-to-width steps (fixed 32px under 600).
        let mut title_font = gpui::font(title_font());
        title_font.weight = FontWeight(800.);
        let title_size = if w < 600. {
            32.
        } else {
            [96., 72., 48.]
                .into_iter()
                .find(|&s| text_width(window, &page.name, s, title_font.clone()) <= text_w)
                .unwrap_or(32.)
        };

        let colors = page.colors;
        let liked = page.kind == PageKind::Liked;
        let image = if liked {
            Some(LIKED_SONGS_IMAGE.to_string())
        } else {
            self.playlist_image(page).map(|u| images::variant(&u, images::Variant::Medium))
        };
        let total_ms: u64 = page.tracks.iter().map(|t| t.duration_ms).sum();
        let count = page.total.max(page.tracks.len());
        let songs = format!("{count} song{}", if count == 1 { "" } else { "s" });
        let length = playlist::total_duration(total_ms);

        // ---- header ----
        let avatar_url = page.owner_avatar.as_deref().map(user_image_160);
        let large = w >= 600.;
        let album = page.kind == PageKind::Album;
        // measured: album meta text is rgba(255,255,255,.7); playlists use the cover's textSubdued
        let sub = if album || liked { rgba(0xFFFFFFB3) } else { c(colors.subdued) };
        let avatar_size = if large { 24. } else { 20. };
        let avatar = avatar_url.as_deref().and_then(|u| images::cache(cx).update(cx, |c, cx| c.get_sized(u, avatar_size, cx)));
        let owner_line = div()
            .h(px(24.))
            .flex()
            .flex_row()
            .items_center()
            .text_size(px(14.))
            .line_height(px(19.2))
            .when(page.can_edit, |d| {
                d.child(
                    div()
                        .size(px(24.))
                        .flex_none()
                        .rounded_full()
                        .bg(rgba(0xFFFFFF1A))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(icon("icons/pl_invite12.svg", 12., WHITE)),
                )
            })
            .child(
                div().when(page.can_edit, |d| d.ml(px(-0.7))).flex_none().h_full().child(match avatar {
                    Some(a) => img(ImageSource::Render(a)).size(px(avatar_size)).rounded_full().into_any_element(),
                    None => div().size(px(avatar_size)).rounded_full().bg(c(0x535353)).into_any_element(),
                }),
            )
            .child(
                self.link("pl-owner".into(), page.owner.clone(), page.owner_uri.clone(), cx)
                    .ml(px(4.))
                    .font_weight(FontWeight::BOLD)
                    .text_color(c(WHITE)),
            )
            // measured: "•" is its own span with 4px margins; ", " has a 1px right margin
            .child(div().mx(px(4.)).flex_none().text_color(sub).child("\u{2022}"))
            .when_some(page.year.clone(), |d, y| {
                d.child(div().flex_none().text_color(sub).child(y))
                    .child(div().mx(px(4.)).flex_none().text_color(sub).child("\u{2022}"))
            })
            .child(
                div()
                    .min_w_0()
                    .flex()
                    .flex_row()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_color(sub)
                    .child(div().flex_none().child(songs))
                    .child(div().flex_none().mr(px(1.)).child(", "))
                    .child(one_line(length).min_w_0()),
            );

        let title = div()
            .font_family(title_font.family.clone())
            .font_weight(FontWeight(800.))
            .text_size(px(title_size))
            .line_height(px(title_size * 1.2))
            .text_color(c(WHITE))
            .when(title_size == 32., |d| d.line_clamp(3).text_ellipsis())
            .when(title_size > 32., |d| d.whitespace_nowrap())
            .child(page.name.clone());

        let header = div()
            .relative()
            .flex_none()
            .w_full()
            .h(px(hdr_h))
            .bg(c(colors.base))
            .child(div().absolute().inset_0().bg(if liked {
                // measured: linear-gradient(transparent, rgba(0,0,0,.5)) over rgb(80,56,160)
                linear_gradient(180., linear_color_stop(rgba(0x00000000), 0.), linear_color_stop(rgba(0x00000080), 1.))
            } else {
                linear_gradient(180., linear_color_stop(c(colors.top), 0.), linear_color_stop(rgba(colors.top << 8), 1.))
            }))
            .child(
                div()
                    .absolute()
                    .left(px(cs))
                    .top(px(hdr_h - cs - cover_size))
                    .size(px(cover_size))
                    .rounded(px(4.))
                    .shadow(vec![BoxShadow {
                        color: gpui_hsla(0., 0., 0., 0.5),
                        offset: point(px(0.), px(4.)),
                        blur_radius: px(60.),
                        spread_radius: px(0.),
                    }])
                    .child(cover(image.as_deref(), cover_size, 4., cx)),
            )
            .child(
                div()
                    .absolute()
                    .left(px(text_x))
                    .w(px(text_w))
                    .bottom(px(cs))
                    .flex()
                    .flex_col()
                    .child(div().text_size(px(14.)).line_height(px(19.2)).text_color(c(WHITE)).child(page.label))
                    .child(title)
                    .child(owner_line.mt(px(8.))),
            );

        // ---- action bar ----
        let play_size = if large { 56. } else { 48. };
        let icon_size = if large { 32. } else { 24. };
        let playing_here = self
            .playback
            .as_ref()
            .and_then(|p| p.context.as_ref())
            .is_some_and(|ctx| ctx.uri == page.uri);
        let is_playing = playing_here && self.playback.as_ref().is_some_and(|p| p.is_playing);
        let page_uri = page.uri.clone();
        let play = div()
            .id("pl-play")
            .size(px(play_size))
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
            .child(icon(if is_playing { "icons/pause24.svg" } else { "icons/play24.svg" }, 24., 0x000000));
        let explore = page.tracks.first().and_then(|t| t.image.as_deref()).map(|u| images::variant(u, images::Variant::Small));
        let bar_icon = move |id: &'static str, path: &'static str| {
            div()
                .id(id)
                .w(px(icon_size))
                .h(px(play_size))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .child(icon(path, icon_size, TEXT_SUBDUED))
        };
        let shuffle_on = self.playback.as_ref().is_some_and(|p| p.shuffle_state) && playing_here;
        let action_bar = div()
            .flex_none()
            .h(px(play_size))
            .mt(px(cs))
            .px(px(cs))
            .flex()
            .flex_row()
            .items_center()
            .gap(px(cs))
            .child(play)
            .when_some(explore.filter(|_| !liked), |d, url| {
                d.child(
                    div()
                        .id("pl-explore")
                        .relative()
                        .p(px(4.))
                        .flex_none()
                        .cursor_pointer()
                        .child(cover_rect(Some(&url), 30., 40., cx))
                        // Lottie frame drawn over the thumbnail (2px #A7A7A7 rounded stroke).
                        .child(
                            svg().path("icons/pl_explore_frame.svg").absolute().top_0().left_0().w(px(38.)).h(px(48.)).text_color(c(0xA7A7A7)),
                        ),
                )
            })
            .child(bar_icon("pl-shuffle", "icons/pl_shuffle24.svg").when(shuffle_on, |d| {
                d.child(div())
            }))
            .when(album, |d| d.child(bar_icon("pl-save", "icons/add_circle24.svg")))
            .child(bar_icon("pl-download", "icons/pl_download24.svg"))
            .when(page.can_edit, |d| d.child(bar_icon("pl-invite", "icons/pl_adduser24.svg")))
            .when(!liked, |d| d.child(bar_icon("pl-more", "icons/pl_more24.svg")))
            .child(div().flex_1())
            .when(!album, |d| d.child(
                div()
                    .id("pl-search")
                    .size(px(32.))
                    .flex_none()
                    .rounded_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .child(icon_rgba("icons/pl_search16.svg", 16., ICON_DIM)),
            ))
            .child(
                div()
                    .id("pl-sort")
                    .when(!album, |d| d.ml(px(8. - cs)))
                    .h(px(32.))
                    .px(px(8.))
                    .flex_none()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(8.))
                    .rounded(px(4.))
                    .cursor_pointer()
                    .text_size(px(14.))
                    .line_height(px(19.2))
                    .text_color(rgba(ICON_DIM))
                    .child(if album { "List" } else if liked { "Recently added" } else { "Custom order" })
                    .child(icon_rgba("icons/pl_list16.svg", 16., ICON_DIM)),
            );

        let pill = |id: &'static str, path: &'static str, label: &'static str| {
            div()
                .id(id)
                .h(px(32.))
                .pl(px(12.))
                .pr(px(16.))
                .flex()
                .flex_row()
                .items_center()
                .gap(px(8.))
                .rounded_full()
                .bg(rgba(0xFFFFFF1A))
                .hover(|s| s.bg(rgba(0xFFFFFF33)))
                .cursor_pointer()
                .text_size(px(14.))
                .line_height(px(19.2))
                .font_weight(FontWeight::BOLD)
                .text_color(c(WHITE))
                .child(icon(path, 16., WHITE))
                .child(label)
        };
        let chip_font = gpui::font(ui_font());
        let chips_w: f32 = page.filters.iter().map(|f| text_width(window, f, 14., chip_font.clone()) + 24. + 8.).sum::<f32>() - 8.;
        let chips_overflow = chips_w > w - 2. * cs;
        let chips = (liked && !page.filters.is_empty()).then(|| {
            div()
                .relative()
                .mt(px(cs + 8.))
                .child(div()
                .id("liked-chips")
                .px(px(cs))
                .flex()
                .flex_row()
                .gap(px(8.))
                .overflow_x_scroll()
                .children(page.filters.iter().enumerate().map(|(i, f)| {
                    div()
                        .id(gpui::ElementId::Name(format!("liked-chip-{i}").into()))
                        .h(px(32.))
                        .px(px(12.))
                        .flex_none()
                        .flex()
                        .items_center()
                        .rounded_full()
                        .bg(rgba(0xFFFFFF1A))
                        .hover(|s| s.bg(rgba(0xFFFFFF24)))
                        .cursor_pointer()
                        .text_size(px(14.))
                        .line_height(px(14.))
                        .text_color(c(WHITE))
                        .child(f.clone())
                })))
                // measured: a 32px white circle with a 16px chevron at the right edge while chips overflow
                .when(chips_overflow, |d| {
                    d.child(
                        div()
                            .id("liked-chips-next")
                            .absolute()
                            .top_0()
                            .right(px(cs))
                            .size(px(32.))
                            .rounded_full()
                            .bg(c(WHITE))
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(icon("icons/chevron_right.svg", 16., 0x000000)),
                    )
                })
        });
        let pills = page.can_edit.then(|| {
            div()
                .mt(px(cs + 6.))
                .px(px(cs))
                .flex()
                .flex_row()
                .gap(px(8.))
                .child(pill("pl-add", "icons/pl_plus16.svg", "Add"))
                .child(pill("pl-mix", "icons/pl_mix16.svg", "Mix"))
                .child(pill("pl-details", "icons/pl_pencil16.svg", "Name & details"))
        });

        // ---- tracklist ----
        // measured on an album: the list border box starts cs below the action bar
        let list_gap = if pills.is_some() { 6. } else if chips.is_some() { 8. } else if album { cs } else { cs + 6. };
        let head_y = hdr_h + cs + play_size + pills.as_ref().map_or(0., |_| cs + 6. + 32.)
            + chips.as_ref().map_or(0., |_| cs + 8. + 32.) + list_gap + 0.8;
        let list_w = w - 2. * cs;
        let row_w = list_w - 1.6;
        let ncols = if album { 3 } else if w >= 776. { 5 } else if w >= 600. { 4 } else { 3 };
        let spec: Vec<(f32, f32)> = match ncols {
            5 => vec![(180., 4.), (120., 2.), (120., 2.), (120., 1.)],
            4 => vec![(180., 4.), (120., 2.), (120., 1.)],
            _ => vec![(180., 4.), (120., 1.)],
        };
        let gaps = 16. * spec.len() as f32;
        let widths = fr_columns(row_w - 32. - 16. - gaps, &spec);
        let first_w = widths[0];
        let var_ws: Vec<f32> = widths[1..widths.len() - 1].to_vec();
        let last_w = *widths.last().unwrap();

        let head_cell = |text: &'static str, wd: f32| {
            div().w(px(wd)).flex_none().text_size(px(14.)).line_height(px(19.2)).child(text)
        };
        let make_head = || {
        let mut head = div()
            .h(px(36.8))
            .px(px(16.))
            .flex()
            .flex_row()
            .items_center()
            .gap(px(16.))
            .border_b(px(0.8))
            .border_color(rgba(0xFFFFFF1A))
            .text_color(c(TEXT_SUBDUED))
            .child(div().w(px(16.)).flex_none().flex().justify_end().text_size(px(16.)).line_height(px(22.4)).child("#"))
            .child(head_cell("Title", first_w));
        let var_titles = ["Album", "Date added"];
        for (i, vw) in var_ws.iter().enumerate() {
            head = head.child(head_cell(var_titles[i], *vw));
        }
        head = head.child(
            div()
                .w(px(last_w))
                .flex_none()
                .flex()
                .flex_row()
                .justify_end()
                .pr(px(32.))
                .child(icon("icons/pl_clock16.svg", 16., TEXT_SUBDUED)),
        );
        head
        };
        let head = make_head();

        let playing_track = self.playback.as_ref().filter(|_| playing_here).and_then(|p| p.item.as_ref()?.id.clone()).map(|id| format!("spotify:track:{id}"));
        let now = playlist::now_unix();
        let rows = page.tracks.iter().enumerate().map(|(i, t)| {
            self.track_row(i, t, &page.uri, first_w, &var_ws, last_w, playing_track.as_deref() == Some(t.uri.as_str()), now, window, cx)
        });
        let list = div()
            .mt(px(list_gap))
            .mx(px(cs))
            .w(px(list_w))
            .border(px(0.8))
            .border_color(rgba(0x00000000))
            .flex()
            .flex_col()
            .child(head)
            .child(div().h(px(cs - 8.)))
            .children(rows);

        let body = div()
            .relative()
            .flex()
            .flex_col()
            .pb(px(32.))
            .child(if liked {
                div().absolute().top_0().left_0().right_0().h(px(232.)).bg(c(colors.base)).child(
                    div().size_full().bg(linear_gradient(
                        180.,
                        linear_color_stop(rgba(0x00000099), 0.),
                        linear_color_stop(c(SURFACE), 1.),
                    )),
                )
            } else {
                div().absolute().top_0().left_0().right_0().h(px(173.)).bg(linear_gradient(
                    180.,
                    linear_color_stop(rgba(colors.base << 8 | 0xB3), 0.),
                    linear_color_stop(rgba(colors.base << 8), 1.),
                ))
            })
            .child(action_bar)
            .children(pills)
            .children(chips)
            .child(list)
            .when(album, |d| d.child(self.album_footer(page, w, cx)));

        // ---- sticky top bar (measured on an album at header height 186) ----
        // Background (header base colour) fades in from scroll hdr-110 over 111px; the play button
        // and title fade in from hdr-88.6 over 85.2px. The tracklist header sticks at y 64 on #1F1F1F.
        let scroll = -f32::from(self.page_scroll.offset().y);
        let top_bar = self.sticky_bar(&page.name, &page.uri, colors.base, hdr_h, cx);
        let sticky_head = (head_y - scroll < 64.).then(|| {
            div()
                .absolute()
                .top(px(64.))
                .left(px(0.8))
                .right(px(0.8))
                .bg(c(0x1F1F1F))
                .child(make_head().ml(px(cs)).w(px(row_w)))
        });

        panel
            .child(
                div()
                    .id(gpui::ElementId::Name(format!("pl-scroll-{uri}").into()))
                    .track_scroll(&self.page_scroll)
                    .absolute()
                    .inset_0()
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .child(header)
                    .child(body),
            )
            .children(sticky_head)
            .children(top_bar)
            .into_any_element()
    }

    /// The bar that fades in over the top of a scrolled page: header colour, play button, title.
    /// Measured on an album at header height 186: the background fades in from scroll hdr-110 over
    /// 111px; the play button and title from hdr-88.6 over 85.2px.
    pub(super) fn sticky_bar(&self, title: &str, context: &str, color: u32, hdr_h: f32, cx: &mut Context<Self>) -> Option<Div> {
        let scroll = -f32::from(self.page_scroll.offset().y);
        let bg_op = ((scroll - (hdr_h - 110.)) / 111.).clamp(0., 1.);
        let content_op = ((scroll - (hdr_h - 88.6)) / 85.2).clamp(0., 1.);
        if bg_op <= 0. {
            return None;
        }
        let playing_here = self.playback.as_ref().and_then(|p| p.context.as_ref()).is_some_and(|ctx| ctx.uri == context);
        let is_playing = playing_here && self.playback.as_ref().is_some_and(|p| p.is_playing);
        let page_uri = context.to_string();
        Some(
            div()
                .absolute()
                .top_0()
                .left_0()
                .right_0()
                .h(px(64.))
                .child(div().absolute().inset_0().bg(c(color)).opacity(bg_op))
                .child(
                    div()
                        .absolute()
                        .top(px(8.))
                        .left(px(16.))
                        .right(px(16.))
                        .h(px(48.))
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(8.))
                        .opacity(content_op)
                        .child(
                            div()
                                .id("sticky-play")
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
                        .child(
                            one_line(title.to_string())
                                .min_w_0()
                                .font_family(title_font())
                                .font_weight(FontWeight::BOLD)
                                .text_size(px(24.))
                                .line_height(px(28.8))
                                .text_color(c(WHITE)),
                        ),
                ),
        )
    }

    /// Album footer, measured at W 506: release date (14px) and copyright lines (12px, line 16.8)
    /// 32px below the list, "N more release(s)" button (40 tall, #282828, radius 4) at the right;
    /// then "More by <artist>" (24px bold, 67px lower) and one row of cards, (W - 8) / n wide.
    fn album_footer(&self, page: &PlaylistPage, w: f32, cx: &mut Context<Self>) -> AnyElement {
        let info = div()
            .mt(px(32.8))
            .px(px(16.))
            .flex()
            .flex_row()
            .items_center()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .text_color(c(TEXT_SUBDUED))
                    .when_some(page.release_date.clone(), |d, r| d.child(div().text_size(px(14.)).line_height(px(19.2)).child(r)))
                    .children(page.copyrights.iter().map(|l| div().text_size(px(12.)).line_height(px(16.8)).child(l.clone()))),
            )
            .when(page.more_releases > 0, |d| {
                let n = page.more_releases;
                d.child(
                    div()
                        .id("album-releases")
                        .h(px(40.))
                        .px(px(16.))
                        .flex_none()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(8.))
                        .rounded(px(4.))
                        .bg(c(0x282828))
                        .cursor_pointer()
                        .text_size(px(14.))
                        .line_height(px(19.2))
                        .text_color(c(TEXT_SUBDUED))
                        .child(format!("{n} more release{}", if n == 1 { "" } else { "s" }))
                        .child(icon("icons/caret_down16.svg", 16., TEXT_SUBDUED)),
                )
            });
        let cols = (((w - 8.) / 180.).floor() as usize).max(2);
        let card_w = (w - 8.) / cols as f32;
        let cards: Vec<AnyElement> = page
            .more_by
            .iter()
            .take(cols)
            .enumerate()
            .map(|(i, it)| self.sized_card(format!("more-by-{i}").into(), it, card_w, 4., cx))
            .collect();
        let more = (!cards.is_empty()).then(|| {
            div()
                .mt(px(67.2))
                .flex()
                .flex_col()
                .child(
                    div()
                        .pl(px(16.))
                        .pr(px(24.))
                        .flex()
                        .flex_row()
                        .items_end()
                        .child(
                            one_line(format!("More by {}", page.owner))
                                .flex_1()
                                .font_family(title_font())
                                .text_size(px(24.))
                                .line_height(px(28.8))
                                .font_weight(FontWeight::BOLD)
                                .text_color(c(WHITE)),
                        )
                        .child(
                            div()
                                .id("album-discography")
                                .pb(px(1.))
                                .flex_none()
                                .text_size(px(14.))
                                .line_height(px(19.2))
                                .font_weight(FontWeight::BOLD)
                                .text_color(c(TEXT_SUBDUED))
                                .cursor_pointer()
                                .hover(|s| s.underline())
                                .child("See discography"),
                        ),
                )
                .child(div().mt(px(8.)).px(px(4.)).flex().flex_row().children(cards))
        });
        div().flex().flex_col().child(info).children(more).into_any_element()
    }

    /// The header cover: the playlist image, or the library's resolved mosaic for generated covers.
    fn playlist_image(&self, page: &PlaylistPage) -> Option<String> {
        match &page.image {
            Some(u) if u.contains("mosaic.scdn.co") => self
                .library
                .iter()
                .flatten()
                .find(|i| i.uri == page.uri)
                .and_then(|i| i.image.clone())
                .or_else(|| Some(u.clone())),
            other => other.clone(),
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn track_row(
        &self,
        i: usize,
        t: &Track,
        context: &str,
        first_w: f32,
        var_ws: &[f32],
        last_w: f32,
        playing: bool,
        now: i64,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let group: SharedString = format!("pl-row-{i}").into();
        // Rows outside a playlist or album (search results) play from the song's album.
        let from_search = context.is_empty();
        let context = if from_search { t.album_uri.as_str() } else { context };
        let artists = t.artists.iter().map(|a| a.0.as_str()).collect::<Vec<_>>().join(", ");
        let title_color = if playing { GREEN } else { WHITE };
        // Artist names are separate links; when the line would be cut off it stays one ellipsized text.
        let artist_room = first_w - 8. - if t.number == 0 { 52.8 } else { 0.8 } - if t.explicit { 20. } else { 0. };
        let link_artists = text_width(window, &artists, 14., gpui::font(ui_font())) <= artist_room;
        let (ctx, track) = (context.to_string(), t.uri.clone());
        let mut row = div()
            .id(gpui::ElementId::Name(group.clone()))
            .on_mouse_down(MouseButton::Right, self.track_ctx_listener(t, context, cx))
            .group(group.clone())
            .h(px(ROW_H))
            .px(px(16.))
            .flex()
            .flex_row()
            .items_center()
            .gap(px(16.))
            .rounded(px(4.))
            .hover(|s| s.bg(rgba(0xFFFFFF1A)))
            .on_click(cx.listener(move |this, ev: &gpui::ClickEvent, _, cx| {
                if ev.click_count() == 2 {
                    let (ctx, track) = (ctx.clone(), track.clone());
                    this.player(Play::ContextAt(ctx.clone(), track.clone()), cx);
                }
            }))
            // index / hover play
            .child(
                div()
                    .relative()
                    .w(px(16.))
                    .h(px(22.4))
                    .flex_none()
                    .child(
                        div()
                            .absolute()
                            .inset_0()
                            .flex()
                            .justify_center()
                            .text_size(px(16.))
                            .line_height(px(22.4))
                            .text_color(c(if playing { GREEN } else { TEXT_SUBDUED }))
                            .group_hover(group.clone(), |s| s.opacity(0.))
                            .child(format!("{}", if t.number > 0 { t.number as usize } else { i + 1 })),
                    )
                    .child(
                        div()
                            .absolute()
                            .top(px(3.2))
                            .left_0()
                            .opacity(0.)
                            .group_hover(group.clone(), |s| s.opacity(1.))
                            .child(icon("icons/pl_play24b.svg", 16., WHITE)),
                    ),
            )
            // title + artists
            .child(
                div()
                    .w(px(first_w))
                    .flex_none()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(12.))
                    .when(t.number > 0, |d| d.pl(px(0.8)))
                    .when(t.number == 0, |d| d.child(
                        // measured: the image sits 0.8px into the column
                        div().ml(px(0.8)).flex_none().child(cover(
                            t.image.as_deref().map(|u| images::variant(u, images::Variant::Small)).as_deref(),
                            40.,
                            4.,
                            cx,
                        )),
                    ))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            // measured: the texts end 8px before the column does
                            .pr(px(8.))
                            .flex()
                            .flex_col()
                            .child({
                                // The client links the title to the song page (underline on hover);
                                // until there's a song page it opens the song's album.
                                let album = t.album_uri.clone();
                                div().flex().child(
                                    one_line(t.name.clone())
                                        .id(gpui::ElementId::Name(format!("trk-title-{i}").into()))
                                        .min_w_0()
                                        .text_size(px(16.))
                                        .line_height(px(22.4))
                                        .text_color(c(title_color))
                                        .cursor_pointer()
                                        .hover(|s| s.underline())
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            if let Some(p) = Page::for_uri(&album) {
                                                cx.stop_propagation();
                                                this.open(p, cx);
                                            }
                                        })),
                                )
                            })
                            .child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .items_center()
                                    .gap(px(4.))
                                    .text_size(px(14.))
                                    .line_height(px(19.2))
                                    .text_color(c(TEXT_SUBDUED))
                                    .when(t.explicit, |d| {
                                        d.child(
                                            div()
                                                .size(px(16.))
                                                .flex_none()
                                                .rounded(px(4.))
                                                .bg(rgba(0xFFFFFF1A))
                                                .flex()
                                                .items_center()
                                                .justify_center()
                                                .text_size(px(12.))
                                                .font_weight(FontWeight::BOLD)
                                                .text_color(c(WHITE))
                                                .child("E"),
                                        )
                                    })
                                    .child(if link_artists {
                                        let mut line = div()
                                            .flex()
                                            .flex_row()
                                            .whitespace_nowrap()
                                            .group_hover(group.clone(), |s| s.text_color(c(WHITE)));
                                        for (k, (name, uri)) in t.artists.iter().enumerate() {
                                            if k > 0 {
                                                line = line.child(div().flex_none().child(", "));
                                            }
                                            line = line.child(self.link(format!("{group}-a{k}"), name.clone(), uri.clone(), cx));
                                        }
                                        line.into_any_element()
                                    } else {
                                        one_line(artists).min_w_0().group_hover(group.clone(), |s| s.text_color(c(WHITE))).into_any_element()
                                    }),
                            ),
                    ),
            );
        for (k, vw) in var_ws.iter().enumerate() {
            let text = match k {
                0 => t.album.clone(),
                _ => t.added_at.map(|a| playlist::date_added(a, now)).unwrap_or_default(),
            };
            let cell = div().w(px(*vw)).flex_none().min_w_0().flex().text_size(px(14.)).line_height(px(19.2)).text_color(c(TEXT_SUBDUED));
            row = row.child(if k == 0 {
                cell.group_hover(group.clone(), |s| s.text_color(c(WHITE)))
                    .child(self.link(format!("{group}-album"), text, t.album_uri.clone(), cx).flex_shrink().min_w_0().overflow_hidden().text_ellipsis())
            } else {
                cell.child(one_line(text))
            });
        }
        row.child(
            div()
                .w(px(last_w))
                .flex_none()
                .flex()
                .flex_row()
                .items_center()
                .justify_end()
                .child({
                    let uri = t.uri.clone();
                    let liked = self.liked.contains(&t.uri);
                    let btn = div()
                        .id(gpui::ElementId::Name(format!("{group}-add").into()))
                        .w(px(16.))
                        .h(px(32.))
                        .flex_none()
                        .flex()
                        .items_center()
                        .cursor_pointer()
                        .on_click(cx.listener(move |this, ev: &gpui::ClickEvent, _, cx| {
                            cx.stop_propagation();
                            if liked {
                                let (x, y, b) = Self::anchor_of(ev, 16.);
                                this.open_add_popover(uri.clone(), (x, y - 8., b + 8.), cx);
                            } else {
                                this.like_track(uri.clone(), cx);
                            }
                        }));
                    if liked {
                        // measured: liked tracks show the green check at all times
                        btn.child(icon("icons/check_circle.svg", 16., GREEN))
                    } else {
                        btn.opacity(0.)
                            .group_hover(group.clone(), |s| s.opacity(1.))
                            .child(svg().path("icons/add_circle.svg").size(px(16.)).text_color(c(TEXT_SUBDUED)).hover(|s| s.text_color(c(WHITE))))
                    }
                })
                .child(
                    div()
                        .ml(px(12.))
                        .w(px(45.85))
                        .flex_none()
                        .flex()
                        .justify_end()
                        .text_size(px(14.))
                        .line_height(px(19.2))
                        .text_color(c(TEXT_SUBDUED))
                        // Playlists round to the second; albums and search results drop the rest.
                        .child(fmt_time(if t.number > 0 || from_search { t.duration_ms } else { (t.duration_ms + 500) / 1000 * 1000 })),
                )
                .child(
                    div()
                        .ml(px(12.))
                        .mr(px(0.8))
                        .w(px(16.))
                        .flex_none()
                        .opacity(0.)
                        .group_hover(group, |s| s.opacity(1.))
                        .child(icon("icons/more.svg", 16., TEXT_SUBDUED)),
                ),
        )
        .into_any_element()
    }
}

impl Shell {
    /// An in-text link (artist, album, owner): underlined and white on hover, opens its page.
    pub(super) fn link(&self, id: String, text: String, uri: String, cx: &mut Context<Self>) -> Stateful<Div> {
        div()
            .id(gpui::ElementId::Name(id.into()))
            .flex_none()
            .whitespace_nowrap()
            .cursor_pointer()
            .hover(|s| s.underline().text_color(c(WHITE)))
            .on_click(cx.listener(move |this, _, _, cx| {
                cx.stop_propagation();
                if let Some(p) = Page::for_uri(&uri) {
                    this.open(p, cx);
                }
            }))
            .child(text)
    }
}

/// Cover image cropped to a non-square box (object-fit: cover), radius 4.
pub(super) fn cover_rect(url: Option<&str>, w: f32, h: f32, cx: &mut App) -> AnyElement {
    let image = url.and_then(|u| {
        let cropped = format!("crop:{w}x{h}:{u}");
        images::cache(cx).update(cx, |c, cx| c.get_sized(&cropped, w, cx))
    });
    match image {
        Some(i) => img(ImageSource::Render(i))
            .w(px(w))
            .h(px(h))
            .rounded(px(4.))
            .into_any_element(),
        None => div().w(px(w)).h(px(h)).rounded(px(4.)).bg(c(PLACEHOLDER)).into_any_element(),
    }
}

/// User avatars: the client loads the 160px rendition (`ab6775700000b8a8…`).
fn user_image_160(url: &str) -> String {
    const PREFIX: &str = "ab6775700000";
    match url.find(PREFIX) {
        Some(i) if url.len() >= i + PREFIX.len() + 4 => {
            format!("{}b8a8{}", &url[..i + PREFIX.len()], &url[i + PREFIX.len() + 4..])
        }
        _ => url.to_string(),
    }
}
