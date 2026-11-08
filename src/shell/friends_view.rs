//! Listening activity panel (top bar friends button). Measured on the reference:
//!   replaces the right panel; header 64 tall (padding 0 16): "Listening activity" 16/700 white,
//!   close 32px round (hover rgba(255,255,255,.1), white, scale 1.04). Content padding 16.
//!   Jam card: #282828, radius 8, padding 16; 14px #B3B3B3 text, close at 12/12 from the corner;
//!   "Start a Jam" pill 32 tall, green, 14/700 black, padding 4 16 4 36 with a 16px icon at 12.
//!   16 below the card, friend rows 57 tall 24 apart: 40px round avatar (2 down; hover shows a
//!   play button, rgba(0,0,0,.5), opacity .2s), text 52 to the right: name 14/700 and time 12
//!   right-aligned, "track • artist" 12, context with a 16px icon; links rgba(255,255,255,.7),
//!   hover white + underline. Data: `presence-view`.

use super::*;

#[derive(Clone)]
pub struct Friend {
    pub user_uri: String,
    pub name: String,
    pub image: Option<String>,
    pub timestamp: i64,
    pub track_uri: String,
    pub track: String,
    pub album_uri: String,
    pub artist_uri: String,
    pub artist: String,
    pub context_uri: String,
    pub context: String,
}

pub fn parse_friends(v: &serde_json::Value) -> Vec<Friend> {
    let s = |v: &serde_json::Value, p: &str| v.pointer(p).and_then(|x| x.as_str()).unwrap_or_default().to_string();
    let mut out: Vec<Friend> = v
        .get("friends")
        .and_then(|f| f.as_array())
        .map(|list| {
            list.iter()
                .map(|f| Friend {
                    user_uri: s(f, "/user/uri"),
                    name: s(f, "/user/name"),
                    image: Some(s(f, "/user/imageUrl")).filter(|u| !u.is_empty()),
                    timestamp: f.get("timestamp").and_then(|t| t.as_i64()).unwrap_or(0),
                    track_uri: s(f, "/track/uri"),
                    track: s(f, "/track/name"),
                    album_uri: s(f, "/track/album/uri"),
                    artist_uri: s(f, "/track/artist/uri"),
                    artist: s(f, "/track/artist/name"),
                    context_uri: s(f, "/track/context/uri"),
                    context: s(f, "/track/context/name"),
                })
                .collect()
        })
        .unwrap_or_default();
    out.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
    out
}

/// "now", "12 min", "3 hr", "4 d", "2 w" (the client's `time.*.short` strings).
fn ago(ms: i64) -> String {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0);
    let d = (now - ms) as f64;
    let mins = (d / 6e4).round() as i64;
    let hours = (d / 3.6e6).round() as i64;
    let days = (d / 8.64e7).round() as i64;
    let weeks = (d / 8.64e7 / 7.).round() as i64;
    if days >= 7 {
        format!("{weeks} w")
    } else if hours >= 24 {
        format!("{days} d")
    } else if mins >= 60 {
        format!("{hours} hr")
    } else if mins > 0 {
        format!("{mins} min")
    } else {
        "now".into()
    }
}

/// A rgba(255,255,255,.7) link that turns white and underlines on hover.
fn link(id: String, text: String, uri: String, cx: &mut Context<Shell>) -> Stateful<Div> {
    div()
        .id(gpui::ElementId::Name(id.into()))
        .flex_none()
        .cursor_pointer()
        .text_color(rgba(0xFFFFFFB3))
        .hover(|s| s.text_color(c(WHITE)).underline())
        .on_click(cx.listener(move |this, _, _, cx| {
            if let Some(p) = Page::for_uri(&uri) {
                this.open(p, cx);
            }
        }))
        .child(text)
}

impl Shell {
    pub(super) fn toggle_friends(&mut self, cx: &mut Context<Self>) {
        self.right_panel = if self.right_panel == RightPanel::Friends { RightPanel::NowPlaying } else { RightPanel::Friends };
        if self.right_panel == RightPanel::Friends {
            self.load_friends(cx);
        }
        cx.notify();
    }

    fn load_friends(&mut self, cx: &mut Context<Self>) {
        let Some(sp) = self.spotify.clone() else { return };
        cx.spawn(async move |this, cx| {
            let result = cx.background_executor().spawn(async move { sp.buddylist() }).await;
            let _ = this.update(cx, |this, cx| match result {
                Ok(v) => {
                    this.friends = Some(parse_friends(&v));
                    cx.notify();
                }
                Err(e) => eprintln!("buddylist: {e:#}"),
            });
        })
        .detach();
    }

