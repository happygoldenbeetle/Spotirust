//! Player bar, measured and read from the reference client (`dwp-now-playing-bar` and the
//! player-controls chunk). Icon buttons behave like Encore tertiary buttons:
//!   rest #b3b3b3 (right-hand icons rgba(255,255,255,.7)) -> hover white and scale 1.04,
//!   `transition: color .15s, transform .15s` cubic-bezier(.3,0,0,1), .05s while hovered (measured
//!   on every control); the scale is a transform, so the button's box and hit area never change.
//!   Pressed: opacity .7, scale 1.
//!   On (shuffle / repeat): #1ed760 with a 4px dot underneath. Toggled panels (queue / connect /
//!   lyrics): #1db954 -> #1ed760 on hover, with the dot.
//!   Play: white circle, hover #f0f0f0 + scale 1.04, pressed #c7c7c7.
//! Tooltips (all controls but the cover): #282828, white 14px, padding 4/8, radius 4, centred
//! 8px above the button, shown after 200 ms, no fade, gone at once.
//! Labels say what a click does next (e.g. repeat off -> "Enable repeat").

use std::cell::RefCell;

use gpui::{ClickEvent, ScrollDelta, ScrollWheelEvent};

use super::*;

const TIP_DELAY: Duration = Duration::from_millis(200);

/// Hover / press state of the bar's buttons (drives the Encore hover animation and tooltips).
#[derive(Default)]
pub struct BarFx {
    pub hover: Option<(&'static str, Instant)>,
    pub press: Option<&'static str>,
}

/// Bounds + tooltip text of each control, recorded while painting (anchors the tooltip).
pub type TipAnchors = Rc<RefCell<HashMap<&'static str, (Bounds<Pixels>, SharedString)>>>;

use super::fx::{TERTIARY, mix, scaled_icon};

#[derive(Clone, Copy)]
pub enum Look {
    /// Centre controls: #b3b3b3 -> white.
    Subdued,
    /// Right-hand icons: rgba(255,255,255,.7) -> white.
    Dim,
    /// Shuffle / repeat on: #1ed760, dot.
    On,
    /// Its panel / view is open: #1db954 -> #1ed760, dot.
    Toggled,
}

impl Look {
    fn colors(self) -> (u32, u32) {
        match self {
            Look::Subdued => (0xB3B3B3FF, 0xFFFFFFFF),
            Look::Dim => (0xFFFFFFB3, 0xFFFFFFFF),
            Look::On => (0x1ED760FF, 0x1ED760FF),
            Look::Toggled => (0x1DB954FF, 0x1ED760FF),
        }
    }
}

impl Shell {
    /// Hover progress (0..1) of a bar control, eased.
    pub(super) fn fx_t(&self, id: &str) -> f32 {
        self.fades.get(id)
    }

    /// Whether a hover transition is still running (the root then asks for another frame).
    pub(super) fn fx_animating(&self) -> bool {
        self.fades.animating()
    }

    pub(super) fn bar_hovered(&mut self, id: &'static str, on: bool, cx: &mut Context<Self>) {
        let now = Instant::now();
        self.fades.hover(id.into(), on, if on { TERTIARY.enter } else { TERTIARY.leave }, TERTIARY.ease);
        if on {
            if self.bar_fx.hover.map(|h| h.0) != Some(id) {
                if let Some((prev, _)) = self.bar_fx.hover {
                    self.fades.hover(prev.into(), false, TERTIARY.leave, TERTIARY.ease);
                }
                self.bar_fx.hover = Some((id, now));
                cx.spawn(async move |this, cx| {
                    cx.background_executor().timer(TIP_DELAY).await;
                    let _ = this.update(cx, |_, cx| cx.notify());
                })
                .detach();
            }
        } else if self.bar_fx.hover.map(|h| h.0) == Some(id) {
            self.bar_fx.hover = None;
        }
        cx.notify();
    }

    /// The tooltip to show: the hovered control's, once hovered for 200 ms.
    pub(super) fn bar_tip(&self) -> Option<(Bounds<Pixels>, SharedString)> {
        let (id, at) = self.bar_fx.hover?;
        if at.elapsed() < TIP_DELAY {
            return None;
        }
        self.bar_tips.borrow().get(id).cloned().filter(|(_, t)| !t.is_empty())
    }

