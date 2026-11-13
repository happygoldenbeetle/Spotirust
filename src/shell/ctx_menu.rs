//! Right-click context menus (tracks, playlists, albums, artists, Your Library). Measured on the
//! reference: #282828, radius 4, padding 4, the sort-menu shadow; width = content (min 160);
//! items 40 tall (radius 2, padding 8 8 8 12): a 16px icon, text 12 after it (14px
//! rgba(255,255,255,.9)), hover rgba(255,255,255,.1) + white; submenu items end in a 16px
//! right-pointing triangle. Dividers 1px rgba(255,255,255,.1). The menu opens 8 below the
//! pointer, flips above it when it doesn't fit, else shifts to 10 from the window edge.
//! Submenus open on hover, 4 over the menu's right edge, level with their item.

use super::playlist_view::text_width;
use super::*;
use gpui::MouseDownEvent;

const MIN_W: f32 = 160.;
const ITEM_H: f32 = 40.;

/// What was right-clicked.
#[derive(Clone)]
pub enum Target {
    Track {
        uri: String,
        name: String,
        album_uri: String,
        artists: Vec<(String, String)>,
        /// The playlist the row is in, when you can edit it ("Remove from this playlist").
        editable_playlist: Option<String>,
        /// Row on an album page: "Hide in this album", no "Go to album".
        on_album: bool,
    },
    Playlist { uri: String, name: String, owned: bool },
    Album { uri: String, name: String },
    Artist { uri: String, name: String },
    LikedSongs,
    /// Empty space in Your Library.
    LibraryBackground,
}

#[derive(Clone)]
enum Icon {
    Svg(&'static str),
    /// The Liked Songs cover ("Add to Liked Songs") or its green check (saved).
    Liked(bool),
    None,
}

#[derive(Clone, PartialEq)]
enum Sub {
    Share,
    AddToPlaylist,
    Artists,
    Sleep,
    Folder,
}

#[derive(Clone)]
enum Act {
    /// Not something this client can do yet (Spotify doesn't expose it): just closes.
    Nothing,
    Queue(String),
    SetLiked(String, bool),
    Go(Page),
    Radio(String),
    Library(String, bool),
    Public(String, bool),
    RemoveFromPlaylist(String, String),
    CreatePlaylist,
    CopyLink(String),
    OpenUrl(String),
    AddTo(String, String),
    NewPlaylistWith(String),
    Sleep(Option<u64>),
}

#[derive(Clone)]
enum Row {
    Item { label: String, icon: Icon, sub: Option<Sub>, external: bool, dim: bool, act: Act },
    Divider,
}

fn item(label: impl Into<String>, icon: &'static str, act: Act) -> Row {
    Row::Item { label: label.into(), icon: Icon::Svg(icon), sub: None, external: false, dim: false, act }
}

fn sub(label: impl Into<String>, icon: &'static str, s: Sub) -> Row {
    Row::Item { label: label.into(), icon: Icon::Svg(icon), sub: Some(s), external: false, dim: false, act: Act::Nothing }
}

fn plain(label: impl Into<String>, act: Act) -> Row {
    Row::Item { label: label.into(), icon: Icon::None, sub: None, external: false, dim: false, act }
}

fn report() -> Row {
    Row::Item {
        label: "Report".into(),
        icon: Icon::None,
        sub: None,
        external: true,
        dim: true,
        act: Act::OpenUrl("https://support.spotify.com/article/report-content/".into()),
    }
}

fn web_link(uri: &str) -> String {
    let mut parts = uri.split(':').skip(1);
    match (parts.next(), parts.next()) {
        (Some(kind), Some(id)) => format!("https://open.spotify.com/{kind}/{id}"),
        _ => "https://open.spotify.com/".into(),
    }
}

pub struct CtxMenu {
    pub pos: gpui::Point<Pixels>,
    pub target: Target,
    /// Opened from Your Library (adds Create playlist / folder for playlists).
    pub from_library: bool,
    /// Open submenu: the item index and its top.
    pub sub: Option<(usize, f32)>,
    /// Liked Songs state of a track target, once looked up.
    pub liked: Option<bool>,
}

impl Shell {
    pub(super) fn open_ctx_menu(&mut self, target: Target, pos: gpui::Point<Pixels>, from_library: bool, cx: &mut Context<Self>) {
        self.lib_menu = false;
        self.create_open = false;
        self.profile_open = false;
        if let (Target::Track { uri, .. }, Some(api)) = (&target, self.api.clone()) {
            let id = uri.rsplit(':').next().unwrap_or_default().to_string();
            let uri = uri.clone();
            cx.spawn(async move |this, cx| {
                let res = cx.background_executor().spawn(async move { api.is_saved(&id) }).await;
                let _ = this.update(cx, |this, cx| {
                    if let (Ok(saved), Some(m)) = (res, this.ctx_menu.as_mut()) {
                        if matches!(&m.target, Target::Track { uri: u, .. } if *u == uri) {
                            m.liked = Some(saved);
                            cx.notify();
                        }
                    }
                });
            })
            .detach();
        }
        self.ctx_menu = Some(CtxMenu { pos, target, from_library, sub: None, liked: None });
        cx.notify();
    }

