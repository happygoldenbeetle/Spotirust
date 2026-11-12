//! Miniplayer (player bar "Open Miniplayer") and full screen ("Enter Full screen").
//!
//! Miniplayer, measured from the reference's picture-in-picture window (330x342, black):
//!   top strip 30px: settings (12px, x6), drag handle (centre), close X (12px, right 9);
//!   card at (4,30) 321.6x248.4, radius 8, the cover's colour, cover centred (212px, radius 8);
//!   title 20/700 white at (8,288), artists 13px #b3b3b3 at y315, saved check 24px #1db954.
//!   Hover: card overlaid with linear-gradient(rgba(18,18,18,.53), #0a0a0a) and white controls
//!   (mute, shuffle, previous, 48px play, next, repeat, copy link) centred at y125; times 11px
//!   #b3b3b3 at y249 and a 4px progress bar at y270 (x10, width 309.6).

use gpui::{AppContext as _, WeakEntity};

use super::*;

pub struct MiniPlayer {
    shell: WeakEntity<Shell>,
    hovered: bool,
    /// Volume pop-up over the mute button: shown while hovered or dragged; its track's bounds.
    vol_hover: bool,
    vol_drag: bool,
    vol_track: std::rc::Rc<std::cell::Cell<Bounds<Pixels>>>,
    _observe: Option<gpui::Subscription>,
}