    /// Hover / press / tooltip wiring shared by every bar control.
    fn bar_wire(&self, el: Stateful<Div>, id: &'static str, tip: SharedString, cx: &mut Context<Self>) -> Stateful<Div> {
        let tips = self.bar_tips.clone();
        el.relative()
            .child(
                canvas(
                    move |b, _, _| {
                        tips.borrow_mut().insert(id, (b, tip));
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .inset_0(),
            )
            .on_hover(cx.listener(move |this, h: &bool, _, cx| this.bar_hovered(id, *h, cx)))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, _, cx| {
                    this.bar_fx.press = Some(id);
                    cx.notify();
                }),
            )
    }

    /// A 32px Encore tertiary icon button.
    fn bar_icon(&self, id: &'static str, path: &'static str, tip: impl Into<SharedString>, look: Look, cx: &mut Context<Self>) -> Stateful<Div> {
        let t = self.fx_t(id);
        let pressed = self.bar_fx.press == Some(id);
        let scale = if pressed { 1. } else { 1. + 0.04 * t };
        let (rest, hover) = look.colors();
        let color = mix(rest, hover, t);
        let dot = matches!(look, Look::On | Look::Toggled);
        self.bar_wire(round_button(id, 32.), id, tip.into(), cx)
            .opacity(if pressed { 0.7 } else { 1. })
            .child(scaled_icon(path, 16., scale).text_color(color))
            .when(dot, |d| {
                d.child(div().absolute().bottom_0().left(px(14.)).size(px(4.)).rounded_full().bg(color))
            })
    }

    /// A link in the now-playing widget (title: underline; artists: subdued -> white + underline).
    fn bar_link(&self, id: SharedString, text: String, subdued: bool, page: Option<Page>, cx: &mut Context<Self>) -> Stateful<Div> {
        div()
            .id(gpui::ElementId::Name(id))
            .flex_none()
            .cursor_pointer()
            .text_color(c(if subdued { TEXT_SUBDUED } else { WHITE }))
            .hover(|s| s.text_color(c(WHITE)).underline())
            .child(text)
            .on_click(cx.listener(move |this, _, _, cx| {
                if let Some(p) = page.clone() {
                    this.open(p, cx);
                }
            }))
    }

    pub(super) fn player_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let pb = self.playback.clone();
        let track = pb.as_ref().and_then(|p| p.item.clone());
        let playing = pb.as_ref().is_some_and(|p| p.is_playing);
        let shuffle = pb.as_ref().is_some_and(|p| p.shuffle_state);
        let repeat = pb.as_ref().map(|p| p.repeat_state.clone()).unwrap_or_else(|| "off".into());
        let duration = track.as_ref().map(|t| t.duration_ms).unwrap_or(0);
        // While a bar is dragged, it (and the elapsed time) follows the pointer.
        let (progress, frac) = match self.slider_drag {
            Some((Slider::Seek, f)) => ((f * duration as f32) as u64, f),
            _ => {
                let p = self.progress_ms();
                (p, if duration > 0 { p as f32 / duration as f32 } else { 0. })
            }
        };
        let volume = match self.slider_drag {
            Some((Slider::Volume, f)) => (f * 100.).round() as u32,
            _ => pb.as_ref().and_then(|p| p.device.as_ref()).and_then(|d| d.volume_percent).unwrap_or(100),
        };
        // "Playing on X" only for another device (not this app's own player).
        let remote_device = pb.as_ref().and_then(|p| p.device.as_ref()).filter(|d| {
            let ours = self.local.as_ref().map(|l| l.device_id.as_str());
            d.id.as_deref() != ours || ours.is_none()
        });
        let remote_device = remote_device.filter(|_| self.local.is_none() || !self.local_active()).map(|d| d.name.clone());

        let art_url = track
            .as_ref()
            .and_then(|t| t.album.as_ref())
            .and_then(|a| api::pick_image(&a.images, 64).map(|i| images::variant(&i.url, images::Variant::Small)));

