//! Colors and type sizes measured from the reference client (CSS px == GPUI logical px).
#![allow(dead_code)]

use std::sync::OnceLock;

use gpui::{Hsla, Rgba, SharedString, rgb};

/// Bundled fallback when Spotify's own fonts can't be read from the local install.
pub const FALLBACK_FONT: &str = "Figtree";

static FONTS: OnceLock<(SharedString, SharedString)> = OnceLock::new();

pub fn set_fonts(ui: String, title: String) {
    let _ = FONTS.set((ui.into(), title.into()));
}

/// `SpotifyMixUI` equivalent (body/UI text).
pub fn ui_font() -> SharedString {
    FONTS.get().map(|f| f.0.clone()).unwrap_or_else(|| FALLBACK_FONT.into())
}

/// `SpotifyMixUITitle` equivalent (headings).
pub fn title_font() -> SharedString {
    FONTS.get().map(|f| f.1.clone()).unwrap_or_else(|| FALLBACK_FONT.into())
}

/// UI font plus fallbacks so emoji / symbols in names (e.g. "💌") render like in Chromium.
pub fn ui_font_with_fallbacks() -> gpui::Font {
    let mut font = gpui::font(ui_font());
    font.fallbacks = Some(gpui::FontFallbacks(std::sync::Arc::new(vec![
        "Segoe UI Emoji".to_string(),
        "Segoe UI Symbol".to_string(),
        "Segoe UI".to_string(),
    ])));
    font
}

pub fn c(hex: u32) -> Rgba {
    rgb(hex)
}

pub fn hsla(hex: u32) -> Hsla {
    rgb(hex).into()
}

pub const WHITE: u32 = 0xFFFFFF;
pub const TEXT_SUBDUED: u32 = 0xB3B3B3;
pub const GREEN: u32 = 0x1ED760;
pub const GREEN_HOVER: u32 = 0x3BE477;
pub const BG_BASE: u32 = 0x121212;
pub const BG_PAGE: u32 = 0x101010;
pub const DIVIDER: u32 = 0x292929;
pub const TITLEBAR_CONTROLS: u32 = 0x080808;
pub const CLOSE_HOVER: u32 = 0xE81123;
