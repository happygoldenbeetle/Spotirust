//! The OS tooltip Chromium shows for an element's `title` (the client sets `title` on links in
//! Listening activity, cards and so on). Windows 11 dark style: #2B2B2B, 1px #757575 border,
//! Segoe UI 12px, padding 3 7, appearing below the pointer.

use gpui::{AnyView, App, AppContext, Context, IntoElement, ParentElement, Render, SharedString, Styled, Window, div, px, rgb};

pub struct NativeTip(SharedString);

impl Render for NativeTip {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().pt(px(18.)).child(
            div()
                .px(px(7.))
                .py(px(3.))
                .bg(rgb(0x2B2B2B))
                .border_1()
                .border_color(rgb(0x757575))
                .font_family("Segoe UI")
                .text_size(px(12.))
                .line_height(px(16.))
                .text_color(rgb(0xFFFFFF))
                .whitespace_nowrap()
                .child(self.0.clone()),
        )
    }
}

/// A `.tooltip(..)` builder showing `text` like a native `title` tooltip.
pub fn native_tip(text: impl Into<SharedString>) -> impl Fn(&mut Window, &mut App) -> AnyView + 'static {
    let text = text.into();
    move |_, cx| cx.new(|_| NativeTip(text.clone())).into()
}