    pub(super) fn friends_panel(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let inner_w = self.right_w - 32.;
        let mut list = div().flex().flex_col();
        if !self.jam_card_dismissed {
            list = list.child(
                div()
                    .relative()
                    .mb(px(16.))
                    .p(px(16.))
                    .rounded(px(8.))
                    .bg(c(0x282828))
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .pr(px(46.))
                            .text_size(px(14.))
                            .line_height(px(19.2))
                            .text_color(c(TEXT_SUBDUED))
                            .child("Invite others to your Jam, and listen together from anywhere."),
                    )
                    .child(
                        round_button("jam-card-close", 32.)
                            .absolute()
                            .top(px(12.))
                            .right(px(8.))
                            .group("jam-card-close")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.jam_card_dismissed = true;
                                cx.notify();
                            }))
                            .child(
                                svg()
                                    .path("icons/close16.svg")
                                    .size(px(16.))
                                    .text_color(c(TEXT_SUBDUED))
                                    .group_hover("jam-card-close", |s| s.text_color(c(WHITE))),
                            ),
                    )
                    .child(
                        div().mt(px(8.)).flex().child(
                            div()
                                .id("start-jam")
                                .h(px(32.))
                                .pl(px(36.))
                                .pr(px(16.))
                                .relative()
                                .rounded_full()
                                .bg(c(GREEN))
                                .hover(|s| s.bg(c(0x3BE477)))
                                .cursor_pointer()
                                .flex()
                                .items_center()
                                .text_size(px(14.))
                                .font_weight(FontWeight::BOLD)
                                .text_color(c(0x000000))
                                .child(
                                    div()
                                        .absolute()
                                        .left(px(12.))
                                        .top(px(8.))
                                        .child(svg().path("icons/jam16.svg").size(px(16.)).text_color(c(0x000000))),
                                )
                                .child("Start a Jam"),
                        ),
                    ),
            );
        }
        for (i, f) in self.friends.clone().unwrap_or_default().into_iter().enumerate() {
            let ctx_icon = match f.context_uri.split(':').nth(1) {
                Some("playlist") | Some("collection") => "icons/playlist16.svg",
                Some("user") if f.context_uri.contains(":collection") => "icons/playlist16.svg",
                Some("album") => "icons/album16.svg",
                _ => "icons/track16.svg",
            };
            let group: SharedString = format!("friend-{i}").into();
            let (play_ctx, play_track) = (f.context_uri.clone(), f.track_uri.clone());
            list = list.child(
                div()
                    .when(i > 0, |d| d.mt(px(24.)))
                    .h(px(57.))
                    .pr(px(8.))
                    .flex()
                    .flex_row()
                    .child(
                        div()
                            .id(gpui::ElementId::Name(format!("friend-art-{i}").into()))
                            .group(group.clone())
                            .relative()
                            .mt(px(2.))
                            .size(px(40.))
                            .flex_none()
                            .child(cover(f.image.as_deref(), 40., 20., cx))
                            .child(
                                div()
                                    .id(gpui::ElementId::Name(format!("friend-play-{i}").into()))
                                    .absolute()
                                    .top_0()
                                    .left_0()
                                    .size(px(40.))
                                    .rounded_full()
                                    .bg(rgba(0x00000080))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .cursor_pointer()
                                    .opacity(0.)
                                    .group_hover(group.clone(), |s| s.opacity(1.))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        let (ctx, track) = (play_ctx.clone(), play_track.clone());
                                        this.command(move |a| a.play_context_at(&ctx, &track), cx);
                                    }))
                                    .child(svg().path("icons/play_tri.svg").size(px(16.)).text_color(c(WHITE))),
                            ),
                    )
                    .child(
                        div()
                            .ml(px(12.))
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .line_height(px(17.))
                            .child(
                                div()
                                    .h(px(19.))
                                    .flex()
                                    .flex_row()
                                    .justify_between()
                                    .child(
                                        link(format!("friend-name-{i}"), f.name.clone(), f.user_uri.clone(), cx)
                                            .text_size(px(14.))
                                            .font_weight(FontWeight::BOLD)
                                            .mt(px(-1.6))
                                            .line_height(px(19.2)),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(12.))
                                            .line_height(px(16.8))
                                            .text_color(rgba(0xFFFFFFB3))
                                            .child(ago(f.timestamp)),
                                    ),
                            )
                            .child(
                                div()
                                    .h(px(19.))
                                    .flex()
                                    .flex_row()
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .text_size(px(12.))
                                    .text_color(c(WHITE))
                                    .child(link(format!("friend-track-{i}"), f.track.clone(), f.album_uri.clone(), cx))
                                    .child(div().flex_none().px(px(2.5)).child("\u{2022}"))
                                    .child(link(format!("friend-artist-{i}"), f.artist.clone(), f.artist_uri.clone(), cx)),
                            )
                            .child(
                                div()
                                    .h(px(19.))
                                    .flex()
                                    .flex_row()
                                    .items_start()
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .text_size(px(12.))
                                    .child(div().flex_none().pr(px(4.)).child(svg().path(ctx_icon).size(px(16.)).text_color(rgba(0xFFFFFFB3))))
                                    .child(link(format!("friend-ctx-{i}"), f.context.clone(), f.context_uri.clone(), cx)),
                            ),
                    ),
            );
        }

        div()
            .w(px(self.right_w))
            .flex_none()
            .h_full()
            .rounded(px(8.))
            .bg(c(SURFACE))
            .overflow_hidden()
            .flex()
            .flex_col()
            .child(
                div()
                    .h(px(64.))
                    .flex_none()
                    .px(px(16.))
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_size(px(16.))
                            .line_height(px(22.4))
                            .font_weight(FontWeight::BOLD)
                            .text_color(c(WHITE))
                            .child("Listening activity"),
                    )
                    .child(
                        round_button("friends-close", 32.)
                            .group("friends-close")
                            .hover(|s| s.bg(rgba(0xFFFFFF1A)))
                            .on_click(cx.listener(|this, _, _, cx| this.toggle_friends(cx)))
                            .child(
                                svg()
                                    .path("icons/close16.svg")
                                    .size(px(16.))
                                    .text_color(c(TEXT_SUBDUED))
                                    .group_hover("friends-close", |s| s.text_color(c(WHITE))),
                            ),
                    ),
            )
            .child(
                div()
                    .id("friends-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .p(px(16.))
                    .child(div().w(px(inner_w)).child(list)),
            )
    }
}
