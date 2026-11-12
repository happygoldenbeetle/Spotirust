//! The Now Playing view below the cover (and the Lyrics preview), measured from the reference
//! (aside 349 wide, cards 317):
//!
//! About the artist: #1f1f1f card, radius 8, 28.8 below the artists line (16 below the Lyrics
//!   preview). Image strip 211.3 tall (first gallery image, cover-fit) under a top gradient
//!   rgba(0,0,0,.5) -> 0 at 50%, "About the artist" 16/700 white at (16,16). Below it, padding 16
//!   (24 at the bottom): name 16/700 white with the 16px #c5f0c9 verified badge 4 after it;
//!   "N monthly listeners" 16px #b3b3b3 and a Follow pill (32 tall, padding 4/16, 14/700, 0.8px
//!   #7c7c7c border) 8 below; the bio 14px #b3b3b3 (line 19.2), 3 lines, 12 below. The whole card
//!   opens the About dialog.
//! Credits: #1f1f1f section, radius 8, padding 16, 16 below. Header row 32 tall: "Credits" 16/700,
//!   "Show all" 14/700 #b3b3b3 (the Credits dialog). The first three credits (merged by name),
//!   12 below, 8 padding each: name 16px white, roles 14px #b3b3b3, Follow for the track's artists.
//! Next in queue: same section, "Open queue" 14/700 #b3b3b3; the queue row 8 below.

use super::*;
use crate::artist::{Contributor, NpvInfo};
use gpui::{ObjectFit, StyledImage};
use super::playlist_view::text_width;

const CARD: u32 = 0x1F1F1F;

/// Credits with the same name joined (roles in order); the uri is kept only for the track's
/// own artists, who get a Follow button (the client's rule).
#[derive(Clone)]
pub struct Credit {
    pub name: String,
    pub roles: Vec<String>,
    pub uri: Option<String>,
    /// Songwriter page: the row opens it in the browser and shows the external-link icon.
    pub url: Option<String>,
}

pub fn merge_credits(list: &[&Contributor], main: &[String], keep_uri: bool) -> Vec<Credit> {
    let mut out: Vec<Credit> = Vec::new();
    for c in list {
        if c.name.is_empty() {
            continue;
        }
        let uri = c.uri.clone().filter(|u| keep_uri && main.contains(u));
        if let Some(e) = out.iter_mut().find(|e| e.name == c.name) {
            if !c.role.is_empty() && !e.roles.contains(&c.role) {
                e.roles.push(c.role.clone());
            }
            if e.uri.is_none() {
                e.uri = uri;
            }
            if e.url.is_none() {
                e.url = c.url.clone();
            }
        } else {
            out.push(Credit {
                name: c.name.clone(),
                roles: if c.role.is_empty() { Vec::new() } else { vec![c.role.clone()] },
                uri,
                url: c.url.clone(),
            });
        }
    }
    out
}

/// The Credits dialog's groups ("Artist", "Composition & Lyrics", ...) in first-seen order;
/// only the first group keeps Follow buttons.
pub fn credit_groups(list: &[Contributor], main: &[String]) -> Vec<(String, Vec<Credit>)> {
    let mut groups: Vec<(String, Vec<&Contributor>)> = Vec::new();
    for c in list {
        let g = if c.group.is_empty() { "Other".to_string() } else { c.group.clone() };
        match groups.iter_mut().find(|x| x.0 == g) {
            Some(x) => x.1.push(c),
            None => groups.push((g, vec![c])),
        }
    }
    groups.into_iter().enumerate().map(|(i, (g, l))| (g, merge_credits(&l, main, i == 0))).collect()
}

