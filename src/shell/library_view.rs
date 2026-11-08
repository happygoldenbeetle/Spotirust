//! Your Library: "Sort by / View as" menu, sort orders and the four views.
//! Measured on the reference (library list 264 wide, panel 280):
//!   Menu: two stacked lists 208 wide, #282828, padding 4, shadow 0 16 24 rgba(0,0,0,.3) +
//!   0 6 8 rgba(0,0,0,.2); radius 4 4 0 0 / 0 0 4 4. Right edge on the sort button's, top 7.8
//!   below it. Section titles 12px/700 rgba(255,255,255,.7), padding 12 8 12 12 (40.8 tall).
//!   Items 40 tall, padding 8 8 8 12, 14px rgba(255,255,255,.9), radius 2; hover rgba(255,255,255,.1)
//!   + white. Selected: green, a check (Recents) or a direction arrow (other sorts) 16px at the right.
//!   "View as": #1F1F1F row (radius 8, padding 4, 40 tall) of 48x32 buttons (radius 6), 16px icons
//!   #B3B3B3, selected green. Choosing an item keeps the menu open; Escape / outside click close it.
//!   Views: compact list (32 rows: name • type), list (64 rows), compact grid (cells W/floor(W/102),
//!   padding 8, cover only), grid (cells W/floor(W/116), padding 12, cover, title 8 below,
//!   subtitle 2 below, rows 184).

use super::*;
use crate::library::{LibItem, LibKind};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum LibSort {
    Recents,
    RecentlyAdded,
    Alphabetical,
    Creator,
}

impl LibSort {
    const ALL: [LibSort; 4] = [LibSort::Recents, LibSort::RecentlyAdded, LibSort::Alphabetical, LibSort::Creator];
    pub(super) fn label(self) -> &'static str {
        match self {
            LibSort::Recents => "Recents",
            LibSort::RecentlyAdded => "Recently added",
            LibSort::Alphabetical => "Alphabetical",
            LibSort::Creator => "Creator",
        }
    }
    fn key(self) -> &'static str {
        match self {
            LibSort::Recents => "recents",
            LibSort::RecentlyAdded => "added",
            LibSort::Alphabetical => "alpha",
            LibSort::Creator => "creator",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum LibView {
    CompactList,
    List,
    CompactGrid,
    Grid,
}

impl LibView {
    const ALL: [LibView; 4] = [LibView::CompactList, LibView::List, LibView::CompactGrid, LibView::Grid];
    pub(super) fn icon(self) -> &'static str {
        match self {
            LibView::CompactList => "icons/view_compact_list16.svg",
            LibView::List => "icons/view_list16.svg",
            LibView::CompactGrid => "icons/view_compact_grid16.svg",
            LibView::Grid => "icons/view_grid16.svg",
        }
    }
    fn key(self) -> &'static str {
        match self {
            LibView::CompactList => "compact-list",
            LibView::List => "list",
            LibView::CompactGrid => "compact-grid",
            LibView::Grid => "grid",
        }
    }
}

/// Library view preferences, kept like the client keeps them (locally, per install).
#[derive(Clone, Copy, Debug)]
pub(super) struct LibPrefs {
    pub sort: LibSort,
    /// Sort direction. `None` until a sort is first chosen (the menu then shows a check);
    /// `Some(false)` = forward (down arrow: A-Z, newest first), `Some(true)` = reversed (up arrow).
    pub reversed: Option<bool>,
    pub view: LibView,
}

impl Default for LibPrefs {
    fn default() -> Self {
        Self { sort: LibSort::Recents, reversed: None, view: LibView::List }
    }
}

fn prefs_path() -> Option<std::path::PathBuf> {
    Some(std::path::PathBuf::from(std::env::var_os("APPDATA")?).join("Spotirust").join("library.json"))
}

impl LibPrefs {
    pub(super) fn view_index(&self) -> f32 {
        LibView::ALL.iter().position(|x| *x == self.view).unwrap_or(0) as f32
    }

    pub(super) fn load() -> Self {
        let mut p = Self::default();
        let Some(v) = prefs_path()
            .and_then(|f| std::fs::read(f).ok())
            .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
        else {
            return p;
        };
        if let Some(s) = v.get("sort").and_then(|s| s.as_str()) {
            p.sort = LibSort::ALL.into_iter().find(|x| x.key() == s).unwrap_or(p.sort);
        }
        if let Some(s) = v.get("view").and_then(|s| s.as_str()) {
            p.view = LibView::ALL.into_iter().find(|x| x.key() == s).unwrap_or(p.view);
        }
        p.reversed = v.get("reversed").and_then(|s| s.as_bool());
        p
    }