    pub(super) fn in_library(&self, uri: &str) -> bool {
        if let Some(&on) = self.saved_override.get(uri) {
            return on;
        }
        self.library_items().iter().any(|i| i.uri == uri)
    }

    fn ctx_rows(&self, m: &CtxMenu) -> Vec<Row> {
        let mut rows = Vec::new();
        match &m.target {
            Target::Track { uri, album_uri, artists, editable_playlist, on_album, .. } => {
                rows.push(sub("Add to playlist", "icons/ctx_add_to_playlist.svg", Sub::AddToPlaylist));
                if let Some(pl) = editable_playlist {
                    rows.push(item("Remove from this playlist", "icons/ctx_remove_from_this_playlist.svg", Act::RemoveFromPlaylist(pl.clone(), uri.clone())));
                }
                let liked = m.liked.unwrap_or(false);
                rows.push(Row::Item {
                    label: if liked { "Remove from your Liked Songs" } else { "Add to Liked Songs" }.into(),
                    icon: Icon::Liked(liked),
                    sub: None,
                    external: false,
                    dim: false,
                    act: Act::SetLiked(uri.clone(), !liked),
                });
                rows.push(item("Add to queue", "icons/ctx_add_to_queue.svg", Act::Queue(uri.clone())));
                if *on_album {
                    rows.push(item("Hide in this album", "icons/ctx_hide_in_this_album.svg", Act::Nothing));
                }
                rows.push(item("Exclude from your taste profile", "icons/ctx_exclude_from_your_taste_profile.svg", Act::Nothing));
                rows.push(item("Start a Jam", "icons/ctx_start_a_jam.svg", Act::Nothing));
                rows.push(sub("Sleep timer", "icons/ctx_sleep_timer.svg", Sub::Sleep));
                rows.push(Row::Divider);
                rows.push(item("Go to song radio", "icons/ctx_go_to_song_radio.svg", Act::Radio(uri.clone())));
                if artists.len() > 1 {
                    rows.push(sub("Go to artist", "icons/ctx_go_to_artist.svg", Sub::Artists));
                } else if let Some((_, a)) = artists.first() {
                    rows.push(item("Go to artist", "icons/ctx_go_to_artist.svg", Act::Go(Page::Artist(a.clone()))));
                }
                if !*on_album && !album_uri.is_empty() {
                    rows.push(item("Go to album", "icons/ctx_go_to_album.svg", Act::Go(Page::Album(album_uri.clone()))));
                }
                rows.push(item("View credits", "icons/ctx_view_credits.svg", Act::Nothing));
                rows.push(sub("Share", "icons/ctx_share.svg", Sub::Share));
            }
            Target::Playlist { uri, owned, .. } => {
                if *owned {
                    rows.push(item("Add to queue", "icons/ctx_add_to_queue.svg", Act::Queue(uri.clone())));
                    rows.push(item("Start a Jam", "icons/ctx_start_a_jam.svg", Act::Nothing));
                    rows.push(item("Edit details", "icons/ctx_edit_details.svg", Act::Go(Page::Playlist(uri.clone()))));
                    rows.push(item("Delete", "icons/ctx_delete.svg", Act::Nothing));
                    rows.push(item("Download", "icons/ctx_download.svg", Act::Nothing));
                    if m.from_library {
                        rows.push(Row::Divider);
                        rows.push(item("Create playlist", "icons/ctx_create_playlist.svg", Act::CreatePlaylist));
                        rows.push(item("Create folder", "icons/ctx_create_folder.svg", Act::Nothing));
                        rows.push(Row::Divider);
                    }
                    let public = self.playlist_pages.get(uri).map(|p| p.label == "Public Playlist").unwrap_or(false);
                    rows.push(item(
                        if public { "Make private" } else { "Make public" },
                        "icons/ctx_make_public.svg",
                        Act::Public(uri.clone(), !public),
                    ));
                    rows.push(item("Invite collaborators", "icons/ctx_invite_collaborators.svg", Act::Nothing));
                    rows.push(item("Exclude from your taste profile", "icons/ctx_exclude_from_your_taste_profile.svg", Act::Nothing));
                    rows.push(sub("Move to folder", "icons/ctx_move_to_folder.svg", Sub::Folder));
                    rows.push(sub("Add to other playlist", "icons/ctx_add_to_other_playlist.svg", Sub::AddToPlaylist));
                    let pinned = self.library_items().iter().any(|i| i.uri == *uri && i.pinned);
                    rows.push(item(if pinned { "Unpin playlist" } else { "Pin playlist" }, "icons/ctx_unpin_playlist.svg", Act::Nothing));
                    rows.push(sub("Share", "icons/ctx_share.svg", Sub::Share));
                } else {
                    let saved = self.in_library(uri);
                    rows.push(item(
                        if saved { "Remove from Your Library" } else { "Add to Your Library" },
                        "icons/ctx_add_to_your_library.svg",
                        Act::Library(uri.clone(), !saved),
                    ));
                    rows.push(item("Add to queue", "icons/ctx_add_to_queue.svg", Act::Queue(uri.clone())));
                    rows.push(item("Start a Jam", "icons/ctx_start_a_jam.svg", Act::Nothing));
                    rows.push(report());
                    rows.push(item("Download", "icons/ctx_download.svg", Act::Nothing));
                    rows.push(item("Exclude from your taste profile", "icons/ctx_exclude_from_your_taste_profile.svg", Act::Nothing));
                    rows.push(sub("Add to folder", "icons/ctx_add_to_folder.svg", Sub::Folder));
                    rows.push(sub("Add to other playlist", "icons/ctx_add_to_other_playlist.svg", Sub::AddToPlaylist));
                    rows.push(sub("Share", "icons/ctx_share.svg", Sub::Share));
                }
            }
            Target::Album { uri, .. } => {
                let saved = self.in_library(uri);
                rows.push(item(
                    if saved { "Remove from Your Library" } else { "Add to Your Library" },
                    "icons/ctx_add_to_your_library.svg",
                    Act::Library(uri.clone(), !saved),
                ));
                rows.push(item("Add to queue", "icons/ctx_add_to_queue.svg", Act::Queue(uri.clone())));
                rows.push(sub("Add to playlist", "icons/ctx_add_to_playlist.svg", Sub::AddToPlaylist));
                rows.push(item("Download", "icons/ctx_download.svg", Act::Nothing));
                rows.push(sub("Share", "icons/ctx_share.svg", Sub::Share));
            }
            Target::Artist { uri, .. } => {
                let following = self.in_library(uri);
                rows.push(item(if following { "Unfollow" } else { "Follow" }, "icons/ctx_follow.svg", Act::Library(uri.clone(), !following)));
                rows.push(item("Don't play this artist", "icons/ctx_don_t_play_this_artist.svg", Act::Nothing));
                rows.push(item("Go to artist radio", "icons/ctx_go_to_artist_radio.svg", Act::Radio(uri.clone())));
                rows.push(report());
                rows.push(sub("Share", "icons/ctx_share.svg", Sub::Share));
            }
            Target::LikedSongs => {
                rows.push(item("Download", "icons/ctx_download.svg", Act::Nothing));
                rows.push(item("Unpin playlist", "icons/ctx_unpin_playlist.svg", Act::Nothing));
            }
            Target::LibraryBackground => {
                rows.push(item("Create playlist", "icons/ctx_create_playlist.svg", Act::CreatePlaylist));
                rows.push(item("Create folder", "icons/ctx_create_folder.svg", Act::Nothing));
            }
        }
        rows
    }

