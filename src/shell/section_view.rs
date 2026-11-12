//! A Home section's "Show all" page (`/section/<id>`), measured in the reference (main 835 wide):
//!   title: the section's own (e.g. "Popular albums" for the "Popular albums and singles" shelf),
//!   SpotifyMixUITitle 32/700, 74 below the panel top, at the content spacing;
//!   cards: a grid 32 below the title, (W - 2 * spacing + 24) wide from spacing - 12 (the cards' own
//!   12px padding lines their images up with the title), columns of at least 180, rows 24 apart.
//!   The client loads the first 20 items (`homeSection`, sectionItemsLimit 20).

use super::*;
use super::browse_view::content_spacing;

impl Shell {
    pub(super) fn load_section(&mut self, uri: String, cx: &mut Context<Self>) {
        let Some(sp) = self.spotify.clone() else { return };
        if self.section_pages.contains_key(&uri) || !self.loading_pages.insert(uri.clone()) {
            return;
        }
        // a Browse category's shelf loads through its own query
        let browse = self.is_browse_section(&uri);
        cx.spawn(async move |this, cx| {
            let u = uri.clone();
            let result = cx
                .background_executor()
                .spawn(async move { if browse { sp.browse_section(&u) } else { sp.home_section(&u) } })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.loading_pages.remove(&uri);
                let parse = if browse { home::parse_browse_section } else { home::parse_section };
                match result.map(|v| parse(&v)) {
                    Ok(Some(shelf)) => {
                        this.section_pages.insert(uri, shelf);
                    }
                    Ok(None) => eprintln!("section {uri}: nothing in the response"),
                    Err(e) => eprintln!("section {uri}: {e:#}"),
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn section_view(&self, uri: &str, title: &str, main_w: f32, cx: &mut Context<Self>) -> AnyElement {
        let pad = content_spacing(main_w);
        let grid_w = main_w - 2. * pad + 24.;
        let cols = ((grid_w / 180.).floor() as usize).max(1);
        let card_w = grid_w / cols as f32;
        let page = self.section_pages.get(uri);
        let title = page.map(|p| p.title.clone()).unwrap_or_else(|| title.to_string());
        let mut grid = div().mt(px(32.)).ml(px(pad - 12.)).flex().flex_col().gap(px(24.));
        if let Some(p) = page {
            for (row, chunk) in p.items.chunks(cols).enumerate() {
                grid = grid.child(div().flex().flex_row().items_start().children(chunk.iter().enumerate().map(|(k, it)| {
                    self.sized_card(format!("sec-{}", row * cols + k).into(), it, card_w, 0., cx)
                })));
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
                div()
                    .id("section-scroll")
                    .track_scroll(&self.page_scroll)
                    .absolute()
                    .inset_0()
                    .overflow_y_scroll()
                    .pb(px(32.))
                    .child(
                        div()
                            .mt(px(74.))
                            .px(px(pad))
                            .font_family(title_font())
                            .text_size(px(32.))
                            .line_height(px(38.))
                            .font_weight(FontWeight::BOLD)
                            .text_color(c(WHITE))
                            .child(title),
                    )
                    .child(grid),
            )
            .into_any_element()
    }
}
