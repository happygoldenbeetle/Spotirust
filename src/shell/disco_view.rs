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
//!   Data: `queryArtistDiscographyOverview` (name + how many albums / singles / compilations),
//!   `queryArtistDiscography{All,Albums,Singles,Compilations}` (20 at a time, order DATE_DESC /
//!   DATE_ASC / NAME_DESC / NAME_ASC) and `queryAlbumTracks` per release.
//!   Menus (#282828, 160 wide, padding 4, radius 4, the context-menu shadow), 8 below the buttons:
//!   the filter's left-aligned with it (All, then Albums / Singles and EPs / Compilations when the
//!   artist has any; the picked one green with a check); the sort's right edge 3.2 past the
//!   button's ("Sort by": Release date / Name, the picked one green with a down arrow when
//!   descending, up when ascending, picking it again flips it, a new one starts descending;
//!   "View as": List / Grid with their icons). Picking closes the menu. The filter button turns
//!   white at once on hover or while open (its caret flips up); the sort button fades to white
//!   (0.05s in / 0.15s out) and its icon is the view's. Grid: cards in columns of at least 180 from
//!   104 down, 12 inside the content spacing, subtitle "2025 • Single".

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

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DiscoFilter {
    All,
    Albums,
    Singles,
    Compilations,
}

impl DiscoFilter {
    const ALL: [DiscoFilter; 4] = [DiscoFilter::All, DiscoFilter::Albums, DiscoFilter::Singles, DiscoFilter::Compilations];
    fn label(self) -> &'static str {
        match self {
            DiscoFilter::All => "All",
            DiscoFilter::Albums => "Albums",
            DiscoFilter::Singles => "Singles and EPs",
            DiscoFilter::Compilations => "Compilations",
        }
    }
    /// The operation suffix; the response sits under its lowercase.
    fn which(self) -> &'static str {
        match self {
            DiscoFilter::All => "All",
            DiscoFilter::Albums => "Albums",
            DiscoFilter::Singles => "Singles",
            DiscoFilter::Compilations => "Compilations",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DiscoSort {
    Date,
    Name,
}

/// Sort and view, shared by every artist's discography.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DiscoPrefs {
    pub sort: DiscoSort,
    pub desc: bool,
    pub grid: bool,
}

impl Default for DiscoPrefs {
    fn default() -> Self {
        DiscoPrefs { sort: DiscoSort::Date, desc: true, grid: false }
    }
}

impl DiscoPrefs {
    fn order(self) -> &'static str {
        match (self.sort, self.desc) {
            (DiscoSort::Date, true) => "DATE_DESC",
            (DiscoSort::Date, false) => "DATE_ASC",
            (DiscoSort::Name, true) => "NAME_DESC",
            (DiscoSort::Name, false) => "NAME_ASC",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DiscoMenu {
    Filter,
    Sort,
}

pub struct DiscoPage {
    pub name: String,
    /// albums, singles + EPs, compilations
    pub counts: [usize; 3],
    pub filter: DiscoFilter,
    /// the order the releases were fetched in
    pub order: &'static str,
    pub releases: Vec<Release>,
    pub total: usize,
    /// bumped on every refetch; an older loader stops when it sees a newer one
    pub seq: u64,
}

fn s<'a>(v: &'a Value, p: &str) -> &'a str {
    v.pointer(p).and_then(Value::as_str).unwrap_or_default()
}

