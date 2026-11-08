//! Logged-out screen: blurred "fake web player" backdrop + centered login card.
//! All coordinates are the reference client's CSS px.

use std::sync::Arc;

use gpui::{
    App, Bounds, Context, FontWeight, ImageSource, InteractiveElement, IntoElement, ObjectFit,
    ParentElement, Pixels, Render, RenderImage, StatefulInteractiveElement, Styled, StyledImage,
    Window,
    canvas, div, fill, img, point, px, size, svg,
};
use qrcode::QrCode;

use crate::backdrop::{self, BackdropAssets};
use crate::theme::*;
use crate::titlebar::overlay_titlebar;

const SIGNUP_URL: &str = "https://www.spotify.com/signup";

/// Emitted once the browser sign-in completed and tokens were saved.
pub struct LoggedIn(pub crate::auth::Token);

impl gpui::EventEmitter<LoggedIn> for LoginView {}

pub struct LoginView {
    logging_in: bool,
    qr: Vec<bool>,
    qr_width: usize,
    assets: Option<Arc<BackdropAssets>>,
    backdrop: Option<Arc<RenderImage>>,
    backdrop_key: Option<(u32, u32, u32)>,
    composing: bool,
}

impl LoginView {
    pub fn new() -> Self {
        // Placeholder payload until the real device-login flow is wired up.
        let code = QrCode::new(b"https://accounts.spotify.com/login").expect("qr");
        let qr_width = code.width();
        let qr = code
            .to_colors()
            .into_iter()
            .map(|c| c == qrcode::Color::Dark)
            .collect();
        let assets = match BackdropAssets::load() {
            Ok(a) => Some(Arc::new(a)),
            Err(e) => {
                eprintln!("backdrop assets unavailable: {e:#}");
                None
            }
        };
        Self { logging_in: false, qr, qr_width, assets, backdrop: None, backdrop_key: None, composing: false }
    }

