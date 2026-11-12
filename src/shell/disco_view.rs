//! An artist's full discography (`/artist/<id>/discography/all`), measured in the reference
//! (main 825 wide):
//!   a 40px header row 64 below the panel top (sticky there): the artist's name SpotifyMixUITitle
//!   24/700 at the content spacing; on the right the type filter ("All" + caret, 14px
//!   rgba(255,255,255,.7), padding 0 8, gap 8) and the sort ("Release date" 14px #b3b3b3 with the
//!   list icon, padding 4 36 4 16), ending at the content spacing.
//!   Releases from 168, each: a 136px cover (radius 4) 32 in from the spacing; 24 right of it the
//!   title 32/700 (line 38.4), 8 below "Single • 2026 • 1 song" 14px rgba(255,255,255,.7); 96 below
//!   the top a 32px white play button, then save / download / more (24px, 16 apart). 32 below the
//!   cover the album tracklist (# · Title 2fr · Plays 1fr · duration 120) and 64 below it the next.
//!   Data: `queryArtistDiscographyOverview`, `queryArtistDiscographyAll` (20 at a time, newest first)
//!   and `queryAlbumTracks` per release.

use super::browse_view::content_spacing;
use super::playlist_view::text_width;
use super::*;
use crate::playlist::Track;
use serde_json::Value;

pub struct Release {
    pub uri: String,
    pub name: String,
    pub kind: &'static str,
    pub year: String,
    pub count: u64,
    pub image: Option<String>,
    pub tracks: Option<Vec<Track>>,
}

pub struct DiscoPage {
    pub name: String,
    pub releases: Vec<Release>,
    pub total: usize,
}

fn s<'a>(v: &'a Value, p: &str) -> &'a str {
    v.pointer(p).and_then(Value::as_str).unwrap_or_default()
}

fn releases(v: &Value) -> (Vec<Release>, usize) {
    let all = v.pointer("/data/artistUnion/discography/all");
    let total = all.and_then(|a| a.get("totalCount")).and_then(Value::as_u64).unwrap_or(0) as usize;
    let items = all.and_then(|a| a.get("items")).and_then(Value::as_array);
    let out = items
        .into_iter()
        .flatten()
        .filter_map(|it| it.pointer("/releases/items/0"))
        .map(|r| Release {
            uri: s(r, "/uri").to_string(),
            name: s(r, "/name").to_string(),
            kind: match s(r, "/type") {
                "SINGLE" => "Single",
                "EP" => "EP",
                "COMPILATION" => "Compilation",
                _ => "Album",
            },
            year: r.pointer("/date/year").and_then(Value::as_u64).map(|y| y.to_string()).unwrap_or_default(),
            count: r.pointer("/tracks/totalCount").and_then(Value::as_u64).unwrap_or(0),
            image: r
                .pointer("/coverArt/sources")
                .and_then(Value::as_array)
                .and_then(|a| a.iter().find(|x| x.get("width").and_then(Value::as_u64) == Some(300)).or(a.first()))
                .map(|x| s(x, "/url").to_string()),
            tracks: None,
        })
        .collect();
    (out, total)
}

fn album_tracks(v: &Value, album_uri: &str) -> Vec<Track> {
    let items = v.pointer("/data/albumUnion/tracksV2/items").and_then(Value::as_array);
    items
        .into_iter()
        .flatten()
        .filter_map(|it| {
            let t = it.get("track")?;
            Some(Track {
                uid: s(it, "/uid").to_string(),
                uri: s(t, "/uri").to_string(),
                name: s(t, "/name").to_string(),
                artists: t
                    .pointer("/artists/items")
                    .and_then(Value::as_array)
                    .map(|a| a.iter().map(|x| (s(x, "/profile/name").to_string(), s(x, "/uri").to_string())).collect())
                    .unwrap_or_default(),
                album: String::new(),
                album_uri: album_uri.to_string(),
                image: None,
                duration_ms: t.pointer("/duration/totalMilliseconds").and_then(Value::as_u64).unwrap_or(0),
                explicit: s(t, "/contentRating/label") == "EXPLICIT",
                playable: t.pointer("/playability/playable").and_then(Value::as_bool).unwrap_or(true),
                added_at: None,
                disc: t.get("discNumber").and_then(Value::as_u64).unwrap_or(1) as u32,
                number: t.get("trackNumber").and_then(Value::as_u64).unwrap_or(0) as u32,
                plays: s(t, "/playcount").parse().ok(),
            })
        })
        .collect()
}

