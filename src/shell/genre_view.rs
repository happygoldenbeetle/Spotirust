//! A Browse category page (`/genre/<id>`), measured in the reference (main 835 x viewport 818):
//!   header 40vh: the category's wide image (cover, position 50% 15%, scale 1.1) over its colour,
//!   linear-gradient(transparent, rgba(0,0,0,.6)) on top; title SpotifyMixUITitle 96/800 (line 1.2),
//!   its bottom at the content spacing above the header's.
//!   Under the header a 232px band of the colour under linear-gradient(rgba(0,0,0,.6), #121212).
//!   Shelves from 32 + 16 below the header, 24 apart: a 48px header (title 24/700, "Show all"
//!   14/700 8 from the right) and one row of cards 8 below it, (W - 2 * spacing + 24) wide from
//!   spacing - 12, columns of at least 180.
//!   Data: pathfinder `browsePage` (10 sections); "Show all" opens the section (`browseSection`).

use super::browse_view::content_spacing;
use super::*;

pub struct GenrePage {
    pub title: String,
    pub color: u32,
    pub image: Option<String>,
    pub shelves: Vec<home::Shelf>,
}

pub fn parse_genre(v: &serde_json::Value) -> Option<GenrePage> {
    let b = v.pointer("/data/browse")?;
    let title = b.pointer("/header/title/transformedLabel").and_then(|t| t.as_str()).unwrap_or_default().to_string();
    let color = b
        .pointer("/header/color/hex")
        .and_then(|h| h.as_str())
        .and_then(|h| u32::from_str_radix(h.trim_start_matches('#'), 16).ok())
        .unwrap_or(0x535353);
    // the middle source (~1500 wide) stays sharp at full width without the 2660px one
    let sources = b.pointer("/header/backgroundImage/sources").and_then(|s| s.as_array());
    let image = sources
        .and_then(|s| s.get(1).or_else(|| s.last()))
        .and_then(|s| s.get("url"))
        .and_then(|u| u.as_str())
        .map(str::to_string);
    let shelves = b
        .pointer("/sections/items")
        .and_then(|s| s.as_array())
        .map(|a| a.iter().filter_map(home::shelf).collect())
        .unwrap_or_default();
    Some(GenrePage { title, color, image, shelves })
}

