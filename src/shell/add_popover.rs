//! "Add to playlist" popover: opened by the green check of a liked track (player bar, tracklist
//! rows). Measured from the reference (`form`, 292 wide, #1f1f1f, radius 8, menu shadow):
//!   header 40 tall, "Add to playlist" 12/700 #b3b3b3 at x20;
//!   search 32 tall at x12 (268 wide): rgba(255,255,255,.1), radius 4, icon at x8, "Find a playlist";
//!   4px gap, "New playlist" row 48 tall (plus icon 16 at x8, 16px white text at x36);
//!   1px rgba(255,255,255,.1) rule 8px below (inset 8), then a scrolling list (max 293.2 tall):
//!   "Saved in" / "Recently updated" 14/700 #b3b3b3 (12 above, 4 below, x8); rows 56 tall,
//!   padding 8, radius 6, hover rgba(255,255,255,.1): 40px cover (radius 4, #282828), name 16px
//!   white at x52, "N songs" 12px #b3b3b3 (green pin first when pinned), at the right a 16px
//!   circle (1px #7c7c7c) or the green check;
//!   footer 56 tall, padding 12/24: "Cancel" (14/700 tertiary) and, once something changed,
//!   "Done" (white pill). Nothing is saved until Done: one change says "Added to X." /
//!   "Removed from X.", several say "Changes saved."

use std::collections::HashMap;

use gpui::{ClickEvent, ScrollHandle};

use super::*;
use crate::library::LibKind;

const W: f32 = 292.;
const LIST_MAX: f32 = 293.2;
const ROW_H: f32 = 56.;

pub struct AddPop {
    pub track: String,
    /// Button the popover hangs from: left, top, bottom (window px).
    pub anchor: (f32, f32, f32),
    pub filter: String,
    pub liked_now: bool,
    pub liked_want: bool,
    /// Playlist -> wanted state (only rows the user toggled).
    pub want: HashMap<String, bool>,
    pub scroll: ScrollHandle,
}

impl Shell {
    /// Opens the popover for `track` under (or above) the button at `anchor`.
    pub(super) fn open_add_popover(&mut self, track: String, anchor: (f32, f32, f32), cx: &mut Context<Self>) {
        let liked = self.track_liked(&track);
        self.ctx_menu = None;
        self.add_pop = Some(AddPop {
            track: track.clone(),
            anchor,
            filter: String::new(),
            liked_now: liked,
            liked_want: liked,
            want: HashMap::new(),
            scroll: ScrollHandle::new(),
        });
        cx.notify();
        self.load_playlist_membership(track, cx);
    }

    /// Anchor for a square button of `size` px under the click (the button's own bounds).
    pub(super) fn anchor_of(ev: &ClickEvent, size: f32) -> (f32, f32, f32) {
        let p = ev.position();
        let (x, y) = (f32::from(p.x), f32::from(p.y));
        (x - size / 2., y - size / 2., y + size / 2.)
    }

    /// A track's title, from the player or any loaded tracklist.
    fn track_name(&self, track: &str) -> Option<String> {
        let id = track.rsplit(':').next().unwrap_or_default();
        if let Some(t) = self.playback.as_ref().and_then(|p| p.item.as_ref()).filter(|t| t.id.as_deref() == Some(id)) {
            return Some(t.name.clone());
        }
        self.playlist_pages.values().flat_map(|p| p.tracks.iter()).find(|t| t.uri == track).map(|t| t.name.clone())
    }

    fn track_liked(&self, track: &str) -> bool {
        if self.liked.contains(track) {
            return true;
        }
        let id = track.rsplit(':').next().unwrap_or_default();
        self.saved.as_ref().is_some_and(|(sid, s)| sid == id && *s)
    }

    /// Your editable playlists, in library order.
    fn editable_playlists(&self) -> Vec<LibItem> {
        let me_name = self.me.as_ref().and_then(|m| m.display_name.clone()).unwrap_or_default();
        self.library_items().into_iter().filter(|i| i.kind == LibKind::Playlist && i.owner == me_name).collect()
    }