    fn sub_rows(&self, m: &CtxMenu, s: &Sub) -> Vec<Row> {
        let uri = match &m.target {
            Target::Track { uri, .. } | Target::Playlist { uri, .. } | Target::Album { uri, .. } | Target::Artist { uri, .. } => uri.clone(),
            _ => String::new(),
        };
        match s {
            Sub::Share => {
                let (copy, embed) = match uri.split(':').nth(1) {
                    Some("track") => ("Copy link to Song", Some("Embed track")),
                    Some("playlist") => ("Copy link to playlist", Some("Embed playlist")),
                    Some("album") => ("Copy link to album", Some("Embed album")),
                    Some("artist") => ("Copy link to artist", Some("Embed artist")),
                    _ => ("Copy link", None),
                };
                let mut rows = vec![item(copy, "icons/ctx_copy_link.svg", Act::CopyLink(web_link(&uri)))];
                if let Some(e) = embed {
                    let mut parts = uri.split(':').skip(1);
                    let url = format!("https://open.spotify.com/embed/{}/{}", parts.next().unwrap_or_default(), parts.next().unwrap_or_default());
                    rows.push(item(e, "icons/ctx_embed.svg", Act::OpenUrl(url)));
                }
                rows
            }
            Sub::AddToPlaylist => {
                let me_name = self.me.as_ref().and_then(|m| m.display_name.clone()).unwrap_or_default();
                let mut rows = vec![item("New playlist", "icons/ctx_add_to_playlist.svg", Act::NewPlaylistWith(uri.clone())), Row::Divider];
                for p in self.library_items().iter().filter(|i| i.kind == crate::library::LibKind::Playlist && i.owner == me_name && i.uri != uri) {
                    rows.push(plain(p.name.clone(), Act::AddTo(p.uri.clone(), uri.clone())));
                }
                rows
            }
            Sub::Artists => match &m.target {
                Target::Track { artists, .. } => artists.iter().map(|(name, a)| plain(name.clone(), Act::Go(Page::Artist(a.clone())))).collect(),
                _ => Vec::new(),
            },
            Sub::Sleep => [5, 10, 15, 30, 45]
                .iter()
                .map(|n| plain(format!("{n} minutes"), Act::Sleep(Some(*n))))
                .chain([plain("1 hour", Act::Sleep(Some(60))), plain("End of track", Act::Sleep(None))])
                .collect(),
            Sub::Folder => vec![plain("Create folder", Act::Nothing)],
        }
    }

