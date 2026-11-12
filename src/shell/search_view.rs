//! Search: the top bar's box with its dropdown, and the results pages.
//! Measured on the reference at 1538x818 (main view 714.4 wide):
//!   Box 474x48 (rounded full, #1F1F1F; #2A2A2A hovered or focused, with a 2px inset white ring
//!   that fades in over .22s). Text 16px from x 48; placeholder #B3B3B3 with "Ctrl" "L" key caps
//!   (1px #7C7C7C, radius 4, padding 2/6) 14px after it while focused. Clear (24px X) centred at
//!   x 397 once there's text; then the divider and Browse.
//!   Dropdown 4px under the box, same width: #2A2A2A, radius 8, padding 8, at most 398 tall
//!   (590 when the window is 1200 or taller), scrolling. Empty box: "Recent searches" (16px bold,
//!   padding 8/8/4), 64px rows, then "Clear recent searches" (32px pill, margin 8). Typing: a
//!   24px hint row (up, down "Navigate" … "Enter" "Search", 12px), then 56px completions (the
//!   typed part bold white) and 64px hits. Rows: radius 6, #404040 hovered, 48px picture (round
//!   for people) with a play overlay on hover, title 16px, second line 14px #B3B3B3.
//!   Results page: sticky 64px chip bar (#121212), content padded by the content spacing.
//!   All: top result card 104 tall (rgba(255,255,255,.1), radius 6, 72px picture, 24px bold
//!   title, 48px green play), 16 below it the "Featuring" shelf (48px header, 133.33px cards)
//!   and 64px rows (#1F1F1F hovered). Songs: a tracklist. The rest: card grids, columns at least
//!   180 wide, 24 below the chips.
//! Behaviour: typing only fills the dropdown; Enter (or a completion) opens `/search/<query>`.
//! Switching chips or searching again replaces the search page in history, and leaving it
//! empties the box. Escape drops the keyboard selection, then the text.

use gpui::ScrollWheelEvent;

use super::browse_view::content_spacing;
use super::playlist_view::{fr_columns, text_width};
use super::*;
use crate::search::{self, Entry, Filter, Hit, HitKind, Results};

const BOX_W: f32 = 474.;
/// Room for the typed text: the box minus its 48 / 96 padding.
const TEXT_W: f32 = 330.;
const DD_BG: u32 = 0x2A2A2A;
const DD_HOVER: u32 = 0x404040;
const SHELF_CARD_W: f32 = 1200. / 9.;
/// A shelf card with a two line title: 12 + 109.33 + 8 + 68 + 12.
const SHELF_H: f32 = 1200. / 9. - 24. + 100.;
const BORDER: u32 = 0x7C7C7C;

pub struct SearchUi {
    pub text: String,
    /// Byte offset of the caret in `text`.
    pub caret: usize,
    /// The whole text is selected (Ctrl+A, Ctrl+L, double click).
    pub all: bool,
    pub focused: bool,
    /// Counts focus changes: restarts the ring / dropdown fades and ends old blink timers.
    pub focus_seq: usize,
    /// Last edit or caret move (the caret blinks from here).
    pub edit_at: Instant,
    /// The dropdown's list while typing, with the query it answers.
    pub suggestions: Option<(String, Vec<Hit>)>,
    pub recents: Option<Vec<Hit>>,
    /// Row picked with the arrow keys.
    pub selected: Option<usize>,
    /// Newest suggestions request (older answers are dropped).
    pub seq: u64,
    pub scroll: gpui::ScrollHandle,
    pub box_bounds: Rc<Cell<Bounds<Pixels>>>,
    pub dd_bounds: Rc<Cell<Bounds<Pixels>>>,
    /// First card shown per shelf, keyed by query and the shelf's place on the page.
    pub shelf_at: HashMap<(String, usize), usize>,
    /// Newest recent-searches read (an older answer would bring back what was just removed).
    pub recents_seq: u64,
}

impl SearchUi {
    pub fn new() -> Self {
        Self {
            text: String::new(),
            caret: 0,
            all: false,
            focused: false,
            focus_seq: 0,
            edit_at: Instant::now(),
            suggestions: None,
            recents: None,
            selected: None,
            seq: 0,
            scroll: gpui::ScrollHandle::new(),
            box_bounds: Rc::default(),
            dd_bounds: Rc::default(),
            shelf_at: HashMap::new(),
            recents_seq: 0,
        }
    }

    fn set_text(&mut self, text: String) {
        self.caret = text.len();
        self.text = text;
        self.all = false;
    }

    /// Replaces the selection (or inserts at the caret).
    fn insert(&mut self, new: &str) {
        if self.all {
            self.text.clear();
            self.caret = 0;
            self.all = false;
        }
        self.text.insert_str(self.caret, new);
        self.caret += new.len();
    }

    fn prev(&self) -> usize {
        self.text[..self.caret].char_indices().next_back().map_or(0, |(i, _)| i)
    }

    fn next(&self) -> usize {
        self.text[self.caret..].chars().next().map_or(self.caret, |ch| self.caret + ch.len_utf8())
    }

    /// Start of the word before the caret (Ctrl+Left, Ctrl+Backspace).
    fn word_start(&self) -> usize {
        let before = &self.text[..self.caret];
        let trimmed = before.trim_end();
        trimmed.rfind(char::is_whitespace).map_or(0, |i| i + trimmed[i..].chars().next().map_or(1, char::len_utf8))
    }

    /// Start of the next word (Ctrl+Right, Ctrl+Delete), as on Windows: the rest of this word and
    /// the spaces after it.
    fn word_end(&self) -> usize {
        let after = &self.text[self.caret..];
        let rest = &after[after.find(char::is_whitespace).unwrap_or(after.len())..];
        self.caret + after.len() - rest.trim_start().len()
    }
}

/// Where a hit row is shown, which decides its paddings and trailing buttons.
#[derive(Clone, Copy, PartialEq)]
enum RowIn {
    Recents,
    Suggestions,
    Page,
}

fn ease_in(t: f32) -> f32 {
    t * t
}

fn name_id(id: String) -> gpui::ElementId {
    gpui::ElementId::Name(id.into())
}

impl Shell {
    // ---------- state ------------------------------------------------------------------------

    /// Puts the caret in the box (Ctrl+L and a click do this) and opens the dropdown.
    pub(super) fn focus_search(&mut self, select_all: bool, cx: &mut Context<Self>) {
        if !self.search.focused {
            self.search.focused = true;
            self.search.focus_seq += 1;
            let seq = self.search.focus_seq;
            // The caret blinks every 530ms, as Chromium's does.
            cx.spawn(async move |this, cx| {
                loop {
                    cx.background_executor().timer(Duration::from_millis(265)).await;
                    let on = this.update(cx, |this, cx| {
                        let on = this.search.focused && this.search.focus_seq == seq;
                        if on {
                            cx.notify();
                        }
                        on
                    });
                    if !on.unwrap_or(false) {
                        break;
                    }
                }
            })
            .detach();
            if self.search.recents.is_none() {
                self.load_recents(cx);
            }
            let query = self.search.text.trim().to_string();
            if !query.is_empty() && self.search.suggestions.as_ref().is_none_or(|s| s.0 != query) {
                self.fetch_suggestions(cx);
            }
        }
        self.search.edit_at = Instant::now();
        if select_all {
            self.search.all = !self.search.text.is_empty();
            self.search.caret = self.search.text.len();
        }
        cx.notify();
    }

    pub(super) fn blur_search(&mut self, cx: &mut Context<Self>) {
        if self.search.focused {
            self.search.focused = false;
            self.search.focus_seq += 1;
            self.search.selected = None;
            self.search.all = false;
            cx.notify();
        }
    }