    /// Which of your playlists hold the track (and their song counts), fetched in the background.
    fn load_playlist_membership(&mut self, track: String, cx: &mut Context<Self>) {
        let Some(api) = self.api.clone() else { return };
        let playlists: Vec<String> = self.editable_playlists().into_iter().map(|p| p.uri).collect();
        cx.spawn(async move |this, cx| {
            let a = api.clone();
            let counts = cx.background_executor().spawn(async move { (a.my_playlist_counts(), a.liked_count()) }).await;
            let _ = this.update(cx, |this, cx| {
                if let Ok(c) = counts.0 {
                    // This count lags behind changes: prefer one taken from the playlist's tracks.
                    for (u, n) in c {
                        if !this.pl_contents.contains_key(&u) {
                            this.pl_counts.insert(u, n);
                        }
                    }
                }
                if let Ok(n) = counts.1 {
                    this.pl_counts.insert(LIKED_URI.to_string(), n);
                }
                cx.notify();
            });
            for pl in playlists {
                let a = api.clone();
                let uri = pl.clone();
                let items = cx.background_executor().spawn(async move { a.context_tracks(&uri) }).await;
                let open = this
                    .update(cx, |this, cx| {
                        if let Ok(items) = items {
                            this.pl_counts.insert(pl.clone(), items.len() as u32);
                            this.pl_contents.insert(pl, items.into_iter().collect());
                            cx.notify();
                        }
                        this.add_pop.as_ref().is_some_and(|p| p.track == track)
                    })
                    .unwrap_or(false);
                if !open {
                    break;
                }
            }
        })
        .detach();
    }

    fn in_playlist(&self, pl: &str, track: &str) -> bool {
        self.pl_contents.get(pl).is_some_and(|s| s.contains(track))
    }

    pub(super) fn close_add_popover(&mut self, cx: &mut Context<Self>) {
        if self.add_pop.take().is_some() {
            cx.notify();
        }
    }

    /// Typing goes to "Find a playlist" while the popover is open. Returns whether it was handled.
    pub(super) fn add_popover_key(&mut self, ev: &gpui::KeyDownEvent, cx: &mut Context<Self>) -> bool {
        let Some(pop) = self.add_pop.as_mut() else { return false };
        let k = &ev.keystroke;
        match k.key.as_str() {
            "escape" => {
                self.add_pop = None;
            }
            "backspace" => {
                if k.modifiers.control {
                    pop.filter.clear();
                } else {
                    pop.filter.pop();
                }
            }
            "enter" => {}
            _ if k.modifiers.control || k.modifiers.alt => return false,
            _ => match k.key_char.as_deref() {
                Some(ch) if !ch.chars().any(char::is_control) => pop.filter.push_str(ch),
                _ => return false,
            },
        }
        cx.notify();
        true
    }

    fn toggle_in_pop(&mut self, pl: Option<String>, cx: &mut Context<Self>) {
        let track = match self.add_pop.as_ref() {
            Some(p) => p.track.clone(),
            None => return,
        };
        let current = pl.as_deref().map(|u| self.in_playlist(u, &track));
        let Some(pop) = self.add_pop.as_mut() else { return };
        match (pl, current) {
            (Some(u), Some(now)) => {
                let cur = pop.want.get(&u).copied().unwrap_or(now);
                if !cur == now {
                    pop.want.remove(&u);
                } else {
                    pop.want.insert(u, !cur);
                }
            }
            _ => pop.liked_want = !pop.liked_want,
        }
        cx.notify();
    }

    fn pop_changed(&self) -> bool {
        let Some(pop) = self.add_pop.as_ref() else { return false };
        pop.liked_want != pop.liked_now || pop.want.iter().any(|(u, w)| *w != self.in_playlist(u, &pop.track))
    }