    /// Opens Spotify's sign-in page in the browser and waits for the local callback.
    fn start_login(&mut self, cx: &mut Context<Self>) {
        if self.logging_in {
            return;
        }
        let pending = match crate::auth::begin_login() {
            Ok(p) => p,
            Err(e) => {
                eprintln!("login: {e:#}");
                return;
            }
        };
        cx.open_url(&pending.url);
        self.logging_in = true;
        cx.spawn(async move |this, cx| {
            let result = cx.background_executor().spawn(async move { pending.wait() }).await;
            let _ = this.update(cx, |this, cx| {
                this.logging_in = false;
                match result {
                    Ok(token) => cx.emit(LoggedIn(token)),
                    Err(e) => eprintln!("login failed: {e:#}"),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Recompose the blurred backdrop whenever the viewport size or DPI changes.
    fn ensure_backdrop(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(assets) = self.assets.clone() else { return };
        let vp = window.viewport_size();
        let scale = window.scale_factor();
        let key = (
            (f32::from(vp.width) * 100.) as u32,
            (f32::from(vp.height) * 100.) as u32,
            (scale * 100.) as u32,
        );
        if self.composing || self.backdrop_key == Some(key) {
            return;
        }
        self.composing = true;
        let (w, h) = (f32::from(vp.width), f32::from(vp.height));
        cx.spawn_in(window, async move |this, cx| {
            let image = cx
                .background_executor()
                .spawn(async move {
                    let mut rgba = backdrop::compose(&assets, w, h, scale);
                    for p in rgba.pixels_mut() {
                        p.0.swap(0, 2); // gpui wants BGRA
                    }
                    Arc::new(RenderImage::new(vec![image::Frame::new(rgba)]))
                })
                .await;
            let _ = this.update_in(cx, |this, window, cx| {
                if let Some(old) = this.backdrop.replace(image) {
                    cx.drop_image(old, Some(window));
                }
                this.backdrop_key = Some(key);
                this.composing = false;
                cx.notify(); // re-check: the window may have been resized meanwhile
            });
        })
        .detach();
    }
}

// ---------- login card -----------------------------------------------------------------------

fn brand_mark(d: f32, bg: u32, fg: u32) -> impl IntoElement {
    div()
        .size(px(d))
        .rounded_full()
        .bg(c(bg))
        .flex()
        .items_center()
        .justify_center()
        .child(svg().path("icons/mark_bars.svg").size(px(d * 0.78)).text_color(c(fg)))
}

fn logo() -> impl IntoElement {
    div()
        .h(px(38.))
        .flex()
        .flex_row()
        .items_center()
        .justify_center()
        .gap(px(8.))
        .child(brand_mark(38., WHITE, 0x000000))
        .child(
            div()
                .font_family(title_font())
                .text_size(px(30.))
                .line_height(px(38.))
                .font_weight(FontWeight::BOLD)
                .text_color(c(WHITE))
                .child("Spotirust"),
        )
}

fn title_line(text: &'static str) -> impl IntoElement {
    div()
        .font_family(title_font())
        .text_size(px(24.))
        .line_height(px(28.8))
        .font_weight(FontWeight::BOLD)
        .text_color(c(WHITE))
        .child(text)
}

fn left_column(
    on_login: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    div()
        .w(px(303.5))
        .h(px(292.))
        .flex()
        .flex_col()
        .items_center()
        .text_center()
        .child(logo())
        .child(
            div()
                .mt(px(26.3))
                .flex()
                .flex_col()
                .items_center()
                .child(title_line("Millions of songs."))
                .child(title_line("Free on Spotify.")),
        )
        .child(
            div()
                .id("login-button")
                .mt(px(26.3))
                .h(px(48.))
                .pl(px(32.))
                .pr(px(56.))
                .relative()
                .flex()
                .items_center()
                .rounded_full()
                .bg(c(GREEN))
                .hover(|s| s.bg(c(GREEN_HOVER)))
                .cursor_pointer()
                .on_click(on_login)
                .text_size(px(16.))
                .font_weight(FontWeight::BOLD)
                .text_color(c(0x000000))
                .child("Log in")
                .child(
                    svg()
                        .path("icons/external.svg")
                        .absolute()
                        .right(px(24.))
                        .top(px(12.))
                        .size(px(24.))
                        .text_color(c(0x000000)),
                ),
        )
        .child(
            div()
                .mt(px(26.3))
                .h(px(23.2))
                .flex()
                .flex_row()
                .items_center()
                .text_size(px(16.))
                .line_height(px(23.2))
                .text_color(c(TEXT_SUBDUED))
                .child("New to Spotify?\u{00a0}")
                .child(
                    div()
                        .id("signup")
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(4.))
                        .text_color(c(WHITE))
                        .cursor_pointer()
                        .hover(|s| s.underline())
                        .on_click(|_, _, cx| cx.open_url(SIGNUP_URL))
                        .child("Sign up free")
                        .child(svg().path("icons/external.svg").size(px(16.)).text_color(c(WHITE))),
                ),
        )
        .child(
            div()
                .id("settings")
                .mt(px(14.3))
                .h(px(32.))
                .px(px(16.))
                .flex()
                .items_center()
                .rounded_full()
                .text_size(px(14.))
                .font_weight(FontWeight::BOLD)
                .text_color(c(TEXT_SUBDUED))
                .hover(|s| s.text_color(c(WHITE)))
                .cursor_pointer()
                .child("Settings"),
        )
}

fn qr_canvas(modules: Vec<bool>, width: usize) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |bounds: Bounds<Pixels>, _, window: &mut Window, _: &mut App| {
            let cell = bounds.size.width / width as f32;
            for y in 0..width {
                for x in 0..width {
                    if modules[y * width + x] {
                        window.paint_quad(fill(
                            Bounds::new(
                                point(bounds.origin.x + cell * x as f32, bounds.origin.y + cell * y as f32),
                                size(cell, cell),
                            ),
                            c(0x000000),
                        ));
                    }
                }
            }
        },
    )
    .size(px(136.))
}

fn right_column(qr: Vec<bool>, qr_width: usize) -> impl IntoElement {
    div()
        .w(px(303.5))
        .h(px(292.))
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .text_center()
        .child(
            div()
                .size(px(160.))
                .p(px(12.))
                .rounded(px(8.))
                .bg(c(WHITE))
                .relative()
                .child(qr_canvas(qr, qr_width))
                .child(
                    div()
                        .absolute()
                        .left(px(62.))
                        .top(px(62.))
                        .size(px(36.))
                        .p(px(4.))
                        .bg(c(WHITE))
                        .child(brand_mark(28., 0x000000, WHITE)),
                ),
        )
        .child(div().mt(px(24.)).child(title_line("Scan code to log in")))
        .child(
            div()
                .mt(px(8.))
                .text_size(px(16.))
                .line_height(px(22.4))
                .text_color(c(TEXT_SUBDUED))
                .flex()
                .flex_col()
                .items_center()
                .child("On your mobile phone, open the camera")
                .child("or the QR scanner app to scan this code."),
        )
}

impl Render for LoginView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.ensure_backdrop(window, cx);

        let backdrop = match &self.backdrop {
            // Fill: while a resize recompose is in flight, stretch instead of letterboxing.
            Some(image) => img(ImageSource::Render(image.clone()))
                .object_fit(ObjectFit::Fill)
                .absolute()
                .inset_0()
                .size_full()
                .into_any_element(),
            None => div().absolute().inset_0().bg(c(0x000000)).into_any_element(),
        };

        div()
            .size_full()
            .relative()
            .font_family(ui_font())
            .text_size(px(14.))
            .text_color(c(TEXT_SUBDUED))
            .bg(c(0x000000))
            .child(backdrop)
            // .App__content starts 30px down; the card is centered inside it.
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .pt(px(30.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        div()
                            .w(px(768.))
                            .h(px(356.))
                            .p(px(32.))
                            .rounded(px(8.))
                            .bg(c(BG_BASE))
                            .flex()
                            .flex_row()
                            .gap(px(48.))
                            .child(left_column(cx.listener(|this, _, _, cx| this.start_login(cx))))
                            .child(div().w(px(1.)).h(px(292.)).bg(c(DIVIDER)))
                            .child(right_column(self.qr.clone(), self.qr_width)),
                    ),
            )
            .child(overlay_titlebar())
    }
}
