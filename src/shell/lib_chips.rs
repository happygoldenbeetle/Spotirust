//! Your Library's filter chips, measured in the reference:
//!   chips 32 tall, padding 4px 12px, radius full, 14px/400, 8 apart: rgba(255,255,255,.1) with
//!   white text, the picked ones white with black text (background-color, color .15s productive).
//!   Picking a type puts a round 32px "Clear filters" x (rgba(255,255,255,.1), #b3b3b3 icon) in
//!   front and offers that type's own chips (Playlists: "By you", "By Spotify", "Mixed"); picking
//!   one of those keeps just the two picked chips. The row scrolls sideways: past its end the
//!   last 120px fade out, and while the row is hovered a 32px round #1f1f1f arrow at that edge
//!   (16px chevron at .7 opacity) scrolls it.

use super::*;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Chip {
    Playlists,
    Podcasts,
    Albums,
    Artists,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Sub {
    ByYou,
    BySpotify,
    Mixed,
}

impl Chip {
    const ALL: [Chip; 4] = [Chip::Playlists, Chip::Podcasts, Chip::Albums, Chip::Artists];

    pub fn label(self) -> &'static str {
        match self {
            Chip::Playlists => "Playlists",
            Chip::Podcasts => "Podcasts & Shows",
            Chip::Albums => "Albums",
            Chip::Artists => "Artists",
        }
    }

    fn has(self, kind: LibKind) -> bool {
        match self {
            Chip::Playlists => matches!(kind, LibKind::Playlist | LibKind::LikedSongs | LibKind::Folder),
            Chip::Podcasts => kind == LibKind::Show,
            Chip::Albums => kind == LibKind::Album,
            Chip::Artists => kind == LibKind::Artist,
        }
    }

    fn subs(self) -> &'static [Sub] {
        match self {
            Chip::Playlists => &[Sub::ByYou, Sub::BySpotify, Sub::Mixed],
            _ => &[],
        }
    }
}

impl Sub {
    fn label(self) -> &'static str {
        match self {
            Sub::ByYou => "By you",
            Sub::BySpotify => "By Spotify",
            Sub::Mixed => "Mixed",
        }
    }

    fn has(self, item: &LibItem, me_name: &str) -> bool {
        match self {
            Sub::ByYou => item.kind == LibKind::LikedSongs || (!me_name.is_empty() && item.owner == me_name),
            Sub::BySpotify => item.owner == "Spotify",
            Sub::Mixed => item.mixed,
        }
    }
}

impl Shell {
    /// Whether a row passes the picked chips.
    pub(super) fn lib_chip_matches(&self, item: &LibItem, me_name: &str) -> bool {
        let Some(chip) = self.lib_chip else { return true };
        chip.has(item.kind) && self.lib_sub.is_none_or(|s| s.has(item, me_name))
    }

    fn set_lib_chip(&mut self, chip: Option<Chip>, sub: Option<Sub>, cx: &mut Context<Self>) {
        self.lib_chip = chip;
        self.lib_sub = sub;
        self.lib_chips_scroll.set_offset(gpui::point(px(0.), px(0.)));
        cx.notify();
    }

    fn chip(&self, id: String, label: &'static str, on: bool, cx: &mut Context<Self>, click: impl Fn(&mut Self, &mut Context<Self>) + 'static) -> AnyElement {
        div()
            .id(gpui::ElementId::Name(id.into()))
            .flex_none()
            .h(px(32.))
            .px(px(12.))
            .flex()
            .items_center()
            .rounded_full()
            .cursor_pointer()
            .text_size(px(14.))
            .whitespace_nowrap()
            .when(on, |d| d.bg(c(WHITE)).text_color(c(0x000000)).hover(|s| s.bg(c(0xF0F0F0))))
            .when(!on, |d| d.bg(rgba(0xFFFFFF1A)).text_color(c(WHITE)).hover(|s| s.bg(rgba(0xFFFFFF24))))
            .on_click(cx.listener(move |this, _, _, cx| click(this, cx)))
            .child(label)
            .into_any_element()
    }