    /// Done: saves every change, then one toast.
    fn apply_add_popover(&mut self, cx: &mut Context<Self>) {
        let Some(pop) = self.add_pop.take() else { return };
        let Some(api) = self.api.clone() else { return };
        let names: HashMap<String, String> = self.editable_playlists().into_iter().map(|p| (p.uri, p.name)).collect();
        let mut changes: Vec<(String, bool, String)> = Vec::new(); // (playlist or "", add, name)
        if pop.liked_want != pop.liked_now {
            changes.push((String::new(), pop.liked_want, "Liked Songs".into()));
        }
        for (u, w) in &pop.want {
            if *w != self.in_playlist(u, &pop.track) {
                changes.push((u.clone(), *w, names.get(u).cloned().unwrap_or_default()));
            }
        }
        if changes.is_empty() {
            cx.notify();
            return;
        }
        let toast = match changes.as_slice() {
            [(_, true, name)] => format!("Added to {name}."),
            [(_, false, name)] => format!("Removed from {name}."),
            _ => "Changes saved.".to_string(),
        };
        // Show the result now (the tracklist checks, the bar's button, the popover next time).
        let track = pop.track.clone();
        for (u, add, _) in &changes {
            if u.is_empty() {
                self.set_liked_local(&track, *add);
            } else {
                let set = self.pl_contents.entry(u.clone()).or_default();
                if *add {
                    set.insert(track.clone());
                } else {
                    set.remove(&track);
                }
                if let Some(n) = self.pl_counts.get_mut(u) {
                    *n = if *add { *n + 1 } else { n.saturating_sub(1) };
                }
                if !*add {
                    if let Some(page) = self.playlist_pages.get_mut(u) {
                        let before = page.tracks.len();
                        page.tracks.retain(|t| t.uri != track);
                        page.total = page.total.saturating_sub(before - page.tracks.len());
                    }
                }
            }
        }
        let changed: Vec<String> = changes.iter().map(|(u, _, _)| if u.is_empty() { LIKED_URI.to_string() } else { u.clone() }).collect();
        let id = track.rsplit(':').next().unwrap_or_default().to_string();
        self.background_then(
            cx,
            toast,
            move || {
                for (u, add, _) in changes {
                    if u.is_empty() {
                        api.set_saved(&id, add)?;
                    } else if add {
                        api.add_to_playlist(&u, std::slice::from_ref(&track))?;
                    } else {
                        api.remove_from_playlist(&u, &track)?;
                    }
                }
                Ok(())
            },
            move |this, cx| {
                // The page being viewed refreshes in place; the others load again when opened.
                let open = match &this.page {
                    Page::Liked => Some(LIKED_URI.to_string()),
                    Page::Playlist(u) => Some(u.clone()),
                    _ => None,
                };
                for u in &changed {
                    if open.as_ref() == Some(u) {
                        this.refresh_page(u.clone(), cx);
                    } else {
                        this.playlist_pages.remove(u);
                    }
                }
                // Covers and the library order follow a playlist's tracks.
                if changed.iter().any(|u| u != LIKED_URI) {
                    this.reload_library(cx);
                }
            },
        );
        cx.notify();
    }

    /// Updates every place that shows whether `track` is liked.
    pub(super) fn set_liked_local(&mut self, track: &str, on: bool) {
        if on {
            self.liked.insert(track.to_string());
        } else {
            self.liked.remove(track);
        }
        let id = track.rsplit(':').next().unwrap_or_default();
        if self.playing_track_id().as_deref() == Some(id) {
            self.saved = Some((id.to_string(), on));
        }
        if let Some(n) = self.pl_counts.get_mut(LIKED_URI) {
            *n = if on { *n + 1 } else { n.saturating_sub(1) };
        }
    }

    /// The "+" of a track that isn't liked yet: likes it at once ("Added to Liked Songs.").
    pub(super) fn like_track(&mut self, track: String, cx: &mut Context<Self>) {
        let Some(api) = self.api.clone() else { return };
        self.set_liked_local(&track, true);
        let id = track.rsplit(':').next().unwrap_or_default().to_string();
        self.background_then(cx, "Added to Liked Songs.".into(), move || api.set_saved(&id, true), |this, cx| {
            this.playlist_pages.remove(LIKED_URI);
            if matches!(this.page, Page::Liked) {
                this.page_changed(cx);
            }
        });
        cx.notify();
    }