    fn rows_width(&self, rows: &[Row], window: &Window) -> f32 {
        let font = gpui::font(ui_font());
        rows.iter()
            .map(|r| match r {
                Row::Item { label, icon, sub, external, .. } => {
                    let icon_w = if matches!(icon, Icon::None) { 0. } else { 28. };
                    let trail = if sub.is_some() || *external { 16. + 16. } else { 0. };
                    12. + icon_w + text_width(window, label, 14., font.clone()) + trail + 8. + 8.
                }
                Row::Divider => 0.,
            })
            .fold(MIN_W, f32::max)
            .min(350.)
    }

    fn rows_height(rows: &[Row]) -> f32 {
        8. + rows.iter().map(|r| if matches!(r, Row::Divider) { 1. } else { ITEM_H }).sum::<f32>()
    }

    pub(super) fn ctx_menu_view(&self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let m = self.ctx_menu.as_ref()?;
        let vp = window.viewport_size();
        let (vw, vh) = (f32::from(vp.width), f32::from(vp.height));
        let rows = self.ctx_rows(m);
        let w = self.rows_width(&rows, window);
        let h = Self::rows_height(&rows);
        let (px_, py) = (f32::from(m.pos.x), f32::from(m.pos.y));
        let x = if px_ + w > vw - 10. { (px_ - w).max(10.) } else { px_ };
        let y = if py + 8. + h <= vh - 10. {
            py + 8.
        } else if py - 8. - h >= 10. {
            py - 8. - h
        } else {
            (vh - 10. - h).max(10.)
        };

        let menu = self.rows_list(&rows, w, None, cx);
        // the open submenu and where it goes
        let mut sub_el = None;
        let mut sub_rect: Option<(f32, f32, f32, f32)> = None;
        if let Some((i, top_in_menu)) = m.sub {
            if let Some(Row::Item { sub: Some(s), .. }) = rows.get(i) {
                let srows = self.sub_rows(m, s);
                let playlists = *s == Sub::AddToPlaylist;
                let sw = if playlists { 273.2 } else { self.rows_width(&srows, window) };
                let full_h = Self::rows_height(&srows) + if playlists { 40. } else { 0. };
                let sh = if playlists { full_h.min(320.) } else { full_h };
                let sx = if x + w - 4. + sw > vw - 10. { x - sw + 4. } else { x + w - 4. };
                let sy = (y + top_in_menu).min(vh - 10. - sh).max(10.);
                sub_rect = Some((sx, sy, sw, sh));
                sub_el = Some(
                    div()
                        .absolute()
                        .left(px(sx))
                        .top(px(sy))
                        .child(self.rows_list(&srows, sw, Some((playlists, sh)), cx)),
                );
            }
        }
        // A press outside both menus closes them (and still reaches what's under the pointer).
        let main_rect = (x, y, w, h);
        let inside = move |p: gpui::Point<Pixels>, r: (f32, f32, f32, f32)| {
            let (px_, py) = (f32::from(p.x), f32::from(p.y));
            px_ >= r.0 && px_ < r.0 + r.2 && py >= r.1 && py < r.1 + r.3
        };
        let root = div()
            .absolute()
            .left(px(x))
            .top(px(y))
            .on_mouse_down_out(cx.listener(move |this, ev: &MouseDownEvent, _, cx| {
                if inside(ev.position, main_rect) || sub_rect.is_some_and(|r| inside(ev.position, r)) {
                    return;
                }
                if this.ctx_menu.as_ref().is_some_and(|m| m.pos != ev.position) || ev.button == MouseButton::Left {
                    this.close_ctx(cx);
                }
            }))
            .child(menu);
        Some(div().absolute().top_0().left_0().child(root).children(sub_el).into_any_element())
    }

