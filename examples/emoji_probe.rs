//! Renders an emoji through several font setups to see which path GPUI draws.
//! cargo run --example emoji_probe

use gpui::{
    App, Application, Bounds, Context, Window, WindowBounds, WindowOptions, div, prelude::*, px, rgb, size,
};

struct Probe;

impl Render for Probe {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let row = |label: &str, family: &str| {
            div()
                .flex()
                .gap(px(16.))
                .child(div().w(px(220.)).child(label.to_string()))
                .child(div().font_family(family.to_string()).child("A \u{1F48C} \u{2764}\u{FE0F} \u{1F600} B"))
        };
        div()
            .size_full()
            .bg(rgb(0x121212))
            .text_color(rgb(0xffffff))
            .text_size(px(24.))
            .p(px(20.))
            .flex()
            .flex_col()
            .gap(px(12.))
            .child(row("Segoe UI", "Segoe UI"))
            .child(row("Segoe UI Emoji", "Segoe UI Emoji"))
            .child(row("Segoe UI Symbol", "Segoe UI Symbol"))
            .child(row("Arial", "Arial"))
    }
}

fn main() {
    Application::new().run(|cx: &mut App| {
        // Created hidden; the capture script parks it off-screen before showing it.
        let bounds = Bounds::centered(None, size(px(700.), px(300.)), cx);
        cx.open_window(
            WindowOptions { window_bounds: Some(WindowBounds::Windowed(bounds)), show: false, ..Default::default() },
            |_, cx| cx.new(|_| Probe),
        )
        .unwrap();
    });
}