    fn pop_row(
        &self,
        id: String,
        art: AnyElement,
        name: String,
        count: Option<u32>,
        pinned: bool,
        checked: bool,
        pl: Option<String>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let sub = count.map(|n| if n == 1 { "1 song".to_string() } else { format!("{n} songs") }).unwrap_or_default();
        div()
            .id(gpui::ElementId::Name(id.into()))
            .h(px(ROW_H))
            .p(px(8.))
            .rounded(px(6.))
            .flex()
            .flex_row()
            .items_center()
            .cursor_pointer()
            .hover(|s| s.bg(rgba(0xFFFFFF1A)))
            .on_click(cx.listener(move |this, _, _, cx| this.toggle_in_pop(pl.clone(), cx)))
            .child(div().size(px(40.)).flex_none().rounded(px(4.)).overflow_hidden().bg(c(PLACEHOLDER)).child(art))
            .child(
                div()
                    .ml(px(12.))
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(one_line(name).text_size(px(16.)).line_height(px(22.4)).text_color(c(WHITE)))
                    .child(
                        div()
                            .h(px(19.2))
                            .flex()
                            .flex_row()
                            .items_center()
                            .text_size(px(12.))
                            .line_height(px(16.8))
                            .text_color(c(TEXT_SUBDUED))
                            .when(pinned, |d| d.child(div().w(px(16.)).flex_none().child(icon("icons/pin.svg", 12., GREEN))))
                            .child(sub),
                    ),
            )
            .child(div().size(px(16.)).flex_none().map(|d| {
                if checked {
                    d.child(icon("icons/check_circle.svg", 16., GREEN))
                } else {
                    d.rounded_full().border_1().border_color(c(0x7C7C7C))
                }
            }))
            .into_any_element()
    }

    pub(super) fn add_popover_view(&self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let pop = self.add_pop.as_ref()?;
        let vp = window.viewport_size();
        let (vw, vh) = (f32::from(vp.width), f32::from(vp.height));
        let q = pop.filter.to_lowercase();
        let matches = |name: &str| q.is_empty() || name.to_lowercase().contains(&q);

        // rows: (element, saved-in)
        let liked_on = pop.liked_want;
        let mut saved_in: Vec<AnyElement> = Vec::new();
        let mut others: Vec<AnyElement> = Vec::new();
        if matches("Liked Songs") {
            let row = self.pop_row(
                "pop-liked".into(),
                liked_songs_art(40., 4.),
                "Liked Songs".into(),
                self.pl_counts.get(LIKED_URI).copied(),
                true,
                liked_on,
                None,
                cx,
            );
            if pop.liked_now { saved_in.push(row) } else { others.push(row) }
        }
        for (i, p) in self.editable_playlists().into_iter().enumerate() {
            if !matches(&p.name) {
                continue;
            }
            let now = self.in_playlist(&p.uri, &pop.track);
            let checked = pop.want.get(&p.uri).copied().unwrap_or(now);
            // The library's cover (resolved mosaics included).
            let art = lib_art(&p, 40., cx);
            let row = self.pop_row(format!("pop-pl-{i}"), art, p.name.clone(), self.pl_counts.get(&p.uri).copied(), p.pinned, checked, Some(p.uri.clone()), cx);
            if now { saved_in.push(row) } else { others.push(row) }
        }
        let title = |t: &'static str, top: f32| {
            div().pt(px(top)).pb(px(4.)).pl(px(8.)).text_size(px(14.)).line_height(px(19.2)).font_weight(FontWeight::BOLD).text_color(c(TEXT_SUBDUED)).child(t)
        };
        let n_rows = saved_in.len() + others.len();
        let content_h = if saved_in.is_empty() { 0. } else { 35.2 + 0.2 } + if others.is_empty() { 0. } else { 35.2 } + n_rows as f32 * ROW_H + 8.;
        let list_h = content_h.min(LIST_MAX);
        let total_h = 40. + 32. + 4. + 48. + 8.8 + list_h + 56.;

        // Under the button when it fits, else above it, else pushed inside the window (10px in).
        let (ax, atop, abot) = pop.anchor;
        let x = ax.clamp(10., (vw - 10. - W).max(10.));
        let y = if abot + 8. + total_h <= vh - 10. {
            abot + 8.
        } else if atop - 8. - total_h >= 10. {
            atop - 8. - total_h
        } else {
            (vh - 10. - total_h).max(10.)
        };
        let changed = self.pop_changed();
        let filter = pop.filter.clone();

        let search = div()
            .mx(px(12.))
            .h(px(32.))
            .rounded(px(4.))
            .bg(rgba(0xFFFFFF1A))
            .flex()
            .flex_row()
            .items_center()
            .pl(px(8.))
            .child(svg().path("icons/lib_search.svg").size(px(16.)).flex_none().text_color(rgba(0xFFFFFFB3)))
            .child(
                div()
                    .ml(px(8.))
                    .flex_1()
                    .min_w_0()
                    .text_size(px(14.))
                    .text_color(if filter.is_empty() { rgba(0xFFFFFFB3) } else { rgba(0xFFFFFFFF) })
                    .child(if filter.is_empty() { "Find a playlist".to_string() } else { filter.clone() }),
            );

