//! What's New (the bell, `/content-feed`), measured in the reference (main 825 wide):
//!   a column at most 800 wide, centred, with the content spacing inside; 48 below the panel top:
//!   "What's New" SpotifyMixUITitle 32/700 (38.4 line, 4 below), "The latest releases from
//!   artists, podcasts, and shows you follow." 14px #b3b3b3, chips 16 below ("Music",
//!   "Podcast & Shows": 32 tall, 4px 12px, rgba(255,255,255,.1), white when picked, 8 apart).
//!   Nothing new: "We don't have any updates for you yet" 32/700 centred 104 below the chips,
//!   then 24 below it the explanation (16px white, line 22.4).
//!   Data: pathfinder `queryWhatsNewFeed` (50 items; Music asks for ALBUM, Podcast & Shows for
//!   EPISODE). The bell shows its filled icon while the page is open.

use super::*;
use crate::home::Item;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum NewFilter {
    Music,
    Podcasts,
}

impl NewFilter {
    fn label(self) -> &'static str {
        match self {
            NewFilter::Music => "Music",
            NewFilter::Podcasts => "Podcast & Shows",
        }
    }

    pub fn content_type(self) -> &'static str {
        match self {
            NewFilter::Music => "ALBUM",
            NewFilter::Podcasts => "EPISODE",
        }
    }
}

/// The feed for one filter (None = everything).
pub(super) struct NewFeed {
    pub filter: Option<NewFilter>,
    pub items: Vec<Item>,
}

fn parse_feed(v: &serde_json::Value) -> Vec<Item> {
    // the feed's entries carry the usual `{ uri, content: { data } }` wrappers
    let items = v
        .pointer("/data/whatsNewFeedItems/items")
        .or_else(|| v.pointer("/data/whatsNewFeed/items"))
        .and_then(|a| a.as_array());
    items.into_iter().flatten().filter_map(crate::home::parse_item).collect()
}

impl Shell {
    pub(super) fn load_whats_new(&mut self, cx: &mut Context<Self>) {
        let Some(sp) = self.spotify.clone() else { return };
        let filter = self.new_filter;
        if self.whats_new.as_ref().is_some_and(|f| f.filter == filter) || !self.loading_pages.insert("whats-new".into()) {
            return;
        }
        cx.spawn(async move |this, cx| {
            let result = cx.background_executor().spawn(async move { sp.whats_new(filter.map(|f| f.content_type())) }).await;
            let _ = this.update(cx, |this, cx| {
                this.loading_pages.remove("whats-new");
                match result {
                    Ok(v) => this.whats_new = Some(NewFeed { filter, items: parse_feed(&v) }),
                    Err(e) => eprintln!("what's new: {e:#}"),
                }
                // the chip may have changed while this loaded
                if this.new_filter != filter {
                    this.load_whats_new(cx);
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn whats_new_view(&self, main_w: f32, cx: &mut Context<Self>) -> AnyElement {
        let pad = super::browse_view::content_spacing(main_w);
        let col_w = main_w.min(800.);
        let feed = self.whats_new.as_ref().filter(|f| f.filter == self.new_filter);
        let chip = |f: NewFilter, cx: &mut Context<Self>| {
            let on = self.new_filter == Some(f);
            div()
                .id(gpui::ElementId::Name(format!("new-{f:?}").into()))
                .h(px(32.))
                .px(px(12.))
                .flex()
                .items_center()
                .rounded_full()
                .cursor_pointer()
                .text_size(px(14.))
                .when(on, |d| d.bg(c(WHITE)).text_color(c(0x000000)).hover(|s| s.bg(c(0xF0F0F0))))
                .when(!on, |d| d.bg(rgba(0xFFFFFF1A)).text_color(c(WHITE)).hover(|s| s.bg(rgba(0xFFFFFF24))))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.new_filter = if this.new_filter == Some(f) { None } else { Some(f) };
                    this.load_whats_new(cx);
                    cx.notify();
                }))
                .child(f.label())
        };
        let body: AnyElement = match feed {
            None => div().into_any_element(),
            Some(f) if f.items.is_empty() => div()
                .mt(px(104.))
                .flex()
                .flex_col()
                .child(
                    div()
                        .flex()
                        .justify_center()
                        .font_family(title_font())
                        .text_size(px(32.))
                        .line_height(px(38.4))
                        .font_weight(FontWeight::BOLD)
                        .text_color(c(WHITE))
                        .child("We don't have any updates for you yet"),
                )
                .child(
                    div()
                        .mt(px(24.))
                        .text_size(px(16.))
                        .line_height(px(22.4))
                        .text_color(c(WHITE))
                        .child("When there's news, we'll post it here. Follow your favorite artists and podcasts to stay updated on them too."),
                )
                .into_any_element(),
            // releases: one card per item (the client's feed rows aren't measured yet; your feed is empty)
            Some(f) => {
                let grid_w = col_w - 2. * pad + 24.;
                let cols = ((grid_w / 180.).floor() as usize).max(1);
                let card_w = grid_w / cols as f32;
                let mut grid = div().mt(px(24.)).ml(px(-12.)).flex().flex_col().gap(px(24.));
                for (r, chunk) in f.items.chunks(cols).enumerate() {
                    grid = grid.child(div().flex().flex_row().items_start().children(
                        chunk.iter().enumerate().map(|(k, it)| self.sized_card(format!("new-{}", r * cols + k).into(), it, card_w, 0., cx)),
                    ));
                }
                grid.into_any_element()
            }
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
                div().id("new-scroll").track_scroll(&self.page_scroll).absolute().inset_0().overflow_y_scroll().pb(px(40.)).child(
                    div()
                        .mx_auto()
                        .w(px(col_w))
                        .px(px(pad))
                        .pt(px(48.))
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .pb(px(4.))
                                .font_family(title_font())
                                .text_size(px(32.))
                                .line_height(px(38.4))
                                .font_weight(FontWeight::BOLD)
                                .text_color(c(WHITE))
                                .child("What's New"),
                        )
                        .child(
                            div()
                                .text_size(px(14.))
                                .line_height(px(19.2))
                                .text_color(c(TEXT_SUBDUED))
                                .child("The latest releases from artists, podcasts, and shows you follow."),
                        )
                        .child(
                            div()
                                .mt(px(16.))
                                .flex()
                                .flex_row()
                                .gap(px(8.))
                                .child(chip(NewFilter::Music, cx))
                                .child(chip(NewFilter::Podcasts, cx)),
                        )
                        .child(body),
                ),
            )
            .into_any_element()
    }
}