    fn save(&self) {
        let Some(path) = prefs_path() else { return };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let v = serde_json::json!({ "sort": self.sort.key(), "view": self.view.key(), "reversed": self.reversed });
        // (reversed: null until a sort is first chosen)
        let _ = std::fs::write(path, v.to_string());
    }
}

/// Orders library items like the client: pinned items stay on top (in their own order);
/// Recents keeps the server order.
pub(super) fn sort_items(items: &mut [LibItem], prefs: LibPrefs) {
    let lower = |s: &str| s.to_lowercase();
    match prefs.sort {
        LibSort::Recents => return,
        LibSort::RecentlyAdded => items.sort_by(|a, b| b.added_at.cmp(&a.added_at)),
        LibSort::Alphabetical => items.sort_by(|a, b| lower(&a.name).cmp(&lower(&b.name))),
        LibSort::Creator => items.sort_by(|a, b| lower(&a.owner).cmp(&lower(&b.owner)).then_with(|| lower(&a.name).cmp(&lower(&b.name)))),
    }
    if prefs.reversed == Some(true) {
        items.reverse();
    }
    // stable: pins first, keeping their relative order
    items.sort_by_key(|i| !i.pinned);
}

const MENU_W: f32 = 208.;

pub(super) fn menu_list(top: bool) -> Div {
    div()
        .w(px(MENU_W))
        .p(px(4.))
        .bg(c(0x282828))
        .when(top, |d| d.rounded_tl(px(4.)).rounded_tr(px(4.)))
        .when(!top, |d| d.rounded_bl(px(4.)).rounded_br(px(4.)))
        .shadow(vec![
            gpui::BoxShadow {
                color: gpui::hsla(0., 0., 0., 0.3),
                offset: gpui::point(px(0.), px(16.)),
                blur_radius: px(24.),
                spread_radius: px(0.),
            },
            gpui::BoxShadow {
                color: gpui::hsla(0., 0., 0., 0.2),
                offset: gpui::point(px(0.), px(6.)),
                blur_radius: px(8.),
                spread_radius: px(0.),
            },
        ])
        .flex()
        .flex_col()
}

fn menu_title(text: &'static str) -> Div {
    div()
        .h(px(40.8))
        .pt(px(12.))
        .pl(px(12.))
        .pr(px(8.))
        .text_size(px(12.))
        .line_height(px(16.8))
        .font_weight(FontWeight::BOLD)
        .text_color(rgba(0xFFFFFFB3))
        .child(text)
}

impl Shell {
    pub(super) fn set_lib_prefs(&mut self, prefs: LibPrefs, cx: &mut Context<Self>) {
        self.lib_prefs = prefs;
        prefs.save();
        cx.notify();
    }