    fn close_ctx(&mut self, cx: &mut Context<Self>) {
        self.ctx_menu = None;
        cx.notify();
    }

    /// A menu box. `sub`: this is a submenu (playlists: with the search field, scrolling, `h` tall).
    fn rows_list(&self, rows: &[Row], w: f32, sub: Option<(bool, f32)>, cx: &mut Context<Self>) -> AnyElement {
        let is_sub = sub.is_some();
        let mut list = div()
            .id(if is_sub { "ctx-sub" } else { "ctx-main" })
            .w(px(w))
            .p(px(4.))
            .rounded(px(4.))
            .bg(c(0x282828))
            .shadow(vec![
                gpui::BoxShadow { color: gpui::hsla(0., 0., 0., 0.3), offset: gpui::point(px(0.), px(16.)), blur_radius: px(24.), spread_radius: px(0.) },
                gpui::BoxShadow { color: gpui::hsla(0., 0., 0., 0.2), offset: gpui::point(px(0.), px(6.)), blur_radius: px(8.), spread_radius: px(0.) },
            ])
            .flex()
            .flex_col()
            .occlude();
        if let Some((true, h)) = sub {
            list = list.h(px(h)).overflow_y_scroll().child(
                // "Find a playlist" (filtering not wired yet)
                div()
                    .flex_none()
                    .h(px(40.))
                    .p(px(2.))
                    .child(
                        div()
                            .relative()
                            .h(px(32.))
                            .rounded(px(4.))
                            .bg(rgba(0xFFFFFF1A))
                            .pl(px(32.))
                            .flex()
                            .items_center()
                            .text_size(px(14.))
                            .text_color(rgba(0xFFFFFFB3))
                            .child(div().absolute().left(px(8.)).top(px(8.)).child(svg().path("icons/pl_search16.svg").size(px(16.)).text_color(rgba(0xFFFFFFB3))))
                            .child("Find a playlist"),
                    ),
            );
        }
        let mut top = 4. + if matches!(sub, Some((true, _))) { 40. } else { 0. };
        for (i, row) in rows.iter().enumerate() {
            match row {
                Row::Divider => {
                    list = list.child(div().flex_none().h(px(1.)).bg(rgba(0xFFFFFF1A)));
                    top += 1.;
                }
                Row::Item { label, icon, sub: has_sub, external, dim, act } => {
                    let item_top = top;
                    top += ITEM_H;
                    let act = act.clone();
                    let opens_sub = has_sub.is_some();
                    let open = !is_sub && self.ctx_menu.as_ref().and_then(|m| m.sub).is_some_and(|(s, _)| s == i);
                    let color = if *dim { c(0x949494) } else { rgba(0xFFFFFFE6) };
                    let icon_el = match icon {
                        Icon::Svg(p) => Some(svg().path(*p).size(px(16.)).flex_none().text_color(color).into_any_element()),
                        Icon::Liked(false) => Some(cover(Some(LIKED_SONGS_IMAGE), 16., 2., cx)),
                        Icon::Liked(true) => Some(svg().path("icons/check_circle.svg").size(px(16.)).flex_none().text_color(c(GREEN)).into_any_element()),
                        Icon::None => None,
                    };
                    list = list.child(
                        div()
                            .id(gpui::ElementId::Name(format!("ctx-{}-{i}", if is_sub { "s" } else { "m" }).into()))
                            .h(px(ITEM_H))
                            .flex_none()
                            .pl(px(12.))
                            .pr(px(8.))
                            .rounded(px(2.))
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(12.))
                            .cursor_pointer()
                            .text_size(px(14.))
                            .whitespace_nowrap()
                            .text_color(color)
                            .when(open, |d| d.bg(rgba(0xFFFFFF1A)).text_color(c(WHITE)))
                            .when(!*dim, |d| d.hover(|s| s.bg(rgba(0xFFFFFF1A)).text_color(c(WHITE))))
                            .when(!is_sub, |d| {
                                d.on_hover(cx.listener(move |this, h: &bool, _, cx| {
                                    if !*h {
                                        return;
                                    }
                                    if let Some(m) = this.ctx_menu.as_mut() {
                                        let new = if opens_sub { Some((i, item_top - 4.)) } else { None };
                                        if m.sub != new {
                                            m.sub = new;
                                            cx.notify();
                                        }
                                    }
                                }))
                            })
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if opens_sub {
                                    // clicking a submenu row opens it too (keyboard / touch path)
                                    if let Some(m) = this.ctx_menu.as_mut() {
                                        m.sub = Some((i, item_top - 4.));
                                        cx.notify();
                                    }
                                    return;
                                }
                                this.run_ctx_action(act.clone(), cx);
                            }))
                            .children(icon_el)
                            .child(div().flex_1().min_w_0().overflow_hidden().child(label.clone()))
                            .when(opens_sub, |d| {
                                d.child(
                                    svg()
                                        .path("icons/ctx_arrow.svg")
                                        .size(px(16.))
                                        .flex_none()
                                        .with_transformation(gpui::Transformation::rotate(gpui::radians(std::f32::consts::FRAC_PI_2)))
                                        .text_color(color),
                                )
                            })
                            .when(*external, |d| d.child(svg().path("icons/external16.svg").size(px(16.)).flex_none().text_color(color))),
                    );
                }
            }
        }
        list.into_any_element()
    }

    /// Creates a playlist called `name` holding `uri` (a track, or a whole album / playlist) and opens it.
    pub(super) fn new_playlist_with(&mut self, uri: String, name: String, cx: &mut Context<Self>) {
        let (Some(api), Some(me)) = (self.api.clone(), self.me.clone()) else { return };
        cx.spawn(async move |this, cx| {
            let res = cx
                .background_executor()
                .spawn(async move {
                    let pl = api.create_playlist(&me.id, &name)?;
                    let uris = if uri.contains(":track:") { vec![uri] } else { api.context_tracks(&uri)? };
                    api.add_to_playlist(&pl, &uris)?;
                    anyhow::Ok(pl)
                })
                .await;
            let _ = this.update(cx, |this, cx| match res {
                Ok(pl) => {
                    this.reload_library(cx);
                    this.open(Page::Playlist(pl), cx);
                }
                Err(e) => {
                    eprintln!("new playlist: {e:#}");
                    this.toast = Some(("Something went wrong. Try again.".into(), Instant::now()));
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn run_ctx_action(&mut self, act: Act, cx: &mut Context<Self>) {
        let target_name = self.ctx_menu.as_ref().map(|m| match &m.target {
            Target::Track { name, .. } | Target::Playlist { name, .. } | Target::Album { name, .. } | Target::Artist { name, .. } => name.clone(),
            _ => String::new(),
        });
        self.ctx_menu = None;
        cx.notify();
        let api = self.api.clone();
        let toast = |this: &mut Shell, text: &str| this.toast = Some((text.to_string(), Instant::now()));
        match act {
            Act::Nothing => {}
            Act::Go(p) => self.open(p, cx),
            Act::CopyLink(url) => {
                cx.write_to_clipboard(gpui::ClipboardItem::new_string(url));
                toast(self, "Link copied to clipboard");
            }
            Act::OpenUrl(url) => cx.open_url(&url),
            Act::CreatePlaylist => self.create_playlist(cx),
            Act::Queue(uri) => {
                let Some(api) = api else { return };
                let add = move || {
                    if uri.contains(":track:") || uri.contains(":episode:") {
                        api.add_to_queue(&uri)
                    } else {
                        for t in api.context_tracks(&uri)? {
                            api.add_to_queue(&t)?;
                        }
                        Ok(())
                    }
                };
                // With no active device (the listed one went away) the client queues on itself:
                // load the last state here, paused, then add.
                let retry = add.clone();
                cx.spawn(async move |this, cx| {
                    let res = cx.background_executor().spawn(async move { add() }).await;
                    let res = match res {
                        Err(e) if super::no_device(&e) => {
                            let woke = this
                                .update(cx, |this, _| {
                                    let local = this.local.clone()?;
                                    this.local_fallback(&local, &Play::Pause).ok()
                                })
                                .ok()
                                .flatten()
                                .is_some();
                            if woke {
                                cx.background_executor().timer(std::time::Duration::from_millis(1500)).await;
                                cx.background_executor().spawn(async move { retry() }).await
                            } else {
                                Err(e)
                            }
                        }
                        r => r,
                    };
                    let _ = this.update(cx, |this, cx| {
                        this.background_then(cx, "Added to queue".into(), move || res, |_, _| {});
                    });
                })
                .detach();
            }
            Act::SetLiked(uri, saved) => {
                let Some(api) = api else { return };
                let id = uri.rsplit(':').next().unwrap_or_default().to_string();
                if saved {
                    self.liked.insert(uri.clone());
                } else {
                    self.liked.remove(&uri);
                }
                if self.playing_track_id().as_deref() == Some(id.as_str()) {
                    self.saved = Some((id.clone(), saved));
                }
                self.background(cx, if saved { "Added to Liked Songs." } else { "Removed from Liked Songs." }, move || api.set_saved(&id, saved));
            }
            Act::Library(uri, saved) => {
                let Some(api) = api else { return };
                let artist = uri.contains(":artist:");
                let _ = artist;
                let text = if saved { "Added to Your Library." } else { "Removed from Your Library." }.to_string();
                self.background_then(cx, text, move || api.set_in_library(&uri, saved), |this, cx| this.reload_library(cx));
            }
            Act::Public(uri, public) => {
                let Some(api) = api else { return };
                let text = if public { "Playlist is now public." } else { "Playlist is now private." };
                let uri2 = uri.clone();
                self.background_then(cx, text.to_string(), move || api.set_playlist_public(&uri, public), move |this, cx| {
                    this.playlist_pages.remove(&uri2);
                    this.loading_pages.remove(&uri2);
                    this.reload_library(cx);
                    this.page_changed(cx);
                });
            }
            Act::RemoveFromPlaylist(pl, uri) => {
                let Some(api) = api else { return };
                let name = self.playlist_pages.get(&pl).map(|p| p.name.clone()).unwrap_or_default();
                let pl2 = pl.clone();
                // The row goes now; the page then refreshes in place (no blank, no jump to the top).
                if let Some(page) = self.playlist_pages.get_mut(&pl) {
                    let before = page.tracks.len();
                    page.tracks.retain(|t| t.uri != uri);
                    page.total = page.total.saturating_sub(before - page.tracks.len());
                }
                if let Some(set) = self.pl_contents.get_mut(&pl) {
                    set.remove(&uri);
                }
                self.background_then(cx, format!("Removed from {name}."), move || api.remove_from_playlist(&pl, &uri), move |this, cx| {
                    this.refresh_page(pl2, cx);
                    this.reload_library(cx);
                });
            }
            Act::AddTo(pl, uri) => {
                let Some(api) = api else { return };
                let name = self.library_items().iter().find(|i| i.uri == pl).map(|i| i.name.clone()).unwrap_or_default();
                let pl2 = pl.clone();
                self.background_then(
                    cx,
                    format!("Added to {name}"),
                    move || {
                        let uris = if uri.contains(":track:") || uri.contains(":episode:") { vec![uri] } else { api.context_tracks(&uri)? };
                        api.add_to_playlist(&pl, &uris)
                    },
                    move |this, _| {
                        this.playlist_pages.remove(&pl2);
                    },
                );
            }
            Act::NewPlaylistWith(uri) => {
                let name = target_name.unwrap_or_else(|| "New playlist".into());
                self.new_playlist_with(uri, name, cx);
            }
            Act::Radio(seed) => {
                let Some(sp) = self.spotify.clone() else { return };
                cx.spawn(async move |this, cx| {
                    let res = cx.background_executor().spawn(async move { sp.radio_for(&seed) }).await;
                    let _ = this.update(cx, |this, cx| match res {
                        Ok(pl) => this.open(Page::Playlist(pl), cx),
                        Err(e) => {
                            eprintln!("radio: {e:#}");
                            this.toast = Some(("Something went wrong. Try again.".into(), Instant::now()));
                            cx.notify();
                        }
                    });
                })
                .detach();
            }
            Act::Sleep(minutes) => {
                let Some(api) = api else { return };
                // "End of track": what's left of the playing track
                let after = match minutes {
                    Some(m) => Duration::from_secs(m * 60),
                    None => {
                        let left = self
                            .playback
                            .as_ref()
                            .and_then(|p| p.item.as_ref().map(|t| t.duration_ms.saturating_sub(p.progress_ms.unwrap_or(0))))
                            .unwrap_or(0);
                        Duration::from_millis(left.saturating_sub(self.playback_at.elapsed().as_millis() as u64).max(500))
                    }
                };
                self.sleep_seq += 1;
                let seq = self.sleep_seq;
                toast(self, "OK, your sleep timer is set.");
                cx.spawn(async move |this, cx| {
                    cx.background_executor().timer(after).await;
                    let still = this.update(cx, |this, _| this.sleep_seq == seq).unwrap_or(false);
                    if still {
                        let _ = cx.background_executor().spawn(async move { api.pause() }).await;
                    }
                })
                .detach();
            }
        }
    }

    /// Runs `f` off the main thread; shows `ok` as a toast when it succeeds.
    fn background(&mut self, cx: &mut Context<Self>, ok: &str, f: impl FnOnce() -> anyhow::Result<()> + Send + 'static) {
        self.background_then(cx, ok.to_string(), f, |_, _| {});
    }

    pub(super) fn background_then(
        &mut self,
        cx: &mut Context<Self>,
        ok: String,
        f: impl FnOnce() -> anyhow::Result<()> + Send + 'static,
        then: impl FnOnce(&mut Shell, &mut Context<Shell>) + 'static,
    ) {
        cx.spawn(async move |this, cx| {
            let res = cx.background_executor().spawn(async move { f() }).await;
            let _ = this.update(cx, |this, cx| {
                match res {
                    Ok(()) => {
                        this.toast = Some((ok, Instant::now()));
                        then(this, cx);
                    }
                    Err(e) => {
                        let msg = format!("{e:#}");
                        eprintln!("context menu: {msg}");
                        let text = if msg.contains("NO_ACTIVE_DEVICE") || msg.contains("404") {
                            "Couldn't find an active device. Start Spotify on a device first."
                        } else if msg.contains("PREMIUM_REQUIRED") {
                            "Spotify Premium is required for this."
                        } else {
                            "Something went wrong. Try again."
                        };
                        this.toast = Some((text.into(), Instant::now()));
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }
}

impl Shell {
    /// What a `spotify:` URI's menu is.
    pub(super) fn target_for(&self, uri: &str, name: &str) -> Option<Target> {
        let me_name = self.me.as_ref().and_then(|m| m.display_name.clone()).unwrap_or_default();
        Some(match uri.split(':').nth(1)? {
            "playlist" => Target::Playlist {
                uri: uri.into(),
                name: name.into(),
                owned: self.playlist_pages.get(uri).map(|p| p.can_edit).unwrap_or(false)
                    || self.library_items().iter().any(|i| i.uri == uri && i.owner == me_name),
            },
            "album" => Target::Album { uri: uri.into(), name: name.into() },
            "artist" => Target::Artist { uri: uri.into(), name: name.into() },
            "collection" => Target::LikedSongs,
            "user" if uri.contains(":collection") => Target::LikedSongs,
            _ => return None,
        })
    }

    /// Right-click handler for a playlist / album / artist / Liked Songs item.
    pub(super) fn ctx_listener(
        &self,
        uri: String,
        name: String,
        from_library: bool,
        cx: &mut Context<Self>,
    ) -> impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static {
        cx.listener(move |this, ev: &MouseDownEvent, _, cx| {
            cx.stop_propagation();
            if let Some(t) = this.target_for(&uri, &name) {
                this.open_ctx_menu(t, ev.position, from_library, cx);
            }
        })
    }

    /// Right-click handler for a tracklist row in `context` (playlist / album / Liked Songs).
    pub(super) fn track_ctx_listener(&self, t: &crate::playlist::Track, context: &str, cx: &mut Context<Self>) -> impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static {
        let target = self.track_target(t, context);
        cx.listener(move |this, ev: &MouseDownEvent, _, cx| {
            cx.stop_propagation();
            this.open_ctx_menu(target.clone(), ev.position, false, cx);
        })
    }

    /// The menu target of a tracklist row (`context`: the playlist / album it's listed in, or "").
    pub(super) fn track_target(&self, t: &crate::playlist::Track, context: &str) -> Target {
        Target::Track {
            uri: t.uri.clone(),
            name: t.name.clone(),
            album_uri: t.album_uri.clone(),
            artists: t.artists.iter().map(|(n, u)| (n.clone(), u.clone())).collect(),
            editable_playlist: (context.contains(":playlist:") && self.playlist_pages.get(context).is_some_and(|p| p.can_edit)).then(|| context.to_string()),
            on_album: context.contains(":album:"),
        }
    }

    /// Right-click on empty space in Your Library.
    pub(super) fn library_bg_ctx(&mut self, pos: gpui::Point<Pixels>, cx: &mut Context<Self>) {
        self.open_ctx_menu(Target::LibraryBackground, pos, true, cx);
    }
}