impl Shell {
    pub(super) fn open_miniplayer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(h) = self.miniplayer.take() {
            // Already open: the button closes it (its label is then "Close Miniplayer").
            // If it was closed some other way (Alt+F4), the click opens a new one instead.
            if h.update(cx, |_, window, _| window.remove_window()).is_ok() {
                cx.notify();
                return;
            }
        }
        let shell = cx.entity();
        // Opens at the bottom right of the main window, like the client's picture-in-picture,
        // kept on a display (GPUI turns a window placed off every display into a full-screen one).
        let main = window.bounds();
        let size = gpui::size(px(330.), px(342.));
        let want = gpui::point(main.origin.x + main.size.width - size.width - px(24.), main.origin.y + main.size.height - size.height - px(110.));
        let centre = main.center();
        let display = cx
            .displays()
            .into_iter()
            .map(|d| d.bounds())
            .find(|b| b.contains(&centre))
            .or_else(|| cx.primary_display().map(|d| d.bounds()));
        let origin = match display {
            Some(d) => gpui::point(
                want.x.clamp(d.origin.x + px(16.), (d.origin.x + d.size.width - size.width - px(16.)).max(d.origin.x)),
                want.y.clamp(d.origin.y + px(16.), (d.origin.y + d.size.height - size.height - px(64.)).max(d.origin.y)),
            ),
            None => want,
        };
        let bounds = Bounds::new(origin, size);
        // Opening a window draws it at once, and the miniplayer reads this view while drawing:
        // open it after this update has finished (reading it mid-update panics).
        cx.defer(move |cx| {
            let observed = shell.clone();
            let opened = cx.open_window(
                gpui::WindowOptions {
                    window_bounds: Some(gpui::WindowBounds::Windowed(bounds)),
                    titlebar: Some(gpui::TitlebarOptions { title: Some("Spotify MiniPlayer".into()), appears_transparent: true, traffic_light_position: None }),
                    focus: std::env::var_os("SPOTIRUST_NOFOCUS").is_none(),
                    window_min_size: Some(gpui::size(px(250.), px(250.))),
                    ..Default::default()
                },
                move |_, cx| {
                    cx.new(|cx| {
                        let observe = cx.observe(&observed, |_, _, cx| cx.notify());
                        MiniPlayer { shell: observed.downgrade(), hovered: false, vol_hover: false, vol_drag: false, vol_track: Default::default(), _observe: Some(observe) }
                    })
                },
            );
            match opened {
                Ok(h) => {
                    shell.update(cx, |this, cx| {
                        this.miniplayer = Some(h);
                        cx.notify();
                    });
                    // Always on top (the client's miniplayer floats over other windows).
                    keep_on_top(cx);
                }
                Err(e) => eprintln!("miniplayer: {e:#}"),
            }
        });
    }

    /// Full screen: the window goes full screen and shows the now-playing view over everything.
    pub(super) fn toggle_fullscreen(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.cinema = !self.cinema;
        if window.is_fullscreen() != self.cinema {
            window.toggle_fullscreen();
        }
        cx.notify();
    }

    /// The full-screen view (drawn over the whole window while `cinema` is on).
    pub(super) fn cinema_view(&self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.cinema {
            return None;
        }
        let vp = window.viewport_size();
        let (vw, vh) = (f32::from(vp.width), f32::from(vp.height));
        let track = self.playback.as_ref().and_then(|p| p.item.clone());
        let bg = self.lyrics.as_ref().and_then(|l| l.1.as_ref()).map(|l| l.background).unwrap_or(0x535353);
        let art = track
            .as_ref()
            .and_then(|t| t.album.as_ref())
            .and_then(|a| api::pick_image(&a.images, 640).map(|i| i.url.clone()));
        let art_size = (vh - 360.).min(vw * 0.45).max(120.);
        let ctx_name = self.context_name.as_ref().map(|c| c.1.clone()).unwrap_or_default();
        Some(
            div()
                .id("cinema")
                .absolute()
                .inset_0()
                .bg(linear_gradient(180., linear_color_stop(c(bg), 0.), linear_color_stop(c(0x000000), 1.)))
                .flex()
                .flex_col()
                .child(
                    div()
                        .h(px(64.))
                        .px(px(32.))
                        .flex()
                        .flex_row()
                        .items_center()
                        .child(div().flex_1().text_size(px(16.)).font_weight(FontWeight::BOLD).text_color(c(WHITE)).child(ctx_name))
                        .child(
                            round_button("cinema-exit", 32.)
                                .group("cinema-exit")
                                .on_click(cx.listener(|this, _, window, cx| this.toggle_fullscreen(window, cx)))
                                .child(
                                    svg()
                                        .path("icons/lib_collapse16.svg")
                                        .size(px(16.))
                                        .text_color(rgba(0xFFFFFFB3))
                                        .group_hover("cinema-exit", |s| s.text_color(c(WHITE))),
                                ),
                        ),
                )
                .child(
                    div()
                        .flex_1()
                        .min_h_0()
                        .px(px(64.))
                        .flex()
                        .flex_row()
                        .items_end()
                        .gap(px(32.))
                        .pb(px(32.))
                        .child(cover(art.as_deref(), art_size, 8., cx))
                        .when_some(track, |d, t| {
                            d.child(
                                div()
                                    .min_w_0()
                                    .flex()
                                    .flex_col()
                                    .child(
                                        one_line(t.name.clone())
                                            .font_family(title_font())
                                            .text_size(px(64.))
                                            .line_height(px(76.8))
                                            .font_weight(FontWeight::BOLD)
                                            .text_color(c(WHITE)),
                                    )
                                    .child(
                                        one_line(t.artists.iter().map(|a| a.name.as_str()).collect::<Vec<_>>().join(", "))
                                            .text_size(px(24.))
                                            .text_color(rgba(0xFFFFFFB3)),
                                    ),
                            )
                        }),
                )
                .child(self.player_bar(cx))
                .into_any_element(),
        )
    }
}

#[cfg(windows)]
fn keep_on_top(_: &mut gpui::App) {
    use windows::Win32::UI::WindowsAndMessaging::{FindWindowW, HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SetWindowPos};
    use windows::core::w;
    unsafe {
        if let Ok(hwnd) = FindWindowW(None, w!("Spotify MiniPlayer")) {
            let _ = SetWindowPos(hwnd, Some(HWND_TOPMOST), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
        }
    }
}

#[cfg(not(windows))]
fn keep_on_top(_: &mut gpui::App) {}

impl Shell {
    /// The playing track's 640px cover.
    pub(super) fn cover_url(&self) -> Option<String> {
        let t = self.playback.as_ref()?.item.as_ref()?;
        api::pick_image(&t.album.as_ref()?.images, 640).map(|i| i.url.clone())
    }

    /// Keeps `mini_color` on the playing cover while the miniplayer is open.
    pub(super) fn refresh_mini_color(&mut self, cx: &mut Context<Self>) {
        if self.miniplayer.is_none() {
            return;
        }
        let (Some(url), Some(sp)) = (self.cover_url(), self.spotify.clone()) else { return };
        if self.mini_color.as_ref().is_some_and(|(u, _)| *u == url) || self.mini_color_loading.as_deref() == Some(url.as_str()) {
            return;
        }
        self.mini_color_loading = Some(url.clone());
        cx.spawn(async move |this, cx| {
            let u = url.clone();
            let color = cx
                .background_executor()
                .spawn(async move { sp.extracted_colors(&[u]).ok().and_then(|v| crate::user::extracted_color(&v)) })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.mini_color_loading = None;
                if let Some(col) = color {
                    this.mini_color = Some((url, col));
                    cx.notify();
                }
            });
        })
        .detach();
    }
}

