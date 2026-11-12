//! The artist About dialog and the track Credits dialog, measured from the reference:
//!
//! Backdrop rgba(0,0,0,.7) over the whole window; clicking it (or Escape) closes the dialog.
//! About: 768 wide, 70% of the window tall, centred, #121212, radius 8, all of it scrolls.
//!   Gallery strip 768x432 (images contain-fit), Next / Previous 32px round buttons
//!   (rgba(0,0,0,.7), arrow rgba(255,255,255,.7)) 48 from the sides at y 200; Close the same,
//!   16 from the top right. Body padding 40: a 179.9 column of stats (32/700 numbers, line 38.4,
//!   "Followers" / "Monthly Listeners" 14px #b3b3b3, 40 apart; then the top cities 14/700 white over
//!   "N listeners" 14px #b3b3b3, 16 apart; "Report" 14/700 #b3b3b3 48 below), and the biography
//!   (16px #b3b3b3, line 22.4, links white, paragraphs 16 apart). Below it the "Verified by Spotify"
//!   card (#282828, radius 6, padding 16, 32 below) and "Registered artist" (#1f1f1f, radius 8).
//! Credits: 440 wide, content tall, centred, #121212, radius 8. Header padding 24 with a 0.8px
//!   rgba(255,255,255,.1) rule: "Credits" 24/700 title font, the track name 16/700, Close 32px.
//!   Body padding 24: role groups (20/700, line 24, 16 below) of credit rows; "Sources" 16/700 over
//!   the labels 14px #b3b3b3; "Report error" 14/700 #b3b3b3 with the external icon.

use super::npv_sections::{credit_groups, Credit};
use super::*;
use crate::artist::About;
use gpui::{HighlightStyle, StyledImage, UnderlineStyle};

const VERIFIED_URL: &str = "https://support.spotify.com/article/verified-by-spotify/";
const REPORT_URL: &str = "https://support.spotify.com/article/report-content/";
const CREDITS_REPORT_URL: &str = "https://support.spotify.com/artists/article/song-credits/";

pub struct AboutDialog {
    pub about: About,
    /// Gallery image shown.
    pub index: usize,
    /// (paragraph, link) under the pointer: underlined.
    pub hover: Option<(usize, usize)>,
}

pub struct CreditsDialog {
    pub track_name: String,
    pub groups: Vec<(String, Vec<Credit>)>,
    pub sources: Vec<String>,
}

