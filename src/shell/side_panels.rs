//! Right sidebar panels opened from the player bar, measured from the reference (420px wide):
//!
//! Queue: tabs "Queue" / "Recently played" at (8,8), 48 tall, padding 6/12, 14/700; selected white
//!   with a 2px #1ed760 bar 6px above the bottom, as wide as the text; other tab #b3b3b3. Close X
//!   (32px) 16 from the top / right. "Now playing" / "Next from: <context>" 16/700 at x16. Rows
//!   64 tall at x8 (width - 16): 48px cover (radius 4) at x16, title 16px at x76 (green when it's
//!   the playing track), artists 14px #b3b3b3. Row hover: #1f1f1f (radius 4), cover dimmed
//!   rgba(0,0,0,.6) under a 24px play icon, artists white, "..." at the right.
//! Connect: "Connect" 16/700 at (16,20.8); current device card at (8,64) 120 tall #1f1f1f radius 8
//!   (24px icon + name in #1ed760, "Listen with friends anywhere" + "Start a Jam" pill); other
//!   devices 56px rows (24px icon at x20, name 16px at x56), hover #1f1f1f; two help links at the
//!   bottom (16px, external icon #b3b3b3 at the right).

use super::*;
use crate::api::{Device, Track};

const ROW_HOVER: u32 = 0x1F1F1F;

/// Queue panel data: (now playing, up next) and the recently played list.
#[derive(Default, Clone)]
pub struct QueueData {
    pub now: Option<Track>,
    pub next: Vec<Track>,
    pub recent: Vec<Track>,
    /// Built from the playing context (the built-in player's queue): re-derived while shown.
    pub local: bool,
}