        let new_playlist = div()
            .id("pop-new")
            .mx(px(12.))
            .h(px(48.))
            .p(px(8.))
            .rounded(px(6.))
            .flex()
            .flex_row()
            .items_center()
            .cursor_pointer()
            .hover(|s| s.bg(rgba(0xFFFFFF1A)))
            .on_click(cx.listener(|this, _, _, cx| {
                let Some(track) = this.add_pop.take().map(|p| p.track) else { return };
                let name = this.track_name(&track).unwrap_or_else(|| "New playlist".into());
                this.new_playlist_with(track, name, cx);
            }))
            .child(icon("icons/plus.svg", 16., WHITE))
            .child(div().ml(px(12.)).text_size(px(16.)).line_height(px(22.4)).text_color(c(WHITE)).child("New playlist"));

        let mut list = div().id("pop-list").h(px(list_h)).overflow_y_scroll().track_scroll(&pop.scroll).px(px(12.)).pb(px(8.)).flex().flex_col();
        if !saved_in.is_empty() {
            list = list.child(title("Saved in", 12.)).children(saved_in);
        }
        if !others.is_empty() {
            list = list.child(title("Recently updated", 12.)).children(others);
        }

        let footer = div()
            .h(px(56.))
            .px(px(24.))
            .flex()
            .flex_row()
            .items_center()
            .justify_end()
            .gap(px(16.))
            .child(
                div()
                    .id("pop-cancel")
                    .h(px(32.))
                    .flex()
                    .items_center()
                    .cursor_pointer()
                    .text_size(px(14.))
                    .font_weight(FontWeight::BOLD)
                    .text_color(c(TEXT_SUBDUED))
                    .hover(|s| s.text_color(c(WHITE)))
                    .on_click(cx.listener(|this, _, _, cx| this.close_add_popover(cx)))
                    .child("Cancel"),
            )
            .when(changed, |d| {
                d.child(
                    div()
                        .id("pop-done")
                        .h(px(32.))
                        .px(px(16.))
                        .rounded_full()
                        .bg(c(WHITE))
                        .flex()
                        .items_center()
                        .cursor_pointer()
                        .text_size(px(14.))
                        .font_weight(FontWeight::BOLD)
                        .text_color(c(0x000000))
                        .hover(|s| s.bg(c(0xF0F0F0)))
                        .on_click(cx.listener(|this, _, _, cx| this.apply_add_popover(cx)))
                        .child("Done"),
                )
            });

        let panel = div()
            .id("add-popover")
            .absolute()
            .left(px(x))
            .top(px(y))
            .w(px(W))
            .rounded(px(8.))
            .bg(c(ELEVATED))
            .shadow(vec![
                gpui::BoxShadow { color: gpui::hsla(0., 0., 0., 0.3), offset: gpui::point(px(0.), px(16.)), blur_radius: px(24.), spread_radius: px(0.) },
                gpui::BoxShadow { color: gpui::hsla(0., 0., 0., 0.2), offset: gpui::point(px(0.), px(6.)), blur_radius: px(8.), spread_radius: px(0.) },
            ])
            .flex()
            .flex_col()
            .occlude()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .h(px(40.))
                    .px(px(20.))
                    .flex()
                    .items_center()
                    .text_size(px(12.))
                    .font_weight(FontWeight::BOLD)
                    .text_color(c(TEXT_SUBDUED))
                    .child("Add to playlist"),
            )
            .child(search)
            .child(div().h(px(4.)))
            .child(new_playlist)
            .child(div().mx(px(20.)).mt(px(8.)).h(px(0.8)).bg(rgba(0xFFFFFF1A)))
            .child(list)
            .child(footer);

        // A click anywhere else cancels.
        Some(
            div()
                .absolute()
                .inset_0()
                .child(
                    div()
                        .id("add-popover-backdrop")
                        .absolute()
                        .inset_0()
                        .on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, cx| this.close_add_popover(cx))),
                )
                .child(panel)
                .into_any_element(),
        )
    }
}