    /// After a page change: the box shows the search page's query, and nothing elsewhere.
    pub(super) fn sync_search_box(&mut self) {
        match &self.page {
            Page::Search(query, _) => {
                if !self.search.focused {
                    self.search.set_text(query.clone());
                }
            }
            _ => {
                self.search.set_text(String::new());
                self.search.suggestions = None;
            }
        }
    }

    /// The text changed: the selection goes and the dropdown asks for this query.
    fn search_edited(&mut self, cx: &mut Context<Self>) {
        self.search.selected = None;
        self.search.edit_at = Instant::now();
        self.search.scroll.set_offset(gpui::Point::default());
        if self.search.text.trim().is_empty() {
            self.search.seq += 1;
            self.search.suggestions = None;
        } else {
            self.fetch_suggestions(cx);
        }
        cx.notify();
    }

    fn fetch_suggestions(&mut self, cx: &mut Context<Self>) {
        self.search.seq += 1;
        let seq = self.search.seq;
        let query = self.search.text.trim().to_string();
        let Some(sp) = self.spotify.clone() else { return };
        cx.spawn(async move |this, cx| {
            // Typing fast asks once, for what's there when it pauses.
            cx.background_executor().timer(Duration::from_millis(120)).await;
            if this.update(cx, |this, _| this.search.seq != seq).unwrap_or(true) {
                return;
            }
            let q = query.clone();
            let result = cx.background_executor().spawn(async move { sp.search_suggestions(&q) }).await;
            let _ = this.update(cx, |this, cx| {
                if this.search.seq != seq {
                    return;
                }
                match result {
                    Ok(v) => {
                        this.search.suggestions = Some((query, search::parse_suggestions(&v)));
                        this.search.selected = None;
                    }
                    Err(e) => eprintln!("search suggestions: {e:#}"),
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn load_recents(&mut self, cx: &mut Context<Self>) {
        let Some(sp) = self.spotify.clone() else { return };
        self.search.recents_seq += 1;
        let seq = self.search.recents_seq;
        cx.spawn(async move |this, cx| {
            let result = cx.background_executor().spawn(async move { sp.recent_searches() }).await;
            let _ = this.update(cx, |this, cx| {
                if this.search.recents_seq != seq {
                    return;
                }
                match result {
                    Ok(v) => {
                        this.search.recents = Some(search::parse_recents(&v));
                        if this.search.text.trim().is_empty() {
                            this.search.selected = None;
                        }
                    }
                    Err(e) => eprintln!("recent searches: {e:#}"),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Saves (`save`) or removes recent searches on the service, then reads the list again.
    fn change_recents(&mut self, save: bool, uris: Vec<String>, cx: &mut Context<Self>) {
        let Some(sp) = self.spotify.clone() else { return };
        // Test copies leave your search history alone.
        if uris.is_empty() || std::env::var_os("SPOTIRUST_NO_SEARCH_HISTORY").is_some() {
            return;
        }
        cx.spawn(async move |this, cx| {
            let result = cx.background_executor().spawn(async move { sp.change_recent_searches(save, &uris) }).await;
            if let Err(e) = result {
                eprintln!("recent searches: {e:#}");
            }
            let _ = this.update(cx, |this, cx| this.load_recents(cx));
        })
        .detach();
    }

    /// A result you opened goes to the top of the recent searches.
    fn remember_search(&mut self, hit: &Hit, cx: &mut Context<Self>) {
        if hit.kind == HitKind::Query {
            return;
        }
        if let Some(recents) = self.search.recents.as_mut() {
            recents.retain(|h| h.uri != hit.uri);
            recents.insert(0, hit.clone());
        }
        self.search.recents_seq += 1;
        self.change_recents(true, vec![hit.uri.clone()], cx);
    }

    /// Removes one recent search, or all of them.
    fn forget_searches(&mut self, uri: Option<String>, cx: &mut Context<Self>) {
        let Some(recents) = self.search.recents.as_mut() else { return };
        let uris: Vec<String> = match &uri {
            Some(u) => vec![u.clone()],
            None => recents.iter().map(|h| h.uri.clone()).collect(),
        };
        recents.retain(|h| !uris.contains(&h.uri));
        self.search.selected = None;
        self.search.recents_seq += 1;
        self.change_recents(false, uris, cx);
        cx.notify();
    }

    /// The dropdown's rows: recent searches for an empty box, else suggestions.
    fn dropdown_rows(&self) -> Option<(bool, &[Hit])> {
        let s = &self.search;
        if !s.focused {
            return None;
        }
        if s.text.trim().is_empty() {
            s.recents.as_deref().filter(|r| !r.is_empty()).map(|r| (true, r))
        } else {
            s.suggestions.as_ref().map(|x| x.1.as_slice()).filter(|r| !r.is_empty()).map(|r| (false, r))
        }
    }

    /// Opens `/search/<query>`. On a search page already, it takes that page's place in history.
    pub(super) fn submit_search(&mut self, query: String, cx: &mut Context<Self>) {
        self.search.set_text(query.clone());
        self.blur_search(cx);
        let page = Page::Search(query.clone(), Filter::All);
        if matches!(self.page, Page::Search(..)) {
            if self.page != page {
                self.page = page;
                self.page_changed(cx);
            } else {
                // A page that failed to load is asked for again.
                self.load_search(&query, Filter::All, cx);
            }
        } else {
            self.open(page, cx);
        }
    }

    /// The other filter of the same search (chips replace the page, they don't add history).
    fn set_search_filter(&mut self, filter: Filter, cx: &mut Context<Self>) {
        let Page::Search(query, current) = self.page.clone() else { return };
        if current != filter {
            self.page = Page::Search(query, filter);
            self.page_changed(cx);
        } else {
            self.load_search(&query, Filter::All, cx);
            self.load_search(&query, filter, cx);
        }
    }

    /// A click (or Enter) on a hit: completions search, the rest open their page.
    fn activate_hit(&mut self, hit: &Hit, cx: &mut Context<Self>) {
        if hit.kind == HitKind::Query {
            self.submit_search(hit.name.clone(), cx);
            return;
        }
        // Songs have no page of their own here yet: they open their album.
        let page = match hit.kind {
            HitKind::Track => hit.album.as_ref().and_then(|a| Page::for_uri(&a.1)),
            HitKind::Album | HitKind::Playlist | HitKind::Artist | HitKind::User => Page::for_uri(&hit.uri),
            _ => None,
        };
        self.remember_search(hit, cx);
        if let Some(page) = page {
            self.blur_search(cx);
            self.open(page, cx);
        }
    }

    fn play_hit(&mut self, hit: &Hit, cx: &mut Context<Self>) {
        self.remember_search(hit, cx);
        match hit.kind {
            HitKind::Track => {
                if let Some((_, album)) = hit.album.as_ref().filter(|a| !a.1.is_empty()) {
                    self.player(Play::ContextAt(album.clone(), hit.uri.clone()), cx);
                }
            }
            _ if hit.context() => self.player(Play::Context(hit.uri.clone()), cx),
            _ => {}
        }
    }

    /// Follows / saves (or the reverse) an artist or album from its row.
    pub(super) fn set_hit_saved(&mut self, uri: String, saved: bool, cx: &mut Context<Self>) {
        let Some(api) = self.api.clone() else { return };
        let text = if saved { "Added to Your Library." } else { "Removed from Your Library." }.to_string();
        self.background_then(cx, text, move || api.set_in_library(&uri, saved), |this, cx| this.reload_library(cx));
    }

    /// Test hook: the caret in the box with `text` typed (and SPOTIRUST_SEARCH_SELECT rows down).
    pub(super) fn debug_type(&mut self, text: &str, cx: &mut Context<Self>) {
        self.focus_search(false, cx);
        let text = text.trim_matches('_');
        if !text.is_empty() {
            self.search.insert(text);
            self.search_edited(cx);
        }
        let down: usize = std::env::var("SPOTIRUST_SEARCH_SELECT").ok().and_then(|n| n.parse().ok()).unwrap_or(0);
        if down > 0 {
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(Duration::from_secs(6)).await;
                let _ = this.update(cx, |this, cx| {
                    this.search.selected = Some(down - 1);
                    cx.notify();
                });
            })
            .detach();
        }
    }

    // ---------- keyboard ---------------------------------------------------------------------

    /// Keys go to the box while it has the caret. Returns whether the key was taken.
    pub(super) fn search_key(&mut self, ev: &gpui::KeyDownEvent, cx: &mut Context<Self>) -> bool {
        // A context menu opened from a row takes the keys (Escape closes it).
        if !self.search.focused || self.ctx_menu.is_some() {
            return false;
        }
        let k = &ev.keystroke;
        let ctrl = k.modifiers.control;
        let rows = self.dropdown_rows().map(|(recents, hits)| (recents, hits.to_vec()));
        let count = rows.as_ref().map_or(0, |r| r.1.len());
        let mut edited = false;
        let s = &mut self.search;
        match k.key.as_str() {
            "escape" => {
                if s.selected.take().is_none() && !s.text.is_empty() {
                    s.set_text(String::new());
                    edited = true;
                }
            }
            "enter" => {
                let picked = s.selected.and_then(|i| rows.as_ref()?.1.get(i).cloned());
                match picked {
                    Some(hit) => self.activate_hit(&hit, cx),
                    None => {
                        let query = self.search.text.trim().to_string();
                        if !query.is_empty() {
                            self.submit_search(query, cx);
                        }
                    }
                }
                return true;
            }
            "tab" => {
                self.blur_search(cx);
                return true;
            }
            "down" | "up" if !ctrl => {
                if count > 0 {
                    s.selected = match (k.key.as_str(), s.selected.map(|i| i.min(count - 1))) {
                        ("down", None) => Some(0),
                        ("down", Some(i)) => Some((i + 1).min(count - 1)),
                        (_, Some(0)) | (_, None) => None,
                        (_, Some(i)) => Some(i - 1),
                    };
                    let (recents, hits) = rows.as_ref().unwrap();
                    self.reveal_selected(*recents, hits);
                }
            }
            "left" => {
                s.caret = if s.all || ctrl && k.modifiers.shift { 0 } else if ctrl { s.word_start() } else { s.prev() };
                s.all = false;
            }
            "right" => {
                s.caret = if s.all { s.text.len() } else if ctrl { s.word_end() } else { s.next() };
                s.all = false;
            }
            "home" => {
                s.caret = 0;
                s.all = false;
            }
            "end" => {
                s.caret = s.text.len();
                s.all = false;
            }
            "backspace" | "delete" => {
                let back = k.key == "backspace";
                if s.all {
                    s.set_text(String::new());
                } else {
                    let (from, to) = match (back, ctrl) {
                        (true, true) => (s.word_start(), s.caret),
                        (true, false) => (s.prev(), s.caret),
                        (false, true) => (s.caret, s.word_end()),
                        (false, false) => (s.caret, s.next()),
                    };
                    s.text.replace_range(from..to, "");
                    s.caret = from;
                }
                edited = true;
            }
            "a" | "l" if ctrl => {
                s.all = !s.text.is_empty();
                s.caret = s.text.len();
            }
            "c" | "x" if ctrl => {
                if s.all {
                    cx.write_to_clipboard(gpui::ClipboardItem::new_string(s.text.clone()));
                    if k.key == "x" {
                        s.set_text(String::new());
                        edited = true;
                    }
                }
            }
            "v" if ctrl => {
                if let Some(pasted) = cx.read_from_clipboard().and_then(|i| i.text()) {
                    // One line, as a text field pastes it: line breaks become spaces.
                    let line: String = pasted.replace("\r\n", " ").replace(['\r', '\n'], " ").chars().filter(|ch| !ch.is_control()).collect();
                    if !line.is_empty() {
                        s.insert(&line);
                        edited = true;
                    }
                }
            }
            _ if ctrl || k.modifiers.alt || k.modifiers.platform => return false,
            _ => match k.key_char.as_deref() {
                Some(typed) if !typed.chars().any(char::is_control) => {
                    s.insert(typed);
                    edited = true;
                }
                _ => return false,
            },
        }
        if edited {
            self.search_edited(cx);
        } else {
            self.search.edit_at = Instant::now();
        }
        cx.notify();
        true
    }

    /// Scrolls the dropdown so the row picked with the arrow keys is in view.
    fn reveal_selected(&self, recents: bool, hits: &[Hit]) {
        let Some(i) = self.search.selected.filter(|&i| i < hits.len()) else {
            self.search.scroll.set_offset(gpui::Point::default());
            return;
        };
        let height = |h: &Hit| if h.kind == HitKind::Query { 56. } else { 64. };
        let top = 8. + if recents { 34.4 } else { 24. } + hits[..i].iter().map(height).sum::<f32>();
        let bottom = top + height(&hits[i]);
        let view = f32::from(self.search.dd_bounds.get().size.height);
        let y = -f32::from(self.search.scroll.offset().y);
        let target = if top < y + 8. {
            top - 8.
        } else if bottom > y + view - 8. {
            bottom - view + 8.
        } else {
            return;
        };
        self.search.scroll.set_offset(gpui::point(px(0.), px(-target.max(0.))));
    }

    /// How far the typed text is scrolled left so the caret stays in the box.
    fn search_scroll_x(&self, window: &Window) -> f32 {
        let s = &self.search;
        let font = gpui::font(ui_font());
        let total = text_width(window, &s.text, 16., font.clone());
        let room = TEXT_W - 2.;
        if total <= room {
            return 0.;
        }
        let caret = text_width(window, &s.text[..s.caret], 16., font);
        (caret - room).clamp(0., total - room)
    }

    // ---------- the box ----------------------------------------------------------------------

    pub(super) fn search_box(&self, window: &Window, cx: &mut Context<Self>) -> impl IntoElement {
        let s = &self.search;
        let focused = s.focused;
        let seq = s.focus_seq;
        let has_text = !s.text.is_empty();
        let font = gpui::font(ui_font());
        let scroll_x = self.search_scroll_x(window);
        let caret_x = text_width(window, &s.text[..s.caret], 16., font) - scroll_x;
        let caret_on = focused && !s.all && (s.edit_at.elapsed().as_millis() / 530) % 2 == 0;
        let fade = || Animation::new(Duration::from_millis(220)).with_easing(ease_in);
        let hover_t = self.fades.get("search");
        let icon_s = if self.fades.on("search-icon") { 1.04 } else { 1. };

        // 2px inset ring: fades in with focus and out without it.
        let ring = div().absolute().inset_0().rounded_full().border_2().border_color(c(WHITE));
        let ring = if focused {
            ring.with_animation(name_id(format!("search-ring-in-{seq}")), fade(), |d, t| d.opacity(t)).into_any_element()
        } else if seq > 0 {
            ring.with_animation(name_id(format!("search-ring-out-{seq}")), fade(), |d, t| d.opacity(1. - t)).into_any_element()
        } else {
            div().into_any_element()
        };

        let key_cap = |label: &'static str| {
            div()
                .flex_none()
                .h(px(26.4))
                .px(px(6.))
                .rounded(px(4.))
                .border_1()
                .border_color(c(BORDER))
                .flex()
                .items_center()
                .text_size(px(16.))
                .line_height(px(22.4))
                .text_color(c(TEXT_SUBDUED))
                .child(label)
        };
        let content = if has_text {
            div()
                .absolute()
                .left(px(-scroll_x))
                .top(px(12.8))
                .whitespace_nowrap()
                .text_size(px(16.))
                .line_height(px(22.4))
                .text_color(c(WHITE))
                .when(s.all, |d| d.bg(rgba(0x3390FF99)))
                .child(s.text.clone())
                .into_any_element()
        } else {
            let hint = div().ml(px(14.)).flex().flex_row().gap(px(4.)).child(key_cap("Ctrl")).child(key_cap("L"));
            div()
                .absolute()
                .left_0()
                .top_0()
                .h(px(48.))
                .flex()
                .flex_row()
                .items_center()
                .whitespace_nowrap()
                .text_size(px(16.))
                .line_height(px(22.4))
                .text_color(c(TEXT_SUBDUED))
                .child("What do you want to play?")
                .when(focused, |d| {
                    d.child(hint.with_animation(name_id(format!("search-hint-{seq}")), fade(), |d, t| d.opacity(t)))
                })
                .into_any_element()
        };

        let bounds = s.box_bounds.clone();
        div()
            .id("search")
            .occlude()
            .relative()
            .w(px(BOX_W))
            .h(px(48.))
            .rounded_full()
            // measured: background, box-shadow (1px inset rgba(255,255,255,.1)), color .22s ease-in
            .bg(if focused { c(0x2A2A2A) } else { super::fx::mix(0x1F1F1FFF, 0x2A2A2AFF, hover_t) })
            .child(div().absolute().inset_0().rounded_full().border_1().border_color(super::fx::mix(0xFFFFFF00, 0xFFFFFF1A, hover_t)))
            .cursor(CursorStyle::IBeam)
            .on_hover(cx.listener(|this, h: &bool, _, cx| {
                let tr = super::fx::SEARCH;
                this.fades.hover("search".into(), *h, if *h { tr.enter } else { tr.leave }, tr.ease);
                cx.notify();
            }))
            .child(canvas(move |b, _, _| bounds.set(b), |_, _, _, _| {}).absolute().inset_0())
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, ev: &MouseDownEvent, window, cx| {
                    // Caret to the character boundary nearest the click.
                    let x = f32::from(ev.position.x - this.search.box_bounds.get().origin.x) - 48. + this.search_scroll_x(window);
                    let font = gpui::font(ui_font());
                    let text = this.search.text.clone();
                    let mut best = (f32::MAX, 0);
                    for i in text.char_indices().map(|(i, _)| i).chain([text.len()]) {
                        let d = (text_width(window, &text[..i], 16., font.clone()) - x).abs();
                        if d < best.0 {
                            best = (d, i);
                        }
                    }
                    this.search.caret = best.1;
                    this.search.all = false;
                    this.search.selected = None;
                    this.focus_search(ev.click_count >= 2, cx);
                }),
            )
            .on_mouse_down_out(cx.listener(|this, ev: &MouseDownEvent, _, cx| {
                if this.search.focused && !this.search.dd_bounds.get().contains(&ev.position) {
                    this.blur_search(cx);
                }
            }))
            .child(
                // measured: white while the field is hovered or focused (color .22s ease-in); the
                // icon itself scales 1.04 under the pointer
                self.on_fade(
                    div().id("search-icon").absolute().left(px(12.)).top(px(12.)).size(px(24.)).child(
                        super::fx::scaled_icon("icons/search.svg", 24., icon_s)
                            .text_color(if focused { c(WHITE) } else { super::fx::mix(0xB3B3B3FF, 0xFFFFFFFF, hover_t) }),
                    ),
                    "search-icon",
                    super::fx::INSTANT,
                    cx,
                ),
            )
            .child(
                div()
                    .absolute()
                    .left(px(48.))
                    .top_0()
                    .w(px(TEXT_W))
                    .h(px(48.))
                    .overflow_hidden()
                    .child(content)
                    .when(caret_on, |d| d.child(div().absolute().left(px(caret_x)).top(px(13.)).w(px(1.)).h(px(22.)).bg(c(WHITE)))),
            )
            .child(ring)
            .when(has_text, |d| {
                d.child(
                    div()
                        .id("search-clear")
                        .absolute()
                        .left(px(381.4))
                        .top(px(8.))
                        .size(px(32.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .cursor_pointer()
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.search.set_text(String::new());
                            this.search_edited(cx);
                            this.focus_search(false, cx);
                        }))
                        .child(svg().path("icons/close24.svg").size(px(24.)).text_color(c(TEXT_SUBDUED)).hover(|s| s.text_color(c(WHITE)))),
                )
            })
            .child(div().absolute().left(px(421.2)).top(px(12.)).w(px(1.)).h(px(24.)).bg(c(BORDER)))
            .child(
                div()
                    .id("browse-button")
                    .absolute()
                    .left(px(434.))
                    .top(px(12.))
                    .cursor_pointer()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.blur_search(cx);
                        this.open(Page::Browse, cx);
                    }))
                    .on_hover(cx.listener(|this, h: &bool, _, cx| {
                        let tr = super::fx::TERTIARY;
                        this.fades.hover("browse-button".into(), *h, if *h { tr.enter } else { tr.leave }, tr.ease);
                        cx.notify();
                    }))
                    .child({
                        let t = self.fades.get("browse-button");
                        if self.page == Page::Browse {
                            super::fx::scaled_icon("icons/browse_filled.svg", 24., 1. + 0.04 * t).text_color(c(WHITE))
                        } else {
                            super::fx::scaled_icon("icons/browse.svg", 24., 1. + 0.04 * t).text_color(super::fx::mix(0xB3B3B3FF, 0xFFFFFFFF, t))
                        }
                    }),
            )
    }

    // ---------- the dropdown -----------------------------------------------------------------

    pub(super) fn search_dropdown(&self, window: &Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let Some((recents, hits)) = self.dropdown_rows() else {
            self.search.dd_bounds.set(Bounds::default());
            return None;
        };
        let vw = f32::from(window.viewport_size().width);
        let vh = f32::from(window.viewport_size().height);
        let x = (vw - 530.) / 2. + 56.;
        let max_h = if vh >= 1200. { 590. } else { 398. };
        let typed = self.search.suggestions.as_ref().map(|s| s.0.clone()).unwrap_or_default();

        let header = if recents {
            div()
                .flex_none()
                .pt(px(8.))
                .px(px(8.))
                .pb(px(4.))
                .text_size(px(16.))
                .line_height(px(22.4))
                .font_weight(FontWeight::BOLD)
                .text_color(c(WHITE))
                .child("Recent searches")
        } else {
            let arrow = |path: &'static str| {
                div()
                    .size(px(20.))
                    .flex_none()
                    .rounded(px(4.))
                    .border_1()
                    .border_color(c(TEXT_SUBDUED))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(icon(path, 12.4, TEXT_SUBDUED))
            };
            let small = |text: &'static str| div().flex_none().text_size(px(12.)).line_height(px(16.8)).text_color(c(TEXT_SUBDUED)).child(text);
            div()
                .flex_none()
                .h(px(24.))
                .px(px(4.))
                .pb(px(4.))
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(8.))
                        .child(arrow("icons/arrow_up16.svg"))
                        .child(arrow("icons/arrow_down16.svg"))
                        .child(small("Navigate")),
                )
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(8.))
                        .child(
                            div()
                                .h(px(20.))
                                .px(px(4.))
                                .rounded(px(4.))
                                .border_1()
                                .border_color(c(TEXT_SUBDUED))
                                .flex()
                                .items_center()
                                .text_size(px(12.))
                                .line_height(px(18.))
                                .text_color(c(TEXT_SUBDUED))
                                .child("Enter"),
                        )
                        .child(small("Search")),
                )
        };

        let place = if recents { RowIn::Recents } else { RowIn::Suggestions };
        let rows: Vec<AnyElement> = hits
            .iter()
            .enumerate()
            .map(|(i, h)| self.hit_row(format!("dd-{i}"), h, place, 458., self.search.selected == Some(i), &typed, window, cx))
            .collect();
        let footer = recents.then(|| {
            div().flex_none().p(px(8.)).flex().child(
                div()
                    .id("dd-clear-recents")
                    .h(px(32.))
                    .px(px(16.))
                    .rounded_full()
                    .border_1()
                    .border_color(c(BORDER))
                    .hover(|s| s.border_color(c(WHITE)))
                    .flex()
                    .items_center()
                    .text_size(px(14.))
                    .line_height(px(19.2))
                    .font_weight(FontWeight::BOLD)
                    .text_color(c(WHITE))
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _, _, cx| this.forget_searches(None, cx)))
                    .child("Clear recent searches"),
            )
        });

        let bounds = self.search.dd_bounds.clone();
        let shadow = |y: f32, blur: f32, alpha: f32| gpui::BoxShadow {
            color: gpui::hsla(0., 0., 0., alpha),
            offset: gpui::point(px(0.), px(y)),
            blur_radius: px(blur),
            spread_radius: px(0.),
        };
        Some(
            div()
                .id("search-dropdown")
                .occlude()
                .absolute()
                .left(px(x))
                .top(px(60.))
                .w(px(BOX_W))
                .rounded(px(8.))
                .bg(c(DD_BG))
                .shadow(vec![shadow(16., 24., 0.3), shadow(6., 8., 0.2)])
                .overflow_hidden()
                .child(canvas(move |b, _, _| bounds.set(b), |_, _, _, _| {}).absolute().inset_0())
                .child(
                    div()
                        .id("search-dropdown-scroll")
                        .track_scroll(&self.search.scroll)
                        .overflow_y_scroll()
                        .max_h(px(max_h))
                        .p(px(8.))
                        .flex()
                        .flex_col()
                        .child(header)
                        .children(rows)
                        .children(footer),
                )
                .with_animation(
                    name_id(format!("search-dd-{}", self.search.focus_seq)),
                    Animation::new(Duration::from_millis(220)).with_easing(ease_in),
                    |d, t| d.opacity(t),
                )
                .into_any_element(),
        )
    }

    /// One result row: picture, title, second line and its buttons.
    #[allow(clippy::too_many_arguments)]
    fn hit_row(
        &self,
        id: String,
        hit: &Hit,
        place: RowIn,
        row_w: f32,
        selected: bool,
        typed: &str,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let group: SharedString = id.clone().into();
        let query = hit.kind == HitKind::Query;
        let page = place == RowIn::Page;
        let (pad_l, pad_r) = if page { (12., 12.) } else { (8., 4.) };
        let hover_bg = if page { ELEVATED } else { DD_HOVER };
        let radius = if hit.round() || query { 24. } else { 4. };
        let can_play = matches!(hit.kind, HitKind::Track | HitKind::Album | HitKind::Playlist | HitKind::Artist);

        // ---- picture, with the play overlay ----
        let media = div()
            .relative()
            .size(px(48.))
            .flex_none()
            .child(if query {
                div()
                    .size(px(48.))
                    .rounded_full()
                    .bg(c(PLACEHOLDER))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(icon("icons/search.svg", 24., TEXT_SUBDUED))
                    .into_any_element()
            } else {
                cover(hit.image(64).as_deref(), 48., radius, cx)
            })
            .when(can_play, |d| {
                let h = hit.clone();
                d.child(
                    div()
                        .id(name_id(format!("{id}-play")))
                        .absolute()
                        .inset_0()
                        .rounded(px(radius))
                        .bg(rgba(0x00000080))
                        .flex()
                        .items_center()
                        .justify_center()
                        .opacity(if selected { 1. } else { 0. })
                        .group_hover(group.clone(), |s| s.opacity(1.))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            this.play_hit(&h, cx);
                        }))
                        .child(icon("icons/play24.svg", 24., WHITE)),
                )
            });

        // ---- trailing buttons ----
        let mut trailing: Vec<AnyElement> = Vec::new();
        let mut burst = if hit.kind == HitKind::Track { self.like_burst(&hit.uri, 16.) } else { None };
        let mut circle_button = |id: String, on: bool| {
            div().id(name_id(id)).w(px(16.)).h(px(32.)).flex_none().flex().items_center().cursor_pointer().child(if let Some(b) = burst.take() {
                b
            } else if on {
                icon("icons/check_circle.svg", 16., GREEN).into_any_element()
            } else {
                svg().path("icons/add_circle.svg").size(px(16.)).text_color(c(TEXT_SUBDUED)).hover(|s| s.text_color(c(WHITE))).into_any_element()
            })
        };
        if place == RowIn::Recents {
            let uri = hit.uri.clone();
            trailing.push(
                div()
                    .id(name_id(format!("{id}-remove")))
                    .size(px(32.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .opacity(if selected { 1. } else { 0. })
                    .group_hover(group.clone(), |s| s.opacity(1.))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        this.forget_searches(Some(uri.clone()), cx);
                    }))
                    .child(svg().path("icons/close16.svg").size(px(16.)).text_color(c(TEXT_SUBDUED)).hover(|s| s.text_color(c(WHITE))))
                    .into_any_element(),
            );
        } else if !query {
            if page {
                let target = self.hit_target(hit);
                trailing.push(
                    div()
                        .id(name_id(format!("{id}-more")))
                        .w(px(16.))
                        .h(px(32.))
                        .mr(px(8.))
                        .flex_none()
                        .flex()
                        .items_center()
                        .cursor_pointer()
                        .opacity(0.)
                        .group_hover(group.clone(), |s| s.opacity(1.))
                        .on_click(cx.listener(move |this, ev: &gpui::ClickEvent, _, cx| {
                            cx.stop_propagation();
                            if let Some(t) = target.clone() {
                                this.open_ctx_menu(t, ev.position(), false, cx);
                            }
                        }))
                        .child(svg().path("icons/more.svg").size(px(16.)).text_color(c(TEXT_SUBDUED)).hover(|s| s.text_color(c(WHITE))))
                        .into_any_element(),
                );
            }
            match hit.kind {
                HitKind::Artist => {
                    let (uri, following) = (hit.uri.clone(), self.in_library(&hit.uri));
                    trailing.push(
                        div()
                            .id(name_id(format!("{id}-follow")))
                            .h(px(32.))
                            .px(px(16.))
                            .flex_none()
                            .rounded_full()
                            .border_1()
                            .border_color(c(BORDER))
                            .hover(|s| s.border_color(c(WHITE)))
                            .flex()
                            .items_center()
                            .text_size(px(14.))
                            .line_height(px(19.2))
                            .font_weight(FontWeight::BOLD)
                            .text_color(c(WHITE))
                            .cursor_pointer()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                cx.stop_propagation();
                                this.set_hit_saved(uri.clone(), !following, cx);
                            }))
                            .child(if following { "Following" } else { "Follow" })
                            .into_any_element(),
                    );
                }
                HitKind::Track => {
                    let (uri, liked) = (hit.uri.clone(), self.liked.contains(&hit.uri));
                    trailing.push(
                        circle_button(format!("{id}-add"), liked)
                            .on_click(cx.listener(move |this, ev: &gpui::ClickEvent, _, cx| {
                                cx.stop_propagation();
                                if liked {
                                    let (x, y, b) = Self::anchor_of(ev, 16.);
                                    this.open_add_popover(uri.clone(), (x, y - 8., b + 8.), cx);
                                } else {
                                    this.like_track(uri.clone(), cx);
                                }
                            }))
                            .into_any_element(),
                    );
                }
                HitKind::Album => {
                    let (uri, saved) = (hit.uri.clone(), self.in_library(&hit.uri));
                    trailing.push(
                        circle_button(format!("{id}-save"), saved)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                cx.stop_propagation();
                                this.set_hit_saved(uri.clone(), !saved, cx);
                            }))
                            .into_any_element(),
                    );
                }
                _ => {}
            }
        }
        // Measured: 12px between the text and the buttons; the dropdown's buttons end 8px
        // before the row's padding (the recent row's X fills it).
        let trailing_w: f32 = match (place, hit.kind) {
            (RowIn::Recents, _) => 32.,
            (_, HitKind::Query) => 0.,
            (_, HitKind::Artist) => 77. + if page { 40. } else { 8. },
            (_, HitKind::Track | HitKind::Album) => 16. + if page { 40. } else { 8. },
            _ => 0.,
        };
        let text_w = row_w - pad_l - 48. - 12. - pad_r - if trailing_w > 0. { trailing_w + 12. } else { 0. };

        // ---- texts ----
        let texts = if query {
            // The typed part stands out, the completion follows in grey.
            let lower = hit.name.to_lowercase();
            let typed = typed.to_lowercase();
            let split = if !typed.is_empty() && lower.starts_with(&typed) && hit.name.is_char_boundary(typed.len()) { typed.len() } else { 0 };
            div()
                .w(px(text_w))
                .flex_none()
                .flex()
                .flex_row()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_size(px(16.))
                .line_height(px(22.4))
                .child(div().flex_none().font_weight(FontWeight::BOLD).text_color(c(WHITE)).child(hit.name[..split].to_string()))
                .child(one_line(hit.name[split..].to_string()).min_w_0().text_color(c(TEXT_SUBDUED)))
        } else {
            let title = div()
                .flex()
                .flex_row()
                .items_center()
                .child(
                    one_line(hit.name.clone())
                        .id(name_id(format!("{id}-title")))
                        .min_w_0()
                        .text_size(px(16.))
                        .line_height(px(22.4))
                        .text_color(c(WHITE))
                        .hover(|s| s.underline()),
                )
                .when(hit.verified, |d| d.child(div().ml(px(4.)).flex_none().child(icon("icons/verified.svg", 12., 0xC5F0C9))));
            div()
                .w(px(text_w))
                .flex_none()
                .flex()
                .flex_col()
                .gap(px(2.))
                .child(title)
                .child(self.hit_subtitle(&id, hit, text_w, window, cx))
        };

        let h2 = hit.clone();
        let mut row = div()
            .id(name_id(id.clone()))
            .group(group.clone())
            .relative()
            .flex_none()
            .w(px(row_w))
            .h(px(if query { 56. } else { 64. }))
            .pl(px(pad_l))
            .pr(px(pad_r))
            .rounded(px(6.))
            .flex()
            .flex_row()
            .items_center()
            .gap(px(12.))
            .cursor_pointer()
            .hover(move |s| s.bg(c(hover_bg)))
            .when(selected, |d| d.bg(c(hover_bg)))
            .on_click(cx.listener(move |this, _, _, cx| this.activate_hit(&h2, cx)))
            .child(media)
            .child(texts);
        if let Some(target) = self.hit_target(hit) {
            row = row.on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, ev: &MouseDownEvent, _, cx| {
                    cx.stop_propagation();
                    this.open_ctx_menu(target.clone(), ev.position, false, cx);
                }),
            );
        }
        if !trailing.is_empty() {
            row = row.child(
                div()
                    .flex_1()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_end()
                    .gap(px(16.))
                    .when(!page && place != RowIn::Recents, |d| d.pr(px(8.)))
                    .children(trailing),
            );
        }
        // Keyboard selection: a 2px white outline 1px outside the row, with a dark edge around it.
        row.when(selected, |d| {
            let ring = |out: f32| div().absolute().top(px(-out)).left(px(-out)).right(px(-out)).bottom(px(-out)).rounded(px(6. + out));
            d.child(ring(4.).border_1().border_color(rgba(0x00000099))).child(ring(3.).border_2().border_color(c(WHITE)))
        })
        .into_any_element()
    }

    /// The row's second line: [E] "Song • Artist, Artist" with each name a link.
    fn hit_subtitle(&self, id: &str, hit: &Hit, room: f32, window: &Window, cx: &mut Context<Self>) -> Div {
        let names = hit.people.iter().map(|p| p.0.as_str()).collect::<Vec<_>>().join(", ");
        let lead = if hit.people.is_empty() { hit.label.to_string() } else { format!("{} \u{2022} ", hit.label) };
        let full = format!("{lead}{names}");
        let room = room - if hit.explicit { 20. } else { 0. };
        let links = hit.people.iter().all(|p| !p.1.is_empty()) && text_width(window, &full, 14., gpui::font(ui_font())) <= room;
        div()
            .flex()
            .flex_row()
            .items_center()
            .text_size(px(14.))
            .line_height(px(19.2))
            .text_color(c(TEXT_SUBDUED))
            .when(hit.explicit, |d| {
                d.child(
                    div()
                        .size(px(16.))
                        .mr(px(4.))
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
            .child(if links {
                let mut line = div().flex().flex_row().whitespace_nowrap().child(div().flex_none().child(lead));
                for (k, (name, uri)) in hit.people.iter().enumerate() {
                    if k > 0 {
                        line = line.child(div().flex_none().child(", "));
                    }
                    line = line.child(self.link(format!("{id}-p{k}"), name.clone(), uri.clone(), cx));
                }
                line.into_any_element()
            } else {
                one_line(full).min_w_0().into_any_element()
            })
    }

    /// What a right click (or the row's "…") acts on.
    fn hit_target(&self, hit: &Hit) -> Option<ctx_menu::Target> {
        match hit.kind {
            HitKind::Track => Some(ctx_menu::Target::Track {
                uri: hit.uri.clone(),
                name: hit.name.clone(),
                album_uri: hit.album.as_ref().map(|a| a.1.clone()).unwrap_or_default(),
                artists: hit.people.clone(),
                editable_playlist: None,
                on_album: false,
            }),
            HitKind::Album | HitKind::Playlist | HitKind::Artist => self.target_for(&hit.uri, &hit.name),
            _ => None,
        }
    }

    // ---------- results pages ----------------------------------------------------------------

    pub(super) fn load_search(&mut self, query: &str, filter: Filter, cx: &mut Context<Self>) {
        let Some(sp) = self.spotify.clone() else { return };
        let key = (query.to_string(), filter);
        if self.search_pages.contains_key(&key) || !self.loading_pages.insert(format!("search:{}:{query}", filter.path())) {
            return;
        }
        // A few searches back stay ready for Back; older ones are fetched again.
        if self.search_pages.len() > 40 {
            let keep = query.to_string();
            self.search_pages.retain(|k, _| k.0 == keep);
        }
        cx.spawn(async move |this, cx| {
            let (q, f) = key.clone();
            let result = cx
                .background_executor()
                .spawn(async move { if f == Filter::All { sp.search_top(&q, 0) } else { sp.search_kind(f.path(), &q, 0) } })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.loading_pages.remove(&format!("search:{}:{}", key.1.path(), key.0));
                match result {
                    Ok(v) => {
                        this.search_pages.insert(key.clone(), search::parse_results(key.1, &v));
                        this.check_search_end(cx);
                    }
                    Err(e) => eprintln!("search: {e:#}"),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Once the page has laid out, loads more if its end is already near (a tall window, or
    /// scrolling that didn't use the wheel).
    fn check_search_end(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_millis(150)).await;
            let _ = this.update(cx, |this, cx| this.search_near_end(cx));
        })
        .detach();
    }

    /// Near the end of a filter page: the next page of results joins it.
    fn search_near_end(&mut self, cx: &mut Context<Self>) {
        let Page::Search(query, filter) = self.page.clone() else { return };
        if filter == Filter::All {
            return;
        }
        let left = f32::from(self.page_scroll.max_offset().height) + f32::from(self.page_scroll.offset().y);
        if left > 900. {
            return;
        }
        let key = (query.clone(), filter);
        let Some(next) = self.search_pages.get(&key).and_then(|r| r.next.filter(|n| (*n as usize) < r.total)) else { return };
        let Some(sp) = self.spotify.clone() else { return };
        let loading = format!("search-more:{}:{query}", filter.path());
        if !self.loading_pages.insert(loading.clone()) {
            return;
        }
        cx.spawn(async move |this, cx| {
            let q = query.clone();
            let result = cx.background_executor().spawn(async move { sp.search_kind(filter.path(), &q, next) }).await;
            let _ = this.update(cx, |this, cx| {
                this.loading_pages.remove(&loading);
                let more = match result {
                    Ok(v) => search::parse_results(filter, &v),
                    Err(e) => {
                        eprintln!("search: {e:#}");
                        return;
                    }
                };
                if let Some(page) = this.search_pages.get_mut(&key) {
                    // Pages of only unsupported items still move the offset on.
                    page.next = more.next;
                    page.entries.extend(more.entries);
                }
                this.check_search_end(cx);
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn search_view(&self, query: &str, filter: Filter, main_w: f32, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let pad = content_spacing(main_w);
        let w = main_w - 2. * pad;
        let all = self.search_pages.get(&(query.to_string(), Filter::All));
        let results = self.search_pages.get(&(query.to_string(), filter));

        let chip = |f: Filter| {
            let selected = f == filter;
            div()
                .id(name_id(format!("search-chip-{}", f.path())))
                .h(px(32.))
                .px(px(12.))
                .flex_none()
                .flex()
                .items_center()
                .rounded_full()
                .cursor_pointer()
                .text_size(px(14.))
                .when(selected, |d| d.bg(c(WHITE)).text_color(c(0x000000)))
                .when(!selected, |d| d.bg(rgba(0xFFFFFF1A)).text_color(c(WHITE)).hover(|s| s.bg(rgba(0xFFFFFF24))).active(|s| s.bg(rgba(0xFFFFFF36))))
                .on_click(cx.listener(move |this, _, _, cx| this.set_search_filter(f, cx)))
                .child(f.label())
        };
        let chips: Vec<Filter> = all.map(|r| r.chips.clone()).unwrap_or_default();
        let chip_bar = div()
            .absolute()
            .top_0()
            .left_0()
            .right_0()
            .h(px(64.))
            .bg(c(SURFACE))
            .px(px(pad))
            .overflow_hidden()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(8.))
            .when(all.is_some(), |d| d.child(chip(Filter::All)).children(chips.into_iter().map(chip)));

        let body = match (filter, results) {
            (_, None) => div().into_any_element(),
            (_, Some(r)) if r.entries.is_empty() => self.search_empty(query),
            (Filter::All, Some(r)) => self.search_all(r, w, window, cx),
            (Filter::Tracks, Some(r)) => self.search_tracks(r, main_w, pad, window, cx),
            (_, Some(r)) => self.search_grid(r, w, cx),
        };

        div()
            .flex_1()
            .min_w_0()
            .h_full()
            .rounded(px(8.))
            .bg(c(SURFACE))
            .overflow_hidden()
            .relative()
            .child(
                div()
                    .id(name_id(format!("search-scroll-{}", filter.path())))
                    .track_scroll(&self.page_scroll)
                    .absolute()
                    .inset_0()
                    .overflow_y_scroll()
                    .on_scroll_wheel(cx.listener(|this, _: &ScrollWheelEvent, _, cx| this.search_near_end(cx)))
                    .flex()
                    .flex_col()
                    .child(div().h(px(64.)).flex_none())
                    .child(div().px(px(pad)).pb(px(16.)).flex().flex_col().child(body)),
            )
            .child(chip_bar)
            .into_any_element()
    }

    fn search_empty(&self, query: &str) -> AnyElement {
        div()
            .pt(px(120.))
            .flex()
            .flex_col()
            .items_center()
            .child(
                div()
                    .font_family(title_font())
                    .text_size(px(24.))
                    .line_height(px(28.8))
                    .font_weight(FontWeight::BOLD)
                    .text_color(c(WHITE))
                    .child(format!("No results found for \"{query}\"")),
            )
            .child(
                div()
                    .mt(px(16.))
                    .text_size(px(16.))
                    .line_height(px(22.4))
                    .text_color(c(WHITE))
                    .child("Please make sure your words are spelled correctly, or use fewer or different keywords."),
            )
            .into_any_element()
    }

    /// "All": the top result's card, shelves and rows in the service's order.
    fn search_all(&self, r: &Results, w: f32, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let mut out = div().flex().flex_col();
        let mut first = true;
        for (i, entry) in r.entries.iter().enumerate() {
            out = match entry {
                Entry::Hit(h) if first => {
                    first = false;
                    out.child(self.top_result(h, w, window, cx))
                }
                Entry::Hit(h) => out.child(self.hit_row(format!("sr-{i}"), h, RowIn::Page, w, false, "", window, cx)),
                Entry::Section { title, hits } => out.child(self.search_shelf(i, title, hits, w, cx)),
            };
        }
        out.into_any_element()
    }

    fn top_result(&self, hit: &Hit, w: f32, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let radius = if hit.round() { 36. } else { 4. };
        let can_play = matches!(hit.kind, HitKind::Track | HitKind::Album | HitKind::Playlist | HitKind::Artist);
        let text_w = w - 32. - 72. - 24. - if can_play { 72. } else { 0. };
        let (h1, h2) = (hit.clone(), hit.clone());
        let mut card = div()
            .id("search-top")
            .w(px(w))
            .h(px(104.))
            .mb(px(16.))
            .flex_none()
            .p(px(16.))
            .rounded(px(6.))
            .bg(rgba(0xFFFFFF1A))
            .hover(|s| s.bg(rgba(0xFFFFFF24)))
            .active(|s| s.bg(rgba(0xFFFFFF36)))
            .cursor_pointer()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(24.))
            .on_click(cx.listener(move |this, _, _, cx| this.activate_hit(&h1, cx)))
            .child(
                div()
                    .size(px(72.))
                    .flex_none()
                    .rounded(px(radius))
                    .shadow(vec![gpui::BoxShadow {
                        color: gpui::hsla(0., 0., 0., 0.5),
                        offset: gpui::point(px(0.), px(4.)),
                        blur_radius: px(60.),
                        spread_radius: px(0.),
                    }])
                    .child(cover(hit.image(160).as_deref(), 72., radius, cx)),
            )
            .child(
                div()
                    .w(px(text_w))
                    .flex_none()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .child(
                        div().flex().child(
                            one_line(hit.name.clone())
                                .id("search-top-title")
                                .min_w_0()
                                .font_family(title_font())
                                .text_size(px(24.))
                                .line_height(px(28.8))
                                .font_weight(FontWeight::BOLD)
                                .text_color(c(WHITE))
                                .hover(|s| s.underline()),
                        ),
                    )
                    .child(self.hit_subtitle("search-top", hit, text_w, window, cx)),
            );
        if let Some(target) = self.hit_target(hit) {
            card = card.on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, ev: &MouseDownEvent, _, cx| {
                    cx.stop_propagation();
                    this.open_ctx_menu(target.clone(), ev.position, false, cx);
                }),
            );
        }
        card.when(can_play, |d| {
            d.child(
                div().flex_1().flex().justify_end().child(
                    green_play(48.).child(icon("icons/play.svg", 48. * 0.42, 0x000000))
                        .id("search-top-play")
                        .hover(|s| s.bg(c(GREEN_HOVER)))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            this.play_hit(&h2, cx);
                        })),
                ),
            )
        })
        .into_any_element()
    }

    /// A shelf of cards ("Featuring …"): as many as fit, the rest reached with the arrows.
    fn search_shelf(&self, index: usize, title: &str, hits: &[Hit], w: f32, cx: &mut Context<Self>) -> AnyElement {
        let group: SharedString = format!("search-shelf-{index}").into();
        let visible = (w / SHELF_CARD_W).floor().max(1.) as usize;
        let last_start = hits.len().saturating_sub(visible);
        let Page::Search(query, _) = &self.page else { return div().into_any_element() };
        let shelf_key = (query.clone(), index);
        let at = self.search.shelf_at.get(&shelf_key).copied().unwrap_or(0).min(last_start);
        let cards: Vec<AnyElement> = hits
            .iter()
            .enumerate()
            .map(|(i, h)| {
                let hit = h.clone();
                div()
                    .id(name_id(format!("ssw-{index}-{i}")))
                    .flex_none()
                    .on_click(cx.listener(move |this, _, _, cx| this.remember_search(&hit, cx)))
                    .child(self.sized_card(format!("ss-{index}-{i}").into(), &h.card(), SHELF_CARD_W, 4., cx))
                    .into_any_element()
            })
            .collect();
        let arrow = |id: String, path: &'static str, to: usize| {
            let shelf_key = shelf_key.clone();
            div()
                .id(name_id(id))
                .absolute()
                .top(px(SHELF_H / 2. - 16.))
                .size(px(32.))
                .rounded_full()
                .bg(c(ELEVATED))
                .hover(|s| s.bg(c(0x2A2A2A)))
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .opacity(0.)
                .group_hover(group.clone(), |s| s.opacity(1.))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.search.shelf_at.insert(shelf_key.clone(), to);
                    cx.notify();
                }))
                .child(icon_rgba(path, 16., 0xFFFFFFB3))
        };
        let fade = |start: bool| {
            let (from, to) = if start { (0x121212FF, 0x12121200) } else { (0x12121200, 0x121212FF) };
            div()
                .absolute()
                .top_0()
                .bottom_0()
                .w(px(120.))
                .when(start, |d| d.left_0())
                .when(!start, |d| d.right_0())
                .bg(linear_gradient(90., linear_color_stop(rgba(from), 0.), linear_color_stop(rgba(to), 1.)))
        };
        div()
            .group(group.clone())
            .flex_none()
            .w(px(w))
            .flex()
            .flex_col()
            .child(
                div().h(px(48.)).mb(px(8.)).flex().items_end().child(
                    one_line(title.to_string())
                        .font_family(title_font())
                        .text_size(px(24.))
                        .line_height(px(28.8))
                        .font_weight(FontWeight::BOLD)
                        .text_color(c(WHITE)),
                ),
            )
            .child(
                div()
                    .relative()
                    .w(px(w))
                    .h(px(SHELF_H))
                    .overflow_hidden()
                    .child(div().ml(px(-(at as f32) * SHELF_CARD_W)).flex().flex_row().items_start().children(cards))
                    // The client masks 120px at an end that has more cards.
                    .when(at > 0, |d| d.child(fade(true)))
                    .when(at < last_start, |d| d.child(fade(false)))
                    .when(at > 0, |d| d.child(arrow(format!("search-shelf-{index}-prev"), "icons/chevron_left.svg", at.saturating_sub(visible)).left(px(8.))))
                    .when(at < last_start, |d| {
                        d.child(arrow(format!("search-shelf-{index}-next"), "icons/chevron_right.svg", (at + visible).min(last_start)).right(px(8.)))
                    }),
            )
            .into_any_element()
    }

    /// "Songs": the results as a tracklist (#, Title, Album, duration).
    fn search_tracks(&self, r: &Results, main_w: f32, pad: f32, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let list_w = main_w - 2. * pad;
        let row_w = list_w - 1.6;
        let spec: Vec<(f32, f32)> = if main_w >= 600. { vec![(180., 4.), (120., 2.), (120., 1.)] } else { vec![(180., 4.), (120., 1.)] };
        let widths = fr_columns(row_w - 32. - 16. - 16. * spec.len() as f32, &spec);
        let first_w = widths[0];
        let var_ws: Vec<f32> = widths[1..widths.len() - 1].to_vec();
        let last_w = *widths.last().unwrap();
        let cell = |text: &'static str, wd: f32| div().w(px(wd)).flex_none().text_size(px(14.)).line_height(px(19.2)).child(text);
        let head = div()
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
            .child(cell("Title", first_w))
            .children(var_ws.iter().map(|vw| cell("Album", *vw)))
            .child(
                div()
                    .w(px(last_w))
                    .flex_none()
                    .flex()
                    .flex_row()
                    .justify_end()
                    .pr(px(32.))
                    .child(icon("icons/pl_clock16.svg", 16., TEXT_SUBDUED)),
            );
        let playing = self.playback.as_ref().and_then(|p| p.item.as_ref()?.id.clone()).map(|id| format!("spotify:track:{id}"));
        let now = pl::now_unix();
        let rows: Vec<AnyElement> = r
            .hits()
            .filter(|h| h.kind == HitKind::Track)
            .enumerate()
            .map(|(i, h)| {
                let t = h.track();
                let hit = h.clone();
                div()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, ev: &MouseDownEvent, _, cx| {
                            if ev.click_count == 2 {
                                this.remember_search(&hit, cx);
                            }
                        }),
                    )
                    .child(self.track_row(i, &t, "", first_w, &var_ws, last_w, playing.as_deref() == Some(t.uri.as_str()), now, window, cx))
                    .into_any_element()
            })
            .collect();
        div()
            .w(px(list_w))
            .border(px(0.8))
            .border_color(rgba(0x00000000))
            .flex()
            .flex_col()
            .child(head)
            .child(div().h(px(pad - 8.)))
            .children(rows)
            .into_any_element()
    }

    /// Albums, artists, playlists, profiles, audiobooks, authors: a grid of cards.
    fn search_grid(&self, r: &Results, w: f32, cx: &mut Context<Self>) -> AnyElement {
        let cols = ((w / 180.).floor() as usize).max(1);
        let card_w = w / cols as f32;
        let hits: Vec<&Hit> = r.hits().collect();
        let rows = hits.chunks(cols).enumerate().map(|(row, chunk)| {
            div().flex().flex_row().items_start().children(chunk.iter().enumerate().map(|(k, h)| {
                let i = row * cols + k;
                let hit = (*h).clone();
                div()
                    .id(name_id(format!("sgw-{i}")))
                    .flex_none()
                    .on_click(cx.listener(move |this, _, _, cx| this.remember_search(&hit, cx)))
                    .child(self.sized_card(format!("sg-{i}").into(), &h.card(), card_w, 4., cx))
            }))
        });
        div().mt(px(24.)).flex().flex_col().children(rows).into_any_element()
    }
}