fn releases(v: &Value, which: &str) -> (Vec<Release>, usize) {
    let all = v.pointer(&format!("/data/artistUnion/discography/{}", which.to_lowercase()));
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
        if let Some(p) = self.disco_pages.get(&artist) {
            // cached under another sort (changed on some other artist's page): refetch
            if p.order != self.disco_prefs.order() {
                self.fetch_disco(artist, cx);
            }
            return;
        }
        if !self.loading_pages.insert(format!("disco:{artist}")) {
            return;
        }
        cx.spawn(async move |this, cx| {
            let a = artist.clone();
            let overview = cx.background_executor().spawn(async move { sp.discography_overview(&a).ok() }).await;
            let count = |k: &str| {
                overview.as_ref().and_then(|v| v.pointer(&format!("/data/artistUnion/discography/{k}/totalCount"))).and_then(Value::as_u64).unwrap_or(0) as usize
            };
            let name = overview.as_ref().map(|v| s(v, "/data/artistUnion/profile/name").to_string()).unwrap_or_default();
            let counts = [count("albums"), count("singles"), count("compilations")];
            this.update(cx, |this, cx| {
                this.loading_pages.remove(&format!("disco:{artist}"));
                let page = DiscoPage { name, counts, filter: DiscoFilter::All, order: "", releases: Vec::new(), total: 0, seq: 0 };
                this.disco_pages.insert(artist.clone(), page);
                this.fetch_disco(artist, cx);
            })
            .ok();
        })
        .detach();
    }

    /// (Re)loads the releases for the page's filter and the current sort: 20 at a time, each page
    /// shown as it lands, its releases' tracks right after it.
    fn fetch_disco(&mut self, artist: String, cx: &mut Context<Self>) {
        let Some(sp) = self.spotify.clone() else { return };
        let order = self.disco_prefs.order();
        let Some(p) = self.disco_pages.get_mut(&artist) else { return };
        p.seq += 1;
        p.order = order;
        p.releases.clear();
        p.total = 0;
        let (seq, which) = (p.seq, p.filter.which());
        cx.notify();
        cx.spawn(async move |this, cx| {
            // runs `f` on the page if this loader is still the current one
            let current = |this: &mut Shell| this.disco_pages.get_mut(&artist).filter(|p| p.seq == seq).is_some();
            let mut offset = 0;
            loop {
                let (sp2, a) = (sp.clone(), artist.clone());
                let page = cx.background_executor().spawn(async move { sp2.discography(&a, which, order, offset, 20).map(|v| releases(&v, which)) }).await;
                let (list, total) = page.unwrap_or_default();
                let uris: Vec<String> = list.iter().map(|r| r.uri.clone()).collect();
                offset += list.len();
                let ok = this
                    .update(cx, |this, cx| {
                        if !current(this) {
                            return false;
                        }
                        let p = this.disco_pages.get_mut(&artist).unwrap();
                        p.total = total;
                        p.releases.extend(list);
                        cx.notify();
                        true
                    })
                    .unwrap_or(false);
                if !ok {
                    return;
                }
                for uri in uris.iter() {
                    let (sp3, u) = (sp.clone(), uri.clone());
                    let tracks = cx.background_executor().spawn(async move { sp3.album_tracks(&u).map(|v| album_tracks(&v, &u)) }).await;
                    let ok = this
                        .update(cx, |this, cx| {
                            if !current(this) {
                                return false;
                            }
                            if let Some(r) = this.disco_pages.get_mut(&artist).and_then(|p| p.releases.iter_mut().find(|r| &r.uri == uri)) {
                                r.tracks = Some(tracks.unwrap_or_default());
                            }
                            cx.notify();
                            true
                        })
                        .unwrap_or(false);
                    if !ok {
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

    fn set_disco_filter(&mut self, artist: &str, f: DiscoFilter, cx: &mut Context<Self>) {
        self.disco_menu = None;
        if let Some(p) = self.disco_pages.get_mut(artist)
            && p.filter != f
        {
            p.filter = f;
            self.page_scroll.set_offset(gpui::point(px(0.), px(0.)));
            self.fetch_disco(artist.to_string(), cx);
        }
        cx.notify();
    }

    fn set_disco_sort(&mut self, artist: &str, sort: DiscoSort, cx: &mut Context<Self>) {
        self.disco_menu = None;
        let p = &mut self.disco_prefs;
        if p.sort == sort {
            p.desc = !p.desc;
        } else {
            p.sort = sort;
            p.desc = true;
        }
        self.page_scroll.set_offset(gpui::point(px(0.), px(0.)));
        self.fetch_disco(artist.to_string(), cx);
        cx.notify();
    }

    /// The filter / sort menu, in panel coordinates.
    fn disco_menu_el(&self, artist: &str, page: &DiscoPage, filter_left: f32, sort_right: f32, cx: &mut Context<Self>) -> Option<AnyElement> {
        let which = self.disco_menu?;
        let item = |id: String, label: &'static str, lead: Option<&'static str>, mark: Option<&'static str>| {
            let on = mark.is_some();
            div()
                .id(gpui::ElementId::Name(id.into()))
                .h(px(40.))
                .pl(px(12.))
                .pr(px(8.))
                .rounded(px(2.))
                .flex()
                .flex_row()
                .items_center()
                .gap(px(12.))
                .cursor_pointer()
                .text_size(px(14.))
                .line_height(px(19.2))
                .text_color(if on { c(GREEN) } else { rgba(0xFFFFFFE6) })
                .hover(move |st| {
                    let st = st.bg(rgba(0xFFFFFF1A));
                    if on { st } else { st.text_color(c(WHITE)) }
                })
                .when_some(lead, |d, l| d.child(svg().path(l).flex_none().size(px(16.)).text_color(if on { c(GREEN) } else { rgba(0xFFFFFFE6) })))
                .child(one_line(label).flex_1().min_w_0())
                .when_some(mark, |d, m| d.child(icon(m, 16., GREEN)))
        };
        let title = |t: &'static str| {
            div().h(px(40.8)).pt(px(12.)).pl(px(12.)).pr(px(8.)).text_size(px(12.)).line_height(px(16.8)).font_weight(FontWeight::BOLD).text_color(rgba(0xFFFFFFB3)).child(t)
        };
        let mut list = super::library_view::menu_list(true).w(px(160.)).rounded(px(4.)).absolute().top(px(108.));
        match which {
            DiscoMenu::Filter => {
                list = list.left(px(filter_left));
                for f in DiscoFilter::ALL {
                    let shown = match f {
                        DiscoFilter::All => true,
                        DiscoFilter::Albums => page.counts[0] > 0,
                        DiscoFilter::Singles => page.counts[1] > 0,
                        DiscoFilter::Compilations => page.counts[2] > 0,
                    };
                    if !shown {
                        continue;
                    }
                    let a = artist.to_string();
                    let mark = (page.filter == f).then_some("icons/check16.svg");
                    list = list.child(
                        item(format!("disco-f-{}", f.which()), f.label(), None, mark)
                            .on_click(cx.listener(move |this, _, _, cx| this.set_disco_filter(&a, f, cx))),
                    );
                }
            }
            DiscoMenu::Sort => {
                list = list.left(px(sort_right + 3.2 - 160.)).child(title("Sort by"));
                let p = self.disco_prefs;
                for (sort, label) in [(DiscoSort::Date, "Release date"), (DiscoSort::Name, "Name")] {
                    let a = artist.to_string();
                    let mark = (p.sort == sort).then_some(if p.desc { "icons/arrow_down16.svg" } else { "icons/arrow_up16.svg" });
                    list = list.child(
                        item(format!("disco-s-{label}"), label, None, mark).on_click(cx.listener(move |this, _, _, cx| this.set_disco_sort(&a, sort, cx))),
                    );
                }
                list = list.child(title("View as"));
                for (grid, label, ic) in [(false, "List", "icons/view_list16.svg"), (true, "Grid", "icons/view_grid16.svg")] {
                    let mark = (p.grid == grid).then_some("icons/check16.svg");
                    list = list.child(item(format!("disco-v-{label}"), label, Some(ic), mark).on_click(cx.listener(move |this, _, _, cx| {
                        this.disco_menu = None;
                        this.disco_prefs.grid = grid;
                        cx.notify();
                    })));
                }
            }
        }
        Some(
            list.id("disco-menu")
                .occlude()
                .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                    this.disco_menu = None;
                    this.menu_closed_at = std::time::Instant::now();
                    cx.notify();
                }))
                .into_any_element(),
        )
    }

    fn toggle_disco_menu(&mut self, m: DiscoMenu, cx: &mut Context<Self>) {
        // a click on the open menu's own button: the outside-click already closed it
        if self.disco_menu.is_none() && self.menu_closed_at.elapsed().as_millis() < 400 && self.disco_menu_last == Some(m) {
            return;
        }
        self.disco_menu = if self.disco_menu == Some(m) { None } else { Some(m) };
        self.disco_menu_last = Some(m);
        cx.notify();
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

        let grid = self.disco_prefs.grid;
        let mut body = div().flex().flex_col().pt(px(if grid { 104. } else { 168. })).pb(px(16.));
        if grid {
            // cards in columns of at least 180, 12 inside the content spacing
            let gw = list_w - 24.;
            let cols = ((gw / 180.).floor() as usize).max(1);
            let card_w = gw / cols as f32;
            let items: Vec<crate::home::Item> = page
                .releases
                .iter()
                .map(|r| crate::home::Item {
                    uri: r.uri.clone(),
                    kind: crate::home::Kind::Album,
                    name: r.name.clone(),
                    subtitle: format!("{} \u{2022} {}", r.year, r.kind),
                    image: r.image.clone(),
                    color: None,
                    hide_title: false,
                })
                .collect();
            for (ri, row) in items.chunks(cols).enumerate() {
                body = body.child(div().ml(px(pad + 12.)).flex().flex_row().items_start().children(
                    row.iter().enumerate().map(|(k, it)| self.sized_card(format!("disco-card-{}", ri * cols + k).into(), it, card_w, 4., cx)),
                ));
            }
        }
        for (ri, r) in page.releases.iter().enumerate().filter(|_| !grid) {
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
        let uf = gpui::font(ui_font());
        let filter_open = self.disco_menu == Some(DiscoMenu::Filter);
        let sort_open = self.disco_menu == Some(DiscoMenu::Sort);
        let filter_label = page.filter.label();
        let sort_label = match self.disco_prefs.sort {
            DiscoSort::Date => "Release date",
            DiscoSort::Name => "Name",
        };
        // where the buttons sit (for the menus): sort ends at the content spacing, filter before it
        let sort_right = w - pad;
        let sort_w = 16. + text_width(window, sort_label, 14., uf.clone()) + 36.;
        let filter_w = 8. + text_width(window, filter_label, 14., uf.clone()) + 8. + 16. + 8.;
        let filter_left = sort_right - sort_w - filter_w;
        let filter = div()
            .id("disco-filter")
            .group("disco-filter")
            .h(px(32.))
            .px(px(8.))
            .flex()
            .flex_row()
            .items_center()
            .gap(px(8.))
            .rounded(px(4.))
            .cursor_pointer()
            .text_size(px(14.))
            .line_height(px(19.2))
            .text_color(if filter_open { c(WHITE) } else { rgba(0xFFFFFFB3) })
            .hover(|s| s.text_color(c(WHITE)))
            .on_click(cx.listener(|this, _, _, cx| this.toggle_disco_menu(DiscoMenu::Filter, cx)))
            .child(filter_label)
            .child(
                svg()
                    .path("icons/caret_down16.svg")
                    .size(px(16.))
                    .text_color(if filter_open { c(WHITE) } else { rgba(0xFFFFFFB3) })
                    .group_hover("disco-filter", |s| s.text_color(c(WHITE)))
                    .when(filter_open, |s| s.with_transformation(gpui::Transformation::rotate(gpui::radians(std::f32::consts::PI)))),
            );
        let k = if sort_open { 1. } else { self.fades.get("disco-sort") };
        let sort_col = fx::mix(0xB3B3B3FF, 0xFFFFFFFF, k);
        let sort = self.on_fade(
            div()
                .id("disco-sort")
                .h(px(32.))
                .pl(px(16.))
                .pr(px(36.))
                .relative()
                .flex()
                .items_center()
                .cursor_pointer()
                .text_size(px(14.))
                .line_height(px(19.2))
                .text_color(sort_col)
                .on_click(cx.listener(|this, _, _, cx| this.toggle_disco_menu(DiscoMenu::Sort, cx)))
                .child(sort_label)
                .child(
                    div().absolute().right(px(12.)).top(px(8.)).child(
                        fx::scaled_icon(if self.disco_prefs.grid { "icons/view_grid16.svg" } else { "icons/view_list16.svg" }, 16., 1. + 0.04 * k)
                            .text_color(sort_col),
                    ),
                ),
            "disco-sort",
            fx::TERTIARY,
            cx,
        );
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
        let menu = self.disco_menu_el(artist, page, filter_left, sort_right, cx);
        panel
            .child(div().id("disco-scroll").track_scroll(&self.page_scroll).absolute().inset_0().overflow_y_scroll().child(body))
            .child(header)
            .children(menu)
            .into_any_element()
    }
}
