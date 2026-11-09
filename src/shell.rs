//! Logged-in app shell: top bar, Your Library, main view, right panel, player bar.
//! Geometry is measured from the reference client at 1551x831 CSS px.

use std::{
    cell::Cell,
    rc::Rc,
    sync::Arc,
    time::{Duration, Instant},
};

use gpui::{MouseDownEvent, 
    Animation, AnimationExt, AnyElement, App, Bounds, Context, CursorStyle, Div, MouseMoveEvent, FontWeight, ImageSource, InteractiveElement,
    IntoElement, MouseButton, ParentElement, Pixels, Render, SharedString, Stateful,
    StatefulInteractiveElement, Styled, Task, Window, WindowControlArea, canvas, div, img,
    linear_color_stop, linear_gradient, prelude::FluentBuilder, px, relative, rgba, svg,
};

mod artist_view;
mod create_menu;
mod profile_menu;
mod friends_view;
mod browse_view;
mod ctx_menu;
mod native_tip;
use native_tip::native_tip;
mod library_view;
mod user_view;
mod playlist_view;

use std::collections::{HashMap, HashSet};

use crate::{
    api::{self, Api, Me, Playback, Playlist},
    artist::{self as ar, ArtistPage},
    playlist::{self as pl, PlaylistPage},
    user::UserPage,
    home::{self, HomeFeed, Item, Kind, Shelf},
    images,
    library::{self, LibItem, LibKind},
    spotify::Spotify,
    theme::*,
};

const SURFACE: u32 = 0x121212; // panels
const ELEVATED: u32 = 0x1F1F1F; // home button, search, create pill
const CHIP: u32 = 0x2A2A2A;
const PLACEHOLDER: u32 = 0x282828;
const TRACK: u32 = 0x4D4D4D;
const ICON_DIM: u32 = 0xFFFFFFB3; // rgba(255,255,255,.7)

pub struct Shell {
    api: Option<Arc<Api>>,
    me: Option<Me>,
    playlists: Vec<Playlist>,
    playback: Option<Playback>,
    playback_at: Instant,
    _poll: Option<Task<()>>,
    spotify: Option<Arc<Spotify>>,
    home: Option<HomeFeed>,
    home_error: Option<String>,
    /// Header tint; follows the hovered shortcut like the real client.
    home_color: u32,
    chip: usize,
    /// Screen bounds of the seek / volume tracks, recorded at paint time for click-to-set.
    seek_bounds: Rc<Cell<Bounds<Pixels>>>,
    volume_bounds: Rc<Cell<Bounds<Pixels>>>,
    toast: Option<(String, Instant)>,
    /// Hovered shortcut/card group; `hover_seq` restarts the play-button animation.
    hovered: Option<SharedString>,
    hover_seq: usize,
    /// Your Library rows from `libraryV3` (falls back to Web API playlists until it loads).
    library: Option<Vec<LibItem>>,
    /// Sidebar widths (CSS px). The library collapses to a 72px rail below ~177px.
    left_w: f32,
    left_collapsed: bool,
    right_w: f32,
    drag: Option<Side>,
    /// Now Playing view header: (context uri, display name).
    context_name: Option<(String, String)>,
    /// Lyrics preview for (track id): None while unknown / when the track has none.
    lyrics: Option<(String, Option<Lyrics>)>,
    /// (track id, in Liked Songs) for the playing track.
    saved: Option<(String, bool)>,
    /// Seek / volume bar being dragged, with the fraction under the pointer.
    slider_drag: Option<(Slider, f32)>,
    /// Root focus, so the client's keyboard shortcuts work without clicking first.
    focus: gpui::FocusHandle,
    focused_once: bool,
    /// Main view page, with the back / forward history.
    page: Page,
    back: Vec<Page>,
    forward: Vec<Page>,
    playlist_pages: HashMap<String, PlaylistPage>,
    loading_pages: HashSet<String>,
    /// Main view scroll position of the current page (reset on navigation).
    page_scroll: gpui::ScrollHandle,
    artist_pages: HashMap<String, ArtistPage>,
    user_pages: HashMap<String, UserPage>,
    lib_prefs: library_view::LibPrefs,
    lib_menu: bool,
    /// View-as highlight animation: index it slides from, and a counter that restarts it.
    lib_view_from: f32,
    lib_view_seq: usize,
    /// Library panel hovered (slides the title right and reveals the collapse icon), and when.
    lib_hover: bool,
    /// Create (+) menu open; hovered item and the last un-hovered one, with when (icon animation).
    create_open: bool,
    /// Profile (avatar) menu open, its "Your Updates" list and scroll.
    profile_open: bool,
    right_panel: RightPanel,
    friends: Option<Vec<friends_view::Friend>>,
    jam_card_dismissed: bool,
    browse: Option<Vec<browse_view::BrowseCard>>,
    /// Open right-click menu.
    ctx_menu: Option<ctx_menu::CtxMenu>,
    /// Sleep timer generation (a newer timer cancels older ones).
    sleep_seq: u64,
    notifications: Option<Vec<profile_menu::Notification>>,
    profile_scroll: gpui::ScrollHandle,
    create_toggled_at: std::time::Instant,
    /// When a menu was last closed by a click outside it: the click that closed it, if it lands on
    /// the menu's own button, must not reopen it.
    menu_closed_at: std::time::Instant,
    create_hover: Option<(usize, std::time::Instant)>,
    create_unhover: Option<(usize, std::time::Instant)>,
    lib_hover_at: std::time::Instant,
    /// Track URIs in Liked Songs (tracklists show a green check for these).
    liked: HashSet<String>,
    /// Artist page: "See more" expanded (10 popular tracks), selected discography filter.
    artist_more: bool,
    disco_filter: usize,
}

/// What the right panel shows.
#[derive(Clone, Copy, PartialEq)]
pub enum RightPanel {
    NowPlaying,
    /// Listening activity (top bar friends button).
    Friends,
}

#[derive(Clone, PartialEq)]
pub enum Page {
    Home,
    Playlist(String),
    Album(String),
    Artist(String),
    /// `spotify:user:<id>`
    User(String),
    /// Liked Songs (`spotify:collection:tracks` in the client's router).
    Liked,
    /// Browse ("Browse all" categories; `/search` in the client's router).
    Browse,
}

impl Page {
    /// The page a `spotify:` URI links to, if it has one yet.
    pub fn for_uri(uri: &str) -> Option<Self> {
        match uri.split(':').nth(1)? {
            "playlist" => Some(Self::Playlist(uri.to_string())),
            "album" => Some(Self::Album(uri.to_string())),
            "artist" => Some(Self::Artist(uri.to_string())),
            "collection" => Some(Self::Liked),
            "user" if uri.ends_with(":collection") || uri.ends_with(":collection:tracks") => Some(Self::Liked),
            "user" => Some(Self::User(uri.to_string())),
            _ => None,
        }
    }
}

/// Spotify's colour-lyrics for the playing track (lines + the card colours chosen for its cover).
#[derive(Clone)]
pub struct Lyrics {
    pub lines: Vec<String>,
    pub background: u32,
    pub text: u32,
}