/// 10566 -> "10,566".
fn grouped(n: u64) -> String {
    let d = n.to_string();
    let mut out = String::new();
    for (i, ch) in d.chars().enumerate() {
        if i > 0 && (d.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

pub(super) fn plays_text(n: u64) -> String {
    grouped(n)
}

impl Shell {
    pub(super) fn load_disco(&mut self, artist: String, cx: &mut Context<Self>) {
        let Some(sp) = self.spotify.clone() else { return };
        if self.disco_pages.contains_key(&artist) || !self.loading_pages.insert(format!("disco:{artist}")) {
            return;
        }
        cx.spawn(async move |this, cx| {
            // the name, then 20 releases at a time (each page shown as it lands), each release's
            // tracks right after its page
            let (sp2, a) = (sp.clone(), artist.clone());
            let name = cx
                .background_executor()
                .spawn(async move { sp2.discography_overview(&a).ok().map(|v| s(&v, "/data/artistUnion/profile/name").to_string()) })
                .await
                .unwrap_or_default();
            let mut offset = 0;
            loop {
                let (sp2, a) = (sp.clone(), artist.clone());
                let page = cx.background_executor().spawn(async move { sp2.discography_all(&a, offset, 20).map(|v| releases(&v)) }).await;
                let (list, total) = page.unwrap_or_default();
                let uris: Vec<String> = list.iter().map(|r| r.uri.clone()).collect();
                offset += list.len();
                let n = name.clone();
                let alive = this.update(cx, |this, cx| {
                    this.loading_pages.remove(&format!("disco:{artist}"));
                    let p = this.disco_pages.entry(artist.clone()).or_insert_with(|| DiscoPage { name: n, releases: Vec::new(), total });
                    p.releases.extend(list);
                    cx.notify();
                });
                if alive.is_err() {
                    return;
                }
                for uri in uris.iter() {
                    let (sp3, u) = (sp.clone(), uri.clone());
                    let tracks = cx.background_executor().spawn(async move { sp3.album_tracks(&u).map(|v| album_tracks(&v, &u)) }).await;
                    let alive = this.update(cx, |this, cx| {
                        if let Some(r) = this.disco_pages.get_mut(&artist).and_then(|p| p.releases.iter_mut().find(|r| &r.uri == uri)) {
                            r.tracks = Some(tracks.unwrap_or_default());
                        }
                        cx.notify();
                    });
                    if alive.is_err() {
                        return;
                    }
                }
                if uris.is_empty() || offset >= total {
                    break;
                }
            }
        })
        .detach();
    }

    pub(super) fn disco_view(&self, artist: &str, main_w: f32, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let panel = div().flex_1().min_w_0().h_full().rounded(px(8.)).bg(c(SURFACE)).overflow_hidden().relative();
        let Some(page) = self.disco_pages.get(artist) else {
            return panel.into_any_element();
        };
        let w = main_w;
        let pad = content_spacing(w);
        let list_w = w - 2. * pad;
        let row_w = list_w - 1.6;
        // [index 16] [title 2fr] [plays 1fr] [duration 120], 16 apart, rows padded 16
        let space = row_w - 32. - 16. - 48. - 120.;
        let (first_w, plays_w, last_w) = (space * 2. / 3., space / 3., 120.);
        let playing_ctx = self.playback.as_ref().filter(|p| p.is_playing).and_then(|p| p.context.as_ref()).map(|c| c.uri.clone());
        let playing_track = self.playback.as_ref().and_then(|p| p.item.as_ref()?.id.clone()).map(|id| format!("spotify:track:{id}"));
        let now = crate::playlist::now_unix();
        let mut tf = gpui::font(title_font());
        tf.weight = FontWeight::BOLD;

        let mut body = div().flex().flex_col().pt(px(168.)).pb(px(16.));
        for (ri, r) in page.releases.iter().enumerate() {
            let here = playing_ctx.as_deref() == Some(r.uri.as_str());
            let saved = self.in_library(&r.uri);
            let cover = r.image.as_deref();
            let meta = format!("{} \u{2022} {} \u{2022} {} song{}", r.kind, r.year, r.count, if r.count == 1 { "" } else { "s" });
            let title_room = list_w - 64. - 136. - 24.;
            let title_fits = text_width(window, &r.name, 32., tf.clone()) <= title_room;
            let (uri_play, uri_save, uri_more, uri_open) = (r.uri.clone(), r.uri.clone(), r.uri.clone(), r.uri.clone());
            let name_more = r.name.clone();
            let head = div()
                .mx(px(pad))
                .px(px(32.))
                .pb(px(32.))
                .flex()
                .flex_row()
                .gap(px(24.))
                .child(div().flex_none().size(px(136.)).rounded(px(4.)).overflow_hidden().child(super::playlist_view::cover_rect(cover, 136., 136., cx)))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .id(gpui::ElementId::Name(format!("disco-title-{ri}").into()))
                                .font_family(title_font())
                                .text_size(px(32.))
                                .line_height(px(38.4))
                                .font_weight(FontWeight::BOLD)
                                .text_color(c(WHITE))
                                .cursor_pointer()
                                .hover(|s| s.underline())
                                .when(!title_fits, |d| d.whitespace_nowrap().overflow_hidden().text_ellipsis())
                                .on_click(cx.listener(move |this, _, _, cx| this.open(Page::Album(uri_open.clone()), cx)))
                                .child(r.name.clone()),
                        )
                        .child(div().mt(px(8.)).text_size(px(14.)).line_height(px(19.2)).text_color(rgba(0xFFFFFFB3)).child(meta))
                        .child(
                            div()
                                .mt(px(96. - 38.4 - 8. - 19.2 - 8.))
                                .h(px(48.))
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap(px(16.))
                                .child(
                                    round_button(format!("disco-play-{ri}"), 32.)
                                        .bg(c(WHITE))
                                        .hover(|s| s.bg(c(0xF0F0F0)))
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            if this.playback.as_ref().and_then(|p| p.context.as_ref()).is_some_and(|c| c.uri == uri_play) {
                                                this.toggle_play(cx);
                                            } else {
                                                this.player(Play::Context(uri_play.clone()), cx);
                                            }
                                        }))
                                        .child(icon(if here { "icons/pause.svg" } else { "icons/play.svg" }, 16., 0x000000)),
                                )
                                .child(
                                    div()
                                        .id(gpui::ElementId::Name(format!("disco-save-{ri}").into()))
                                        .size(px(24.))
                                        .cursor_pointer()
                                        .group(format!("disco-save-{ri}"))
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            let on = !this.in_library(&uri_save);
                                            this.set_hit_saved(uri_save.clone(), on, cx);
                                        }))
                                        .child(if saved {
                                            icon("icons/check_circle24.svg", 24., GREEN).into_any_element()
                                        } else {
                                            svg()
                                                .path("icons/add_circle24.svg")
                                                .size(px(24.))
                                                .text_color(c(TEXT_SUBDUED))
                                                .group_hover(format!("disco-save-{ri}"), |s| s.text_color(c(WHITE)))
                                                .into_any_element()
                                        }),
                                )
                                .child(
                                    div()
                                        .id(gpui::ElementId::Name(format!("disco-dl-{ri}").into()))
                                        .size(px(24.))
                                        .group(format!("disco-dl-{ri}"))
                                        .child(
                                            svg()
                                                .path("icons/pl_download24.svg")
                                                .size(px(24.))
                                                .text_color(c(TEXT_SUBDUED))
                                                .group_hover(format!("disco-dl-{ri}"), |s| s.text_color(c(WHITE))),
                                        ),
                                )
                                .child(
                                    div()
                                        .id(gpui::ElementId::Name(format!("disco-more-{ri}").into()))
                                        .size(px(24.))
                                        .cursor_pointer()
                                        .group(format!("disco-more-{ri}"))
                                        .on_click(cx.listener(move |this, ev: &gpui::ClickEvent, _, cx| {
                                            let target = this.target_for(&uri_more, &name_more);
                                            if let Some(t) = target {
                                                this.open_ctx_menu(t, ev.position(), false, cx);
                                            }
                                        }))
                                        .child(
                                            svg()
                                                .path("icons/pl_more24.svg")
                                                .size(px(24.))
                                                .text_color(c(TEXT_SUBDUED))
                                                .group_hover(format!("disco-more-{ri}"), |s| s.text_color(c(WHITE))),
                                        ),
                                ),
                        ),
                );
            let mut list = div().mx(px(pad)).w(px(list_w)).mb(px(64.)).border(px(0.8)).border_color(rgba(0x00000000)).flex().flex_col();
            if let Some(tracks) = &r.tracks {
                list = list
                    .child(
                        div()
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
                            .child(div().w(px(first_w)).flex_none().text_size(px(14.)).line_height(px(19.2)).child("Title"))
                            .child(
                                div()
                                    .w(px(plays_w))
                                    .flex_none()
                                    .child(div().w(px(100.)).flex().justify_end().text_size(px(14.)).line_height(px(19.2)).child("Plays")),
                            )
                            .child(
                                div()
                                    .w(px(last_w))
                                    .flex_none()
                                    .flex()
                                    .flex_row()
                                    .justify_end()
                                    .pr(px(32.))
                                    .child(icon("icons/pl_clock16.svg", 16., TEXT_SUBDUED)),
                            ),
                    )
                    .child(div().h(px(pad - 8.)))
                    .children(tracks.iter().enumerate().map(|(i, t)| {
                        self.track_row(ri * 1000 + i, t, &r.uri, first_w, &[plays_w], last_w, here && playing_track.as_deref() == Some(t.uri.as_str()), now, window, cx)
                    }));
            }
            body = body.child(head).child(list);
        }

        // header row: sticky at 64; once the list scrolls under it, the top gets the panel colour
        let scrolled = f32::from(self.page_scroll.offset().y) < 0.;
        let filter = div()
            .h(px(32.))
            .px(px(8.))
            .flex()
            .flex_row()
            .items_center()
            .gap(px(8.))
            .rounded(px(4.))
            .text_size(px(14.))
            .line_height(px(19.2))
            .text_color(rgba(0xFFFFFFB3))
            .child("All")
            .child(svg().path("icons/caret_down16.svg").size(px(16.)).text_color(rgba(0xFFFFFFB3)));
        let sort = div()
            .h(px(32.))
            .pl(px(16.))
            .pr(px(36.))
            .relative()
            .flex()
            .items_center()
            .text_size(px(14.))
            .line_height(px(19.2))
            .text_color(c(TEXT_SUBDUED))
            .child("Release date")
            .child(div().absolute().right(px(12.)).top(px(8.)).child(icon("icons/view_list16.svg", 16., TEXT_SUBDUED)));
        let artist_uri = artist.to_string();
        let header = div()
            .absolute()
            .top_0()
            .left_0()
            .right_0()
            .h(px(104.))
            .when(scrolled, |d| d.bg(c(SURFACE)))
            .child(
                div()
                    .absolute()
                    .top(px(64.))
                    .left_0()
                    .right_0()
                    .h(px(40.))
                    .px(px(pad))
                    .flex()
                    .flex_row()
                    .items_center()
                    .child(
                        div().flex_1().min_w_0().flex().child(
                            one_line(page.name.clone())
                                .id("disco-artist")
                                .font_family(title_font())
                                .text_size(px(24.))
                                .line_height(px(28.8))
                                .font_weight(FontWeight::BOLD)
                                .text_color(c(WHITE))
                                .cursor_pointer()
                                .hover(|s| s.underline())
                                .on_click(cx.listener(move |this, _, _, cx| this.open(Page::Artist(artist_uri.clone()), cx))),
                        ),
                    )
                    .child(filter)
                    .child(sort),
            );
        panel
            .child(div().id("disco-scroll").track_scroll(&self.page_scroll).absolute().inset_0().overflow_y_scroll().child(body))
            .child(header)
            .into_any_element()
    }
}