impl MiniPlayer {
    /// Volume from a pointer height over the pop-up's track (top = 100%).
    fn set_vol_at(&mut self, y: f32, cx: &mut Context<Self>) {
        let b = self.vol_track.get();
        let h = f32::from(b.size.height);
        if h <= 0. {
            return;
        }
        let f = (1. - (y - f32::from(b.origin.y)) / h).clamp(0., 1.);
        self.with_shell(cx, |s, cx| s.live_volume(f, cx));
        cx.notify();
    }

    fn with_shell<R>(&self, cx: &mut Context<Self>, f: impl FnOnce(&mut Shell, &mut Context<Shell>) -> R) -> Option<R> {
        self.shell.upgrade().map(|s| s.update(cx, f))
    }
}

impl Render for MiniPlayer {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(shell) = self.shell.upgrade() else {
            return div().size_full().bg(c(0x000000)).into_any_element();
        };
        let s = shell.read(cx);
        let pb = s.playback.clone();
        let track = pb.as_ref().and_then(|p| p.item.clone());
        let playing = pb.as_ref().is_some_and(|p| p.is_playing);
        let shuffle = pb.as_ref().is_some_and(|p| p.shuffle_state);
        let repeat = pb.as_ref().map(|p| p.repeat_state.clone()).unwrap_or_else(|| "off".into());
        let volume = pb.as_ref().and_then(|p| p.device.as_ref()).and_then(|d| d.volume_percent).unwrap_or(100);
        let saved = s.is_saved();
        let progress = s.progress_ms();
        let duration = track.as_ref().map(|t| t.duration_ms).unwrap_or(0);
        // The cover's extracted dark colour (the reference shows #8f3300 for Pasoori's cover).
        let card_bg = s.mini_color.as_ref().filter(|(u, _)| Some(u) == s.cover_url().as_ref()).map(|c| c.1).unwrap_or(0x535353);
        let art = track
            .as_ref()
            .and_then(|t| t.album.as_ref())
            .and_then(|a| api::pick_image(&a.images, 300).map(|i| i.url.clone()));
        if playing {
            window.request_animation_frame();
        }
        let vp = window.viewport_size();
        let (vw, vh) = (f32::from(vp.width), f32::from(vp.height));
        let card_w = vw - 8.4;
        let card_h = (vh - 94.).max(80.);
        let art_size = (card_h - 36.).min(card_w - 36.).max(40.);
        let hovered = self.hovered;
        let frac = if duration > 0 { (progress as f32 / duration as f32).clamp(0., 1.) } else { 0. };