        // left: now playing widget
        let saved = self.is_saved();
        let left = div().flex_1().min_w_0().h(px(56.)).flex().flex_row().items_center().when_some(track.clone(), |d, t| {
            let album_page = t.album.as_ref().filter(|a| !a.uri.is_empty()).and_then(|a| Page::for_uri(&a.uri));
            let mut artists = div().flex().flex_row().overflow_hidden().whitespace_nowrap().text_size(px(12.)).line_height(px(16.8));
            for (i, a) in t.artists.iter().enumerate() {
                if i > 0 {
                    artists = artists.child(div().flex_none().text_color(c(TEXT_SUBDUED)).child(", "));
                }
                let page = Page::for_uri(&a.uri);
                artists = artists.child(self.bar_link(format!("np-artist-{i}").into(), a.name.clone(), true, page, cx));
            }
            d.child(
                div()
                    .id("cover-art-button")
                    .flex_none()
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_right(RightPanel::NowPlaying, cx)))
                    .child(cover(art_url.as_deref(), 56., 4., cx)),
            )
            .child(
                div()
                    .ml(px(16.))
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(
                        div().flex().overflow_hidden().whitespace_nowrap().text_size(px(14.)).line_height(px(22.)).child(self.bar_link(
                            "np-title".into(),
                            t.name.clone(),
                            false,
                            album_page,
                            cx,
                        )),
                    )
                    .child(artists),
            )
            .child({
                let id = "add-liked";
                let t_ = self.fx_t(id);
                let pressed = self.bar_fx.press == Some(id);
                let scale = if pressed { 1. } else { 1. + 0.04 * t_ };
                let (path, rest, hover) = if saved {
                    ("icons/check_circle.svg", 0x1ED760FF, 0x3BE477FF)
                } else {
                    ("icons/add_circle.svg", 0xB3B3B3FF, 0xFFFFFFFF)
                };
                let tip = if saved { "Add to playlist" } else { "Add to Liked Songs" };
                self.bar_wire(round_button(id, 32.).ml(px(8.)), id, tip.into(), cx)
                    .opacity(if pressed { 0.7 } else { 1. })
                    .on_click(cx.listener(move |this, ev: &ClickEvent, _, cx| {
                        let Some(id) = this.playing_track_id() else { return };
                        let uri = format!("spotify:track:{id}");
                        if this.is_saved() {
                            this.open_add_popover(uri, Self::anchor_of(ev, 32.), cx);
                        } else {
                            this.like_track(uri, cx);
                        }
                    }))
                    .child(match self.playing_track_id().and_then(|id| self.like_burst(&format!("spotify:track:{id}"), 16.)) {
                        Some(b) => b,
                        None => scaled_icon(path, 16., scale).text_color(mix(rest, hover, t_)).into_any_element(),
                    })
            })
        });

        // centre: controls
        let ctx_name = self.context_name.as_ref().map(|c| c.1.clone()).filter(|n| !n.is_empty());
        let shuffle_tip = match (shuffle, &ctx_name) {
            (false, Some(n)) => format!("Enable Shuffle for {n}"),
            (false, None) => "Enable Shuffle".to_string(),
            (true, Some(n)) => format!("Disable Shuffle for {n}"),
            (true, None) => "Disable Shuffle".to_string(),
        };
        let (next_repeat, repeat_tip) = match repeat.as_str() {
            "context" => ("track", "Enable repeat one"),
            "track" => ("off", "Disable repeat"),
            _ => ("context", "Enable repeat"),
        };
        let play_btn = {
            let id = "playpause";
            let t = self.fx_t(id);
            let pressed = self.bar_fx.press == Some(id);
            let scale = if pressed { 1. } else { 1. + 0.04 * t };
            let bg = if pressed { c(0xC7C7C7) } else { mix(0xFFFFFFFF, 0xF0F0F0FF, t) };
            // The white circle grows (transform) behind a fixed 32px hit area.
            let d = 32. * scale;
            self.bar_wire(round_button(id, 32.), id, if playing { "Pause" } else { "Play" }.into(), cx)
                .on_click(cx.listener(|this, _, _, cx| this.toggle_play(cx)))
                .child(div().absolute().left(px((32. - d) / 2.)).top(px((32. - d) / 2.)).size(px(d)).rounded_full().bg(bg))
                .child(scaled_icon(if playing { "icons/pause.svg" } else { "icons/play.svg" }, 16., scale).text_color(c(0x000000)))
        };
        let center = div()
            .w(relative(0.4))
            .flex_none()
            .flex()
            .flex_col()
            .items_center()
            .child(
                div()
                    .h(px(32.))
                    .flex()
                    .flex_row()
                    .gap(px(16.))
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .gap(px(8.))
                            .child(
                                self.bar_icon("shuffle", "icons/shuffle.svg", shuffle_tip, if shuffle { Look::On } else { Look::Subdued }, cx)
                                    .on_click(cx.listener(move |this, _, _, cx| this.player(Play::Shuffle(!shuffle), cx))),
                            )
                            .child(
                                self.bar_icon("prev", "icons/prev.svg", "Previous", Look::Subdued, cx)
                                    .on_click(cx.listener(|this, _, _, cx| this.player(Play::Prev, cx))),
                            ),
                    )
                    .child(play_btn)
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .gap(px(8.))
                            .child(
                                self.bar_icon("next", "icons/next.svg", "Next", Look::Subdued, cx)
                                    .on_click(cx.listener(|this, _, _, cx| this.player(Play::Next, cx))),
                            )
                            .child(
                                self.bar_icon(
                                    "repeat",
                                    if repeat == "track" { "icons/repeat_one.svg" } else { "icons/repeat.svg" },
                                    repeat_tip,
                                    if repeat != "off" && !repeat.is_empty() { Look::On } else { Look::Subdued },
                                    cx,
                                )
                                .on_click(cx.listener(move |this, _, _, cx| this.player(Play::Repeat(next_repeat.to_string()), cx))),
                            ),
                    ),
            )
            .child(
                div()
                    .mt(px(8.))
                    .w_full()
                    .h(px(16.8))
                    .flex()
                    .flex_row()
                    .items_center()
                    .text_size(px(12.))
                    .text_color(c(TEXT_SUBDUED))
                    .child(div().w(px(50.)).flex().justify_end().child(fmt_time(progress)))
                    .child(
                        slider("seek", frac, self.slider_drag.is_some_and(|d| d.0 == Slider::Seek), self.seek_bounds.clone(), {
                            let this = cx.entity().downgrade();
                            move |f, _, cx| {
                                let _ = this.update(cx, |this, cx| {
                                    this.slider_drag = Some((Slider::Seek, f));
                                    cx.notify();
                                });
                            }
                        })
                        .flex_1()
                        .mx(px(8.)),
                    )
                    .child(div().w(px(50.)).child(fmt_time(duration))),
            );

        // right: panels, volume, miniplayer, full screen
        let panel = self.right_panel;
        let toggled = |p: RightPanel| if panel == p { Look::Toggled } else { Look::Dim };
        let muted = volume == 0;
        let vol_icon = match volume {
            0 => "icons/volume_off.svg",
            v if v < 33 => "icons/volume_low.svg",
            v if v < 66 => "icons/volume_medium.svg",
            _ => "icons/volume_high.svg",
        };
        // Hovering the mute button lights the slider up as if hovered (forceActiveStyles).
        let vol_active = self.slider_drag.is_some_and(|d| d.0 == Slider::Volume) || self.bar_fx.hover.is_some_and(|h| h.0 == "mute");
        let right = div()
            .flex_1()
            .min_w_0()
            .h(px(32.))
            .mt(px(12.))
            .flex()
            .flex_row()
            .justify_end()
            .items_center()
            .child(
                self.bar_icon("lyrics", "icons/mic.svg", "Lyrics", if self.page == Page::Lyrics { Look::Toggled } else { Look::Dim }, cx)
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_lyrics(cx))),
            )
            .child(
                self.bar_icon("queue", "icons/queue.svg", "Queue", toggled(RightPanel::Queue), cx)
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_right(RightPanel::Queue, cx))),
            )
            .child(
                self.bar_icon("connect", "icons/connect16.svg", "Connect to a device", toggled(RightPanel::Devices), cx)
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_right(RightPanel::Devices, cx))),
            )
            .child(
                div()
                    .id("volume-bar")
                    .w(px(125.))
                    .flex()
                    .flex_row()
                    .items_center()
                    .on_scroll_wheel(cx.listener(|this, ev: &ScrollWheelEvent, _, cx| this.wheel_volume(ev, cx)))
                    .child(
                        self.bar_icon("mute", vol_icon, if muted { "Unmute" } else { "Mute" }, Look::Subdued, cx)
                            .on_click(cx.listener(move |this, _, _, cx| this.toggle_mute(volume, cx))),
                    )
                    .child(slider("volume", volume as f32 / 100., vol_active, self.volume_bounds.clone(), {
                        let this = cx.entity().downgrade();
                        move |f, _, cx| {
                            let _ = this.update(cx, |this, cx| {
                                this.slider_drag = Some((Slider::Volume, f));
                                this.live_volume(f, cx);
                                cx.notify();
                            });
                        }
                    })
                    .w(px(93.))),
            )
            .child(
                self.bar_icon("pip", "icons/pip.svg", if self.miniplayer.is_some() { "Close Miniplayer" } else { "Open Miniplayer" }, Look::Dim, cx)
                    .ml(px(8.))
                    .on_click(cx.listener(|this, _, window, cx| this.open_miniplayer(window, cx))),
            )
            .child(
                self.bar_icon("fullscreen", "icons/fullscreen.svg", if self.cinema { "Exit full screen" } else { "Enter Full screen" }, Look::Dim, cx)
                    .on_click(cx.listener(|this, _, window, cx| this.toggle_fullscreen(window, cx))),
            );

        div()
            .flex_none()
            .mx(px(8.))
            .mb(px(8.))
            .flex()
            .flex_col()
            .child(
                div()
                    .h(px(72.))
                    .pt(px(7.6))
                    .px(px(8.))
                    .flex()
                    .flex_row()
                    .items_start()
                    .child(left)
                    .child(center)
                    .child(right),
            )
            .when_some(remote_device, |d, name| {
                d.child(
                    div()
                        .id("playing-on")
                        .h(px(24.))
                        .rounded(px(4.))
                        .bg(c(GREEN))
                        .cursor_pointer()
                        .on_click(cx.listener(|this, _, _, cx| this.toggle_right(RightPanel::Devices, cx)))
                        .flex()
                        .flex_row()
                        .justify_end()
                        .items_center()
                        .gap(px(8.))
                        .pr(px(8.))
                        .text_size(px(14.))
                        .font_weight(FontWeight::BOLD)
                        .text_color(c(0x000000))
                        .child(icon("icons/speaker.svg", 16., 0x000000))
                        .child(format!("Playing on {name}")),
                )
            })
    }

    /// The tooltip layer (drawn last, over everything).
    pub(super) fn bar_tooltip(&self, vw: f32) -> Option<impl IntoElement> {
        let (b, text) = self.bar_tip()?;
        let center = f32::from(b.origin.x) + f32::from(b.size.width) / 2.;
        let top = f32::from(b.origin.y);
        // Centred on the button; near a window edge it's pushed back in (5px margin, like tippy).
        let half = (center - 5.).min(vw - 5. - center).min(300.);
        let row = div().absolute().top_0().h(px(top - 8.)).flex().flex_col().justify_end();
        let row = if half >= 90. {
            row.left(px(center - half)).w(px(half * 2.)).items_center()
        } else if center < vw / 2. {
            row.left(px(5.)).w(px(600.)).items_start()
        } else {
            row.right(px(5.)).w(px(600.)).items_end()
        };
        Some(
            row.child(
                div()
                    .px(px(8.))
                    .py(px(4.))
                    .rounded(px(4.))
                    .bg(c(0x282828))
                    .shadow(vec![
                        gpui::BoxShadow {
                            color: gpui::hsla(0., 0., 0., 0.3),
                            offset: gpui::point(px(0.), px(16.)),
                            blur_radius: px(24.),
                            spread_radius: px(0.),
                        },
                        gpui::BoxShadow {
                            color: gpui::hsla(0., 0., 0., 0.2),
                            offset: gpui::point(px(0.), px(6.)),
                            blur_radius: px(8.),
                            spread_radius: px(0.),
                        },
                    ])
                    .text_size(px(14.))
                    .line_height(px(19.2))
                    .text_color(c(WHITE))
                    .whitespace_nowrap()
                    .child(text),
            ),
        )
    }

    /// Mute keeps the level and restores it (the client's mute helper).
    pub(super) fn toggle_mute(&mut self, volume: u32, cx: &mut Context<Self>) {
        if volume == 0 {
            let v = self.muted_volume.take().filter(|v| *v > 0).unwrap_or(50);
            self.player(Play::Volume(v), cx);
        } else {
            self.muted_volume = Some(volume);
            self.player(Play::Volume(0), cx);
        }
    }

    /// Wheel over the volume control: each notch is 10% (client: wheelDelta / 1200).
    fn wheel_volume(&mut self, ev: &ScrollWheelEvent, cx: &mut Context<Self>) {
        let notches = match ev.delta {
            // Windows reports notches x the system's lines-per-notch (3 by default).
            ScrollDelta::Lines(p) => p.y / 3.,
            ScrollDelta::Pixels(p) => f32::from(p.y) / 120.,
        };
        if notches == 0. {
            return;
        }
        let cur = self.playback.as_ref().and_then(|p| p.device.as_ref()).and_then(|d| d.volume_percent).unwrap_or(100) as f32;
        let v = (cur + notches.clamp(-1., 1.) * 10.).clamp(0., 100.).round();
        self.live_volume(v / 100., cx);
        cx.stop_propagation();
    }

    /// Right sidebar: a button opens its panel, or closes the sidebar when it's already showing.
    pub(super) fn toggle_right(&mut self, panel: RightPanel, cx: &mut Context<Self>) {
        self.right_panel = if self.right_panel == panel { RightPanel::None } else { panel };
        match self.right_panel {
            RightPanel::Queue | RightPanel::NowPlaying => self.load_queue(cx),
            RightPanel::Devices => self.load_devices(cx),
            _ => {}
        }
        cx.notify();
    }
}