impl Shell {
    /// Opens the About dialog for `artist` from whatever has its data (the Now Playing view's
    /// query or the artist page).
    pub(super) fn open_about_for(&mut self, artist: &str, cx: &mut Context<Self>) {
        let about = self
            .npv
            .as_ref()
            .map(|n| &n.about)
            .filter(|a| a.uri == artist)
            .or_else(|| self.artist_pages.get(artist).map(|p| &p.about))
            .cloned();
        // With nothing cached it opens empty and fills in.
        let about = about.unwrap_or_else(|| About { uri: artist.to_string(), ..Default::default() });
        self.about_scroll.set_offset(gpui::point(px(0.), px(0.)));
        self.about_dialog = Some(AboutDialog { about, index: 0, hover: None });
        cx.notify();
        // Like the client, the dialog loads its own query (world rank, social links), and
        // fills in once it lands.
        let Some(sp) = self.spotify.clone() else { return };
        let uri = artist.to_string();
        cx.spawn(async move |this, cx| {
            let u = uri.clone();
            let res = cx.background_executor().spawn(async move { sp.artist_about(&u) }).await;
            let _ = this.update(cx, |this, cx| {
                let v = match res {
                    Ok(v) => v,
                    Err(e) => return eprintln!("artist about: {e:#}"),
                };
                let Some(a) = v.pointer("/data/artistUnion") else { return };
                let mut fresh = ar::parse_about(a);
                fresh.uri = uri.clone();
                match this.about_dialog.as_mut() {
                    Some(d) if d.about.uri == uri => {
                        if fresh.name.is_empty() {
                            fresh.name = d.about.name.clone();
                        }
                        if fresh.gallery.is_empty() {
                            fresh.gallery = d.about.gallery.clone();
                        }
                        d.index = d.index.min(fresh.gallery.len().saturating_sub(1));
                        d.about = fresh;
                    }
                    // Closed (or showing another artist) meanwhile.
                    _ => return,
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Opens the playing track's Credits dialog (Now Playing view "Show all").
    pub(super) fn open_credits(&mut self, cx: &mut Context<Self>) {
        let playing = self.playback.as_ref().and_then(|p| p.item.as_ref());
        let (Some(t), Some(info)) = (playing, self.npv.as_ref()) else { return };
        if info.track != t.uri {
            return;
        }
        let groups = credit_groups(&info.contributors, &self.playing_artist_uris());
        self.credits_dialog = Some(CreditsDialog { track_name: t.name.clone(), groups, sources: info.sources.clone() });
        cx.notify();
    }

    fn dialog_close_button(&self, id: &'static str, on: impl Fn(&mut Self, &mut Context<Self>) + 'static, cx: &mut Context<Self>) -> Stateful<Div> {
        round_button(id, 32.)
            .group(id)
            .on_click(cx.listener(move |this, _, _, cx| on(this, cx)))
            .child(svg().path("icons/close16.svg").size(px(16.)).text_color(c(TEXT_SUBDUED)).group_hover(id, |s| s.text_color(c(WHITE))))
    }

    /// The dimmed backdrop both dialogs sit on; a press on it closes them.
    fn dialog_backdrop(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        div()
            .id("dialog-backdrop")
            .absolute()
            .inset_0()
            .bg(rgba(0x000000B3))
            .occlude()
            .flex()
            .items_center()
            .justify_center()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    this.about_dialog = None;
                    this.credits_dialog = None;
                    cx.notify();
                }),
            )
    }

    pub(super) fn about_dialog_view(&self, window: &Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        if let Some(d) = self.credits_dialog.as_ref() {
            return Some(self.credits_dialog_el(d, window, cx));
        }
        let d = self.about_dialog.as_ref()?;
        let a = &d.about;
        let vh = f32::from(window.viewport_size().height);
        let w = 768.;
        let gallery_h = 432.;
        let shown = a.gallery.get(d.index).cloned();
        let image = shown.and_then(|u| images::cache(cx).update(cx, |c, cx| c.get_sized(&u, w, cx)));
        let count = a.gallery.len();
        let arrow = |id: &'static str, path: &'static str, step: isize, cx: &mut Context<Self>| {
            round_button(id, 32.)
                .group(id)
                .absolute()
                .top(px(200.))
                .bg(rgba(0x000000B3))
                .on_click(cx.listener(move |this, _, _, cx| {
                    if let Some(d) = this.about_dialog.as_mut() {
                        let n = d.about.gallery.len() as isize;
                        d.index = (d.index as isize + step).clamp(0, (n - 1).max(0)) as usize;
                        cx.notify();
                    }
                }))
                .child(svg().path(path).size(px(24.)).text_color(rgba(0xFFFFFFB3)).group_hover(id, |s| s.text_color(c(WHITE))))
        };
        let strip = div()
            .relative()
            .w(px(w))
            .h(px(gallery_h))
            .flex_none()
            .bg(c(0x000000))
            .when_some(image, |d, i| d.child(img(ImageSource::Render(i)).absolute().inset_0().size_full().object_fit(gpui::ObjectFit::Contain)))
            .when(d.index > 0, |s| s.child(arrow("about-prev", "icons/chevron_left.svg", -1, cx).left(px(48.))))
            .when(d.index + 1 < count, |s| s.child(arrow("about-next", "icons/chevron_right.svg", 1, cx).right(px(48.))));

        // stats column
        let stat = |n: u64, label: &'static str| {
            div()
                .flex()
                .flex_col()
                .child(div().text_size(px(32.)).line_height(px(38.4)).font_weight(FontWeight::BOLD).font_family(title_font()).text_color(c(WHITE)).child(ar::grouped(n)))
                .child(div().text_size(px(14.)).line_height(px(19.2)).text_color(c(TEXT_SUBDUED)).child(label))
        };
        // Column widths as the client's flexbox gives them: the bio column is 509 wide, the stats
        // column as wide as its widest number plus 40, both shrinking (by size) into the 688 row.
        let num_font = gpui::font(title_font()).bold();
        let widest = [a.followers, a.monthly_listeners]
            .iter()
            .filter(|n| **n > 0)
            .map(|n| super::playlist_view::text_width(window, &ar::grouped(*n), 32., num_font.clone()) + 40.)
            .fold(80_f32, f32::max);
        let (stats_w, bio_w) = if widest + 509. > 688. { (widest * 688. / (widest + 509.), 509. * 688. / (widest + 509.)) } else { (widest, 509.) };
        let mut stats = div().w(px(stats_w)).flex_none().flex().flex_col();
        if a.world_rank > 0 {
            // "#24 in the world": an 80px #0d72ea disc, 40 above the numbers.
            stats = stats.child(
                div()
                    .mb(px(40.))
                    .size(px(80.))
                    .rounded_full()
                    .bg(c(0x0D72EA))
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .text_color(c(WHITE))
                    .child(div().text_size(px(24.)).line_height(px(28.8)).font_weight(FontWeight::BOLD).font_family(title_font()).child(format!("#{}", a.world_rank)))
                    .child(div().mt(px(-4.)).text_size(px(12.)).line_height(px(16.8)).child("in the world")),
            );
        }
        stats = stats.child(stat(a.followers, "Followers"));
        if a.monthly_listeners > 0 {
            stats = stats.child(stat(a.monthly_listeners, "Monthly Listeners").mt(px(40.)));
        }
        let mut cities = div().mt(px(40.)).flex().flex_col().gap(px(16.));
        for (city, n) in a.cities.iter() {
            cities = cities.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(1.6))
                    .child(div().text_size(px(14.)).line_height(px(19.2)).font_weight(FontWeight::BOLD).text_color(c(WHITE)).child(city.clone()))
                    .child(div().text_size(px(14.)).line_height(px(19.2)).text_color(c(TEXT_SUBDUED)).child(format!("{} listeners", ar::grouped(*n)))),
            );
        }
        if !a.cities.is_empty() {
            stats = stats.child(cities);
        }
        // Social links: 24px icon + 4 + 14px label, white (the client's computed color), 16 apart.
        if !a.links.is_empty() {
            let mut links = div().mt(px(24.)).pt(px(16.)).flex().flex_col().gap(px(16.));
            for (i, (name, url)) in a.links.iter().enumerate() {
                let id = SharedString::from(format!("about-link-{i}"));
                let path = match name.as_str() {
                    "Instagram" => "icons/instagram24.svg",
                    "X" => "icons/x24.svg",
                    _ => "icons/external24.svg",
                };
                let url = url.clone();
                links = links.child(
                    div()
                        .id(gpui::ElementId::Name(id.clone()))
                        .group(id.clone())
                        .h(px(24.))
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(4.))
                        .cursor_pointer()
                        .text_size(px(14.))
                        .line_height(px(19.2))
                        .text_color(c(WHITE))
                        .on_click(move |_, _, cx| cx.open_url(&url))
                        .child(svg().path(path).size(px(24.)).flex_none().text_color(c(WHITE)))
                        .child(name.clone()),
                );
            }
            stats = stats.child(links);
        }
        stats = stats.child(
            div().mt(px(if a.links.is_empty() { 48. } else { 32. })).flex().child(
                div()
                    .id("about-report")
                    .group("about-report")
                    .h(px(32.))
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(8.))
                    .cursor_pointer()
                    .text_size(px(14.))
                    .line_height(px(19.2))
                    .font_weight(FontWeight::BOLD)
                    .text_color(c(TEXT_SUBDUED))
                    .hover(|s| s.text_color(c(WHITE)).underline())
                    .on_click(|_, _, cx| cx.open_url(REPORT_URL))
                    .child("Report")
                    .child(svg().path("icons/external16.svg").size(px(16.)).text_color(c(TEXT_SUBDUED)).group_hover("about-report", |s| s.text_color(c(WHITE)))),
            ),
        );

        // biography: paragraphs with their artist links
        let entity = cx.entity().downgrade();
        let mut bio = div().mt(px(-4.)).w(px(bio_w)).flex_none().flex().flex_col().gap(px(16.));
        for (pi, (text, links)) in ar::bio_paragraphs(&a.bio).into_iter().enumerate() {
            let highlights: Vec<(std::ops::Range<usize>, HighlightStyle)> = links
                .iter()
                .enumerate()
                .map(|(li, (r, _))| {
                    let hovered = d.hover == Some((pi, li));
                    (
                        r.clone(),
                        HighlightStyle {
                            color: Some(c(WHITE).into()),
                            underline: hovered.then(|| UnderlineStyle { thickness: px(1.), color: Some(c(WHITE).into()), wavy: false }),
                            ..Default::default()
                        },
                    )
                })
                .collect();
            let ranges: Vec<std::ops::Range<usize>> = links.iter().map(|(r, _)| r.clone()).collect();
            let uris: Vec<String> = links.iter().map(|(_, u)| u.clone()).collect();
            let hover_ranges = ranges.clone();
            let click_entity = entity.clone();
            let hover_entity = entity.clone();
            bio = bio.child(
                div().text_size(px(16.)).line_height(px(22.4)).text_color(c(TEXT_SUBDUED)).child(
                    gpui::InteractiveText::new(gpui::ElementId::Name(format!("about-p{pi}").into()), gpui::StyledText::new(text).with_highlights(highlights))
                        .on_click(ranges, move |li, _, cx| {
                            let uri = uris[li].clone();
                            let _ = click_entity.update(cx, |this, cx| {
                                if let Some(p) = Page::for_uri(&uri) {
                                    this.about_dialog = None;
                                    this.open(p, cx);
                                }
                            });
                        })
                        .on_hover(move |ix, _, _, cx| {
                            let link = ix.and_then(|ix| hover_ranges.iter().position(|r| r.contains(&ix))).map(|li| (pi, li));
                            let _ = hover_entity.update(cx, |this, cx| {
                                if let Some(d) = this.about_dialog.as_mut() {
                                    let new = link.or(d.hover.filter(|h| h.0 != pi));
                                    if d.hover != new {
                                        d.hover = new;
                                        cx.notify();
                                    }
                                }
                            });
                        }),
                ),
            );
        }
        if a.verified {
            bio = bio.child(
                div()
                    .mt(px(16.))
                    .rounded(px(6.))
                    .bg(c(0x282828))
                    .p(px(16.))
                    .flex()
                    .flex_col()
                    .gap(px(6.4))
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(8.))
                            .child(icon("icons/verified24.svg", 24., 0xC5F0C9))
                            .child(div().text_size(px(14.)).line_height(px(19.2)).font_weight(FontWeight::BOLD).text_color(c(WHITE)).child("Verified by Spotify")),
                    )
                    .child(
                        div()
                            .text_size(px(14.))
                            .line_height(px(19.2))
                            .text_color(c(TEXT_SUBDUED))
                            .child(
                                gpui::InteractiveText::new(
                                    "about-verified-text",
                                    gpui::StyledText::new(
                                        "This artist has grown an active fanbase to be eligible for review and has met Spotify\u{2019}s criteria for profile authenticity. Learn more",
                                    )
                                    .with_highlights([(
                                        125..135,
                                        HighlightStyle {
                                            underline: Some(UnderlineStyle { thickness: px(1.), color: Some(c(TEXT_SUBDUED).into()), wavy: false }),
                                            ..Default::default()
                                        },
                                    )]),
                                )
                                .on_click(vec![125..135], |_, _, cx| cx.open_url(VERIFIED_URL)),
                            ),
                    ),
            );
        }
        if a.registered {
            bio = bio.child(
                div()
                    .rounded(px(8.))
                    .bg(c(0x1F1F1F))
                    .p(px(16.))
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(div().text_size(px(16.)).line_height(px(22.4)).font_weight(FontWeight::BOLD).text_color(c(WHITE)).child("Registered artist"))
                    .child(div().text_size(px(14.)).line_height(px(19.2)).text_color(c(TEXT_SUBDUED)).child("This artist has a claimed profile in Spotify for Artists.")),
            );
        }

        let dialog = div()
            .id("about-dialog")
            .relative()
            .w(px(w))
            .h(px((vh * 0.7).round()))
            .rounded(px(8.))
            .bg(c(SURFACE))
            .overflow_hidden()
            .occlude()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .id("about-scroll")
                    .track_scroll(&self.about_scroll)
                    .size_full()
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .child(strip)
                    .child(div().p(px(40.)).flex().flex_row().items_start().child(stats).child(bio)),
            )
            .child(
                self.dialog_close_button(
                    "about-close",
                    |this, cx| {
                        this.about_dialog = None;
                        cx.notify();
                    },
                    cx,
                )
                .absolute()
                .top(px(16.))
                .right(px(16.))
                .bg(rgba(0x000000B3)),
            );
        Some(self.dialog_backdrop(cx).child(dialog).into_any_element())
    }

    fn credits_dialog_el(&self, d: &CreditsDialog, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let mut body = div().p(px(24.)).flex().flex_col();
        for (gi, (group, credits)) in d.groups.iter().enumerate() {
            let mut g = div()
                .when(gi > 0, |d| d.mt(px(24.)))
                .flex()
                .flex_col()
                .child(div().pb(px(16.)).text_size(px(20.)).line_height(px(24.)).font_weight(FontWeight::BOLD).text_color(c(WHITE)).child(group.clone()));
            for (ci, cr) in credits.iter().enumerate() {
                g = g.child(self.credit_row(format!("cred-{gi}-{ci}"), cr, 392., window, cx));
            }
            body = body.child(g);
        }
        if !d.sources.is_empty() {
            body = body.child(
                div()
                    .mt(px(24.))
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .child(div().text_size(px(16.)).line_height(px(22.4)).font_weight(FontWeight::BOLD).text_color(c(WHITE)).child("Sources"))
                    .children(d.sources.iter().map(|s| div().text_size(px(14.)).line_height(px(19.2)).text_color(c(TEXT_SUBDUED)).child(s.clone()))),
            );
        }
        body = body.child(
            div().mt(px(12.)).flex().child(
                div()
                    .id("credits-report")
                    .group("credits-report")
                    .h(px(32.))
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(8.))
                    .cursor_pointer()
                    .text_size(px(14.))
                    .line_height(px(19.2))
                    .font_weight(FontWeight::BOLD)
                    .text_color(c(TEXT_SUBDUED))
                    .hover(|s| s.text_color(c(WHITE)))
                    .on_click(|_, _, cx| cx.open_url(CREDITS_REPORT_URL))
                    .child("Report error")
                    .child(svg().path("icons/external16.svg").size(px(16.)).text_color(c(TEXT_SUBDUED)).group_hover("credits-report", |s| s.text_color(c(WHITE)))),
            ),
        );
        let dialog = div()
            .id("credits-dialog")
            .w(px(440.))
            .min_h(px(354.))
            .max_h(px(542.))
            .rounded(px(8.))
            .bg(c(SURFACE))
            .overflow_hidden()
            .occlude()
            .flex()
            .flex_col()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .flex_none()
                    .p(px(24.))
                    .border_b(px(0.8))
                    .border_color(rgba(0xFFFFFF1A))
                    .flex()
                    .flex_row()
                    .items_start()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .pb(px(4.))
                                    .font_family(title_font())
                                    .text_size(px(24.))
                                    .line_height(px(28.8))
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(c(WHITE))
                                    .child("Credits"),
                            )
                            .child(one_line(d.track_name.clone()).text_size(px(16.)).line_height(px(22.4)).font_weight(FontWeight::BOLD).text_color(c(WHITE))),
                    )
                    .child(self.dialog_close_button(
                        "credits-close",
                        |this, cx| {
                            this.credits_dialog = None;
                            cx.notify();
                        },
                        cx,
                    )),
            )
            .child(div().id("credits-scroll").flex_1().min_h_0().overflow_y_scroll().child(body));
        self.dialog_backdrop(cx).child(dialog).into_any_element()
    }
}