    /// The chip row under the header, from the types the library holds.
    pub(super) fn lib_chips_row(&self, all: &[LibItem], me_name: &str, cx: &mut Context<Self>) -> AnyElement {
        let mut chips: Vec<AnyElement> = Vec::new();
        match self.lib_chip {
            None => {
                for chip in Chip::ALL.into_iter().filter(|ch| all.iter().any(|it| ch.has(it.kind))) {
                    chips.push(self.chip(format!("chip-{chip:?}"), chip.label(), false, cx, move |this, cx| {
                        this.set_lib_chip(Some(chip), None, cx)
                    }));
                }
            }
            Some(chip) => {
                chips.push(
                    round_button("chip-clear", 32.)
                        .group("chip-clear")
                        .bg(rgba(0xFFFFFF1A))
                        .hover(|s| s.bg(rgba(0xFFFFFF24)))
                        .on_click(cx.listener(|this, _, _, cx| this.set_lib_chip(None, None, cx)))
                        .child(
                            svg()
                                .path("icons/close16.svg")
                                .size(px(16.))
                                .text_color(c(TEXT_SUBDUED))
                                .group_hover("chip-clear", |s| s.text_color(c(WHITE))),
                        )
                        .into_any_element(),
                );
                chips.push(self.chip(format!("chip-{chip:?}"), chip.label(), true, cx, |this, cx| this.set_lib_chip(None, None, cx)));
                match self.lib_sub {
                    Some(sub) => chips.push(self.chip(format!("sub-{sub:?}"), sub.label(), true, cx, move |this, cx| {
                        this.set_lib_chip(Some(chip), None, cx)
                    })),
                    None => {
                        for &sub in chip.subs().iter().filter(|s| all.iter().any(|it| chip.has(it.kind) && s.has(it, me_name))) {
                            chips.push(self.chip(format!("sub-{sub:?}"), sub.label(), false, cx, move |this, cx| {
                                this.set_lib_chip(Some(chip), Some(sub), cx)
                            }));
                        }
                    }
                }
            }
        }

        let sh = &self.lib_chips_scroll;
        let max = f32::from(sh.max_offset().width);
        let x = -f32::from(sh.offset().x);
        let (more_left, more_right) = (x > 0.5, max - x > 0.5);
        let hovered = self.fades.on("lib-chips");
        let fade = |left: bool| {
            let (a, b) = (gpui::rgba(SURFACE << 8 | 0xFF), gpui::rgba(SURFACE << 8));
            div()
                .absolute()
                .top_0()
                .w(px(120.))
                .h(px(32.))
                .when(left, |d| d.left_0().bg(linear_gradient(90., linear_color_stop(a, 0.), linear_color_stop(b, 1.))))
                .when(!left, |d| d.right_0().bg(linear_gradient(90., linear_color_stop(b, 0.), linear_color_stop(a, 1.))))
        };
        let arrow = |left: bool, cx: &mut Context<Self>| {
            round_button(if left { "chips-left" } else { "chips-right" }, 32.)
                .absolute()
                .top_0()
                .when(left, |d| d.left_0())
                .when(!left, |d| d.right_0())
                .bg(c(ELEVATED))
                .hover(|s| s.bg(c(0x2A2A2A)))
                .on_click(cx.listener(move |this, _, _, cx| {
                    let sh = &this.lib_chips_scroll;
                    let view = f32::from(sh.bounds().size.width);
                    let max = f32::from(sh.max_offset().width);
                    let x = -f32::from(sh.offset().x);
                    let to = if left { (x - view * 0.8).max(0.) } else { (x + view * 0.8).min(max) };
                    sh.set_offset(gpui::point(px(-to), px(0.)));
                    cx.notify();
                }))
                .child(
                    svg()
                        .path(if left { "icons/chevron_left.svg" } else { "icons/chevron_right.svg" })
                        .size(px(16.))
                        .opacity(0.7)
                        .text_color(c(TEXT_SUBDUED)),
                )
        };
        div()
            .id("lib-chips")
            .mt(px(16.))
            .mx(px(16.))
            .h(px(32.))
            .flex_none()
            .relative()
            .on_hover(cx.listener(|this, h: &bool, _, cx| {
                this.fades.hover("lib-chips".into(), *h, 0., fx::Ease::Linear);
                cx.notify();
            }))
            .child(
                div()
                    .id("lib-chips-scroll")
                    .size_full()
                    .flex()
                    .flex_row()
                    .gap(px(8.))
                    .overflow_x_scroll()
                    .track_scroll(sh)
                    .on_scroll_wheel(cx.listener(|_, _, _, cx| cx.notify()))
                    .children(chips),
            )
            .when(more_left, |d| d.child(fade(true)))
            .when(more_right, |d| d.child(fade(false)))
            .when(hovered && more_left, |d| d.child(arrow(true, cx)))
            .when(hovered && more_right, |d| d.child(arrow(false, cx)))
            .into_any_element()
    }
}
