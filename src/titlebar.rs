//! Frameless window chrome: drag area, "•••" app menu button and Win32-style caption buttons.
//! Caption buttons use `WindowControlArea`, so Windows treats them as the real min/max/close
//! (snap layouts on hover, double-click to maximize on the drag area, etc.).

use gpui::{
    AnyElement, InteractiveElement, IntoElement, ParentElement, Styled, WindowControlArea, div,
    px, svg,
};

use crate::theme::*;

pub const TITLEBAR_HEIGHT: f32 = 64.;
const CAPTION_W: f32 = 45.;

fn caption_button(id: &'static str, icon: &'static str, area: WindowControlArea) -> AnyElement {
    let hover_bg = if matches!(area, WindowControlArea::Close) {
        c(CLOSE_HOVER)
    } else {
        gpui::rgba(0xFFFFFF1A)
    };
    div()
        .id(id)
        .w(px(CAPTION_W))
        .h_full()
        .flex()
        .items_center()
        .justify_center()
        .window_control_area(area)
        .hover(move |s| s.bg(hover_bg))
        .child(svg().path(icon).size(px(10.)).text_color(c(WHITE)))
        .into_any_element()
}

/// Min / max / close cluster pinned to the top-right corner (used by the logged-in shell,
/// whose top bar is already black, so no strip background).
pub fn caption_buttons() -> impl IntoElement {
    div()
        .absolute()
        .top_0()
        .right_0()
        .h(px(TITLEBAR_HEIGHT))
        .flex()
        .flex_row()
        .child(caption_button("min", "icons/win_min.svg", WindowControlArea::Min))
        .child(caption_button("max", "icons/win_max.svg", WindowControlArea::Max))
        .child(caption_button("close", "icons/win_close.svg", WindowControlArea::Close))
}

/// Overlay title bar used while logged out: transparent drag strip, dots on the left,
/// a dark caption-button strip on the right.
pub fn overlay_titlebar() -> impl IntoElement {
    div()
        .absolute()
        .top_0()
        .left_0()
        .right_0()
        .h(px(TITLEBAR_HEIGHT))
        .flex()
        .flex_row()
        .child(
            div()
                .id("drag")
                .flex_1()
                .h_full()
                .window_control_area(WindowControlArea::Drag)
                .child(
                    div()
                        .id("app-menu")
                        .absolute()
                        .left(px(20.))
                        .top(px(16.))
                        .size(px(32.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded_full()
                        .text_color(c(WHITE))
                        .cursor_pointer()
                        .child(svg().path("icons/more.svg").size(px(20.)).text_color(c(WHITE))),
                ),
        )
        .child(
            div()
                .h_full()
                .flex()
                .flex_row()
                .bg(c(TITLEBAR_CONTROLS))
                .child(caption_button("min", "icons/win_min.svg", WindowControlArea::Min))
                .child(caption_button("max", "icons/win_max.svg", WindowControlArea::Max))
                .child(caption_button("close", "icons/win_close.svg", WindowControlArea::Close)),
        )
}