/// One line of `text` the way CSS line-clamp: 1 shows it: whole words while they fit in `avail`,
/// then "…" ("Main Artist • Composer •…"). A first word that alone is too wide is cut by letters.
pub(super) fn clamp_words(window: &Window, text: &str, avail: f32, size: f32) -> String {
    let font = gpui::font(ui_font());
    let width = |s: &str| text_width(window, s, size, font.clone());
    if width(text) <= avail {
        return text.to_string();
    }
    let mut fit = String::new();
    for word in text.split(' ') {
        let next = if fit.is_empty() { word.to_string() } else { format!("{fit} {word}") };
        if width(&next) > avail {
            break;
        }
        fit = next;
    }
    if fit.is_empty() {
        let mut cut = String::new();
        for ch in text.chars() {
            let mut next = cut.clone();
            next.push(ch);
            if width(&format!("{next}\u{2026}")) > avail {
                break;
            }
            cut = next;
        }
        return format!("{cut}\u{2026}");
    }
    while !fit.is_empty() && width(&format!("{fit}\u{2026}")) > avail {
        fit.pop();
    }
    format!("{}\u{2026}", fit.trim_end())
}

impl Shell {
    /// Fetches About the artist + Credits for the playing track (once per track).
    pub(super) fn load_npv(&mut self, track: String, artist: String, cx: &mut Context<Self>) {
        if track.is_empty() || artist.is_empty() {
            return;
        }
        if self.npv.as_ref().is_some_and(|n| n.track == track) || self.npv_loading.as_deref() == Some(track.as_str()) {
            return;
        }
        let Some(sp) = self.spotify.clone() else { return };
        self.npv_loading = Some(track.clone());
        cx.spawn(async move |this, cx| {
            let t = track.clone();
            // queryNpvArtist only carries the first 10 credit lines: the full list comes from the
            // Credits dialog's query, and the card's images from the artist's visual identity.
            let res = cx
                .background_executor()
                .spawn(async move {
                    let mut info = ar::parse_npv(&sp.npv_artist(&artist, &t)?, &t).ok_or_else(|| anyhow::anyhow!("no artist"))?;
                    match sp.track_credits(&t) {
                        Ok(v) => {
                            if let Some(tu) = v.pointer("/data/trackUnion") {
                                (info.contributors, info.sources) = ar::parse_credits(tu);
                            }
                        }
                        Err(e) => eprintln!("track credits: {e:#}"),
                    }
                    match sp.visual_identity(&artist) {
                        Ok(vi) => (info.photo, info.avatar) = (vi.photo, vi.avatar),
                        Err(e) => eprintln!("visual identity: {e:#}"),
                    }
                    anyhow::Ok(info)
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.npv_loading.as_deref() != Some(track.as_str()) {
                    return;
                }
                this.npv_loading = None;
                match res {
                    Ok(info) => this.npv = Some(info),
                    Err(e) => eprintln!("npv artist: {e:#}"),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// The playing track's artist uris (credits keep Follow buttons for these).
    pub(super) fn playing_artist_uris(&self) -> Vec<String> {
        self.playback.as_ref().and_then(|p| p.item.as_ref()).map(|t| t.artists.iter().map(|a| a.uri.clone()).collect()).unwrap_or_default()
    }

    /// Outline pill: "Follow" / "Following" for an artist (the library follow state).
    pub(super) fn follow_pill(&self, id: String, uri: String, cx: &mut Context<Self>) -> Stateful<Div> {
        let following = self.in_library(&uri);
        div()
            .id(gpui::ElementId::Name(id.into()))
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
            .on_click(cx.listener(move |this, _, _, cx| {
                cx.stop_propagation();
                if let Some(p) = this.artist_pages.get_mut(&uri) {
                    p.saved = !following;
                }
                this.set_hit_saved(uri.clone(), !following, cx);
            }))
            .child(if following { "Following" } else { "Follow" })
    }

    /// Header of a Now Playing section: title at the left, a subdued text button at the right.
    fn npv_section_head(&self, title: &'static str, id: &'static str, action: &'static str, on: impl Fn(&mut Self, &mut Context<Self>) + 'static, cx: &mut Context<Self>) -> Div {
        div()
            .h(px(32.))
            .flex()
            .flex_row()
            .items_center()
            .child(div().flex_1().text_size(px(16.)).line_height(px(22.4)).font_weight(FontWeight::BOLD).text_color(c(WHITE)).child(title))
            .child(
                div()
                    .id(id)
                    .h(px(32.))
                    .flex()
                    .items_center()
                    .cursor_pointer()
                    .text_size(px(14.))
                    .line_height(px(19.2))
                    .font_weight(FontWeight::BOLD)
                    .text_color(c(TEXT_SUBDUED))
                    .hover(|s| s.text_color(c(WHITE)).underline())
                    .on_click(cx.listener(move |this, _, _, cx| on(this, cx)))
                    .child(action),
            )
    }

    /// About the artist, Credits and Next in queue (each only when there's something to show).
    pub(super) fn npv_sections(&self, inner_w: f32, after_lyrics: bool, window: &Window, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let mut out = Vec::new();
        let playing = self.playback.as_ref().and_then(|p| p.item.as_ref()).map(|t| t.uri.clone()).unwrap_or_default();
        let info: Option<NpvInfo> = self.npv.clone().filter(|n| n.track == playing);
        let mut gap = if after_lyrics { 16. } else { 28.8 };
        if let Some(info) = info.as_ref() {
            // The client drops the card for artists without a biography.
            if !info.about.bio.trim().is_empty() {
                out.push(self.npv_about_card(info, inner_w, gap, cx));
                gap = 16.;
            }
            let main = self.playing_artist_uris();
            let all: Vec<&Contributor> = info.contributors.iter().collect();
            let credits = merge_credits(&all, &main, true);
            if !credits.is_empty() {
                let mut sec = div()
                    .mt(px(gap))
                    .flex_none()
                    .rounded(px(8.))
                    .bg(c(CARD))
                    .p(px(16.))
                    .flex()
                    .flex_col()
                    .child(self.npv_section_head("Credits", "npv-credits-all", "Show all", |this, cx| this.open_credits(cx), cx));
                let mut rows = div().mt(px(12.)).flex().flex_col();
                for (i, cr) in credits.iter().take(3).enumerate() {
                    rows = rows.child(self.credit_row(format!("npv-credit-{i}"), cr, inner_w - 32., window, cx));
                }
                sec = sec.child(rows);
                out.push(sec.into_any_element());
                gap = 16.;
            }
            if let Some(k) = info.concerts.first() {
                out.push(self.npv_tour(k, gap, inner_w, window, cx));
                gap = 16.;
            }
        }
        // Next in queue: the queue's next track (the built-in player lists its context instead).
        let next = self.queue.as_ref().and_then(|q| if q.local { self.context_queue().1.into_iter().next() } else { q.next.first().cloned() });
        if let Some(t) = next {
            out.push(
                div()
                    .mt(px(gap))
                    .flex_none()
                    .rounded(px(8.))
                    .bg(c(CARD))
                    .p(px(16.))
                    .flex()
                    .flex_col()
                    .child(self.npv_section_head("Next in queue", "npv-open-queue", "Open queue", |this, cx| this.toggle_right(RightPanel::Queue, cx), cx))
                    .child(div().mt(px(8.)).mx(px(-16.)).child(self.queue_row("npv-next".into(), &t, false, cx)))
                    .into_any_element(),
            );
        }
        out
    }

    /// A credit: name over its roles (" • "-joined, one line). The track's own artists get a
    /// Follow button and open their page; songwriters with a page get the external-link icon and
    /// open it in the browser; anyone else is plain text.
    pub(super) fn credit_row(&self, id: String, cr: &Credit, row_w: f32, window: &Window, cx: &mut Context<Self>) -> Stateful<Div> {
        // Room for the roles: the row less the Follow pill (or the external icon) and the gap.
        let following = cr.uri.as_ref().is_some_and(|u| self.in_library(u));
        let trail = if cr.uri.is_some() {
            text_width(window, if following { "Following" } else { "Follow" }, 14., gpui::font(ui_font()).bold()) + 30.4 + 1.6 + 12.
        } else if cr.url.is_some() {
            24. + 12.
        } else {
            0.
        };
        let roles = clamp_words(window, &cr.roles.join(" \u{2022} "), row_w - trail, 14.);
        let group = SharedString::from(id.clone());
        let name = one_line(cr.name.clone()).text_size(px(16.)).line_height(px(22.4)).text_color(c(WHITE));
        let (uri, url) = (cr.uri.clone(), cr.url.clone());
        let clickable = uri.is_some() || url.is_some();
        div()
            .id(gpui::ElementId::Name(id.clone().into()))
            .group(group.clone())
            .py(px(8.))
            .flex()
            .flex_row()
            .items_center()
            .gap(px(8.))
            .when(clickable, |d| d.cursor_pointer())
            .on_click(cx.listener(move |this, _, _, cx| {
                if let Some(uri) = uri.clone() {
                    this.credits_dialog = None;
                    this.open(Page::Artist(uri), cx);
                } else if let Some(url) = url.clone() {
                    cx.open_url(&url);
                }
            }))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(if cr.uri.is_some() { name.group_hover(group.clone(), |s| s.underline()) } else { name })
                    // One line clamped between words, like the client ("Main Artist • Composer •…").
                    .when(!roles.is_empty(), |d| d.child(div().mt(px(2.)).whitespace_nowrap().text_size(px(14.)).line_height(px(19.2)).text_color(c(TEXT_SUBDUED)).child(roles))),
            )
            .when_some(cr.uri.clone(), |d, uri| d.child(self.follow_pill(format!("{id}-follow"), uri, cx)))
            .when(cr.uri.is_none() && cr.url.is_some(), |d| {
                d.child(svg().path("icons/external24.svg").size(px(24.)).flex_none().text_color(c(TEXT_SUBDUED)).group_hover(group.clone(), |s| s.text_color(c(WHITE))))
            })
    }

    /// About the artist. Three looks, picked like the client (`NPVArtistAboutV2`): with a photo
    /// (the visual identity's wide / 16:9 / square full-bleed image) it fills a 211.3 strip under
    /// the title; else the square cover as an 80px circle 17 under the title; else a placeholder
    /// circle. The whole card opens the About dialog.
    fn npv_about_card(&self, info: &NpvInfo, inner_w: f32, gap: f32, cx: &mut Context<Self>) -> AnyElement {
        let a = &info.about;
        let title = || div().text_size(px(16.)).line_height(px(22.4)).font_weight(FontWeight::BOLD).text_color(c(WHITE)).child("About the artist");
        let header = match (info.photo.clone(), info.avatar.clone()) {
            (Some(photo), _) => {
                let img_h = 211.3;
                let url = format!("crop:{inner_w:.1}x{img_h:.1}:{photo}");
                let image = images::cache(cx).update(cx, |c, cx| c.get_sized(&url, inner_w, cx));
                div()
                    .relative()
                    .w_full()
                    .h(px(img_h))
                    .bg(c(0x282828))
                    .when_some(image, |d, i| d.child(img(ImageSource::Render(i)).absolute().inset_0().size_full()))
                    .child(div().absolute().inset_0().bg(linear_gradient(180., linear_color_stop(rgba(0x00000080), 0.), linear_color_stop(rgba(0x00000000), 0.5))))
                    .child(title().absolute().top(px(16.)).left(px(16.)))
            }
            (None, avatar) => {
                let image = avatar.and_then(|u| images::cache(cx).update(cx, |c, cx| c.get_sized(&u, 80., cx)));
                div()
                    .h(px(140.))
                    .px(px(16.))
                    .pt(px(16.))
                    .flex()
                    .flex_col()
                    .child(title())
                    .child(
                        div()
                            .mt(px(16.6))
                            .size(px(80.))
                            .rounded_full()
                            .overflow_hidden()
                            .bg(c(0x282828))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(match image {
                                Some(i) => img(ImageSource::Render(i)).size_full().rounded_full().object_fit(ObjectFit::Cover).into_any_element(),
                                None => icon("icons/ctx_go_to_artist.svg", 32., TEXT_SUBDUED).into_any_element(),
                            }),
                    )
            }
        };
        let bio = a.bio_preview();
        let uri = a.uri.clone();
        div()
            .id("npv-about")
            .mt(px(gap))
            .flex_none()
            .rounded(px(8.))
            .overflow_hidden()
            .bg(c(CARD))
            .cursor_pointer()
            .flex()
            .flex_col()
            .on_click(cx.listener(move |this, _, _, cx| this.open_about_for(&uri, cx)))
            .child(header)
            .child(
                div()
                    .p(px(16.))
                    .pb(px(24.))
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(4.))
                            .child(one_line(a.name.clone()).text_size(px(16.)).line_height(px(22.4)).font_weight(FontWeight::BOLD).text_color(c(WHITE)))
                            .when(a.verified, |d| d.child(icon("icons/verified.svg", 16., 0xC5F0C9))),
                    )
                    .child(
                        // The listeners line wraps (to two lines beside the pill for big numbers).
                        div()
                            .mt(px(8.))
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(8.))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .text_size(px(16.))
                                    .line_height(px(22.4))
                                    .text_color(c(TEXT_SUBDUED))
                                    .when(a.monthly_listeners > 0, |d| d.child(format!("{} monthly listeners", ar::grouped(a.monthly_listeners)))),
                            )
                            .child(self.follow_pill("npv-about-follow".into(), a.uri.clone(), cx)),
                    )
                    .when(!bio.is_empty(), |d| {
                        d.child(div().mt(px(12.)).text_size(px(14.)).line_height(px(19.2)).text_color(c(TEXT_SUBDUED)).line_clamp(3).text_ellipsis().child(bio))
                    }),
            )
            .into_any_element()
    }

    /// On tour: the artist's next concert. #1f1f1f card, padding 16/16/8, "On tour" 16/700; the
    /// row (65 tall, a #3f3f3f highlight reaching 8 past it on hover) has a 56px #121212 date
    /// tile (month 11/700, day 24/700) and, 16 after it, city 16px white, artist 14px #b3b3b3 and
    /// "Sat 8:30 PM • Venue" 14px white .7, each one line.
    fn npv_tour(&self, k: &ar::Concert, gap: f32, inner_w: f32, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let (month, day, time) = super::artist_view::concert_date(&k.start);
        let _ = cx;
        div()
            .mt(px(gap))
            .flex_none()
            .rounded(px(8.))
            .bg(c(CARD))
            .pt(px(16.))
            .px(px(16.))
            .pb(px(8.))
            .flex()
            .flex_col()
            .child(div().text_size(px(16.)).line_height(px(22.4)).font_weight(FontWeight::BOLD).text_color(c(WHITE)).child("On tour"))
            .child(
                div()
                    .id("npv-tour-0")
                    .mt(px(8.))
                    .mx(px(-8.))
                    .p(px(8.))
                    .rounded(px(4.))
                    .hover(|s| s.bg(c(0x3F3F3F)))
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(16.))
                    .child(
                        div()
                            .size(px(56.))
                            .flex_none()
                            .rounded(px(4.))
                            .bg(c(0x121212))
                            .flex()
                            .flex_col()
                            .items_center()
                            .justify_center()
                            .font_weight(FontWeight::BOLD)
                            .text_color(c(WHITE))
                            .child(div().text_size(px(11.)).line_height(px(15.)).child(month))
                            .child(div().mt(px(2.)).text_size(px(24.)).line_height(px(24.)).child(day)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .h(px(65.))
                            .flex()
                            .flex_col()
                            .child(one_line(k.city.clone()).text_size(px(16.)).line_height(px(22.4)).text_color(c(WHITE)))
                            .child(one_line(k.title.clone()).mt(px(4.6)).text_size(px(14.)).line_height(px(19.2)).text_color(c(TEXT_SUBDUED)))
                            .child(
                                div()
                                    .whitespace_nowrap()
                                    .text_size(px(14.))
                                    .line_height(px(19.2))
                                    .text_color(rgba(0xFFFFFFB3))
                                    .child(clamp_words(window, &format!("{time} \u{2022} {}", k.venue), inner_w - 32. - 56. - 16., 14.)),
                            ),
                    ),
            )
            .into_any_element()
    }
}
