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

const CARD: u32 = 0x1F1F1F;

/// Credits with the same name joined (roles in order); the uri is kept only for the track's
/// own artists, who get a Follow button (the client's rule).
#[derive(Clone)]
pub struct Credit {
    pub name: String,
    pub roles: Vec<String>,
    pub uri: Option<String>,
}

pub fn merge_credits(list: &[&Contributor], main: &[String], keep_uri: bool) -> Vec<Credit> {
    let mut out: Vec<Credit> = Vec::new();
    for c in list {
        if c.name.is_empty() {
            continue;
        }
        let uri = c.uri.clone().filter(|u| keep_uri && main.contains(u));
        if let Some(e) = out.iter_mut().find(|e| e.name == c.name) {
            if !c.role.is_empty() {
                e.roles.push(c.role.clone());
                if e.uri.is_none() {
                    e.uri = uri;
                }
            }
        } else {
            out.push(Credit { name: c.name.clone(), roles: if c.role.is_empty() { Vec::new() } else { vec![c.role.clone()] }, uri });
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
            let res = cx.background_executor().spawn(async move { sp.npv_artist(&artist, &t) }).await;
            let _ = this.update(cx, |this, cx| {
                if this.npv_loading.as_deref() != Some(track.as_str()) {
                    return;
                }
                this.npv_loading = None;
                match res {
                    Ok(v) => this.npv = ar::parse_npv(&v, &track),
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
    pub(super) fn npv_sections(&self, inner_w: f32, after_lyrics: bool, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let mut out = Vec::new();
        let playing = self.playback.as_ref().and_then(|p| p.item.as_ref()).map(|t| t.uri.clone()).unwrap_or_default();
        let info: Option<NpvInfo> = self.npv.clone().filter(|n| n.track == playing);
        let mut gap = if after_lyrics { 16. } else { 28.8 };
        if let Some(info) = info.as_ref() {
            out.push(self.npv_about_card(info, inner_w, gap, cx));
            gap = 16.;
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
                    rows = rows.child(self.credit_row(format!("npv-credit-{i}"), cr, cx));
                }
                sec = sec.child(rows);
                out.push(sec.into_any_element());
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

    /// A credit: name (a link for artists) over its roles, Follow for the track's artists.
    pub(super) fn credit_row(&self, id: String, cr: &Credit, cx: &mut Context<Self>) -> Div {
        let roles = cr.roles.join(", ");
        let name = one_line(cr.name.clone()).text_size(px(16.)).line_height(px(22.4)).text_color(c(WHITE));
        let name = match cr.uri.clone() {
            Some(uri) => div()
                .flex()
                .child(
                    name.id(gpui::ElementId::Name(format!("{id}-name").into()))
                        .cursor_pointer()
                        .hover(|s| s.underline())
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.credits_dialog = None;
                            this.open(Page::Artist(uri.clone()), cx);
                        })),
                )
                .into_any_element(),
            None => name.into_any_element(),
        };
        div()
            .py(px(8.))
            .flex()
            .flex_row()
            .items_center()
            .gap(px(8.))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(name)
                    .when(!roles.is_empty(), |d| d.child(one_line(roles).text_size(px(14.)).line_height(px(19.2)).text_color(c(TEXT_SUBDUED)))),
            )
            .when_some(cr.uri.clone(), |d, uri| d.child(self.follow_pill(format!("{id}-follow"), uri, cx)))
    }

    fn npv_about_card(&self, info: &NpvInfo, inner_w: f32, gap: f32, cx: &mut Context<Self>) -> AnyElement {
        let a = &info.about;
        let img_h = 211.3;
        let image = a.gallery.first().and_then(|u| {
            let url = format!("crop:{inner_w:.1}x{img_h:.1}:{u}");
            images::cache(cx).update(cx, |c, cx| c.get_sized(&url, inner_w, cx))
        });
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
            .when(!a.gallery.is_empty(), |d| {
                d.child(
                    div()
                        .relative()
                        .w_full()
                        .h(px(img_h))
                        .bg(c(0x282828))
                        .when_some(image, |d, i| d.child(img(ImageSource::Render(i)).absolute().inset_0().size_full()))
                        .child(div().absolute().inset_0().bg(linear_gradient(
                            180.,
                            linear_color_stop(rgba(0x00000080), 0.),
                            linear_color_stop(rgba(0x00000000), 0.5),
                        )))
                        .child(
                            div()
                                .absolute()
                                .top(px(16.))
                                .left(px(16.))
                                .text_size(px(16.))
                                .line_height(px(22.4))
                                .font_weight(FontWeight::BOLD)
                                .text_color(c(WHITE))
                                .child("About the artist"),
                        ),
                )
            })
            .child(
                div()
                    .p(px(16.))
                    .pb(px(24.))
                    .flex()
                    .flex_col()
                    .when(a.gallery.is_empty(), |d| {
                        d.child(
                            div()
                                .mb(px(16.))
                                .text_size(px(16.))
                                .line_height(px(22.4))
                                .font_weight(FontWeight::BOLD)
                                .text_color(c(WHITE))
                                .child("About the artist"),
                        )
                    })
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
                        div()
                            .mt(px(8.))
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(8.))
                            .child(
                                one_line(format!("{} monthly listeners", ar::grouped(a.monthly_listeners)))
                                    .flex_1()
                                    .min_w_0()
                                    .text_size(px(16.))
                                    .line_height(px(22.4))
                                    .text_color(c(TEXT_SUBDUED)),
                            )
                            .child(self.follow_pill("npv-about-follow".into(), a.uri.clone(), cx)),
                    )
                    .when(!bio.is_empty(), |d| {
                        d.child(div().mt(px(12.)).text_size(px(14.)).line_height(px(19.2)).text_color(c(TEXT_SUBDUED)).line_clamp(3).text_ellipsis().child(bio))
                    }),
            )
            .into_any_element()
    }
}
