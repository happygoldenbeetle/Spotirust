//! Profile (avatar) menu. Measured on the reference:
//!   328 wide, right edge on the avatar button's right edge, 8.8 below it; #282828, radius 4,
//!   padding 4, sort-menu shadow. Items 40 tall (radius 2, padding 8 8 8 12): 14px
//!   rgba(255,255,255,.9), hover rgba(255,255,255,.1) + white; external links end in a 16px icon.
//!   After "Log out" a full-width 1px rgba(255,255,255,.1) divider, then "Your Updates"
//!   (16/700 white, padding 8 8 0, title padding 8 0) over a list `min(25vh, 256px)` tall with a
//!   classic scrollbar (15.2 wide, thumb rgba(255,255,255,.3)). Update rows 64 tall, radius 4,
//!   padding 8 12: a 48px picture, then 12px "title â€¢ 8w" (time #B3B3B3), unread dot 8px #4687D6.
//!   Data: `gander/v2/GetNotifications`.

use super::*;

const MENU_W: f32 = 328.;
const SCROLLBAR_W: f32 = 15.2;

/// One "Your Updates" row.
#[derive(Clone)]
pub struct Notification {
    pub title: String,
    pub created: Option<i64>,
    pub uri: Option<String>,
    pub is_new: bool,
    pub picture: Picture,
}

#[derive(Clone)]
pub enum Picture {
    /// A user (round, 48).
    User(String),
    /// A playlist or other entity (40, radius 4) with the user's round 24px avatar over its corner.
    UserAndEntity { user: String, entity: String },
    /// An entity alone (48, radius 4).
    Entity(String),
    None,
}