impl Lyrics {
    /// `colors` is the cover's `getDynamicColorsByUris` response: the card uses its
    /// `dark.higherContrast` background and subdued text colours.
    fn parse(v: &serde_json::Value, colors: Option<&serde_json::Value>) -> Option<Self> {
        let lines: Vec<String> = v
            .pointer("/lyrics/lines")?
            .as_array()?
            .iter()
            .filter_map(|l| l.get("words")?.as_str())
            .map(str::trim)
            .filter(|w| !w.is_empty() && *w != "\u{266a}")
            .map(str::to_owned)
            .collect();
        if lines.is_empty() {
            return None;
        }
        let rgb = |key: &str| {
            let c = colors?.pointer(&format!("/data/dynamicColors/0/dark/higherContrast/{key}"))?;
            let ch = |n: &str| c.get(n).and_then(|x| x.as_u64()).map(|x| x as u32);
            Some(ch("red")? << 16 | ch("green")? << 8 | ch("blue")?)
        };
        Some(Self { lines, background: rgb("backgroundBase").unwrap_or(0x535353), text: rgb("textSubdued").unwrap_or(0xFFFFFF) })
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Side {
    Left,
    Right,
}

#[derive(Clone, Copy, PartialEq)]
enum Slider {
    Seek,
    Volume,
}

const LEFT_MIN: f32 = 280.;
const LEFT_MAX: f32 = 420.;
const LEFT_RAIL: f32 = 72.;
/// Dragging the library narrower than this snaps it to the rail (measured: 176 collapses, 178 doesn't).
const LEFT_COLLAPSE_AT: f32 = 177.;
const RIGHT_MIN: f32 = 280.;
const RIGHT_MAX: f32 = 420.;

const HOME_DEFAULT_COLOR: u32 = 0x535353;
/// Cache key for the Liked Songs page.
const LIKED_URI: &str = "spotify:collection:tracks";
const LIKED_SONGS_IMAGE: &str = "https://misc.scdn.co/liked-songs/liked-songs-300.png";
/// Home shortcut tiles load this one instead (measured on the reference).
const LIKED_SONGS_IMAGE_LARGE: &str = "https://misc.scdn.co/liked-songs/liked-songs-640.jpg";

impl Shell {
    pub fn new(api: Option<Arc<Api>>, cx: &mut Context<Self>) -> Self {
        cx.observe(&images::cache(cx), |_, _, cx| cx.notify()).detach();
        let mut this = Self {
            api: api.clone(),
            me: None,
            playlists: Vec::new(),
            playback: None,
            playback_at: Instant::now(),
            _poll: None,
            spotify: None,
            home: None,
            home_error: None,
            home_color: HOME_DEFAULT_COLOR,
            chip: 0,
            seek_bounds: Rc::default(),
            volume_bounds: Rc::default(),
            toast: None,
            hovered: None,
            hover_seq: 0,
            library: None,
            left_w: LEFT_MAX,
            left_collapsed: false,
            right_w: 354.,
            drag: None,
            context_name: None,
            lyrics: None,
            saved: None,
            slider_drag: None,
            focus: cx.focus_handle(),
            focused_once: false,
            page: Page::Home,
            back: Vec::new(),
            forward: Vec::new(),
            playlist_pages: HashMap::new(),
            loading_pages: HashSet::new(),
            page_scroll: gpui::ScrollHandle::new(),
            artist_pages: HashMap::new(),
            user_pages: HashMap::new(),
            lib_prefs: library_view::LibPrefs::load(),
            // Debug: SPOTIRUST_LIBMENU=1 opens the library Sort/View menu (for captures).
            lib_menu: std::env::var_os("SPOTIRUST_LIBMENU").is_some(),
            lib_view_from: library_view::LibPrefs::load().view_index(),
            lib_view_seq: 0,
            lib_hover: false,
            create_open: std::env::var_os("SPOTIRUST_CREATEMENU").is_some(),
            create_hover: None,
            profile_open: false,
            right_panel: RightPanel::NowPlaying,
            friends: None,
            jam_card_dismissed: false,
            browse: None,
            ctx_menu: None,
            sleep_seq: 0,
            notifications: None,
            profile_scroll: gpui::ScrollHandle::new(),
            create_toggled_at: std::time::Instant::now() - std::time::Duration::from_secs(1),
            menu_closed_at: std::time::Instant::now() - std::time::Duration::from_secs(1),
            create_unhover: None,
            lib_hover_at: std::time::Instant::now() - std::time::Duration::from_secs(1),
            liked: HashSet::new(),
            artist_more: false,
            disco_filter: 0,
        };
        // Debug: SPOTIRUST_LEFT=rail | <width> starts with that library width.
        match std::env::var("SPOTIRUST_LEFT").ok().as_deref() {
            Some("rail") => this.left_collapsed = true,
            Some(w) => this.left_w = w.parse().unwrap_or(this.left_w).clamp(LEFT_MIN, LEFT_MAX),
            None => {}
        }
        if let Some(api) = api {
            this.load(api.clone(), cx);
            this._poll = Some(this.poll(api.clone(), cx));
            this.connect_session(api, cx);
        }
        this
    }

    /// Re-reads Your Library (after creating or changing items).
    fn reload_library(&mut self, cx: &mut Context<Self>) {
        let Some(sp) = self.spotify.clone() else { return };
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    let mut items = library::parse(&sp.library(&[])?, "");
                    let uris: Vec<String> = items.iter().filter(|i| i.kind == LibKind::Playlist).map(|i| i.uri.clone()).collect();
                    if let Ok(mixed) = sp.mixed_playlists(&uris) {
                        items.iter_mut().for_each(|i| i.mixed = mixed.contains(&i.uri));
                    }
                    anyhow::Ok(items)
                })
                .await;
            let _ = this.update(cx, |this, cx| match result {
                Ok(items) => {
                    this.library = Some(items);
                    this.resolve_mosaics(cx);
                    cx.notify();
                }
                Err(e) => eprintln!("library reload: {e:#}"),
            });
        })
        .detach();
    }

    /// librespot session (cached credentials) -> private Home feed.
    fn connect_session(&mut self, api: Arc<Api>, cx: &mut Context<Self>) {
        let web = api.token();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    let sp = Arc::new(Spotify::connect(web)?);
                    // Debug: SPOTIRUST_DUMP_PLAYLIST="<uri>|<out.json>" writes the raw fetchPlaylist response.
                    if let Some((uri, out)) = std::env::var("SPOTIRUST_DUMP_PLAYLIST").ok().as_deref().and_then(|s| s.split_once('|')) {
                        match sp.playlist(uri, 0, 25) {
                            Ok(v) => std::fs::write(out, serde_json::to_vec_pretty(&v)?)?,
                            Err(e) => eprintln!("dump playlist: {e:#}"),
                        }
                    }
                    if let Ok(id) = std::env::var("SPOTIRUST_DUMP_USER") {
                        if let Err(e) = sp.user_profile(&id) {
                            eprintln!("dump user: {e:#}");
                        }
                        if let Err(e) = sp.user_top_artists(10) {
                            eprintln!("dump top: {e:#}");
                        }
                        if let Err(e) = sp.notifications() {
                            eprintln!("dump notifications: {e:#}");
                        }
                        if let Err(e) = sp.buddylist() {
                            eprintln!("dump buddylist: {e:#}");
                        }
                        if let Err(e) = sp.browse_all() {
                            eprintln!("dump browse: {e:#}");
                        }
                    }
                    if let Ok(uri) = std::env::var("SPOTIRUST_DUMP_ARTIST") {
                        if let Err(e) = sp.artist(&uri) {
                            eprintln!("dump artist: {e:#}");
                        }
                    }
                    let lib = match sp.library(&[]) {
                        Ok(v) => {
                            if let Some(path) = std::env::var_os("SPOTIRUST_DUMP_LIBRARY") {
                                std::fs::write(path, serde_json::to_vec_pretty(&v)?)?;
                            }
                            let mut items = library::parse(&v, "");
                            let uris: Vec<String> =
                                items.iter().filter(|i| i.kind == LibKind::Playlist).map(|i| i.uri.clone()).collect();
                            match sp.mixed_playlists(&uris) {
                                Ok(mixed) => items.iter_mut().for_each(|i| i.mixed = mixed.contains(&i.uri)),
                                Err(e) => eprintln!("mix state: {e:#}"),
                            }
                            Some(items)
                        }
                        Err(e) => {
                            eprintln!("library: {e:#}");
                            None
                        }
                    };
                    let feed = home::parse(&sp.home("")?);
                    anyhow::Ok((sp, feed, lib))
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok((sp, feed, lib)) => {
                        this.spotify = Some(sp);
                        // Test hook: open Listening activity without clicking (captures).
                        if std::env::var_os("SPOTIRUST_OPEN_FRIENDS").is_some() {
                            this.toggle_friends(cx);
                        }
                        // Test hook: start on a page (`SPOTIRUST_OPEN=spotify:artist:...`).
                        if let Some(p) = std::env::var("SPOTIRUST_OPEN").ok().as_deref().and_then(Page::for_uri) {
                            this.open(p, cx);
                        }
                        this.load_page(LIKED_URI.to_string(), cx);
                        if lib.is_some() {
                            this.library = lib;
                            this.resolve_mosaics(cx);
                        }
                        this.home = Some(feed);
                        // Debug: SPOTIRUST_OPEN=<playlist uri> opens that page once connected.
                        if let Some(page) = std::env::var("SPOTIRUST_OPEN").ok().and_then(|u| Page::for_uri(&u)) {
                            this.open(page, cx);
                        }
                    }
                    Err(e) => {
                        eprintln!("home: {e:#}");
                        this.home_error = Some(format!("{e:#}"));
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Library rows draw playlist mosaics the way the client does (`spotify:mosaic:`, track order);
    /// swaps each `mosaic.scdn.co` cover for that once its order is known.
    fn resolve_mosaics(&mut self, cx: &mut Context<Self>) {
        let Some(api) = self.api.clone() else { return };
        let targets: Vec<String> = self
            .library
            .iter()
            .flatten()
            .filter(|i| i.image.as_deref().is_some_and(|u| u.contains("mosaic.scdn.co")))
            .map(|i| i.uri.clone())
            .collect();
        if targets.is_empty() {
            return;
        }
        cx.spawn(async move |this, cx| {
            let found = cx
                .background_executor()
                .spawn(async move {
                    targets
                        .into_iter()
                        .filter_map(|uri| {
                            let id = uri.rsplit(':').next()?.to_string();
                            match api.mosaic_uri(&id) {
                                Ok(m) => Some((uri, m?)),
                                Err(e) => {
                                    eprintln!("mosaic {uri}: {e:#}");
                                    None
                                }
                            }
                        })
                        .collect::<Vec<_>>()
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                for item in this.library.iter_mut().flatten() {
                    if let Some((_, m)) = found.iter().find(|(u, _)| *u == item.uri) {
                        item.image = Some(m.clone());
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Sends a player command off the UI thread, then refreshes playback state.
    fn command(&mut self, f: impl FnOnce(&Api) -> anyhow::Result<()> + Send + 'static, cx: &mut Context<Self>) {
        let Some(api) = self.api.clone() else { return };
        cx.spawn(async move |this, cx| {
            let a = api.clone();
            let result = cx.background_executor().spawn(async move { f(&a) }).await;
            cx.background_executor().timer(Duration::from_millis(350)).await;
            let a = api.clone();
            let pb = cx.background_executor().spawn(async move { a.playback_or_last() }).await;
            let _ = this.update(cx, |this, cx| {
                if let Err(e) = result {
                    let msg = format!("{e:#}");
                    eprintln!("command: {msg}");
                    let text = if msg.contains("NO_ACTIVE_DEVICE") || msg.contains("404") {
                        "Couldn't find an active device. Start Spotify on a device first.".to_string()
                    } else if msg.contains("PREMIUM_REQUIRED") {
                        "Spotify Premium is required for this.".to_string()
                    } else {
                        "Something went wrong. Try again.".to_string()
                    };
                    this.toast = Some((text, Instant::now()));
                }
                if let Ok(pb) = pb {
                    this.playback = pb;
                    this.refresh_now_playing(cx);
                    this.playback_at = Instant::now();
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn toggle_play(&mut self, cx: &mut Context<Self>) {
        let playing = self.playback.as_ref().is_some_and(|p| p.is_playing);
        // Optimistic: freeze/unfreeze the progress clock immediately.
        let now_ms = self.progress_ms();
        if let Some(pb) = self.playback.as_mut() {
            pb.is_playing = !playing;
            pb.progress_ms = Some(now_ms);
            self.playback_at = Instant::now();
        }
        cx.notify();
        self.command(move |api| if playing { api.pause() } else { api.resume() }, cx);
    }

    /// The desktop client's playback shortcuts: Space play/pause, Ctrl+Right/Left next/previous,
    /// Ctrl+Up/Down volume ±10, Ctrl+S shuffle, Ctrl+R repeat.
    fn shortcut_key(&mut self, ev: &gpui::KeyDownEvent, cx: &mut Context<Self>) {
        let k = &ev.keystroke;
        let ctrl = k.modifiers.control;
        let pb = self.playback.clone();
        if k.key == "escape" && (self.lib_menu || self.create_open || self.profile_open || self.ctx_menu.is_some()) {
            self.ctx_menu = None;
            self.lib_menu = false;
            self.create_open = false;
            self.profile_open = false;
            cx.notify();
            return;
        }
        match (k.key.as_str(), ctrl) {
            ("space", false) if !ev.is_held => self.toggle_play(cx),
            ("right", true) => self.command(|api| api.next(), cx),
            ("left", true) => self.command(|api| api.previous(), cx),
            ("up", true) | ("down", true) => {
                let cur = pb.as_ref().and_then(|p| p.device.as_ref()).and_then(|d| d.volume_percent).unwrap_or(100) as i32;
                let v = (cur + if k.key == "up" { 10 } else { -10 }).clamp(0, 100) as u32;
                if let Some(d) = self.playback.as_mut().and_then(|p| p.device.as_mut()) {
                    d.volume_percent = Some(v);
                }
                cx.notify();
                self.command(move |api| api.set_volume(v), cx);
            }
            ("s", true) => {
                let on = !pb.as_ref().is_some_and(|p| p.shuffle_state);
                self.command(move |api| api.set_shuffle(on), cx);
            }
            ("r", true) => {
                // off -> context -> track -> off, like the repeat button
                let next = match pb.as_ref().map(|p| p.repeat_state.as_str()) {
                    Some("off") | None => "context",
                    Some("context") => "track",
                    _ => "off",
                };
                self.command(move |api| api.set_repeat(next), cx);
            }
            _ => return,
        }
        cx.stop_propagation();
    }

    /// Commits a seek / volume drag on release.
    fn finish_slider(&mut self, cx: &mut Context<Self>) {
        let Some((kind, f)) = self.slider_drag.take() else { return };
        match kind {
            Slider::Seek => {
                let duration = self.playback.as_ref().and_then(|p| p.item.as_ref()).map(|t| t.duration_ms).unwrap_or(0);
                let ms = (f * duration as f32) as u64;
                if let Some(pb) = self.playback.as_mut() {
                    pb.progress_ms = Some(ms);
                }
                self.playback_at = Instant::now();
                self.command(move |api| api.seek(ms), cx);
            }
            Slider::Volume => {
                let v = (f * 100.).round() as u32;
                if let Some(d) = self.playback.as_mut().and_then(|p| p.device.as_mut()) {
                    d.volume_percent = Some(v);
                }
                self.command(move |api| api.set_volume(v), cx);
            }
        }
        cx.notify();
    }

    fn playing_track_id(&self) -> Option<String> {
        self.playback.as_ref()?.item.as_ref()?.id.clone()
    }

    fn is_saved(&self) -> bool {
        match (&self.saved, self.playing_track_id()) {
            (Some((id, saved)), Some(current)) => *id == current && *saved,
            _ => false,
        }
    }

    /// Looks up whether the playing track is in Liked Songs (once per track).
    fn refresh_saved(&mut self, cx: &mut Context<Self>) {
        let (Some(id), Some(api)) = (self.playing_track_id(), self.api.clone()) else { return };
        if self.saved.as_ref().is_some_and(|s| s.0 == id) {
            return;
        }
        self.saved = Some((id.clone(), false));
        cx.spawn(async move |this, cx| {
            let tid = id.clone();
            let res = cx.background_executor().spawn(async move { api.is_saved(&tid) }).await;
            let _ = this.update(cx, |this, cx| match res {
                Ok(saved) if this.saved.as_ref().is_some_and(|s| s.0 == id) => {
                    this.saved = Some((id, saved));
                    cx.notify();
                }
                Ok(_) => {}
                Err(e) => eprintln!("saved state: {e:#}"),
            });
        })
        .detach();
    }

    /// Add / remove the playing track from Liked Songs (optimistic, reverted on failure).
    fn toggle_saved(&mut self, cx: &mut Context<Self>) {
        let (Some(id), Some(api)) = (self.playing_track_id(), self.api.clone()) else { return };
        let target = !self.is_saved();
        self.saved = Some((id.clone(), target));
        cx.notify();
        cx.spawn(async move |this, cx| {
            let tid = id.clone();
            let res = cx.background_executor().spawn(async move { api.set_saved(&tid, target) }).await;
            if let Err(e) = res {
                eprintln!("save track: {e:#}");
                let _ = this.update(cx, |this, cx| {
                    if this.saved.as_ref().is_some_and(|s| s.0 == id) {
                        this.saved = Some((id, !target));
                        cx.notify();
                    }
                });
            }
        })
        .detach();
    }

    /// Opens a page, like clicking a link in the client (pushes history).
    fn open(&mut self, page: Page, cx: &mut Context<Self>) {
        if page == self.page {
            return;
        }
        let prev = std::mem::replace(&mut self.page, page);
        self.back.push(prev);
        self.forward.clear();
        self.page_changed(cx);
    }

    fn go_back(&mut self, cx: &mut Context<Self>) {
        if let Some(p) = self.back.pop() {
            let cur = std::mem::replace(&mut self.page, p);
            self.forward.push(cur);
            self.page_changed(cx);
        }
    }

    fn go_forward(&mut self, cx: &mut Context<Self>) {
        if let Some(p) = self.forward.pop() {
            let cur = std::mem::replace(&mut self.page, p);
            self.back.push(cur);
            self.page_changed(cx);
        }
    }

    fn page_changed(&mut self, cx: &mut Context<Self>) {
        self.page_scroll.set_offset(gpui::Point::default());
        match self.page.clone() {
            Page::Playlist(uri) | Page::Album(uri) => self.load_page(uri, cx),
            Page::Liked => self.load_page(LIKED_URI.to_string(), cx),
            Page::Artist(uri) => {
                self.artist_more = false;
                self.disco_filter = 0;
                self.load_artist(uri, cx);
            }
            Page::User(uri) => self.load_user(uri, cx),
            Page::Browse => self.load_browse(cx),
            Page::Home => {}
        }
        cx.notify();
    }

    /// Loads a collection page (all tracks, paged like the client), cached per URI:
    /// playlists via `fetchPlaylist` (100 a page), albums via `getAlbum` (50), Liked Songs via the
    /// Web API's saved tracks (50).
    fn load_page(&mut self, uri: String, cx: &mut Context<Self>) {
        let Some(sp) = self.spotify.clone() else { return };
        let api = self.api.clone();
        let me = self.me.clone();
        if uri == LIKED_URI && me.is_none() {
            return;
        }
        if self.playlist_pages.contains_key(&uri) || !self.loading_pages.insert(uri.clone()) {
            return;
        }
        cx.spawn(async move |this, cx| {
            let u = uri.clone();
            let result = cx
                .background_executor()
                .spawn(async move {
                    let kind = u.split(':').nth(1).unwrap_or_default().to_string();
                    let mut page = match kind.as_str() {
                        "album" => pl::parse_album(&sp.album(&u, 0, 50)?).ok_or_else(|| anyhow::anyhow!("not an album"))?,
                        "collection" => {
                            let api = api.ok_or_else(|| anyhow::anyhow!("no api"))?;
                            let me = me.ok_or_else(|| anyhow::anyhow!("no profile"))?;
                            let mut page = pl::liked_page(&api, &me)?;
                            match sp.liked_filters() {
                                Ok(v) => page.filters = pl::parse_filters(&v),
                                Err(e) => eprintln!("liked filters: {e:#}"),
                            }
                            page
                        }
                        _ => pl::parse(&sp.playlist(&u, 0, 100)?).ok_or_else(|| anyhow::anyhow!("not a playlist"))?,
                    };
                    while page.tracks.len() < page.total && kind != "collection" {
                        let offset = page.tracks.len() as u32;
                        let more = match kind.as_str() {
                            "album" => pl::parse_album_items(&sp.album(&u, offset, 50)?),
                            _ => pl::parse_items(&sp.playlist(&u, offset, 100)?),
                        };
                        if more.is_empty() {
                            break;
                        }
                        page.tracks.extend(more);
                    }
                    anyhow::Ok(page)
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.loading_pages.remove(&uri);
                match result {
                    Ok(page) => {
                        if uri == LIKED_URI {
                            this.liked = page.tracks.iter().map(|t| t.uri.clone()).collect();
                        }
                        this.playlist_pages.insert(uri, page);
                    }
                    Err(e) => eprintln!("page {uri}: {e:#}"),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// `queryArtistOverview`, cached per URI.
    fn load_artist(&mut self, uri: String, cx: &mut Context<Self>) {
        let Some(sp) = self.spotify.clone() else { return };
        if self.artist_pages.contains_key(&uri) || !self.loading_pages.insert(uri.clone()) {
            return;
        }
        cx.spawn(async move |this, cx| {
            let u = uri.clone();
            let result = cx
                .background_executor()
                .spawn(async move { ar::parse(&sp.artist(&u)?).ok_or_else(|| anyhow::anyhow!("not an artist")) })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.loading_pages.remove(&uri);
                match result {
                    Ok(page) => {
                        this.artist_pages.insert(uri, page);
                    }
                    Err(e) => eprintln!("artist {uri}: {e:#}"),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Profile, followers/following, the header colour and (own profile) top artists.
    fn load_user(&mut self, uri: String, cx: &mut Context<Self>) {
        let Some(sp) = self.spotify.clone() else { return };
        if self.user_pages.contains_key(&uri) || !self.loading_pages.insert(uri.clone()) {
            return;
        }
        cx.spawn(async move |this, cx| {
            let id = uri.rsplit(':').next().unwrap_or_default().to_string();
            let result = cx
                .background_executor()
                .spawn(async move {
                    let (p, followers, following) = sp.user_profile(&id)?;
                    let own = p.get("is_current_user").and_then(|v| v.as_bool()).unwrap_or(false);
                    let top = if own { sp.user_top_artists(10).ok() } else { None };
                    let color = p
                        .get("image_url")
                        .and_then(|v| v.as_str())
                        .filter(|u| !u.is_empty())
                        .and_then(|u| sp.extracted_colors(&[u.to_string()]).ok())
                        .and_then(|v| crate::user::extracted_color(&v));
                    crate::user::parse(&p, &followers, &following, top.as_ref(), color)
                        .ok_or_else(|| anyhow::anyhow!("not a profile"))
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.loading_pages.remove(&uri);
                match result {
                    Ok(page) => {
                        this.user_pages.insert(uri, page);
                    }
                    Err(e) => eprintln!("user {uri}: {e:#}"),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn play_item(&mut self, item: &Item, cx: &mut Context<Self>) {
        let uri = match item.kind {
            Kind::LikedSongs => match &self.me {
                Some(me) => format!("spotify:user:{}:collection", me.id),
                None => return,
            },
            _ => item.uri.clone(),
        };
        self.command(move |api| api.play_context(&uri), cx);
    }

    fn select_chip(&mut self, index: usize, cx: &mut Context<Self>) {
        let (Some(sp), Some(feed)) = (self.spotify.clone(), self.home.as_ref()) else { return };
        if index == self.chip {
            return;
        }
        let facet = feed.chips.get(index).map(|c| c.0.clone()).unwrap_or_default();
        self.chip = index;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx.background_executor().spawn(async move { sp.home(&facet).map(|v| home::parse(&v)) }).await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(mut feed) => {
                        // Keep the chip list from the unfiltered feed.
                        if let Some(old) = &this.home {
                            feed.chips = old.chips.clone();
                        }
                        this.home = Some(feed);
                    }
                    Err(e) => eprintln!("home facet: {e:#}"),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn load(&mut self, api: Arc<Api>, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            let (me, playlists) = cx
                .background_executor()
                .spawn(async move { (api.me(), api.my_playlists()) })
                .await;
            let _ = this.update(cx, |this, cx| {
                match me {
                    Ok(me) => {
                        this.me = Some(me);
                        // Liked Songs loads up front: every tracklist marks liked tracks.
                        this.load_page(LIKED_URI.to_string(), cx);
                    }
                    Err(e) => eprintln!("me: {e:#}"),
                }
                match playlists {
                    Ok(p) => this.playlists = p,
                    Err(e) => eprintln!("playlists: {e:#}"),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Refresh playback every 3s and repaint every second so the progress bar moves.
    fn poll(&mut self, api: Arc<Api>, cx: &mut Context<Self>) -> Task<()> {
        cx.spawn(async move |this, cx| {
            let mut tick = 0u32;
            loop {
                if tick % 3 == 0 {
                    let api = api.clone();
                    let pb = cx.background_executor().spawn(async move { api.playback_or_last() }).await;
                    let ok = this.update(cx, |this, cx| {
                        match pb {
                            Ok(pb) => {
                                this.playback = pb;
                                this.refresh_now_playing(cx);
                                this.playback_at = Instant::now();
                            }
                            Err(e) => eprintln!("playback: {e:#}"),
                        }
                        cx.notify();
                    });
                    if ok.is_err() {
                        break;
                    }
                } else if this.update(cx, |_, cx| cx.notify()).is_err() {
                    break;
                }
                tick += 1;
                cx.background_executor().timer(Duration::from_secs(1)).await;
            }
        })
    }

    /// Fetches what the Now Playing view needs when the context or track changes:
    /// the context's name (header) and the colour lyrics (Lyrics preview card).
    fn refresh_now_playing(&mut self, cx: &mut Context<Self>) {
        let Some(pb) = self.playback.clone() else { return };
        if let (Some(ctx), Some(api)) = (pb.context.as_ref(), self.api.clone()) {
            if self.context_name.as_ref().map(|c| c.0.as_str()) != Some(ctx.uri.as_str()) {
                let uri = ctx.uri.clone();
                self.context_name = Some((uri.clone(), String::new()));
                cx.spawn(async move |this, cx| {
                    let u = uri.clone();
                    let name = cx.background_executor().spawn(async move { api.context_name(&u) }).await;
                    let _ = this.update(cx, |this, cx| {
                        match name {
                            Ok(name) => this.context_name = Some((uri, name)),
                            Err(e) => eprintln!("context name: {e:#}"),
                        }
                        cx.notify();
                    });
                })
                .detach();
            }
        }
        self.refresh_saved(cx);
        let (Some(track), Some(sp)) = (pb.item.as_ref(), self.spotify.clone()) else { return };
        let Some(id) = track.id.clone() else { return };
        if self.lyrics.as_ref().map(|l| l.0.as_str()) == Some(id.as_str()) {
            return;
        }
        let image = track.album.as_ref().and_then(|a| api::pick_image(&a.images, 640)).map(|i| i.url.clone()).unwrap_or_default();
        self.lyrics = Some((id.clone(), None));
        cx.spawn(async move |this, cx| {
            let tid = id.clone();
            let result = cx
                .background_executor()
                .spawn(async move {
                    let colors = sp.dynamic_colors(&image).ok();
                    sp.lyrics(&tid, &image).map(|l| l.map(|l| (l, colors)))
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.lyrics.as_ref().map(|l| l.0.as_str()) != Some(id.as_str()) {
                    return;
                }
                match result {
                    Ok(Some((v, colors))) => {
                        this.lyrics = Some((id, Lyrics::parse(&v, colors.as_ref())));
                    }
                    Ok(None) => {}
                    Err(e) => eprintln!("lyrics: {e:#}"),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn progress_ms(&self) -> u64 {
        let Some(pb) = &self.playback else { return 0 };
        let base = pb.progress_ms.unwrap_or(0);
        let dur = pb.item.as_ref().map(|t| t.duration_ms).unwrap_or(0);
        let live = if pb.is_playing { self.playback_at.elapsed().as_millis() as u64 } else { 0 };
        (base + live).min(dur)
    }
}

// ---------- small building blocks -----------------------------------------------------------

fn icon(path: &'static str, size: f32, color: u32) -> impl IntoElement {
    svg().path(path).size(px(size)).text_color(c(color))
}

fn icon_rgba(path: &'static str, size: f32, color: u32) -> impl IntoElement {
    svg().path(path).size(px(size)).text_color(rgba(color))
}

fn round_button(id: impl Into<SharedString>, size: f32) -> Stateful<Div> {
    div()
        .id(gpui::ElementId::Name(id.into()))
        .size(px(size))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .cursor_pointer()
}

fn cover(url: Option<&str>, size: f32, radius: f32, cx: &mut App) -> AnyElement {
    let image = url.and_then(|u| images::cache(cx).update(cx, |c, cx| c.get_sized(u, size, cx)));
    match image {
        Some(image) => img(ImageSource::Render(image))
            .size(px(size))
            .flex_none()
            .rounded(px(radius))
            .into_any_element(),
        None => div()
            .size(px(size))
            .flex_none()
            .rounded(px(radius))
            .bg(c(PLACEHOLDER))
            .flex()
            .items_center()
            .justify_center()
            .child(icon("icons/note.svg", size / 2., TEXT_SUBDUED))
            .into_any_element(),
    }
}

/// Home shortcut cover: the tile (radius 4, overflow hidden) clips the cover's left corners, and
/// the cover casts `0 8px 24px rgba(0,0,0,.5)` onto the tile; placeholder is rgba(255,255,255,.2).
fn shortcut_cover(url: Option<&str>, size: f32, cx: &mut App) -> AnyElement {
    let image = url.and_then(|u| images::cache(cx).update(cx, |c, cx| c.get_sized(u, size, cx)));
    let frame = div()
        .size(px(size))
        .flex_none()
        .rounded_tl(px(4.))
        .rounded_bl(px(4.))
        .bg(rgba(0xFFFFFF33))
        .shadow(vec![gpui::BoxShadow {
            color: gpui::hsla(0., 0., 0., 0.5),
            offset: gpui::point(px(0.), px(8.)),
            blur_radius: px(24.),
            spread_radius: px(0.),
        }]);
    match image {
        Some(image) => frame
            .child(img(ImageSource::Render(image)).size(px(size)).rounded_tl(px(4.)).rounded_bl(px(4.)))
            .into_any_element(),
        None => frame.into_any_element(),
    }
}

fn fmt_time(ms: u64) -> String {
    let s = ms / 1000;
    format!("{}:{:02}", s / 60, s % 60)
}

fn one_line(text: impl Into<SharedString>) -> Div {
    div().overflow_hidden().whitespace_nowrap().text_ellipsis().child(text.into())
}

// ---------- top bar --------------------------------------------------------------------------

impl Shell {
    fn top_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let avatar_url = self
            .me
            .as_ref()
            .and_then(|m| api::pick_image(&m.images, 64).map(|i| i.url.clone()));
        let avatar = match avatar_url.and_then(|u| images::cache(cx).update(cx, |c, cx| c.get(&u, cx))) {
            Some(image) => img(ImageSource::Render(image)).size(px(32.)).rounded_full().into_any_element(),
            None => div().size(px(32.)).rounded_full().bg(c(0x535353)).into_any_element(),
        };

        div()
            .id("top-bar")
            .relative()
            .h(px(64.))
            .flex_none()
            .w_full()
            .window_control_area(WindowControlArea::Drag)
            // â€¢â€¢â€¢ app menu
            .child(
                round_button("app-menu", 32.)
                    .occlude()
                    .absolute()
                    .left(px(20.))
                    .top(px(16.))
                    .child(icon("icons/more.svg", 16., WHITE)),
            )
            // back / forward
            .child(
                div()
                    .id("back")
                    .occlude()
                    .on_click(cx.listener(|this, _, _, cx| this.go_back(cx)))
                    .absolute()
                    .left(px(72.))
                    .top(px(8.))
                    .w(px(24.))
                    .h(px(48.))
                    .flex()
                    .items_center()
                    .justify_center()
                    // measured: 24px icon, opacity .3 while there's nowhere to go
                    .when(self.back.is_empty(), |d| d.opacity(0.3))
                    .child(icon("icons/back.svg", 24., TEXT_SUBDUED)),
            )
            .child(
                div()
                    .id("forward")
                    .occlude()
                    .on_click(cx.listener(|this, _, _, cx| this.go_forward(cx)))
                    .absolute()
                    .left(px(104.))
                    .top(px(8.))
                    .w(px(24.))
                    .h(px(48.))
                    .flex()
                    .items_center()
                    .justify_center()
                    // measured: 24px icon, opacity .3 while there's nowhere to go
                    .when(self.forward.is_empty(), |d| d.opacity(0.3))
                    .child(icon("icons/forward.svg", 24., TEXT_SUBDUED)),
            )
            // home + search, centered in the window
            .child(
                div()
                    .absolute()
                    .top(px(8.))
                    .left_0()
                    .right_0()
                    .flex()
                    .justify_center()
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .gap(px(8.))
                            .child(
                                round_button("home-button", 48.)
                                    .occlude()
                                    .bg(c(ELEVATED))
                                    .hover(|s| s.bg(c(0x2A2A2A)))
                                    .on_click(cx.listener(|this, _, _, cx| this.open(Page::Home, cx)))
                                    .child(if self.page == Page::Home {
                                        icon("icons/home_filled.svg", 24., WHITE).into_any_element()
                                    } else {
                                        icon("icons/home.svg", 24., TEXT_SUBDUED).into_any_element()
                                    }),
                            )
                            .child(
                                div()
                                    .id("search")
                                    .occlude()
                                    .relative()
                                    .w(px(474.))
                                    .h(px(48.))
                                    .rounded_full()
                                    .bg(c(ELEVATED))
                                    .hover(|s| s.bg(c(0x2A2A2A)))
                                    .flex()
                                    .items_center()
                                    .child(
                                        div()
                                            .absolute()
                                            .left(px(12.))
                                            .top(px(12.))
                                            .child(icon("icons/search.svg", 24., TEXT_SUBDUED)),
                                    )
                                    .child(
                                        div()
                                            .pl(px(48.))
                                            .text_size(px(16.))
                                            .text_color(c(TEXT_SUBDUED))
                                            .child("What do you want to play?"),
                                    )
                                    .child(
                                        div()
                                            .absolute()
                                            .right(px(56.))
                                            .top(px(12.))
                                            .w(px(1.))
                                            .h(px(24.))
                                            .bg(c(0x7C7C7C)),
                                    )
                                    .child(
                                        div()
                                            .id("browse-button")
                                            .absolute()
                                            .left(px(434.))
                                            .top(px(12.))
                                            .cursor_pointer()
                                            .on_click(cx.listener(|this, _, _, cx| this.open(Page::Browse, cx)))
                                            .child(if self.page == Page::Browse {
                                                icon("icons/browse_filled.svg", 24., WHITE).into_any_element()
                                            } else {
                                                icon("icons/browse.svg", 24., TEXT_SUBDUED).into_any_element()
                                            }),
                                    ),
                            ),
                    ),
            )
            // right cluster: what's new, friends, avatar (ends 151px from the right edge)
            .child(
                div()
                    .absolute()
                    .right(px(151.))
                    .top(px(8.))
                    .h(px(48.))
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(8.))
                    .child(round_button("whats-new", 32.).occlude().child(icon("icons/bell.svg", 16., TEXT_SUBDUED)))
                    .child(round_button("friends", 32.)
                            .occlude()
                            .on_click(cx.listener(|this, _, _, cx| this.toggle_friends(cx)))
                            .child(if self.right_panel == RightPanel::Friends {
                                icon("icons/friends_active.svg", 16., WHITE).into_any_element()
                            } else {
                                icon("icons/friends.svg", 16., TEXT_SUBDUED).into_any_element()
                            }))
                    .child(round_button("user", 48.)
                            .occlude()
                            .bg(c(ELEVATED))
                            .on_click(cx.listener(|this, _, _, cx| this.toggle_profile_menu(cx)))
                            .child(avatar)),
            )
            .child(crate::titlebar::caption_buttons())
    }
}

// ---------- Your Library ---------------------------------------------------------------------

impl Shell {
    /// Library rows: `libraryV3` when loaded, else Liked Songs + Web API playlists.
    fn library_items(&self) -> Vec<LibItem> {
        if let Some(items) = &self.library {
            return items.clone();
        }
        let mut out = vec![LibItem {
            uri: "spotify:collection:tracks".into(),
            kind: LibKind::LikedSongs,
            name: "Liked Songs".into(),
            owner: String::new(),
            image: Some("https://misc.scdn.co/liked-songs/liked-songs-300.png".into()),
            pinned: true,
            mixed: false,
            added_at: String::new(),
        }];
        out.extend(self.playlists.iter().map(|p| LibItem {
            uri: format!("spotify:playlist:{}", p.id),
            kind: LibKind::Playlist,
            name: p.name.clone(),
            owner: p.owner.display_name.clone().unwrap_or_else(|| p.owner.id.clone()),
            image: api::pick_image(&p.images, 96).map(|i| i.url.clone()),
            pinned: false,
            mixed: false,
            added_at: String::new(),
        }));
        out
    }

    fn library(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        if self.left_collapsed {
            return self.library_rail(cx);
        }
        let me_name = self.me.as_ref().and_then(|m| m.display_name.clone()).unwrap_or_default();
        let mut items = self.library_items();
        library_view::sort_items(&mut items, self.lib_prefs);
        let rows = self.library_content(&items, &me_name, cx);

        // Hovering the panel: title slides 24 right (transform .1s ease-out); the collapse icon
        // slides in and fades in after it (.1s ease-out, .1s delay). Reversed on leave.
        let ms = self.lib_hover_at.elapsed().as_secs_f32() * 1000.;
        let (tp, ip) = (crate::anim::progress(ms, 0., 100.), crate::anim::progress(ms, 100., 100.));
        let (tp, ip) = if self.lib_hover { (crate::anim::ease_out(tp), crate::anim::ease_out(ip)) } else { (1. - crate::anim::ease_out(tp), 1. - crate::anim::ease_out(ip)) };
        if ms < 200. {
            window.request_animation_frame();
        }
        // measured: pill "Create" from a 332px panel, round 35px "+" below
        let create_pill = self.left_w >= 332.;
        // open menu: the + rotates to an x and turns white (transform, color .2s ease-out)
        let cms = self.create_toggled_at.elapsed().as_secs_f32() * 1000.;
        let cp = crate::anim::ease_out(crate::anim::progress(cms, 0., 200.));
        let cp = if self.create_open { cp } else { 1. - cp };
        if cms < 200. {
            window.request_animation_frame();
        }
        let plus = || {
            let g = 0xB3 as f32 / 255.;
            let v = g + (1. - g) * cp;
            svg()
                .path("icons/plus.svg")
                .size(px(16.))
                .with_transformation(gpui::Transformation::rotate(gpui::radians(std::f32::consts::FRAC_PI_4 * cp)))
                .text_color(gpui::Rgba { r: v, g: v, b: v, a: 1. })
        };
        div()
            .id("library-panel")
            .w(px(self.left_w))
            .flex_none()
            .h_full()
            .rounded(px(8.))
            .bg(c(SURFACE))
            .flex()
            .flex_col()
            .overflow_hidden()
            .on_hover(cx.listener(|this, h: &bool, _, cx| {
                if this.lib_hover != *h {
                    this.lib_hover = *h;
                    this.lib_hover_at = std::time::Instant::now();
                    cx.notify();
                }
            }))
            // header: "Your Library" text box at (20, 22.4); Create pill and expand button at y 16
            .child(
                div()
                    .h(px(51.2))
                    .flex_none()
                    .pt(px(16.))
                    .pl(px(20.))
                    .pr(px(16.))
                    .flex()
                    .flex_row()
                    .items_start()
                    .child(
                        div()
                            .id("lib-title")
                            .group("lib-title")
                            .relative()
                            .flex_1()
                            .min_w_0()
                            .h(px(35.2))
                            .cursor_pointer()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.left_collapsed = true;
                                cx.notify();
                            }))
                            .child(
                                svg()
                                    .path("icons/lib_collapse16.svg")
                                    .absolute()
                                    .top(px(9.5))
                                    .left(px(-44. + 40. * ip))
                                    .size(px(16.))
                                    .opacity(ip)
                                    .text_color(c(TEXT_SUBDUED))
                                    .group_hover("lib-title", |s| s.text_color(c(WHITE))),
                            )
                            .child(
                                one_line("Your Library")
                                    .absolute()
                                    .top(px(6.4))
                                    .left(px(24. * tp))
                                    .right_0()
                                    .text_size(px(16.))
                                    .line_height(px(22.4))
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(c(WHITE)),
                            ),
                    )
                    .child((if create_pill {
                        div()
                            .id("create")
                            .h(px(35.2))
                            .pl(px(16.))
                            .pr(px(16.))
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(8.))
                            .rounded_full()
                            .bg(c(ELEVATED))
                            .hover(|s| s.bg(c(0x2A2A2A)))
                            .cursor_pointer()
                            .child(plus())
                            .child(
                                div()
                                    .text_size(px(14.))
                                    .line_height(px(19.2))
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(c(WHITE))
                                    .child("Create"),
                            )
                    } else {
                        round_button("create", 35.)
                            .bg(c(ELEVATED))
                            .hover(|s| s.bg(c(0x2A2A2A)))
                            .child(plus())
                    })
                    .on_click(cx.listener(|this, _, _, cx| {
                        if !this.create_open && this.menu_closed_at.elapsed().as_millis() < 400 {
                            return;
                        }
                        this.create_open = !this.create_open;
                        this.create_toggled_at = std::time::Instant::now();
                        this.create_hover = None;
                        this.create_unhover = None;
                        cx.notify();
                    })))
                    .child(
                        round_button("expand-library", 35.)
                            .h(px(35.2))
                            .ml(px(8.))
                            .group("expand-library")
                            .child(
                                svg()
                                    .path("icons/expand.svg")
                                    .size(px(16.))
                                    .text_color(c(TEXT_SUBDUED))
                                    .group_hover("expand-library", |s| s.text_color(c(WHITE))),
                            ),
                    ),
            )
            // filter chips at y 67.2
            .child(
                div()
                    .mt(px(16.))
                    .px(px(16.))
                    .h(px(32.))
                    .flex_none()
                    .flex()
                    .flex_row()
                    .gap(px(8.))
                    .child(
                        div()
                            .id("chip-playlists")
                            .h(px(32.))
                            .px(px(12.))
                            .flex()
                            .items_center()
                            .rounded_full()
                            .bg(rgba(0xFFFFFF1A))
                            .hover(|s| s.bg(rgba(0xFFFFFF24)))
                            .cursor_pointer()
                            .text_size(px(14.))
                            .text_color(c(WHITE))
                            .child("Playlists"),
                    ),
            )
            // search + sort at y 109.2; the sort button ends 24 from the panel's right edge
            .child(
                div()
                    .mt(px(10.))
                    .pl(px(16.))
                    .pr(px(24.))
                    .h(px(32.))
                    .flex_none()
                    .flex()
                    .flex_row()
                    .items_center()
                    .child(
                        round_button("lib-search", 32.)
                            .group("lib-search")
                            .hover(|s| s.bg(rgba(0xFFFFFF1A)))
                            .child(
                                svg()
                                    .path("icons/lib_search.svg")
                                    .size(px(16.))
                                    .text_color(rgba(ICON_DIM))
                                    .group_hover("lib-search", |s| s.text_color(c(WHITE))),
                            ),
                    )
                    .child(div().flex_1())
                    .child(
                        div()
                            .id("sort")
                            .group("sort")
                            .h(px(32.))
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(8.))
                            .cursor_pointer()
                            .text_size(px(14.))
                            .line_height(px(19.2))
                            .text_color(c(TEXT_SUBDUED))
                            .hover(|s| s.text_color(c(WHITE)))
                            .on_click(cx.listener(|this, _, _, cx| {
                                if !this.lib_menu && this.menu_closed_at.elapsed().as_millis() < 400 {
                                    return;
                                }
                                this.lib_menu = !this.lib_menu;
                                // opening: the View-as highlight starts on the current view (no slide)
                                this.lib_view_from = this.lib_prefs.view_index();
                                this.lib_view_seq += 1;
                                cx.notify();
                            }))
                            .child(self.lib_prefs.sort.label())
                            .child(
                                svg()
                                    .path(self.lib_prefs.view.icon())
                                    .size(px(16.))
                                    .text_color(c(TEXT_SUBDUED))
                                    .group_hover("sort", |s| s.text_color(c(WHITE))),
                            ),
                    ),
            )
            // rows from y 149.2
            .child(
                div()
                    .id("library-list")
                    .on_mouse_down(MouseButton::Right, cx.listener(|this, ev: &MouseDownEvent, _, cx| this.library_bg_ctx(ev.position, cx)))
                    .mt(px(8.))
                    .flex_1()
                    .min_h_0()
                    .px(px(8.))
                    .overflow_y_scroll()
                    .children(rows),
            )
            .into_any_element()
    }

    /// Collapsed library: library button, round Create button, 48px covers in 64px cells.
    fn library_rail(&self, cx: &mut Context<Self>) -> AnyElement {
        let page = self.page.clone();
        let active = move |item: &LibItem| Page::for_uri(&item.uri).as_ref() == Some(&page);
        let cells: Vec<AnyElement> = self
            .library_items()
            .iter()
            .enumerate()
            .map(|(i, item)| {
                div()
                    .id(gpui::ElementId::Name(format!("rail-{i}").into()))
                    .size(px(64.))
                    .flex_none()
                    .p(px(8.))
                    .rounded(px(6.))
                    .when(active(item), |d| d.bg(c(0x2A2A2A)))
                    .hover(|s| s.bg(c(0x1F1F1F)))
                    .cursor_pointer()
                    .when(Page::for_uri(&item.uri).is_some(), |d| {
                        let uri = item.uri.clone();
                        d.on_click(cx.listener(move |this, _, _, cx| if let Some(p) = Page::for_uri(&uri) { this.open(p, cx) }))
                    })
                    .child(lib_art(item, 48., cx))
                    .into_any_element()
            })
            .collect();
        div()
            .w(px(LEFT_RAIL))
            .flex_none()
            .h_full()
            .rounded(px(8.))
            .bg(c(SURFACE))
            .overflow_hidden()
            .flex()
            .flex_col()
            .items_center()
            .child(
                round_button("open-library", 40.)
                    .mt(px(16.))
                    .group("open-library")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.left_collapsed = false;
                        cx.notify();
                    }))
                    .child(
                        svg()
                            .path("icons/library.svg")
                            .size(px(24.))
                            .text_color(c(TEXT_SUBDUED))
                            .group_hover("open-library", |s| s.text_color(c(WHITE))),
                    ),
            )
            .child(
                round_button("rail-create", 35.)
                    .mt(px(8.))
                    .bg(c(ELEVATED))
                    .hover(|s| s.bg(c(0x2A2A2A)))
                    .child(icon("icons/plus.svg", 16., TEXT_SUBDUED)),
            )
            .child(
                div()
                    .id("rail-list")
                    .mt(px(16.))
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .children(cells),
            )
            .into_any_element()
    }
}

/// Cover for a library row: Liked Songs and playlists square (radius 4), artists round.
fn lib_art(item: &LibItem, size: f32, cx: &mut App) -> AnyElement {
    let radius = if item.kind == LibKind::Artist { size / 2. } else { 4. };
    let url = item.image.as_deref().map(|u| images::variant(u, images::Variant::Small));
    cover(url.as_deref(), size, radius, cx)
}

impl Shell {
/// Context URI to play for a library item (Liked Songs plays the user's collection).
fn library_play_uri(&self, item: &LibItem) -> Option<String> {
    match item.kind {
        LibKind::LikedSongs => self.me.as_ref().map(|me| format!("spotify:user:{}:collection", me.id)),
        LibKind::Folder => None,
        _ => Some(item.uri.clone()),
    }
}

/// Whether `item` is the context that is playing right now.
fn is_playing_context(&self, item: &LibItem) -> bool {
    let Some(ctx) = self.playback.as_ref().filter(|p| p.is_playing).and_then(|p| p.context.as_ref()) else {
        return false;
    };
    ctx.uri == item.uri || (item.kind == LibKind::LikedSongs && ctx.uri.ends_with(":collection"))
}

/// Library row. Measured on the reference: hovering the row darkens the cover
/// (rgba(0,0,0,.5), radius 4) and shows a 24px white play button on it; double-click plays.
fn library_row(&self, index: usize, item: &LibItem, me_name: &str, cx: &mut Context<Self>) -> AnyElement {
    let owner = if item.kind == LibKind::LikedSongs { me_name } else { item.owner.as_str() };
    let subtitle = if owner.is_empty() {
        item.type_label().to_string()
    } else {
        format!("{} \u{2022} {owner}", item.type_label())
    };
    let group: SharedString = format!("lib-row-{index}").into();
    let play_uri = self.library_play_uri(item);
    let playing = self.is_playing_context(item);
    let open_page = Page::for_uri(&item.uri);
    let active = open_page.as_ref() == Some(&self.page);
    let art = div()
        .relative()
        .size(px(48.))
        .flex_none()
        .child(lib_art(item, 48., cx))
        .when_some(play_uri.clone(), |d, uri| {
            d.child(
                div()
                    .id(gpui::ElementId::Name(format!("lib-play-{index}").into()))
                    .absolute()
                    .inset_0()
                    .rounded(px(4.))
                    .bg(rgba(0x0000007F))
                    .flex()
                    .items_center()
                    .justify_center()
                    .opacity(0.)
                    .group_hover(group.clone(), |s| s.opacity(1.))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        if playing {
                            this.toggle_play(cx);
                        } else {
                            let uri = uri.clone();
                            this.command(move |api| api.play_context(&uri), cx);
                        }
                    }))
                    .child(icon(if playing { "icons/pause24.svg" } else { "icons/play24.svg" }, 24., WHITE)),
            )
        });
    div()
        .id(gpui::ElementId::Name(group.clone()))
        .group(group.clone())
        .on_mouse_down(MouseButton::Right, self.ctx_listener(item.uri.clone(), item.name.clone(), true, cx))
        .h(px(64.))
        .p(px(8.))
        .flex()
        .flex_row()
        .items_center()
        .gap(px(12.))
        .rounded(px(6.))
        .when(active, |d| d.bg(c(0x2A2A2A)))
        .hover(move |s| s.bg(c(if active { 0x393939 } else { 0x1F1F1F })))
        .cursor_pointer()
        .on_click(cx.listener(move |this, ev: &gpui::ClickEvent, _, cx| {
            if ev.click_count() == 2 {
                if let Some(uri) = play_uri.clone() {
                    this.command(move |api| api.play_context(&uri), cx);
                }
            } else if let Some(p) = open_page.clone() {
                this.open(p, cx);
            }
        }))
        .child(art)
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(2.))
                .child(one_line(item.name.clone()).text_size(px(16.)).line_height(px(22.4)).text_color(c(WHITE)))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(4.))
                        .text_size(px(14.))
                        .line_height(px(19.2))
                        .text_color(c(TEXT_SUBDUED))
                        .when(item.pinned, |d| d.child(icon("icons/pin.svg", 12., GREEN)))
                        .when(item.mixed, |d| {
                            d.child(
                                div()
                                    .flex_none()
                                    .w(px(12.))
                                    .h(px(12.))
                                    .rounded(px(2.))
                                    .bg(c(TEXT_SUBDUED))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(svg().path("icons/mixed.svg").w(px(8.)).h(px(12.)).text_color(c(0x000000))),
                            )
                        })
                        .child(one_line(subtitle).min_w_0()),
                ),
        )
        .into_any_element()
}
}

// ---------- Home (main view) -----------------------------------------------------------------
// Reference (CSS px, main view 506 wide at a 1312x764 viewport):
//   tint 256 tall: base color + linear-gradient(rgba(0,0,0,.6) -> #121212)
//   chips row 64 tall, chips at y 20, x 40: 32 tall, padding 4/12, 14px, gap 8
//   shortcuts at y 68: rows 48 tall, gap 8, radius 4, bg white/10%, image 48, text 14/700
//   shelves 32 below shortcuts, 16 apart: header 48 (subtitle 12px, title 24/700),
//   carousel 8 below; cards 177.725 wide from x 28, padding 12, radius 6, image 153.725
const CARD_W: f32 = 177.725;
const CARD_IMG: f32 = 153.725;

fn liked_songs_art(size: f32, radius: f32) -> AnyElement {
    div()
        .size(px(size))
        .flex_none()
        .rounded(px(radius))
        .bg(linear_gradient(135., linear_color_stop(c(0x450AF5), 0.), linear_color_stop(c(0xC4EFD9), 1.)))
        .flex()
        .items_center()
        .justify_center()
        .child(icon("icons/heart.svg", size / 3., WHITE))
        .into_any_element()
}

fn item_art(item: &Item, size: f32, radius: f32, cx: &mut App) -> AnyElement {
    // radius 0 = Home shortcut tile, which loads the medium WebP rendition (and the 640 Liked Songs art).
    let shortcut = radius == 0.;
    let url = match &item.image {
        Some(u) if shortcut => Some(images::variant(u, images::Variant::Medium)),
        other => other.clone(),
    };
    match item.kind {
        Kind::LikedSongs if shortcut => shortcut_cover(Some(LIKED_SONGS_IMAGE_LARGE), size, cx),
        _ if shortcut => shortcut_cover(url.as_deref(), size, cx),
        Kind::LikedSongs => cover(Some(LIKED_SONGS_IMAGE), size, radius, cx),
        Kind::Artist | Kind::User if radius > 0. => cover(url.as_deref(), size, size / 2., cx),
        _ => cover(url.as_deref(), size, radius, cx),
    }
}

fn green_play(size: f32) -> Div {
    div()
        .size(px(size))
        .flex_none()
        .rounded_full()
        .bg(c(GREEN))
        .flex()
        .items_center()
        .justify_center()
        .child(icon("icons/play.svg", size * 0.42, 0x000000))
}

impl Shell {
    fn shortcut(&self, i: usize, item: &Item, tile_h: f32, cx: &mut Context<Self>) -> AnyElement {
        let group: SharedString = format!("shortcut-{i}").into();
        let color = item.color.unwrap_or(HOME_DEFAULT_COLOR);
        div()
            .id(gpui::ElementId::Name(group.clone()))
            .group(group.clone())
            .on_mouse_down(MouseButton::Right, self.ctx_listener(item.uri.clone(), item.name.clone(), false, cx))
            .flex_1()
            .min_w_0()
            .h(px(tile_h))
            .rounded(px(4.))
            .overflow_hidden()
            .bg(rgba(0xFFFFFF1A))
            .hover(|s| s.bg(rgba(0xFFFFFF33)))
            .cursor_pointer()
            .flex()
            .flex_row()
            .items_center()
            .on_hover({
                let g = group.clone();
                cx.listener(move |this, hovered: &bool, _, cx| {
                    // Measured: the tint keeps the last hovered shortcut's colour after the pointer leaves.
                    if *hovered {
                        this.home_color = color;
                    }
                    this.set_hovered(g.clone(), *hovered);
                    cx.notify();
                })
            })
            .relative()
            .when(matches!(item.kind, Kind::Playlist | Kind::Album | Kind::Artist | Kind::User | Kind::LikedSongs), |d| {
                let uri = item.uri.clone();
                d.on_click(cx.listener(move |this, _, _, cx| if let Some(p) = Page::for_uri(&uri) { this.open(p, cx) }))
            })
            .child(item_art(item, tile_h, 0., cx))
            // measured: title box from cover + 8 to 8 before the tile's right edge, two-line clamp
            // with ellipsis; the play button is an overlay, not reserved space
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .px(px(8.))
                    .text_size(px(14.))
                    .line_height(px(19.2))
                    .font_weight(FontWeight::BOLD)
                    .text_color(c(WHITE))
                    .line_clamp(2)
                    .text_ellipsis()
                    .child(item.name.clone()),
            )
            .child(
                div()
                    .absolute()
                    .right(px(8.))
                    .top(px((tile_h - 32.) / 2.))
                    .size(px(32.))
                    .child(self.hover_play(&group, 32., false, item, cx)),
            )
            .into_any_element()
    }

    /// Green play button that fades (and for cards, slides up 8px) in when its tile is hovered,
    /// matching the reference: 0.15s, cubic-bezier(0.3, 0, 0, 1).
    fn hover_play(&self, group: &SharedString, size: f32, slide: bool, item: &Item, cx: &mut Context<Self>) -> AnyElement {
        if self.hovered.as_ref() != Some(group) || item.kind == Kind::User {
            return div().into_any_element();
        }
        let it = item.clone();
        let button = green_play(size)
            .id(gpui::ElementId::Name(format!("{group}-play").into()))
            .hover(|s| s.bg(c(GREEN_HOVER)))
            .on_click(cx.listener(move |this, _, _, cx| {
                cx.stop_propagation();
                this.play_item(&it, cx);
            }));
        let anim_id = gpui::ElementId::Name(format!("{group}-in-{}", self.hover_seq).into());
        let easing = |t: f32| 1. - (1. - t).powi(4); // ≈ cubic-bezier(0.3, 0, 0, 1)
        if slide {
            button
                .absolute()
                .right(px(8.))
                .with_animation(anim_id, Animation::new(Duration::from_millis(150)).with_easing(easing), |b, d| {
                    b.opacity(d).bottom(px(8. * d))
                })
                .into_any_element()
        } else {
            button
                .with_animation(anim_id, Animation::new(Duration::from_millis(150)).with_easing(easing), |b, d| b.opacity(d))
                .into_any_element()
        }
    }

    fn set_hovered(&mut self, group: SharedString, hovered: bool) {
        if hovered {
            self.hovered = Some(group);
            self.hover_seq += 1;
        } else if self.hovered.as_ref() == Some(&group) {
            self.hovered = None;
        }
    }

    fn card(&self, shelf: usize, i: usize, item: &Item, cx: &mut Context<Self>) -> AnyElement {
        self.sized_card(format!("card-{shelf}-{i}").into(), item, CARD_W, 0., cx)
    }

    /// A card `w` wide (image `w - 24`); `sub_gap` is the space above the subtitle (4 on album
    /// "More by" grids, where it is a `<time>`).
    fn sized_card(&self, group: SharedString, item: &Item, w: f32, sub_gap: f32, cx: &mut Context<Self>) -> AnyElement {
        let img = w - 24.;
        div()
            .id(gpui::ElementId::Name(group.clone()))
            .group(group.clone())
            .on_mouse_down(MouseButton::Right, self.ctx_listener(item.uri.clone(), item.name.clone(), false, cx))
            .w(px(w))
            .flex_none()
            .p(px(12.))
            .rounded(px(6.))
            .hover(|s| s.bg(c(0x1F1F1F)))
            .cursor_pointer()
            .when(matches!(item.kind, Kind::Playlist | Kind::Album | Kind::LikedSongs), |d| {
                let uri = item.uri.clone();
                d.on_click(cx.listener(move |this, _, _, cx| if let Some(p) = Page::for_uri(&uri) { this.open(p, cx) }))
            })
            .flex()
            .flex_col()
            .on_hover({
                let g = group.clone();
                cx.listener(move |this, hovered: &bool, _, cx| {
                    this.set_hovered(g.clone(), *hovered);
                    cx.notify();
                })
            })
            .child(
                div()
                    .relative()
                    .size(px(img))
                    .child(item_art(item, img, 6., cx))
                    .child(self.hover_play(&group, 48., true, item, cx)),
            )
            .when(!item.hide_title, |d| {
                d.child(
                    one_line(item.name.clone())
                        .when(sub_gap > 0., |d| d.whitespace_normal().line_clamp(2))
                        .mt(px(8.))
                        .text_size(px(16.))
                        .line_height(px(22.4))
                        .text_color(c(WHITE)),
                )
            })
            .when(!item.subtitle.is_empty(), |d| {
                d.child(
                    div()
                        .mt(px(if item.hide_title { 12. } else { sub_gap }))
                        // measured: the subtitle box is 12px narrower than the cover
                        .w(px(img - 12.))
                        .text_size(px(14.))
                        .line_height(px(19.2))
                        .text_color(c(TEXT_SUBDUED))
                        .line_clamp(2)
                        .text_ellipsis()
                        .child(item.subtitle.clone()),
                )
            })
            .into_any_element()
    }

    fn shelf(&self, index: usize, shelf: &Shelf, cx: &mut Context<Self>) -> AnyElement {
        let cards: Vec<AnyElement> =
            shelf.items.iter().enumerate().map(|(i, it)| self.card(index, i, it, cx)).collect();
        div()
            .flex()
            .flex_col()
            .child(
                div()
                    .h(px(48.))
                    .px(px(40.))
                    .flex()
                    .flex_col()
                    .justify_end()
                    .when_some(shelf.subtitle.clone(), |d, sub| {
                        d.child(one_line(sub).text_size(px(12.)).line_height(px(16.8)).text_color(c(TEXT_SUBDUED)))
                    })
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_end()
                            .child(
                                one_line(shelf.title.clone())
                                    .flex_1()
                                    .font_family(title_font())
                                    .text_size(px(24.))
                                    .line_height(px(28.8))
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(c(WHITE)),
                            )
                            .child(
                                div()
                                    .id(gpui::ElementId::Name(format!("show-all-{index}").into()))
                                    .pr(px(8.))
                                    .pb(px(1.))
                                    .text_size(px(14.))
                                    .line_height(px(19.2))
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(c(TEXT_SUBDUED))
                                    .cursor_pointer()
                                    .hover(|s| s.underline())
                                    .child("Show all"),
                            ),
                    ),
            )
            .child(
                div()
                    .id(gpui::ElementId::Name(format!("carousel-{index}").into()))
                    .mt(px(8.))
                    .px(px(28.))
                    .overflow_x_scroll()
                    .flex()
                    .flex_row()
                    .children(cards),
            )
            .into_any_element()
    }

    fn home_view(&self, main_w: f32, cx: &mut Context<Self>) -> impl IntoElement {
        let tint = self.home_color;
        let mut body = div()
            .id("home-scroll")
            .absolute()
            .inset_0()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .pb(px(32.));

        match &self.home {
            None => {
                let msg = match &self.home_error {
                    Some(e) => format!("Couldn't load Home: {e}"),
                    None => String::new(),
                };
                body = body.child(div().p(px(40.)).text_size(px(14.)).text_color(c(TEXT_SUBDUED)).child(msg));
            }
            Some(feed) => {
                // chips
                let chips = feed.chips.iter().enumerate().map(|(i, (_, label))| {
                    let selected = i == self.chip;
                    div()
                        .id(gpui::ElementId::Name(format!("chip-{i}").into()))
                        .h(px(32.))
                        .px(px(12.))
                        .flex()
                        .items_center()
                        .rounded_full()
                        .cursor_pointer()
                        .text_size(px(14.))
                        .when(selected, |d| d.bg(c(WHITE)).text_color(c(0x000000)))
                        .when(!selected, |d| {
                            d.bg(rgba(0xFFFFFF1A)).text_color(c(WHITE)).hover(|s| s.bg(rgba(0xFFFFFF33)))
                        })
                        .on_click(cx.listener(move |this, _, _, cx| this.select_chip(i, cx)))
                        .child(label.clone())
                        .into_any_element()
                });
                body = body.child(
                    div().h(px(64.)).flex_none().pt(px(20.)).px(px(40.)).flex().flex_row().gap(px(8.)).children(chips),
                );

                // Shortcut grid, a container query on the section (content) width, from the
                // reference CSS: 2 columns, 4 at >= 700; tiles 48 tall (gap 8), 64 at >= 1141,
                // 80 at >= 1599 (gap 12).
                let content_w = main_w - 80.;
                let cols = if content_w >= 700. { 4 } else { 2 };
                let (tile_h, tile_gap) = if content_w >= 1599. {
                    (80., 12.)
                } else if content_w >= 1141. {
                    (64., 12.)
                } else {
                    (48., 8.)
                };
                let items: Vec<&Item> = feed.shortcuts.iter().take(8).collect();
                let mut grid = div().mt(px(4.)).px(px(40.)).flex().flex_col().gap(px(tile_gap));
                for (r, row) in items.chunks(cols).enumerate() {
                    let mut row_el = div().flex().flex_row().gap(px(tile_gap));
                    for (k, it) in row.iter().enumerate() {
                        row_el = row_el.child(self.shortcut(r * cols + k, it, tile_h, cx));
                    }
                    for _ in row.len()..cols {
                        row_el = row_el.child(div().flex_1());
                    }
                    grid = grid.child(row_el);
                }
                body = body.child(grid);

                // shelves
                let mut shelves = div().mt(px(32.)).flex().flex_col().gap(px(16.));
                for (i, s) in feed.shelves.iter().enumerate() {
                    shelves = shelves.child(self.shelf(i, s, cx));
                }
                body = body.child(shelves);
            }
        }

        div()
            .flex_1()
            .min_w_0()
            .h_full()
            .rounded(px(8.))
            .bg(c(SURFACE))
            .overflow_hidden()
            .relative()
            .child(
                div().absolute().top_0().left_0().right_0().h(px(256.)).bg(c(tint)).child(
                    div().size_full().bg(linear_gradient(
                        180.,
                        linear_color_stop(rgba(0x00000099), 0.),
                        linear_color_stop(c(SURFACE), 1.),
                    )),
                ),
            )
            .child(body)
    }
}

// ---------- right panel ----------------------------------------------------------------------

impl Shell {
    // Reference (right panel 354 wide at x 950, top 64):
    //   header row at +16: context name 16/700 (line 22.4) at +20.8; More / Expand 32px buttons, right 16
    //   cover at +64, width - 32, radius 8
    //   title 24/700 SpotifyMixUITitle (line 28.8) 16 below the cover; artists 14px (line 19.2) 2.4 below
    //   add button 24x48 at the right; Lyrics preview card 28.8 below the artists: 177 tall, radius 8,
    //   padding 16, colour-lyrics background, "Lyrics preview" 16/700, lines 24px title font
    //   (line 28.8, 14.4 apart) in the colour-lyrics text colour.
    fn now_playing_panel(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let track = self.playback.as_ref().and_then(|p| p.item.clone());
        let art_url = track
            .as_ref()
            .and_then(|t| t.album.as_ref())
            .and_then(|a| api::pick_image(&a.images, 640).map(|i| images::variant(&i.url, images::Variant::Large)));
        let artists = track
            .as_ref()
            .map(|t| t.artists.iter().map(|a| a.name.as_str()).collect::<Vec<_>>().join(", "))
            .unwrap_or_default();
        let header = self.context_name.as_ref().map(|c| c.1.clone()).unwrap_or_default();
        let lyrics = self.lyrics.as_ref().and_then(|l| l.1.clone());
        let saved = self.is_saved();
        let inner_w = self.right_w - 32.;

        let header_button = |id: &'static str, path: &'static str| {
            round_button(id, 32.).group(id).child(
                svg().path(path).size(px(16.)).text_color(c(TEXT_SUBDUED)).group_hover(id, |s| s.text_color(c(WHITE))),
            )
        };

        div()
            .w(px(self.right_w))
            .flex_none()
            .h_full()
            .rounded(px(8.))
            .bg(c(SURFACE))
            .overflow_hidden()
            .child(
                div()
                    .id("npv-scroll")
                    .size_full()
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .px(px(16.))
                    .child(
                        div()
                            .h(px(64.))
                            .flex_none()
                            .pt(px(16.))
                            .flex()
                            .flex_row()
                            .items_start()
                            .child(
                                one_line(header)
                                    .flex_1()
                                    .min_w_0()
                                    .pt(px(4.8))
                                    .text_size(px(16.))
                                    .line_height(px(22.4))
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(c(WHITE)),
                            )
                            .child(header_button("npv-more", "icons/more.svg"))
                            .child(header_button("npv-expand", "icons/expand_panel.svg")),
                    )
                    .when_some(track, |d, t| {
                        d.child(cover(art_url.as_deref(), inner_w, 8., cx))
                            .child(
                                div()
                                    .mt(px(16.))
                                    .flex()
                                    .flex_row()
                                    .items_start()
                                    .gap(px(8.))
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .flex()
                                            .flex_col()
                                            .gap(px(2.4))
                                            .child(
                                                one_line(t.name.clone())
                                                    .font_family(title_font())
                                                    .text_size(px(24.))
                                                    .line_height(px(28.8))
                                                    .font_weight(FontWeight::BOLD)
                                                    .text_color(c(WHITE)),
                                            )
                                            .child(
                                                one_line(artists)
                                                    .text_size(px(14.))
                                                    .line_height(px(19.2))
                                                    .text_color(c(TEXT_SUBDUED)),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .id("npv-add")
                                            .group("npv-add")
                                            .mt(px(1.6))
                                            .w(px(24.))
                                            .h(px(48.))
                                            .flex_none()
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .cursor_pointer()
                                            .on_click(cx.listener(|this, _, _, cx| this.toggle_saved(cx)))
                                            .child(if saved {
                                                svg().path("icons/check_circle24.svg").size(px(24.)).text_color(c(GREEN))
                                            } else {
                                                svg()
                                                    .path("icons/add_circle24.svg")
                                                    .size(px(24.))
                                                    .text_color(c(TEXT_SUBDUED))
                                                    .group_hover("npv-add", |s| s.text_color(c(WHITE)))
                                            }),
                                    ),
                            )
                    })
                    .when_some(lyrics, |d, l| {
                        let lines: Vec<AnyElement> = l
                            .lines
                            .iter()
                            .take(6)
                            .map(|line| {
                                div()
                                    .font_family(title_font())
                                    .text_size(px(24.))
                                    .line_height(px(28.8))
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(c(l.text))
                                    .child(line.clone())
                                    .into_any_element()
                            })
                            .collect();
                        d.child(
                            div()
                                .mt(px(28.8))
                                .h(px(177.))
                                .flex_none()
                                .rounded(px(8.))
                                .overflow_hidden()
                                .bg(c(l.background))
                                .p(px(16.))
                                .flex()
                                .flex_col()
                                .child(
                                    div()
                                        .text_size(px(16.))
                                        .line_height(px(22.4))
                                        .font_weight(FontWeight::BOLD)
                                        .text_color(c(WHITE))
                                        .child("Lyrics preview"),
                                )
                                .child(div().mt(px(16.)).flex().flex_col().gap(px(14.4)).children(lines)),
                        )
                    })
                    .child(div().h(px(16.)).flex_none()),
            )
    }
}

// ---------- player bar -----------------------------------------------------------------------

impl Shell {
    fn player_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let pb = self.playback.clone();
        let track = pb.as_ref().and_then(|p| p.item.clone());
        let playing = pb.as_ref().is_some_and(|p| p.is_playing);
        let shuffle = pb.as_ref().is_some_and(|p| p.shuffle_state);
        let repeat = pb.as_ref().map(|p| p.repeat_state.clone()).unwrap_or_default();
        let duration = track.as_ref().map(|t| t.duration_ms).unwrap_or(0);
        // While a bar is dragged, it (and the elapsed time) follows the pointer.
        let (progress, frac) = match self.slider_drag {
            Some((Slider::Seek, f)) => ((f * duration as f32) as u64, f),
            _ => {
                let p = self.progress_ms();
                (p, if duration > 0 { p as f32 / duration as f32 } else { 0. })
            }
        };
        let volume = match self.slider_drag {
            Some((Slider::Volume, f)) => (f * 100.).round() as u32,
            _ => pb.as_ref().and_then(|p| p.device.as_ref()).and_then(|d| d.volume_percent).unwrap_or(100),
        };
        let remote_device = pb.as_ref().and_then(|p| p.device.as_ref()).map(|d| d.name.clone());

        let art_url = track
            .as_ref()
            .and_then(|t| t.album.as_ref())
            .and_then(|a| api::pick_image(&a.images, 64).map(|i| images::variant(&i.url, images::Variant::Small)));

        // left: now playing widget
        let left = div()
            .flex_1()
            .min_w_0()
            .h(px(56.))
            .flex()
            .flex_row()
            .items_center()
            .when_some(track.clone(), |d, t| {
                d.child(cover(art_url.as_deref(), 56., 4., cx))
                    .child(
                        div()
                            .ml(px(16.))
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(one_line(t.name.clone()).text_size(px(14.)).line_height(px(22.)).text_color(c(WHITE)))
                            .child(
                                one_line(t.artists.iter().map(|a| a.name.as_str()).collect::<Vec<_>>().join(", "))
                                    .text_size(px(12.))
                                    .line_height(px(16.8))
                                    .text_color(c(TEXT_SUBDUED)),
                            ),
                    )
                    .child(
                round_button("add-liked", 32.)
                    .ml(px(8.))
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_saved(cx)))
                    .child(if self.is_saved() {
                        icon("icons/check_circle.svg", 16., GREEN)
                    } else {
                        icon("icons/add_circle.svg", 16., TEXT_SUBDUED)
                    }),
            )
            });

        // Inactive icons brighten to white on hover; active (green) ones get a dot underneath.
        let ctl = |id: &'static str, path: &'static str, active: bool| {
            round_button(id, 32.)
                .group(id)
                .relative()
                .child(
                    svg()
                        .path(path)
                        .size(px(16.))
                        .text_color(c(if active { GREEN } else { TEXT_SUBDUED }))
                        .when(!active, |s| s.group_hover(id, |s| s.text_color(c(WHITE)))),
                )
                .when(active, |d| {
                    d.child(div().absolute().bottom(px(0.)).size(px(4.)).rounded_full().bg(c(GREEN)))
                })
        };
        let next_repeat = match repeat.as_str() {
            "off" | "" => "context",
            "context" => "track",
            _ => "off",
        };
        let center = div()
            .w(relative(0.4))
            .flex_none()
            .flex()
            .flex_col()
            .items_center()
            .child(
                div()
                    .h(px(32.))
                    .flex()
                    .flex_row()
                    .gap(px(16.))
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .gap(px(8.))
                            .child(
                                ctl("shuffle", "icons/shuffle.svg", shuffle)
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.command(move |api| api.set_shuffle(!shuffle), cx)
                                    })),
                            )
                            .child(
                                ctl("prev", "icons/prev.svg", false)
                                    .on_click(cx.listener(|this, _, _, cx| this.command(|api| api.previous(), cx))),
                            ),
                    )
                    .child(
                        round_button("playpause", 32.)
                            .bg(c(WHITE))
                            .hover(|s| s.bg(c(0xF0F0F0)))
                            .on_click(cx.listener(|this, _, _, cx| this.toggle_play(cx)))
                            .child(icon(if playing { "icons/pause.svg" } else { "icons/play.svg" }, 16., 0x000000)),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .gap(px(8.))
                            .child(
                                ctl("next", "icons/next.svg", false)
                                    .on_click(cx.listener(|this, _, _, cx| this.command(|api| api.next(), cx))),
                            )
                            .child(
                                ctl("repeat", "icons/repeat.svg", repeat != "off" && !repeat.is_empty())
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.command(move |api| api.set_repeat(next_repeat), cx)
                                    })),
                            ),
                    ),
            )
            .child(
                div()
                    .mt(px(8.))
                    .w_full()
                    .h(px(16.8))
                    .flex()
                    .flex_row()
                    .items_center()
                    .text_size(px(12.))
                    .text_color(c(TEXT_SUBDUED))
                    .child(div().w(px(50.)).flex().justify_end().child(fmt_time(progress)))
                    .child(
                        slider("seek", frac, self.slider_drag.is_some_and(|d| d.0 == Slider::Seek), self.seek_bounds.clone(), {
                            let this = cx.entity().downgrade();
                            move |f, _, cx| {
                                let _ = this.update(cx, |this, cx| {
                                    this.slider_drag = Some((Slider::Seek, f));
                                    cx.notify();
                                });
                            }
                        })
                        .flex_1()
                        .mx(px(8.)),
                    )
                    .child(div().w(px(50.)).child(fmt_time(duration))),
            );

        let dim = |id: &'static str, path: &'static str| {
            round_button(id, 32.).group(id).child(
                svg().path(path).size(px(16.)).text_color(rgba(ICON_DIM)).group_hover(id, |s| s.text_color(c(WHITE))),
            )
        };
        let right = div()
            .flex_1()
            .min_w_0()
            .h(px(32.))
            .mt(px(12.))
            .flex()
            .flex_row()
            .justify_end()
            .items_center()
            .child(dim("lyrics", "icons/mic.svg"))
            .child(dim("queue", "icons/queue.svg"))
            .child(dim("connect", "icons/device.svg"))
            .child(
                div()
                    .w(px(125.))
                    .flex()
                    .flex_row()
                    .items_center()
                    .child(
                        round_button("mute", 32.)
                            .group("mute")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                let target = if volume == 0 { 50 } else { 0 };
                                this.command(move |api| api.set_volume(target), cx)
                            }))
                            .child(
                                svg()
                                    .path(if volume == 0 { "icons/volume_mute.svg" } else { "icons/volume.svg" })
                                    .size(px(16.))
                                    .text_color(c(TEXT_SUBDUED))
                                    .group_hover("mute", |s| s.text_color(c(WHITE))),
                            ),
                    )
                    .child(
                        slider("volume", volume as f32 / 100., self.slider_drag.is_some_and(|d| d.0 == Slider::Volume), self.volume_bounds.clone(), {
                            let this = cx.entity().downgrade();
                            move |f, _, cx| {
                                let _ = this.update(cx, |this, cx| {
                                    this.slider_drag = Some((Slider::Volume, f));
                                    cx.notify();
                                });
                            }
                        })
                        .w(px(93.)),
                    ),
            )
            .child(dim("pip", "icons/pip.svg").ml(px(8.)))
            .child(dim("fullscreen", "icons/fullscreen.svg"));

        div()
            .flex_none()
            .mx(px(8.))
            .mb(px(8.))
            .flex()
            .flex_col()
            .child(
                div()
                    .h(px(72.))
                    .pt(px(7.6))
                    .px(px(8.))
                    .flex()
                    .flex_row()
                    .items_start()
                    .child(left)
                    .child(center)
                    .child(right),
            )
            .when_some(remote_device, |d, name| {
                d.child(
                    div()
                        .h(px(24.))
                        .rounded(px(4.))
                        .bg(c(GREEN))
                        .flex()
                        .flex_row()
                        .justify_end()
                        .items_center()
                        .gap(px(8.))
                        .pr(px(8.))
                        .text_size(px(14.))
                        .font_weight(FontWeight::BOLD)
                        .text_color(c(0x000000))
                        .child(icon("icons/speaker.svg", 16., 0x000000))
                        .child(format!("Playing on {}", name.to_uppercase())),
                )
            })
    }
}

/// Seek / volume bar. Hover (or an active drag): fill turns green and a 12px knob appears
/// (Spotify behaviour). Pressing starts a drag (`on_press` gets the fraction under the pointer);
/// the root view follows the pointer and commits on release. `bounds` is filled in at paint time.
fn slider(
    id: &'static str,
    frac: f32,
    active: bool,
    bounds: Rc<Cell<Bounds<Pixels>>>,
    on_press: impl Fn(f32, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let frac = frac.clamp(0., 1.);
    let record = bounds.clone();
    let on_set = on_press;
    div()
        .id(id)
        .group(id)
        .relative()
        .h(px(12.))
        .flex()
        .items_center()
        .cursor_pointer()
        .child(canvas(move |b, _, _| record.set(b), |_, _, _, _| {}).absolute().inset_0())
        .child(
            div()
                .relative()
                .w_full()
                .h(px(4.))
                .rounded(px(2.))
                .bg(c(TRACK))
                .child(
                    div()
                        .h_full()
                        .w(relative(frac))
                        .rounded(px(2.))
                        .bg(c(if active { GREEN } else { WHITE }))
                        .group_hover(id, |s| s.bg(c(GREEN))),
                )
                .child(
                    div()
                        .absolute()
                        .top(px(-4.))
                        .left(relative(frac))
                        .ml(px(-6.))
                        .size(px(12.))
                        .rounded_full()
                        .bg(c(WHITE))
                        .opacity(if active { 1. } else { 0. })
                        .group_hover(id, |s| s.opacity(1.)),
                ),
        )
        .on_mouse_down(MouseButton::Left, move |ev, window, cx| {
            let b = bounds.get();
            let w = f32::from(b.size.width);
            if w > 0. {
                let f = (f32::from(ev.position.x - b.origin.x) / w).clamp(0., 1.);
                on_set(f, window, cx);
            }
        })
}

// ---------- root render ----------------------------------------------------------------------

impl Render for Shell {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let _ = &self.api;
        images::set_scale(window.scale_factor());
        if !self.focused_once {
            window.focus(&self.focus);
            self.focused_once = true;
        }
        let vw = f32::from(window.viewport_size().width);
        let left_w = if self.left_collapsed { LEFT_RAIL } else { self.left_w };
        // main view = viewport - outer gutters (8+8) - sidebars - resize gaps (8+8)
        let main_w = vw - 16. - left_w - self.right_w - 16.;
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(c(0x000000))
            .font(ui_font_with_fallbacks())
            .text_size(px(16.))
            .text_color(c(TEXT_SUBDUED))
            .relative()
            .when(self.drag.is_some(), |d| d.cursor(CursorStyle::ResizeColumn))
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|this, ev: &gpui::KeyDownEvent, _, cx| this.shortcut_key(ev, cx)))
            .on_mouse_move(cx.listener(move |this, ev: &MouseMoveEvent, _, cx| {
                if let Some((kind, _)) = this.slider_drag {
                    if !ev.dragging() {
                        this.finish_slider(cx);
                        return;
                    }
                    let b = match kind {
                        Slider::Seek => this.seek_bounds.get(),
                        Slider::Volume => this.volume_bounds.get(),
                    };
                    let w = f32::from(b.size.width);
                    if w > 0. {
                        let f = (f32::from(ev.position.x - b.origin.x) / w).clamp(0., 1.);
                        this.slider_drag = Some((kind, f));
                        cx.notify();
                    }
                    return;
                }
                let Some(side) = this.drag else { return };
                if !ev.dragging() {
                    this.drag = None;
                    cx.notify();
                    return;
                }
                let x = f32::from(ev.position.x);
                match side {
                    // Library: width follows the pointer (x - 8); below ~177 it snaps to the rail.
                    Side::Left => {
                        let w = x - 8.;
                        if w < LEFT_COLLAPSE_AT {
                            this.left_collapsed = true;
                        } else {
                            this.left_collapsed = false;
                            this.left_w = w.clamp(LEFT_MIN, LEFT_MAX);
                        }
                    }
                    // Right panel: its left edge follows the pointer.
                    Side::Right => this.right_w = (vw - 8. - x).clamp(RIGHT_MIN, RIGHT_MAX),
                }
                cx.notify();
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    if this.drag.take().is_some() {
                        cx.notify();
                    }
                    this.finish_slider(cx);
                }),
            )
            .child(self.top_bar(cx))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .px(px(8.))
                    .mb(px(8.))
                    .flex()
                    .flex_row()
                    .child(self.library(window, cx))
                    .child(self.resize_bar(Side::Left, cx))
                    .child({
                        let page = match self.page.clone() {
                            Page::Home => self.home_view(main_w, cx).into_any_element(),
                            Page::Playlist(uri) | Page::Album(uri) => self.playlist_view(&uri, main_w, window, cx),
                            Page::Liked => self.playlist_view(LIKED_URI, main_w, window, cx),
                            Page::Browse => self.browse_view(main_w, window, cx),
                            Page::Artist(uri) => self.artist_view(&uri, main_w, window, cx),
                            Page::User(uri) => self.user_view(&uri, main_w, window, cx),
                        };
                        // GPUI clips children to the panel's rectangle, not its 8px rounded corners, so
                        // full-bleed content (sticky header, About image) would square them off: mask
                        // the corners with the window background, as the client's rounded clip does.
                        let corner = |path: &'static str| svg().path(path).size(px(8.)).absolute().text_color(c(0x000000));
                        div()
                            .relative()
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .flex()
                            .child(page)
                            .child(corner("icons/corner_tl.svg").top_0().left_0())
                            .child(corner("icons/corner_tr.svg").top_0().right_0())
                            .child(corner("icons/corner_bl.svg").bottom_0().left_0())
                            .child(corner("icons/corner_br.svg").bottom_0().right_0())
                    })
                    .child(self.resize_bar(Side::Right, cx))
                    .child(match self.right_panel {
                        RightPanel::NowPlaying => self.now_playing_panel(cx).into_any_element(),
                        RightPanel::Friends => self.friends_panel(cx).into_any_element(),
                    }),
            )
            .child(self.player_bar(cx))
            .children(self.library_menu(cx))
            .children(self.create_menu(window, cx))
            .children(self.profile_menu(window, cx))
            .children(self.ctx_menu_view(window, cx))
            .when_some(self.visible_toast(), |d, text| {
                d.child(
                    div()
                        .absolute()
                        .left_0()
                        .right_0()
                        .bottom(px(120.))
                        .flex()
                        .justify_center()
                        .child(
                            div()
                                .px(px(16.))
                                .py(px(10.))
                                .rounded(px(8.))
                                .bg(c(0xFFFFFF))
                                .text_size(px(14.))
                                .text_color(c(0x000000))
                                .child(text),
                        ),
                )
            })
    }
}

impl Shell {
    /// 8px gap between panels that resizes them. Hover (or drag) shows a 1px #7C7C7C line,
    /// inset 8px top and bottom, centered in the gap.
    fn resize_bar(&self, side: Side, cx: &mut Context<Self>) -> impl IntoElement {
        let id: SharedString = match side {
            Side::Left => "resize-left".into(),
            Side::Right => "resize-right".into(),
        };
        let active = self.drag == Some(side);
        div()
            .id(id.clone())
            .group(id.clone())
            .relative()
            .w(px(8.))
            .h_full()
            .flex_none()
            .cursor(CursorStyle::ResizeColumn)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, _, cx| {
                    this.drag = Some(side);
                    cx.notify();
                }),
            )
            .child(
                div()
                    .absolute()
                    .top(px(8.))
                    .bottom(px(8.))
                    .left(px(3.5))
                    .w(px(1.))
                    .bg(c(0x7C7C7C))
                    .opacity(if active { 1. } else { 0. })
                    .group_hover(id, |s| s.opacity(1.)),
            )
    }

    fn visible_toast(&mut self) -> Option<String> {
        match &self.toast {
            Some((text, at)) if at.elapsed() < Duration::from_secs(3) => Some(text.clone()),
            Some(_) => {
                self.toast = None;
                None
            }
            None => None,
        }
    }
}
