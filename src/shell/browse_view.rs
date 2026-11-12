//! Browse page (`/search` with no query): "Browse all" category cards. Measured on the reference:
//!   content padding `clamp(16, 16 + (W - 600) / 424 * 8, 24)`; the section starts 48 below the
//!   top (32 + 16); header 48 tall with "Browse all" 24/700 at its bottom, 8 above the grid.
//!   Grid: `repeat(auto-fill, minmax(min, 1fr))`, min 180 / 254 / 300 for grid widths < 520 /
//!   < 1280 / more, inset -12, cells padding 12. Cards 16:9, radius 8, the category colour; title
//!   24/700 white (16/700 when the grid is <= 520), padding 16 16 0, 30% right, 3 lines max; art
//!   45% wide, radius 4, bottom-right, `rotate(25deg) translate(18%, -2%)`, shadow 0 2 4 .2.
//!   Data: pathfinder `browseAll`.

use super::*;

#[derive(Clone)]
pub struct BrowseCard {
    pub uri: String,
    pub title: String,
    pub color: u32,
    pub image: Option<String>,
}

pub fn parse_browse(v: &serde_json::Value) -> Vec<BrowseCard> {
    let mut out = Vec::new();
    let sections = v.pointer("/data/browseStart/sections/items").and_then(|s| s.as_array());
    for section in sections.into_iter().flatten() {
        for item in section.pointer("/sectionItems/items").and_then(|i| i.as_array()).into_iter().flatten() {
            let data = item.pointer("/content/data");
            // category pages carry a `cardRepresentation`; client features (Live Events) the fields directly
            let card = data.and_then(|d| d.pointer("/data/cardRepresentation")).or(data);
            let Some(card) = card else { continue };
            let title = card.pointer("/title/transformedLabel").and_then(|t| t.as_str()).unwrap_or_default();
            if title.is_empty() {
                continue;
            }
            let color = card
                .pointer("/backgroundColor/hex")
                .and_then(|h| h.as_str())
                .and_then(|h| u32::from_str_radix(h.trim_start_matches('#'), 16).ok())
                .unwrap_or(0x333333);
            out.push(BrowseCard {
                uri: item.get("uri").and_then(|u| u.as_str()).unwrap_or_default().to_string(),
                title: title.to_string(),
                color,
                image: card.pointer("/artwork/sources/0/url").and_then(|u| u.as_str()).map(str::to_string),
            });
        }
    }
    out
}

/// `--content-spacing` of the main view (`clamp(16px, 16px + (100cqw - 600px) / 424 * 8, 24px)`).
pub fn content_spacing(main_w: f32) -> f32 {
    (16. + (main_w - 600.) / 424. * 8.).clamp(16., 24.)
}

impl Shell {
    pub(super) fn load_browse(&mut self, cx: &mut Context<Self>) {
        let Some(sp) = self.spotify.clone() else { return };
        if self.browse.is_some() || !self.loading_pages.insert("browse".into()) {
            return;
        }
        cx.spawn(async move |this, cx| {
            let result = cx.background_executor().spawn(async move { sp.browse_all() }).await;
            let _ = this.update(cx, |this, cx| {
                this.loading_pages.remove("browse");
                match result {
                    Ok(v) => this.browse = Some(parse_browse(&v)),
                    Err(e) => eprintln!("browse: {e:#}"),
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn browse_view(&self, main_w: f32, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let pad = content_spacing(main_w);
        let section_w = main_w - 2. * pad;
        let grid_w = section_w + 24.;
        let min_col = if grid_w < 520. {
            180.
        } else if grid_w < 1280. {
            254.
        } else {
            300.
        };
        let cols = (grid_w / min_col).floor().max(1.) as usize;
        let col_w = grid_w / cols as f32;
        let card_w = col_w - 24.;
        let card_h = card_w * 0.5625;
        let title_size = if grid_w <= 520. { 16. } else { 24. };
        let scale = window.scale_factor();

        // rotated art geometry (see images::rotated_canvas)
        let s = card_w * 0.45;
        let (sin, cos) = 25f32.to_radians().sin_cos();
        let n = (s * scale).round();
        let canvas = ((n * (cos + sin)).ceil() + 2. * (images::rotated_margin_css() * scale).round()) / scale;
        let center_x = card_w - s / 2. + 0.18 * s * cos + 0.02 * s * sin;
        let center_y = card_h - s / 2. + 0.18 * s * sin - 0.02 * s * cos;

        let cards = self.browse.clone().unwrap_or_default();
        let mut grid = div().ml(px(-12.)).w(px(grid_w)).flex().flex_col();
        for (r, row) in cards.chunks(cols).enumerate() {
            let mut row_el = div().flex().flex_row();
            for (k, card) in row.iter().enumerate() {
                let i = r * cols + k;
                let art = card.image.as_ref().and_then(|u| {
                    let key = format!("rot:25:{u}");
                    images::cache(cx).update(cx, |c, cx| c.get_sized(&key, s, cx))
                });
                row_el = row_el.child(
                    div().w(px(col_w)).p(px(12.)).child(
                        div()
                            .id(gpui::ElementId::Name(format!("browse-{i}").into()))
                            .relative()
                            .w(px(card_w))
                            .h(px(card_h))
                            .rounded(px(8.))
                            .overflow_hidden()
                            .bg(c(card.color))
                            .cursor_pointer()
                            .when_some(Page::for_uri(&card.uri), |d, page| {
                                d.on_click(cx.listener(move |this, _, _, cx| this.open(page.clone(), cx)))
                            })
                            .when_some(art, |d, image| {
                                d.child(
                                    img(ImageSource::Render(image))
                                        .absolute()
                                        .left(px(center_x - canvas / 2.))
                                        .top(px(center_y - canvas / 2.))
                                        .size(px(canvas)),
                                )
                            })
                            .child(
                                div()
                                    .absolute()
                                    .top_0()
                                    .left_0()
                                    .w(px(card_w))
                                    .pt(px(16.))
                                    .pl(px(16.))
                                    .pr(px(card_w * 0.3))
                                    .text_size(px(title_size))
                                    .line_height(px(title_size * 1.2))
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(c(WHITE))
                                    .line_clamp(3)
                                    .child(card.title.clone()),
                            ),
                    ),
                );
            }
            grid = grid.child(row_el);
        }

        div()
            .flex_1()
            .min_w_0()
            .h_full()
            .rounded(px(8.))
            .bg(c(SURFACE))
            .overflow_hidden()
            .child(
                div()
                    .id("browse-scroll")
                    .size_full()
                    .overflow_y_scroll()
                    .track_scroll(&self.page_scroll)
                    .px(px(pad))
                    .pb(px(16.))
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .mt(px(48.))
                            .h(px(48.))
                            .mb(px(8.))
                            .flex()
                            .items_end()
                            .child(
                                div()
                                    .text_size(px(24.))
                                    .line_height(px(28.8))
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(c(WHITE))
                                    .child("Browse all"),
                            ),
                    )
                    .child(grid),
            )
            .into_any_element()
    }
}