    /// The menu, positioned in window coordinates under the sort button (right edges aligned).
    pub(super) fn library_menu(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.lib_menu || self.left_collapsed {
            return None;
        }
        let prefs = self.lib_prefs;
        // sort button: right edge 24 inside the panel (panel starts at x 8), bottom at 64 + 141.2
        let right = 8. + self.left_w - 24. + 0.4;
        let top = 64. + 109.2 + 32. + 7.6;
        let sorts = LibSort::ALL.into_iter().map(|s| {
            let on = s == prefs.sort;
            // client: CheckedIcon = direction set ? (forward ? down arrow : up arrow) : check
            let mark = match (on, prefs.reversed) {
                (false, _) => None,
                (true, None) => Some("icons/check16.svg"),
                (true, Some(false)) => Some("icons/arrow_down16.svg"),
                (true, Some(true)) => Some("icons/arrow_up16.svg"),
            };
            let label = s.label();
            div()
                .id(gpui::ElementId::Name(format!("lib-sort-{}", s.key()).into()))
                .group(gpui::SharedString::from(format!("lib-sort-{}", s.key())))
                .h(px(40.))
                .pl(px(12.))
                .pr(px(8.))
                .rounded(px(2.))
                .flex()
                .flex_row()
                .items_center()
                .cursor_pointer()
                .text_size(px(14.))
                .line_height(px(19.2))
                .text_color(if on { c(GREEN) } else { rgba(0xFFFFFFE6) })
                .hover(move |st| {
                    let st = st.bg(rgba(0xFFFFFF1A));
                    if on { st } else { st.text_color(c(WHITE)) }
                })
                .on_click(cx.listener(move |this, _, _, cx| {
                    let mut p = this.lib_prefs;
                    if p.sort == s {
                        p.reversed = Some(!p.reversed.unwrap_or(false));
                    } else {
                        p.sort = s;
                        p.reversed = Some(false);
                    }
                    this.set_lib_prefs(p, cx);
                }))
                .child(one_line(label).flex_1().min_w_0())
                .when_some(mark, |d, m| d.child(icon(m, 16., GREEN)))
        });
        let views = LibView::ALL.into_iter().map(|v| {
            let on = v == prefs.view;
            let g: gpui::SharedString = format!("lib-view-{}", v.key()).into();
            div()
                .id(gpui::ElementId::Name(g.clone()))
                .group(g.clone())
                .w(px(48.))
                .h(px(32.))
                .rounded(px(6.))
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .hover(|st| st.bg(rgba(0xFFFFFF1A)))
                .on_click(cx.listener(move |this, _, _, cx| {
                    let mut p = this.lib_prefs;
                    if p.view != v {
                        this.lib_view_from = LibView::ALL.iter().position(|x| *x == p.view).unwrap_or(0) as f32;
                        this.lib_view_seq += 1;
                    }
                    p.view = v;
                    this.set_lib_prefs(p, cx);
                }))
                .child(
                    svg()
                        .path(v.icon())
                        .size(px(16.))
                        .text_color(c(if on { GREEN } else { TEXT_SUBDUED }))
                        .when(!on, |s| s.group_hover(g, |s| s.text_color(c(WHITE)))),
                )
        });
        Some(
            div()
                .id("lib-menu")
                .absolute()
                .top(px(top))
                .left(px(right - MENU_W))
                .occlude()
                .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                    this.lib_menu = false;
                    this.menu_closed_at = std::time::Instant::now();
                    cx.notify();
                }))
                .flex()
                .flex_col()
                .child(
                    menu_list(true)
                        .relative()
                        .child(menu_title("Sort by"))
                        .children(sorts)
                        // ::after divider: 0.8px rgba(255,255,255,.1) at the list's bottom, inset 4
                        .child(div().absolute().top(px(208.)).left(px(4.)).right(px(4.)).h(px(0.8)).bg(rgba(0xFFFFFF1A))),
                )
                .child(
                    menu_list(false)
                        .child(menu_title("View as"))
                        .child({
                            // selected highlight slides: transform .1s cubic-bezier(0.3, 0, 0, 1)
                            let to = LibView::ALL.iter().position(|x| *x == prefs.view).unwrap_or(0) as f32;
                            let from = self.lib_view_from;
                            let easing = |t: f32| 1. - (1. - t).powi(4);
                            let indicator = div()
                                .absolute()
                                .top(px(4.))
                                .w(px(48.))
                                .h(px(32.))
                                .rounded(px(6.))
                                .bg(rgba(0xFFFFFF1A))
                                .with_animation(
                                    gpui::ElementId::Name(format!("lib-view-ind-{}", self.lib_view_seq).into()),
                                    gpui::Animation::new(std::time::Duration::from_millis(100)).with_easing(easing),
                                    move |d, t| d.left(px(4. + (from + (to - from) * t) * 48.)),
                                );
                            div()
                                .relative()
                                .h(px(40.))
                                .p(px(4.))
                                .rounded(px(8.))
                                .bg(c(0x1F1F1F))
                                .flex()
                                .flex_row()
                                .child(indicator)
                                .children(views)
                        }),
                )
                .into_any_element(),
        )
    }

    /// Library content for the current view.
    pub(super) fn library_content(&self, items: &[LibItem], me_name: &str, cx: &mut Context<Self>) -> Vec<AnyElement> {
        match self.lib_prefs.view {
            LibView::List => items.iter().enumerate().map(|(i, it)| self.library_row(i, it, me_name, cx)).collect(),
            LibView::CompactList => items.iter().enumerate().map(|(i, it)| self.library_compact_row(i, it, cx)).collect(),
            LibView::CompactGrid | LibView::Grid => {
                let compact = self.lib_prefs.view == LibView::CompactGrid;
                // list area = panel - 16 (8px padding each side)
                let w = self.left_w - 16.;
                let cols = ((w / if compact { 102. } else { 116. }).floor() as usize).max(1);
                let cell = w / cols as f32;
                let cells: Vec<AnyElement> = items
                    .iter()
                    .enumerate()
                    .map(|(i, it)| self.library_cell(i, it, me_name, cell, compact, cx))
                    .collect();
                let mut rows = Vec::new();
                let mut it = cells.into_iter().peekable();
                while it.peek().is_some() {
                    let row: Vec<AnyElement> = it.by_ref().take(cols).collect();
                    rows.push(div().flex().flex_row().children(row).into_any_element());
                }
                rows
            }
        }
    }

    fn open_lib_item(&mut self, uri: &str, cx: &mut Context<Self>) {
        if let Some(p) = Page::for_uri(uri) {
            self.open(p, cx);
        }
    }

    /// Compact list row: 32 tall, 32px leading slot, name (16px white) • type (14px subdued).
    fn library_compact_row(&self, index: usize, item: &LibItem, cx: &mut Context<Self>) -> AnyElement {
        let open_page = Page::for_uri(&item.uri);
        let active = open_page.as_ref() == Some(&self.page);
        let play_uri = self.library_play_uri(item);
        let uri = item.uri.clone();
        div()
            .id(gpui::ElementId::Name(format!("lib-crow-{index}").into()))
            .on_mouse_down(MouseButton::Right, self.ctx_listener(item.uri.clone(), item.name.clone(), true, cx))
            .h(px(32.))
            .pl(px(32.))
            .pr(px(8.))
            .rounded(px(6.))
            .flex()
            .flex_row()
            .items_center()
            .when(active, |d| d.bg(c(0x2A2A2A)))
            .hover(move |s| s.bg(c(if active { 0x393939 } else { 0x1F1F1F })))
            .cursor_pointer()
            .on_click(cx.listener(move |this, ev: &gpui::ClickEvent, _, cx| {
                if ev.click_count() == 2 {
                    if let Some(u) = play_uri.clone() {
                        this.command(move |api| api.play_context(&u), cx);
                    }
                } else {
                    this.open_lib_item(&uri, cx);
                }
            }))
            .when(item.pinned, |d| d.child(div().absolute().left(px(10.)).child(icon("icons/pin.svg", 12., GREEN))))
            .child(one_line(item.name.clone()).flex_shrink().min_w_0().text_size(px(16.)).line_height(px(22.4)).text_color(c(if active { GREEN } else { WHITE })))
            .child(div().mx(px(4.)).flex_none().text_size(px(14.)).line_height(px(19.2)).child("\u{2022}"))
            .child(one_line(item.type_label()).flex_none().text_size(px(14.)).line_height(px(19.2)))
            .relative()
            .into_any_element()
    }

    /// Grid cell: compact = cover only (padding 8); default = card (padding 12, title, subtitle).
    fn library_cell(&self, index: usize, item: &LibItem, me_name: &str, w: f32, compact: bool, cx: &mut Context<Self>) -> AnyElement {
        let pad = if compact { 8. } else { 12. };
        let img = w - pad * 2.;
        let open_page = Page::for_uri(&item.uri);
        let active = open_page.as_ref() == Some(&self.page);
        let play_uri = self.library_play_uri(item);
        let uri = item.uri.clone();
        let owner = if item.kind == LibKind::LikedSongs { me_name } else { item.owner.as_str() };
        let subtitle = if owner.is_empty() { item.type_label().to_string() } else { format!("{}\u{2022}{owner}", item.type_label()) };
        let radius = if item.kind == LibKind::Artist { img / 2. } else { 6. };
        let url = item.image.clone();
        div()
            .id(gpui::ElementId::Name(format!("lib-cell-{index}").into()))
            .on_mouse_down(MouseButton::Right, self.ctx_listener(item.uri.clone(), item.name.clone(), true, cx))
            .w(px(w))
            .when(!compact, |d| d.h(px(184.)))
            .flex_none()
            .p(px(pad))
            .rounded(px(6.))
            .when(active, |d| d.bg(c(0x2A2A2A)))
            .hover(move |s| s.bg(c(if active { 0x393939 } else { 0x1F1F1F })))
            .cursor_pointer()
            .on_click(cx.listener(move |this, ev: &gpui::ClickEvent, _, cx| {
                if ev.click_count() == 2 {
                    if let Some(u) = play_uri.clone() {
                        this.command(move |api| api.play_context(&u), cx);
                    }
                } else {
                    this.open_lib_item(&uri, cx);
                }
            }))
            .child(cover(url.as_deref(), img, radius, cx))
            .when(!compact, |d| {
                d.child(one_line(item.name.clone()).mt(px(8.)).text_size(px(16.)).line_height(px(22.4)).text_color(c(WHITE)))
                    .child(one_line(subtitle).mt(px(2.)).text_size(px(14.)).line_height(px(19.2)).text_color(c(TEXT_SUBDUED)))
            })
            .into_any_element()
    }
}