impl Shell {
    pub(super) fn load_queue(&mut self, cx: &mut Context<Self>) {
        let Some(api) = self.api.clone() else { return };
        cx.spawn(async move |this, cx| {
            let res = cx
                .background_executor()
                .spawn(async move {
                    let (now, next) = api.queue()?;
                    let recent = api.recently_played().unwrap_or_default();
                    anyhow::Ok(QueueData { now, next, recent, local: false })
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                match res {
                    Ok(mut q) => {
                        // The Web API only knows the queue of another active device: for the
                        // built-in player (or none), list the context from the playing track on.
                        if q.now.is_none() || this.local_active() {
                            q.local = true;
                            this.ensure_context_page(cx);
                        }
                        this.queue = Some(q);
                    }
                    Err(e) => {
                        eprintln!("queue: {e:#}");
                        this.queue = Some(QueueData { local: true, ..Default::default() });
                        this.ensure_context_page(cx);
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Loads the playing context's tracks (the built-in player's queue is listed from them).
    fn ensure_context_page(&mut self, cx: &mut Context<Self>) {
        let Some(ctx) = self.playback.as_ref().and_then(|p| p.context.as_ref()).map(|c| c.uri.clone()) else { return };
        let key = match Page::for_uri(&ctx) {
            Some(Page::Liked) => LIKED_URI.to_string(),
            Some(Page::Playlist(_)) | Some(Page::Album(_)) => ctx,
            _ => return,
        };
        if !self.playlist_pages.contains_key(&key) && !self.loading_pages.contains(&key) {
            self.load_page(key, cx);
        }
    }

    /// Now playing + the rest of its context (from the cached page), as the queue shows them.
    fn context_queue(&self) -> (Option<Track>, Vec<Track>) {
        let pb = self.playback.as_ref();
        let now = pb.and_then(|p| p.item.clone());
        let Some(ctx) = pb.and_then(|p| p.context.as_ref()).map(|c| c.uri.clone()) else { return (now, Vec::new()) };
        let key = match Page::for_uri(&ctx) {
            Some(Page::Liked) => LIKED_URI.to_string(),
            _ => ctx,
        };
        let Some(page) = self.playlist_pages.get(&key) else { return (now, Vec::new()) };
        let cur = now.as_ref().map(|t| t.uri.clone()).unwrap_or_default();
        let start = page.tracks.iter().position(|t| t.uri == cur).map(|i| i + 1).unwrap_or(0);
        let next = page
            .tracks
            .iter()
            .skip(start)
            .filter(|t| t.playable)
            .take(80)
            .map(|t| Track {
                id: t.uri.rsplit(':').next().map(str::to_owned),
                uri: t.uri.clone(),
                name: t.name.clone(),
                duration_ms: t.duration_ms,
                artists: t.artists.iter().map(|(name, uri)| api::Artist { name: name.clone(), uri: uri.clone() }).collect(),
                album: Some(api::Album {
                    name: t.album.clone(),
                    uri: t.album_uri.clone(),
                    images: t.image.iter().map(|u| api::Image { url: u.clone(), width: Some(64), height: Some(64) }).collect(),
                }),
            })
            .collect();
        (now, next)
    }

    pub(super) fn load_devices(&mut self, cx: &mut Context<Self>) {
        let Some(api) = self.api.clone() else { return };
        cx.spawn(async move |this, cx| {
            let res = cx.background_executor().spawn(async move { api.devices() }).await;
            let _ = this.update(cx, |this, cx| {
                match res {
                    Ok(d) => this.devices = Some(d),
                    Err(e) => eprintln!("devices: {e:#}"),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Plays on `device`: the built-in player takes over directly, others via the Web API.
    fn transfer_to(&mut self, device: Device, cx: &mut Context<Self>) {
        let ours = self.local.as_ref().is_some_and(|l| device.id.as_deref() == Some(l.device_id.as_str()));
        if ours {
            if let Some(local) = self.local.clone() {
                if let Err(e) = local.take_over(true) {
                    eprintln!("take over: {e:#}");
                }
            }
        } else if let Some(id) = device.id.clone() {
            let playing = self.playback.as_ref().is_some_and(|p| p.is_playing);
            self.command(move |api| api.transfer(&id, playing), cx);
        }
        // The panel follows once the server has moved playback.
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_millis(1200)).await;
            let _ = this.update(cx, |this, cx| this.load_devices(cx));
        })
        .detach();
    }

    fn panel_close(&self, id: &'static str, cx: &mut Context<Self>) -> Stateful<Div> {
        round_button(id, 32.)
            .group(id)
            .on_click(cx.listener(|this, _, _, cx| {
                this.right_panel = RightPanel::None;
                cx.notify();
            }))
            .child(svg().path("icons/close16.svg").size(px(16.)).text_color(c(TEXT_SUBDUED)).group_hover(id, |s| s.text_color(c(WHITE))))
    }

    fn panel_shell(&self) -> Div {
        div().w(px(self.right_w)).flex_none().h_full().rounded(px(8.)).bg(c(SURFACE)).flex().flex_col().overflow_hidden()
    }

    /// One queue / recently-played row.
    fn queue_row(&self, key: String, t: &Track, playing: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let group: SharedString = format!("qrow-{key}").into();
        let art = t
            .album
            .as_ref()
            .and_then(|a| api::pick_image(&a.images, 48).map(|i| images::variant(&i.url, images::Variant::Small)));
        let uri = t.uri.clone();
        let ctx_uri = self.playback.as_ref().and_then(|p| p.context.as_ref()).map(|c| c.uri.clone());
        let target = ctx_menu::Target::Track {
            uri: t.uri.clone(),
            name: t.name.clone(),
            album_uri: t.album.as_ref().map(|a| a.uri.clone()).unwrap_or_default(),
            artists: t.artists.iter().map(|a| (a.uri.clone(), a.name.clone())).collect(),
            editable_playlist: None,
            on_album: false,
        };
        let mut artists = div().flex().flex_row().overflow_hidden().whitespace_nowrap().text_size(px(14.)).line_height(px(19.2));
        for (i, a) in t.artists.iter().enumerate() {
            if i > 0 {
                artists = artists.child(div().flex_none().child(", "));
            }
            let page = Page::for_uri(&a.uri);
            artists = artists.child(
                div()
                    .id(gpui::ElementId::Name(format!("{group}-a{i}").into()))
                    .flex_none()
                    .cursor_pointer()
                    .hover(|s| s.underline())
                    .child(a.name.clone())
                    .on_click(cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        if let Some(p) = page.clone() {
                            this.open(p, cx);
                        }
                    })),
            );
        }
        div()
            .id(gpui::ElementId::Name(group.clone()))
            .group(group.clone())
            .mx(px(8.))
            .h(px(64.))
            .flex_none()
            .relative()
            .rounded(px(4.))
            .hover(|s| s.bg(c(ROW_HOVER)))
            .cursor_pointer()
            .flex()
            .flex_row()
            .items_center()
            .on_click(cx.listener(move |this, _, _, cx| {
                // Clicking a row plays it (from its place in the context when there is one).
                match ctx_uri.clone() {
                    Some(c) => this.player(Play::ContextAt(c, uri.clone()), cx),
                    None => this.player(Play::Context(uri.clone()), cx),
                }
            }))
            .child(
                div()
                    .ml(px(8.))
                    .size(px(48.))
                    .flex_none()
                    .relative()
                    .rounded(px(4.))
                    .bg(c(PLACEHOLDER))
                    .child(cover(art.as_deref(), 48., 4., cx))
                    .child(
                        div()
                            .absolute()
                            .inset_0()
                            .rounded(px(4.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .bg(rgba(0x00000099))
                            .opacity(0.)
                            .group_hover(group.clone(), |s| s.opacity(1.))
                            .child(icon("icons/play24.svg", 24., WHITE)),
                    ),
            )
            .child(
                div()
                    .ml(px(12.))
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .text_color(c(TEXT_SUBDUED))
                    .group_hover(group.clone(), |s| s.text_color(c(WHITE)))
                    .child(
                        one_line(t.name.clone())
                            .text_size(px(16.))
                            .line_height(px(22.4))
                            .text_color(c(if playing { GREEN } else { WHITE })),
                    )
                    .child(artists),
            )
            .child(
                div()
                    .id(gpui::ElementId::Name(format!("{group}-more").into()))
                    .w(px(16.))
                    .h(px(32.))
                    .mx(px(8.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .opacity(0.)
                    .group_hover(group.clone(), |s| s.opacity(1.))
                    .child(icon("icons/more.svg", 16., TEXT_SUBDUED))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, ev: &MouseDownEvent, _, cx| {
                            cx.stop_propagation();
                            this.open_ctx_menu(target.clone(), ev.position, false, cx);
                        }),
                    ),
            )
    }

    fn section_title(&self, text: impl IntoElement) -> Div {
        div()
            .px(px(16.))
            .text_size(px(16.))
            .line_height(px(22.4))
            .font_weight(FontWeight::BOLD)
            .text_color(c(WHITE))
            .flex()
            .flex_row()
            .child(text)
    }

    pub(super) fn queue_panel(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let tab = |id: &'static str, label: &'static str, idx: u8, cx: &mut Context<Self>| {
            let selected = self.queue_tab == idx;
            div()
                .id(id)
                .h(px(48.))
                .px(px(12.))
                .flex()
                .flex_col()
                .justify_center()
                .cursor_pointer()
                .text_size(px(14.))
                .line_height(px(14.))
                .font_weight(FontWeight::BOLD)
                .text_color(c(if selected { WHITE } else { TEXT_SUBDUED }))
                .hover(|s| s.text_color(c(WHITE)))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.queue_tab = idx;
                    cx.notify();
                }))
                .child(
                    div()
                        .relative()
                        .child(label)
                        .when(selected, |d| {
                            d.child(div().absolute().left_0().right_0().bottom(px(-11.)).h(px(2.)).bg(c(GREEN)))
                        }),
                )
        };
        let header = div()
            .h(px(64.))
            .flex_none()
            .pl(px(8.))
            .pr(px(16.))
            .flex()
            .flex_row()
            .items_center()
            .child(tab("tab-queue", "Queue", 0, cx))
            .child(tab("tab-recent", "Recently played", 1, cx))
            .child(div().flex_1())
            .child(self.panel_close("queue-close", cx));

        let mut q = self.queue.clone().unwrap_or_default();
        if q.local {
            (q.now, q.next) = self.context_queue();
        }
        let playing_id = self.playing_track_id();
        let mut body = div().id("queue-scroll").flex_1().min_h_0().overflow_y_scroll().flex().flex_col().pb(px(16.));
        if self.queue_tab == 0 {
            if let Some(now) = q.now.as_ref() {
                body = body
                    .child(self.section_title("Now playing").mt(px(20.8)).mb(px(6.8)))
                    .child(self.queue_row("now".into(), now, true, cx));
            }
            if !q.next.is_empty() {
                let ctx = self.context_name.as_ref().map(|c| c.1.clone()).filter(|n| !n.is_empty());
                let ctx_page = self.playback.as_ref().and_then(|p| p.context.as_ref()).and_then(|c| Page::for_uri(&c.uri));
                let title = match ctx {
                    Some(name) => div()
                        .flex()
                        .flex_row()
                        .child("Next from:\u{a0}")
                        .child(
                            div()
                                .id("queue-ctx-link")
                                .cursor_pointer()
                                .hover(|s| s.underline())
                                .child(name)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if let Some(p) = ctx_page.clone() {
                                        this.open(p, cx);
                                    }
                                })),
                        )
                        .into_any_element(),
                    None => div().child("Next up").into_any_element(),
                };
                body = body.child(self.section_title(title).mt(px(28.8)).mb(px(6.8)));
                for (i, t) in q.next.iter().enumerate() {
                    body = body.child(self.queue_row(format!("n{i}"), t, false, cx));
                }
            }
        } else {
            body = body.child(div().h(px(8.)));
            for (i, t) in q.recent.iter().enumerate() {
                let playing = t.id.is_some() && t.id == playing_id;
                body = body.child(self.queue_row(format!("r{i}"), t, playing, cx));
            }
        }
        self.panel_shell().child(header).child(body)
    }

    pub(super) fn devices_panel(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let ours = self.local.as_ref().map(|l| l.device_id.clone());
        let devices = self.devices.clone().unwrap_or_default();
        let active = devices.iter().find(|d| d.is_active).cloned();
        // The card shows where playback is; this app is "This computer".
        let current_is_us = active.as_ref().is_none_or(|d| d.id == ours);
        let current_name = if current_is_us { "This computer".to_string() } else { active.as_ref().map(|d| d.name.clone()).unwrap_or_default() };
        let others: Vec<Device> = devices
            .iter()
            .filter(|d| d.id != ours && !(d.is_active && !current_is_us))
            .cloned()
            .chain(
                // When something else is playing, this app is in the list to move playback back.
                (!current_is_us).then(|| devices.iter().find(|d| d.id == ours).cloned()).flatten().map(|mut d| {
                    d.name = "This computer".into();
                    d
                }),
            )
            .collect();

        let header = div()
            .h(px(64.))
            .flex_none()
            .pl(px(16.))
            .pr(px(16.))
            .flex()
            .flex_row()
            .items_center()
            .child(div().flex_1().text_size(px(16.)).font_weight(FontWeight::BOLD).text_color(c(WHITE)).child("Connect"))
            .child(self.panel_close("connect-close", cx));

        let card = div()
            .mx(px(8.))
            .h(px(120.))
            .flex_none()
            .rounded(px(8.))
            .bg(c(ROW_HOVER))
            .px(px(20.))
            .flex()
            .flex_col()
            .child(
                div()
                    .mt(px(20.))
                    .h(px(24.))
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(12.))
                    .child(icon("icons/computer24.svg", 24., GREEN))
                    .child(div().text_size(px(16.)).line_height(px(22.4)).text_color(c(GREEN)).child(current_name)),
            )
            .child(
                div()
                    .mt(px(32.))
                    .flex()
                    .flex_row()
                    .items_center()
                    .child(div().flex_1().text_size(px(14.)).text_color(c(TEXT_SUBDUED)).child("Listen with friends anywhere"))
                    .child(
                        div()
                            .id("start-a-jam")
                            .relative()
                            .h(px(32.))
                            .pl(px(36.))
                            .pr(px(16.))
                            .rounded_full()
                            .border(px(0.8))
                            .border_color(c(0x7C7C7C))
                            .hover(|s| s.border_color(c(WHITE)))
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .text_size(px(14.))
                            .font_weight(FontWeight::BOLD)
                            .text_color(c(WHITE))
                            .child(div().absolute().left(px(12.8)).top(px(7.2)).child(icon("icons/jam_start16.svg", 16., WHITE)))
                            .child("Start a Jam"),
                    ),
            );

        let mut list = div().mt(px(24.)).flex().flex_col();
        for (i, d) in others.into_iter().enumerate() {
            let dev = d.clone();
            list = list.child(
                div()
                    .id(gpui::ElementId::Name(format!("device-{i}").into()))
                    .mx(px(8.))
                    .h(px(56.))
                    .rounded(px(4.))
                    .hover(|s| s.bg(c(ROW_HOVER)))
                    .cursor_pointer()
                    .px(px(12.))
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(12.))
                    .on_click(cx.listener(move |this, _, _, cx| this.transfer_to(dev.clone(), cx)))
                    .child(icon("icons/computer24.svg", 24., WHITE))
                    .child(div().text_size(px(16.)).line_height(px(22.4)).text_color(c(WHITE)).child(d.name)),
            );
        }

        let link = |id: &'static str, text: &'static str, url: &'static str, cx: &mut Context<Self>| {
            div()
                .id(id)
                .mx(px(8.))
                .h(px(56.))
                .rounded(px(4.))
                .hover(|s| s.bg(c(ROW_HOVER)))
                .cursor_pointer()
                .px(px(12.))
                .flex()
                .flex_row()
                .items_center()
                .on_click(cx.listener(move |_, _, _, cx| cx.open_url(url)))
                .child(div().flex_1().text_size(px(16.)).line_height(px(22.4)).text_color(c(WHITE)).child(text))
                .child(icon("icons/external16.svg", 16., TEXT_SUBDUED))
        };

        self.panel_shell()
            .child(header)
            .child(card)
            .child(div().id("devices-scroll").flex_1().min_h_0().overflow_y_scroll().child(list))
            .child(
                div()
                    .flex_none()
                    .pb(px(16.))
                    .child(link("dont-see-device", "Don\u{2019}t see your device?", "https://support.spotify.com/article/spotify-connect/", cx))
                    .child(link(
                        "what-can-connect",
                        "What can I connect to?",
                        "https://www.spotify.com/connect?utm_campaign=connect&utm_medium=app&utm_source=desktop",
                        cx,
                    )),
            )
    }
}