        let ctl = |id: &'static str, path: &'static str, on: bool| {
            round_button(id, 32.).group(id).child(
                svg()
                    .path(path)
                    .size(px(16.))
                    .text_color(c(if on { GREEN } else { WHITE }))
                    .group_hover(id, |s| s.size(px(16.64))),
            )
        };
        let vol_icon = match volume {
            0 => "icons/volume_off.svg",
            v if v < 33 => "icons/volume_low.svg",
            v if v < 66 => "icons/volume_medium.svg",
            _ => "icons/volume_high.svg",
        };
        let controls = div()
            .absolute()
            .inset_0()
            .rounded(px(8.))
            .bg(linear_gradient(180., linear_color_stop(rgba(0x12121287), 0.), linear_color_stop(c(0x0A0A0A), 1.)))
            .flex()
            .flex_col()
            .child(
                div()
                    .flex_1()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_center()
                    .child({
                        // Mute, with the vertical volume slider that pops up above it on hover
                        // (20x80 #333 radius 4, a 4x64 track 8px in: rgba(255,255,255,.3) / white).
                        let track = self.vol_track.clone();
                        let shown = self.vol_hover || self.vol_drag;
                        let frac = volume as f32 / 100.;
                        div()
                            .id("mini-vol")
                            .relative()
                            .on_hover(cx.listener(|this, h: &bool, _, cx| {
                                this.vol_hover = *h;
                                cx.notify();
                            }))
                            .child(ctl("mini-mute", vol_icon, false).on_click(cx.listener(move |this, _, _, cx| {
                                this.with_shell(cx, |s, cx| s.toggle_mute(volume, cx));
                            })))
                            .when(shown, |d| {
                                d.child(
                                    div()
                                        .id("mini-vol-pop")
                                        .absolute()
                                        .left(px(6.))
                                        .top(px(-86.))
                                        .w(px(20.))
                                        .h(px(88.))
                                        .child(
                                            div()
                                                .w(px(20.))
                                                .h(px(80.))
                                                .rounded(px(4.))
                                                .bg(c(0x333333))
                                                .pt(px(8.))
                                                .flex()
                                                .justify_center()
                                                .on_mouse_down(MouseButton::Left, cx.listener(|this, ev: &MouseDownEvent, _, cx| {
                                                    this.vol_drag = true;
                                                    this.set_vol_at(f32::from(ev.position.y), cx);
                                                    cx.stop_propagation();
                                                }))
                                                .child(
                                                    div()
                                                        .relative()
                                                        .w(px(4.))
                                                        .h(px(64.))
                                                        .rounded(px(2.))
                                                        .bg(rgba(0xFFFFFF4D))
                                                        .child(canvas(move |b, _, _| track.set(b), |_, _, _, _| {}).absolute().inset_0())
                                                        .child(div().absolute().bottom_0().left_0().w(px(4.)).h(px(64. * frac)).rounded(px(2.)).bg(c(WHITE))),
                                                ),
                                        ),
                                )
                            })
                    })
                    .child(ctl("mini-shuffle", "icons/shuffle.svg", shuffle).on_click(cx.listener(move |this, _, _, cx| {
                        this.with_shell(cx, |s, cx| s.player(Play::Shuffle(!shuffle), cx));
                    })))
                    .child(ctl("mini-prev", "icons/prev.svg", false).on_click(cx.listener(|this, _, _, cx| {
                        this.with_shell(cx, |s, cx| s.player(Play::Prev, cx));
                    })))
                    .child(
                        div().w(px(56.)).flex().justify_center().child(
                            round_button("mini-play", 48.)
                                .bg(c(WHITE))
                                .hover(|s| s.bg(c(0xF0F0F0)))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.with_shell(cx, |s, cx| s.toggle_play(cx));
                                }))
                                .child(icon(if playing { "icons/pause24.svg" } else { "icons/play24.svg" }, 24., 0x000000)),
                        ),
                    )
                    .child(ctl("mini-next", "icons/next.svg", false).on_click(cx.listener(|this, _, _, cx| {
                        this.with_shell(cx, |s, cx| s.player(Play::Next, cx));
                    })))
                    .child(
                        ctl("mini-repeat", if repeat == "track" { "icons/repeat_one.svg" } else { "icons/repeat.svg" }, repeat != "off")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                let next = match repeat.as_str() {
                                    "context" => "track",
                                    "track" => "off",
                                    _ => "context",
                                };
                                this.with_shell(cx, |s, cx| s.player(Play::Repeat(next.into()), cx));
                            })),
                    )
                    .child(ctl("mini-share", "icons/ctx_share.svg", false).on_click(cx.listener(|this, _, _, cx| {
                        this.with_shell(cx, |s, cx| {
                            if let Some(id) = s.playing_track_id() {
                                cx.write_to_clipboard(gpui::ClipboardItem::new_string(format!("https://open.spotify.com/track/{id}")));
                                s.toast = Some(("Link copied to clipboard".into(), Instant::now()));
                                cx.notify();
                            }
                        });
                    }))),
            )
            .child(
                div()
                    .px(px(6.))
                    .flex()
                    .flex_row()
                    .justify_between()
                    .text_size(px(11.))
                    .text_color(c(TEXT_SUBDUED))
                    .child(div().pl(px(4.)).child(fmt_time(progress)))
                    .child(div().pr(px(4.)).child(fmt_time(duration))),
            )
            .child(
                div()
                    .mx(px(6.))
                    .mt(px(6.))
                    .mb(px(4.))
                    .h(px(4.))
                    .rounded(px(2.))
                    .bg(rgba(0xFFFFFF4D))
                    .child(div().h_full().w(relative(frac)).rounded(px(2.)).bg(c(WHITE))),
            );

        div()
            .size_full()
            .bg(c(0x000000))
            .font(ui_font_with_fallbacks())
            .on_mouse_move(cx.listener(|this, ev: &gpui::MouseMoveEvent, _, cx| {
                if this.vol_drag {
                    this.set_vol_at(f32::from(ev.position.y), cx);
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    if std::mem::take(&mut this.vol_drag) {
                        // Commit the final level (live updates are throttled for other devices).
                        this.with_shell(cx, |s, cx| {
                            if let Some(v) = s.playback.as_ref().and_then(|p| p.device.as_ref()).and_then(|d| d.volume_percent) {
                                s.player(Play::Volume(v), cx);
                            }
                        });
                        cx.notify();
                    }
                }),
            )
            .flex()
            .flex_col()
            .child(
                div()
                    .h(px(30.))
                    .flex_none()
                    .relative()
                    .flex()
                    .flex_row()
                    .items_center()
                    .child(div().absolute().inset_0().window_control_area(WindowControlArea::Drag))
                    .child(div().ml(px(6.)).child(icon("icons/mini_settings.svg", 12., TEXT_SUBDUED)))
                    .child(div().flex_1())
                    .child(div().absolute().left(px(vw / 2. - 12.)).top(px(1.)).child(icon("icons/mini_grip.svg", 24., TEXT_SUBDUED)))
                    .child(
                        div()
                            .id("mini-close")
                            .mr(px(9.))
                            .size(px(12.))
                            .cursor_pointer()
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.with_shell(cx, |s, cx| {
                                    s.miniplayer = None;
                                    cx.notify();
                                });
                                window.remove_window();
                            }))
                            .child(icon_rgba("icons/close16.svg", 12., 0xFFFFFFB3)),
                    ),
            )
            .child(
                div()
                    .id("mini-card")
                    .mx(px(4.))
                    .h(px(card_h))
                    .relative()
                    .rounded(px(8.))
                    .bg(c(card_bg))
                    .flex()
                    .items_center()
                    .justify_center()
                    .on_hover(cx.listener(|this, h: &bool, _, cx| {
                        this.hovered = *h;
                        cx.notify();
                    }))
                    .child(cover(art.as_deref(), art_size, 8., cx))
                    .when(hovered, |d| d.child(controls)),
            )
            .when_some(track, |d, t| {
                d.child(
                    div()
                        .mt(px(9.6))
                        .px(px(8.))
                        .flex()
                        .flex_row()
                        .items_center()
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .flex()
                                .flex_col()
                                .child(one_line(t.name.clone()).text_size(px(20.)).line_height(px(24.)).font_weight(FontWeight::BOLD).text_color(c(WHITE)))
                                .child(
                                    one_line(t.artists.iter().map(|a| a.name.as_str()).collect::<Vec<_>>().join(", "))
                                        .mt(px(3.2))
                                        .text_size(px(13.))
                                        .line_height(px(17.6))
                                        .text_color(c(TEXT_SUBDUED)),
                                ),
                        )
                        .child(
                            round_button("mini-add", 24.)
                                .mr(px(12.))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.with_shell(cx, |s, cx| s.toggle_saved(cx));
                                }))
                                .child(icon(if saved { "icons/check_circle24.svg" } else { "icons/add_circle24.svg" }, 24., if saved { 0x1DB954 } else { TEXT_SUBDUED })),
                        ),
                )
            })
            .into_any_element()
    }
}