pub fn parse_notifications(v: &serde_json::Value) -> Vec<Notification> {
    let s = |v: &serde_json::Value, p: &str| v.pointer(p).and_then(|x| x.as_str()).map(str::to_string);
    v.get("notifications")
        .and_then(|n| n.as_array())
        .map(|list| {
            list.iter()
                .map(|n| Notification {
                    title: s(n, "/title").unwrap_or_default().trim().to_string(),
                    created: s(n, "/createdTimestamp").and_then(|t| parse_iso(&t)),
                    uri: s(n, "/action/uri"),
                    is_new: n.get("isNew").and_then(|b| b.as_bool()).unwrap_or(false),
                    picture: if let Some(u) = s(n, "/multiUserImage/userImages/0/imageUrl") {
                        Picture::User(u)
                    } else if let (Some(user), Some(entity)) =
                        (s(n, "/userAndEntityImage/userImage/imageUrl"), s(n, "/userAndEntityImage/entityImage/imageUrl"))
                    {
                        Picture::UserAndEntity { user, entity }
                    } else if let Some(u) = s(n, "/entityImage/imageUrl") {
                        Picture::Entity(u)
                    } else {
                        Picture::None
                    },
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Seconds since the epoch for an ISO-8601 UTC time ("2026-09-11T14:59:06.196Z").
fn parse_iso(t: &str) -> Option<i64> {
    let n = |r: std::ops::Range<usize>| t.get(r)?.parse::<i64>().ok();
    let (y, m, d) = (n(0..4)?, n(5..7)?, n(8..10)?);
    let (hh, mm, ss) = (n(11..13)?, n(14..16)?, n(17..19)?);
    // days from civil (Howard Hinnant)
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    Some(days * 86400 + hh * 3600 + mm * 60 + ss)
}

/// The client's short relative time with spaces removed: "8w", "3d", "5hr", "12min", "now".
fn relative_time(created: i64) -> String {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    let ms = ((now - created) * 1000) as f64;
    let mins = (ms / 1e3 / 60.).round() as i64;
    let hours = (ms / 1e3 / 3600.).round() as i64;
    let days = (ms / 1e3 / 86400.).round() as i64;
    let weeks = (ms / 1e3 / 86400. / 7.).round() as i64;
    if days >= 7 {
        format!("{weeks}w")
    } else if hours >= 24 {
        format!("{days}d")
    } else if mins >= 60 {
        format!("{hours}hr")
    } else if mins > 0 {
        format!("{mins}min")
    } else {
        "now".into()
    }
}

enum Action {
    Url(&'static str),
    Profile,
    LogOut,
    Close,
}

const ITEMS: [(&str, bool, Action); 8] = [
    ("Account", true, Action::Url("https://www.spotify.com/account/overview/")),
    ("Set up your Duo plan", true, Action::Url("https://www.spotify.com/account/duo/")),
    ("Profile", false, Action::Profile),
    ("Recents", false, Action::Close),
    ("Support", true, Action::Url("https://support.spotify.com/")),
    ("Private session", false, Action::Close),
    ("Settings", false, Action::Close),
    ("Log out", false, Action::LogOut),
];

impl Shell {
    pub(super) fn toggle_profile_menu(&mut self, cx: &mut Context<Self>) {
        if !self.profile_open && self.menu_closed_at.elapsed().as_millis() < 400 {
            return;
        }
        self.profile_open = !self.profile_open;
        if self.profile_open {
            self.load_notifications(cx);
        }
        cx.notify();
    }

    fn load_notifications(&mut self, cx: &mut Context<Self>) {
        let Some(sp) = self.spotify.clone() else { return };
        cx.spawn(async move |this, cx| {
            let result = cx.background_executor().spawn(async move { sp.notifications() }).await;
            let _ = this.update(cx, |this, cx| match result {
                Ok(v) => {
                    this.notifications = Some(parse_notifications(&v));
                    cx.notify();
                }
                Err(e) => eprintln!("notifications: {e:#}"),
            });
        })
        .detach();
    }

    pub(super) fn profile_menu(&self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.profile_open {
            return None;
        }
        let close = |this: &mut Shell| this.profile_open = false;
        let mut menu = super::library_view::menu_list(true).w(px(MENU_W)).rounded(px(4.));
        for (i, (label, external, _)) in ITEMS.iter().enumerate() {
            let label = *label;
            menu = menu.child(
                div()
                    .id(gpui::ElementId::Name(format!("profile-item-{i}").into()))
                    .h(px(40.))
                    .flex_none()
                    .pl(px(12.))
                    .pr(px(8.))
                    .rounded(px(2.))
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .cursor_pointer()
                    .text_size(px(14.))
                    .text_color(rgba(0xFFFFFFE6))
                    .hover(|s| s.bg(rgba(0xFFFFFF1A)).text_color(c(WHITE)))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        close(this);
                        match ITEMS[i].2 {
                            Action::Url(url) => cx.open_url(url),
                            Action::Profile => {
                                if let Some(me) = &this.me {
                                    this.open(Page::User(format!("spotify:user:{}", me.id)), cx);
                                }
                            }
                            Action::LogOut => this.log_out(cx),
                            Action::Close => {}
                        }
                        cx.notify();
                    }))
                    .child(label)
                    .when(*external, |d| d.child(svg().path("icons/external16.svg").size(px(16.)).text_color(rgba(0xFFFFFFE6)))),
            );
        }
        menu = menu.child(div().h(px(1.)).flex_none().bg(rgba(0xFFFFFF1A)));

        // Your Updates
        let vh = f32::from(window.viewport_size().height);
        let max_h = (vh * 0.25).min(256.);
        let rows = self.notifications.clone().unwrap_or_default();
        let content_h = rows.len() as f32 * 64.;
        let list_h = content_h.min(max_h);
        let scrollable = content_h > max_h + 0.5;
        let row_w = if scrollable { MENU_W - 8. - SCROLLBAR_W } else { MENU_W - 8. };
        let mut list = div()
            .id("your-updates")
            .w(px(MENU_W - 8.))
            .h(px(list_h))
            .overflow_y_scroll()
            .track_scroll(&self.profile_scroll)
            // the scrollbar thumb follows the list
            .on_scroll_wheel(cx.listener(|_, _, _, cx| cx.notify()))
            .flex()
            .flex_col();
        for (i, n) in rows.iter().enumerate() {
            let uri = n.uri.clone();
            let time = n.created.map(relative_time);
            list = list.child(
                div()
                    .id(gpui::ElementId::Name(format!("update-{i}").into()))
                    .w(px(row_w))
                    .h(px(64.))
                    .flex_none()
                    .px(px(12.))
                    .rounded(px(4.))
                    .flex()
                    .flex_row()
                    .items_center()
                    .cursor_pointer()
                    .hover(|s| s.bg(rgba(0xFFFFFF1A)))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        close(this);
                        if let Some(p) = uri.as_deref().and_then(Page::for_uri) {
                            this.open(p, cx);
                        }
                        cx.notify();
                    }))
                    .child(notification_picture(&n.picture, cx))
                    .child(
                        div()
                            .ml(px(12.))
                            .flex_1()
                            .min_w_0()
                            .text_size(px(12.))
                            .line_height(px(16.8))
                            .text_color(c(WHITE))
                            .child(
                                gpui::StyledText::new(match &time {
                                    Some(t) => format!("{} \u{2022} {t}", n.title),
                                    None => n.title.clone(),
                                })
                                .with_highlights(time.as_ref().map(|t| {
                                    let start = n.title.len();
                                    let end = start + " \u{2022} ".len() + t.len();
                                    (start..end, gpui::HighlightStyle { color: Some(c(TEXT_SUBDUED).into()), ..Default::default() })
                                })),
                            ),
                    )
                    .when(n.is_new, |d| d.child(div().ml(px(8.)).size(px(8.)).flex_none().rounded_full().bg(c(0x4687D6)))),
            );
        }
        let mut updates = div().relative().ml(px(-8.)).w(px(MENU_W - 8.)).child(list);
        if scrollable {
            let off = -f32::from(self.profile_scroll.offset().y);
            let track = list_h - 2. * SCROLLBAR_W;
            let thumb = (track * list_h / content_h).max(16.);
            let top = SCROLLBAR_W + (track - thumb) * (off / (content_h - list_h)).clamp(0., 1.);
            updates = updates
                .child(
                    div()
                        .absolute()
                        .top(px(4.6))
                        .left(px(MENU_W - 8. - SCROLLBAR_W / 2. - 5.))
                        .child(svg().path("icons/sb_up.svg").w(px(10.)).h(px(5.)).text_color(rgba(0xFFFFFF4D))),
                )
                .child(
                    div()
                        .absolute()
                        .top(px(list_h - 4.6 - 5.))
                        .left(px(MENU_W - 8. - SCROLLBAR_W / 2. - 5.))
                        .child(svg().path("icons/sb_down.svg").w(px(10.)).h(px(5.)).text_color(rgba(0xFFFFFF4D))),
                )
                .child(
                    div()
                        .absolute()
                        .top(px(top))
                        .left(px(MENU_W - 8. - SCROLLBAR_W / 2. - 4.))
                        .w(px(8.))
                        .h(px(thumb))
                        .rounded_full()
                        .bg(rgba(0xFFFFFF4D)),
                );
        }
        menu = menu.child(
            div()
                .px(px(8.))
                .pt(px(8.))
                .flex()
                .flex_col()
                .child(
                    div()
                        .pl(px(4.))
                        .py(px(8.))
                        .text_size(px(16.))
                        .line_height(px(22.4))
                        .font_weight(FontWeight::BOLD)
                        .text_color(c(WHITE))
                        .child("Your Updates"),
                )
                .child(updates),
        );

        Some(
            div()
                .id("profile-menu")
                .absolute()
                .top(px(64.8))
                .right(px(150.4))
                .occlude()
                .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                    if this.profile_open {
                        this.profile_open = false;
                        this.menu_closed_at = std::time::Instant::now();
                    }
                    cx.notify();
                }))
                .child(menu)
                .into_any_element(),
        )
    }
}

fn notification_picture(p: &Picture, cx: &mut App) -> AnyElement {
    match p {
        Picture::User(u) => cover(Some(u.as_str()), 48., 24., cx),
        Picture::Entity(u) => cover(Some(u.as_str()), 48., 4., cx),
        Picture::UserAndEntity { user, entity } => div()
            .relative()
            .size(px(48.))
            .flex_none()
            .child(
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .rounded(px(4.))
                    .shadow(vec![gpui::BoxShadow {
                        color: gpui::hsla(0., 0., 0., 0.5),
                        offset: gpui::point(px(0.), px(4.)),
                        blur_radius: px(60.),
                        spread_radius: px(0.),
                    }])
                    .child(cover(Some(entity.as_str()), 40., 4., cx)),
            )
            .child(
                // 24px avatar with a 2px #282828 outline
                div()
                    .absolute()
                    .right(px(-2.))
                    .bottom(px(-2.))
                    .size(px(28.))
                    .rounded_full()
                    .bg(c(0x282828))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(cover(Some(user.as_str()), 24., 12., cx)),
            )
            .into_any_element(),
        Picture::None => div().size(px(48.)).flex_none().rounded_full().bg(c(0x333333)).into_any_element(),
    }
}