impl Shell {
    pub(super) fn load_genre(&mut self, uri: String, cx: &mut Context<Self>) {
        let Some(sp) = self.spotify.clone() else { return };
        if self.genre_pages.contains_key(&uri) || !self.loading_pages.insert(uri.clone()) {
            return;
        }
        cx.spawn(async move |this, cx| {
            let u = uri.clone();
            let result = cx.background_executor().spawn(async move { sp.browse_page(&u) }).await;
            let _ = this.update(cx, |this, cx| {
                this.loading_pages.remove(&uri);
                match result.map(|v| parse_genre(&v)) {
                    Ok(Some(page)) => {
                        this.genre_pages.insert(uri, page);
                    }
                    Ok(None) => eprintln!("genre {uri}: nothing in the response"),
                    Err(e) => eprintln!("genre {uri}: {e:#}"),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Whether `uri` is a shelf on a loaded Browse category (its "Show all" uses `browseSection`).
    pub(super) fn is_browse_section(&self, uri: &str) -> bool {
        self.genre_pages.values().any(|p| p.shelves.iter().any(|s| s.uri == uri))
    }

    pub(super) fn genre_view(&self, uri: &str, main_w: f32, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let panel = div().flex_1().min_w_0().h_full().rounded(px(8.)).bg(c(SURFACE)).overflow_hidden().relative();
        let Some(page) = self.genre_pages.get(uri) else {
            return panel.into_any_element();
        };
        let w = main_w;
        let vh = f32::from(window.viewport_size().height);
        let hdr_h = (vh * 0.4 * 10.).round() / 10.;
        let pad = content_spacing(w);

        let mut tf = gpui::font(title_font());
        tf.weight = FontWeight(800.);
        let title_size = [96., 72., 56.]
            .into_iter()
            .find(|&s| super::playlist_view::text_width(window, &page.title, s, tf.clone()) <= w - 2. * pad)
            .unwrap_or(48.);
        let banner = page.image.as_ref().and_then(|u| {
            let url = format!("crop:{w:.1}x{hdr_h:.1}@0.5,0.15:{u}");
            images::cache(cx).update(cx, |c, cx| c.get_sized(&url, w * 1.1, cx))
        });
        let header = div()
            .relative()
            .flex_none()
            .w_full()
            .h(px(hdr_h))
            .overflow_hidden()
            .bg(c(page.color))
            .when_some(banner, |d, b| {
                d.child(
                    img(ImageSource::Render(b))
                        .absolute()
                        .left(px(-w * 0.05))
                        .top(px(-hdr_h * 0.05))
                        .w(px(w * 1.1))
                        .h(px(hdr_h * 1.1)),
                )
            })
            .child(div().absolute().inset_0().bg(linear_gradient(
                180.,
                linear_color_stop(rgba(0x00000000), 0.),
                linear_color_stop(rgba(0x00000099), 1.),
            )))
            .child(
                div()
                    .absolute()
                    .left(px(pad))
                    .right(px(pad))
                    .bottom(px(pad))
                    .font_family(tf.family.clone())
                    .font_weight(FontWeight(800.))
                    .text_size(px(title_size))
                    .line_height(px(title_size * 1.2))
                    .text_color(c(WHITE))
                    .whitespace_nowrap()
                    .overflow_hidden()
                    .text_ellipsis()
                    .child(page.title.clone()),
            );

        let grid_w = w - 2. * pad + 24.;
        let cols = ((grid_w / 180.).floor() as usize).max(1);
        let card_w = grid_w / cols as f32;
        let shelves: Vec<AnyElement> = page
            .shelves
            .iter()
            .enumerate()
            .map(|(si, shelf)| {
                let show_all = Page::Section(shelf.uri.clone(), shelf.title.clone());
                let open_title = show_all.clone();
                div()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .h(px(48.))
                            .px(px(pad))
                            .flex()
                            .flex_row()
                            .items_end()
                            .child(
                                div().flex_1().min_w_0().flex().child(
                                    one_line(shelf.title.clone())
                                        .id(gpui::ElementId::Name(format!("genre-title-{si}").into()))
                                        .font_family(title_font())
                                        .text_size(px(24.))
                                        .line_height(px(28.8))
                                        .font_weight(FontWeight::BOLD)
                                        .text_color(c(WHITE))
                                        .cursor_pointer()
                                        .hover(|s| s.underline())
                                        .on_click(cx.listener(move |this, _, _, cx| this.open(open_title.clone(), cx))),
                                ),
                            )
                            .child(
                                div()
                                    .id(gpui::ElementId::Name(format!("genre-all-{si}").into()))
                                    .pr(px(8.))
                                    .pb(px(1.))
                                    .text_size(px(14.))
                                    .line_height(px(19.2))
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(c(TEXT_SUBDUED))
                                    .cursor_pointer()
                                    .hover(|s| s.underline())
                                    .on_click(cx.listener(move |this, _, _, cx| this.open(show_all.clone(), cx)))
                                    .child("Show all"),
                            ),
                    )
                    .child(div().mt(px(8.)).ml(px(pad - 12.)).flex().flex_row().items_start().children(
                        shelf.items.iter().take(cols).enumerate().map(|(k, it)| {
                            self.sized_card(format!("genre-{si}-{k}").into(), it, card_w, 0., cx)
                        }),
                    ))
                    .into_any_element()
            })
            .collect();

        let band = div()
            .absolute()
            .top_0()
            .left_0()
            .right_0()
            .h(px(232.))
            .bg(c(page.color))
            .child(div().absolute().inset_0().bg(linear_gradient(
                180.,
                linear_color_stop(rgba(0x00000099), 0.),
                linear_color_stop(rgba(SURFACE << 8 | 0xFF), 1.),
            )));

        panel
            .child(
                div()
                    .id("genre-scroll")
                    .track_scroll(&self.page_scroll)
                    .absolute()
                    .inset_0()
                    .overflow_y_scroll()
                    .child(header)
                    .child(
                        div()
                            .relative()
                            .pb(px(32.))
                            .child(band)
                            .child(div().relative().pt(px(48.)).flex().flex_col().gap(px(24.)).children(shelves)),
                    ),
            )
            .into_any_element()
    }
}
