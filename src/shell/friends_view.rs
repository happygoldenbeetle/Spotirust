//! Listening activity panel (top bar friends button). Measured on the reference:
//!   replaces the right panel; header 64 tall (padding 0 16): "Listening activity" 16/700 white,
//!   close 32px round (hover rgba(255,255,255,.1), white, scale 1.04). Content padding 16.
//!   Jam card: #282828, radius 8, padding 16; 14px #B3B3B3 text, close at 12/12 from the corner;
//!   "Start a Jam" pill 32 tall, green, 14/700 black, padding 4 16 4 36 with a 16px icon at 12.
//!   16 below the card, friend rows 57 tall 24 apart: 40px round avatar (2 down; hover shows a
//!   play button, rgba(0,0,0,.5), opacity .2s), text 52 to the right: name 14/700 and time 12
//!   right-aligned, "track • artist" 12, context with a 16px icon; links rgba(255,255,255,.7),
//!   hover white + underline. Data: `listening-activity` (feed + pushes).

use super::*;

#[derive(Clone)]
pub struct Friend {
    pub user_uri: String,
    pub name: String,
    pub image: Option<String>,
    pub timestamp: i64,
    /// Listening right now: shown first, with the equaliser in place of the time.
    pub playing: bool,
    pub track_uri: String,
    pub track: String,
    pub album_uri: String,
    pub artist_uri: String,
    pub artist: String,
    pub context_uri: String,
    pub context: String,
    /// Where the context's name leads; empty when it has no page (Liked Songs, DJ).
    pub context_link: String,
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
                    playing: f.get("isPlaying").and_then(|p| p.as_bool()).unwrap_or(false),
                    track_uri: s(f, "/track/uri"),
                    track: s(f, "/track/name"),
                    album_uri: s(f, "/track/album/uri"),
                    artist_uri: s(f, "/track/artist/uri"),
                    artist: s(f, "/track/artist/name"),
                    context_uri: s(f, "/track/context/uri"),
                    context_link: match f.pointer("/track/context/navigable").and_then(|n| n.as_bool()) {
                        Some(false) => String::new(),
                        _ => s(f, "/track/context/uri"),
                    },
                    context: s(f, "/track/context/name"),
                })
                .collect()
        })
        .unwrap_or_default();
    // As the client sorts: whoever is playing first, then the most recent.
    out.sort_by(|a, b| b.playing.cmp(&a.playing).then(b.timestamp.cmp(&a.timestamp)));
    out
}

/// One activity from `listening-activity`: who, the track, where it played from, and when.
struct Activity {
    username: String,
    track: String,
    context: Option<String>,
    playing: bool,
    /// Unix ms.
    timestamp: Option<i64>,
}

impl Activity {
    /// `a`: `{entityUri | uri, contextUri, isPlaying, timestamp}` (the feed and pushes name the
    /// track differently).
    fn new(username: &str, a: &serde_json::Value) -> Option<Self> {
        let track = a.get("entityUri").or_else(|| a.get("uri"))?.as_str()?;
        if username.is_empty() || !track.starts_with("spotify:track:") {
            return None;
        }
        Some(Self {
            username: username.to_string(),
            track: track.to_string(),
            context: a.get("contextUri").and_then(|c| c.as_str()).filter(|c| !c.is_empty()).map(str::to_string),
            playing: a.get("isPlaying").and_then(|p| p.as_bool()).unwrap_or(false),
            timestamp: a.get("timestamp").and_then(unix_ms),
        })
    }
}

/// A timestamp as the service writes it: an RFC 3339 string, a number (s or ms), or
/// `{seconds, nanos}`.
fn unix_ms(v: &serde_json::Value) -> Option<i64> {
    let num = |n: f64| if n < 1e11 { (n * 1000.) as i64 } else { n as i64 };
    match v {
        serde_json::Value::Number(n) => n.as_f64().map(num),
        serde_json::Value::Object(o) => {
            let secs = o.get("seconds").and_then(|s| s.as_str().and_then(|s| s.parse::<i64>().ok()).or(s.as_i64()))?;
            Some(secs * 1000 + o.get("nanos").and_then(|n| n.as_i64()).unwrap_or(0) / 1_000_000)
        }
        serde_json::Value::String(s) => s.parse::<f64>().ok().map(num).or_else(|| rfc3339_ms(s)),
        _ => None,
    }
}

/// "2026-11-09T14:03:22.123Z" (or with a +hh:mm offset) as Unix ms.
fn rfc3339_ms(s: &str) -> Option<i64> {
    let (date, time) = s.split_once(['T', 't', ' '])?;
    let mut d = date.split('-').map(|p| p.parse::<i64>().ok());
    let (y, m, day) = (d.next()??, d.next()??, d.next()??);
    let (clock, offset_min) = match time.find(['Z', 'z', '+', '-']) {
        Some(i) => {
            let (clock, zone) = time.split_at(i);
            let mins = match zone.strip_prefix(['+', '-']) {
                Some(hm) => {
                    let (h, mi) = hm.split_once(':').unwrap_or((hm, "0"));
                    let v = h.parse::<i64>().ok()? * 60 + mi.parse::<i64>().ok()?;
                    if zone.starts_with('-') { -v } else { v }
                }
                None => 0,
            };
            (clock, mins)
        }
        None => (time, 0),
    };
    let mut t = clock.split(':');
    let (h, mi) = (t.next()?.parse::<i64>().ok()?, t.next()?.parse::<i64>().ok()?);
    let secs = t.next().unwrap_or("0").parse::<f64>().ok()?;
    // Days since 1970-01-01 (proleptic Gregorian), counting years from March.
    let (y, m) = if m <= 2 { (y - 1, m + 12) } else { (y, m) };
    let days = 365 * y + y.div_euclid(4) - y.div_euclid(100) + y.div_euclid(400) + (153 * (m - 3) + 2) / 5 + day - 719469;
    Some(((days * 24 + h) * 60 + mi - offset_min) * 60_000 + (secs * 1000.) as i64)
}

/// A playlist context from `fetchPlaylist`: its name (when you may view it) and whether the name
/// links to it. As in the client, the DJ and playlists of a special format (a friend's Liked
/// Songs, say) have no page of their own.
fn playlist_context(v: &serde_json::Value) -> Option<(String, bool)> {
    const WITH_PAGE: [&str; 8] = ["artistsets", "chart", "editorial", "format-shows", "format-shows-shuffle", "inspiredby-mix", "sporty", "topic-mix"];
    let p = v.pointer("/data/playlistV2")?;
    let name = p.get("name")?.as_str().filter(|n| !n.is_empty())?;
    if p.pointer("/currentUserCapabilities/canView").and_then(|c| c.as_bool()) == Some(false) {
        return None;
    }
    let format = p.get("format").and_then(|f| f.as_str()).unwrap_or_default();
    let dj = p.get("attributes").and_then(|a| a.as_array()).into_iter().flatten().any(|a| {
        a.get("key").and_then(|k| k.as_str()) == Some("lexicon_set_type") && a.get("value").and_then(|x| x.as_str()) == Some("your_dj")
    });
    Some((name.to_string(), !dj && (format.is_empty() || WITH_PAGE.contains(&format))))
}

/// Names and pictures for activities, in the shape the panel reads (`{timestamp, isPlaying, user,
/// track {album, artist, context}}`). The client asks its metadata service; these are the same
/// facts from the profile, track and playlist calls. `known` is what's on screen: a friend still
/// on the same track from the same place is reused as is (their profile too), without asking.
fn describe(sp: &Spotify, api: &Api, acts: Vec<Activity>, known: &[serde_json::Value]) -> Vec<serde_json::Value> {
    let text = |f: &serde_json::Value, p: &str| f.pointer(p).and_then(|x| x.as_str()).filter(|x| !x.is_empty()).map(str::to_string);
    let mut users: HashMap<String, (String, Option<String>)> =
        known.iter().filter_map(|f| Some((text(f, "/user/uri")?, (text(f, "/user/name")?, text(f, "/user/imageUrl"))))).collect();
    // Per context: its name and whether the name is a link.
    let mut contexts: HashMap<String, Option<(String, bool)>> = HashMap::new();
    let mut out = Vec::new();
    for a in acts {
        let user_uri = format!("spotify:user:{}", a.username);
        let same = known.iter().find(|f| {
            text(f, "/user/uri").as_deref() == Some(user_uri.as_str())
                && text(f, "/track/uri").as_deref() == Some(a.track.as_str())
                && f.get("from").is_some_and(|c| c.as_str() == a.context.as_deref())
        });
        if let Some(f) = same {
            let mut f = f.clone();
            f["timestamp"] = a.timestamp.into();
            f["isPlaying"] = a.playing.into();
            out.push(f);
            continue;
        }
        if !users.contains_key(&user_uri) {
            match sp.user_brief(&a.username) {
                Ok(u) => {
                    users.insert(user_uri.clone(), u);
                }
                Err(e) => eprintln!("friend {}: {e:#}", a.username),
            }
        }
        let Some((name, image)) = users.get(&user_uri).cloned() else { continue };
        let track = match api.track(a.track.rsplit(':').next().unwrap_or_default()) {
            Ok(t) => t,
            Err(e) => {
                eprintln!("friend {}: {e:#}", a.username);
                continue;
            }
        };
        let (Some(album), Some(artist)) = (track.album.as_ref(), track.artists.first()) else { continue };
        // Where it played from; without a name to show for it, the album (as the client does).
        let named = a.context.as_ref().and_then(|ctx| {
            contexts
                .entry(ctx.clone())
                .or_insert_with(|| {
                    if ctx.contains(":collection") {
                        Some(("Liked Songs".to_string(), false))
                    } else if ctx.starts_with("spotify:playlist:") {
                        sp.playlist(ctx, 0, 1).ok().and_then(|v| playlist_context(&v))
                    } else if ctx.starts_with("spotify:album:") {
                        if *ctx == album.uri {
                            Some((album.name.clone(), true))
                        } else {
                            sp.album(ctx, 0, 1).ok().and_then(|v| pl::parse_album(&v)).map(|p| (p.name, true)).filter(|n| !n.0.is_empty())
                        }
                    } else if ctx.starts_with("spotify:artist:") {
                        track.artists.iter().find(|x| x.uri == *ctx).map(|x| (x.name.clone(), true))
                    } else {
                        None
                    }
                })
                .clone()
                .map(|(n, link)| (ctx.clone(), n, link))
        });
        let (ctx_uri, ctx_name, ctx_link) = named.unwrap_or_else(|| (album.uri.clone(), album.name.clone(), true));
        out.push(serde_json::json!({
            "timestamp": a.timestamp,
            "isPlaying": a.playing,
            // The context as the service named it (to recognise the same activity again).
            "from": a.context,
            "user": { "uri": user_uri, "name": name, "imageUrl": image.unwrap_or_default() },
            "track": {
                "uri": a.track,
                "name": track.name,
                "imageUrl": api::pick_image(&album.images, 64).map(|i| i.url.clone()).unwrap_or_default(),
                "album": { "uri": album.uri, "name": album.name },
                "artist": { "uri": artist.uri, "name": artist.name },
                "context": { "uri": ctx_uri, "name": ctx_name, "index": 0, "navigable": ctx_link },
            },
        }));
    }
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

/// A rgba(255,255,255,.7) link that turns white and underlines on hover, with its text as a
/// native tooltip. Contexts without a page (Liked Songs, DJ) are plain text, as in the client.
fn link(id: String, text: String, uri: String, cx: &mut Context<Shell>) -> Stateful<Div> {
    let el = div().id(gpui::ElementId::Name(id.into())).min_w_0().overflow_hidden().text_color(rgba(0xFFFFFFB3));
    let plain = uri.is_empty() || uri.ends_with(":collection") || uri.ends_with(":collection:tracks") || uri.contains(":station:") || uri.contains("37i9dQZF1EYkqdzj48dyYq");
    if plain {
        return el.child(text);
    }
    el.cursor_pointer()
        .hover(|s| s.text_color(c(WHITE)).underline())
        .tooltip(native_tip(text.clone()))
        .on_click(cx.listener(move |this, _, _, cx| {
            if let Some(p) = Page::for_uri(&uri) {
                this.open(p, cx);
            }
        }))
        .child(text)
}

fn friend_cache_path() -> Option<std::path::PathBuf> {
    Some(std::path::PathBuf::from(std::env::var_os("APPDATA")?).join("Spotirust").join("friends.json"))
}

fn load_friend_cache() -> Vec<serde_json::Value> {
    friend_cache_path()
        .and_then(|p| std::fs::read(p).ok())
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

fn save_friend_cache(list: &[serde_json::Value]) {
    if let Some(p) = friend_cache_path() {
        let _ = std::fs::create_dir_all(p.parent().unwrap());
        let _ = std::fs::write(p, serde_json::to_vec(list).unwrap_or_default());
    }
}

impl Shell {
    pub(super) fn toggle_friends(&mut self, cx: &mut Context<Self>) {
        self.right_panel = if self.right_panel == RightPanel::Friends { RightPanel::NowPlaying } else { RightPanel::Friends };
        // As the client does, opening the panel asks for the feed again.
        self.friends_refresh |= self.right_panel == RightPanel::Friends;
        cx.notify();
    }

    /// The Listening activity feed, running from sign-in as the client's does: what the people you
    /// follow last played (`listening-activity/v1/feed`), then their changes as they're pushed to
    /// this connection, which is subscribed per friend and re-subscribed before that runs out or
    /// when the connection is made again. While the panel is open the feed is also read again every
    /// minute, so it stays current even if a push is missed. What was last seen shows until the
    /// feed arrives.
    pub(super) fn start_friend_feed(&mut self, cx: &mut Context<Self>) {
        let (Some(sp), Some(api)) = (self.spotify.clone(), self.api.clone()) else { return };
        self.friends_watch_seq += 1;
        let seq = self.friends_watch_seq;
        if self.friends_raw.is_empty() {
            self.friends_raw = load_friend_cache();
            // Nobody is known to be playing until the feed says so.
            for f in &mut self.friends_raw {
                f["isPlaying"] = false.into();
            }
            if !self.friends_raw.is_empty() {
                self.friends = Some(parse_friends(&serde_json::json!({ "friends": self.friends_raw })));
            }
        }
        self.friends_refresh = true;
        cx.spawn(async move |this, cx| {
            let mut followed: Vec<String> = Vec::new();
            // The connection subscribed on, and when to renew.
            let mut subscribed: Option<(String, Instant)> = None;
            let mut subscribe_at = Instant::now();
            let mut feed_at = Instant::now();
            let mut ticks = 0u32;
            loop {
                let Ok((current, open, refresh, known)) = this.update(cx, |this, _| {
                    let refresh = std::mem::take(&mut this.friends_refresh);
                    (this.friends_watch_seq == seq, this.right_panel == RightPanel::Friends, refresh, this.friends_raw.clone())
                }) else {
                    return;
                };
                if !current {
                    return;
                }
                if refresh || Instant::now() >= feed_at {
                    let (s, a, shown) = (sp.clone(), api.clone(), known.clone());
                    let result = cx
                        .background_executor()
                        .spawn(async move {
                            let feed = s.listening_feed()?;
                            let acts: Vec<Activity> = feed
                                .get("entities")
                                .and_then(|e| e.as_array())
                                .into_iter()
                                .flatten()
                                .filter_map(|e| {
                                    let e = e.get("userEntity").or_else(|| e.get("followEntity"))?;
                                    Activity::new(e.get("uri")?.as_str()?.strip_prefix("spotify:user:")?, e.get("activity")?)
                                })
                                .collect();
                            let names: Vec<String> = acts.iter().map(|a| a.username.clone()).collect();
                            anyhow::Ok((names, describe(&s, &a, acts, &shown)))
                        })
                        .await;
                    match result {
                        Ok((names, list)) => {
                            // Newly seen friends get subscribed on this pass.
                            if names.iter().any(|n| !followed.contains(n)) {
                                subscribed = None;
                            }
                            followed = names;
                            feed_at = Instant::now() + Duration::from_secs(if open { 60 } else { 1800 });
                            let _ = this.update(cx, |this, cx| this.set_friends(list, cx));
                        }
                        Err(e) => {
                            eprintln!("listening activity: {e:#}");
                            feed_at = Instant::now() + Duration::from_secs(60);
                        }
                    }
                }
                if !followed.is_empty() && Instant::now() >= subscribe_at {
                    let (s, names, prev) = (sp.clone(), followed.clone(), subscribed.clone());
                    let result = cx
                        .background_executor()
                        .spawn(async move {
                            let id = s.connection_id()?;
                            if prev.is_some_and(|(on, renew)| on == id && Instant::now() < renew) {
                                return anyhow::Ok(None);
                            }
                            let reply = s.subscribe_listening(&names, &id)?;
                            Ok(Some((id, reply)))
                        })
                        .await;
                    match result {
                        Ok(None) => {}
                        Ok(Some((id, reply))) => {
                            // Renewed a minute before it runs out, as the client does.
                            let now_ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0);
                            let left = reply.get("subscriptionExpirationTimestamp").and_then(unix_ms).map(|t| (t - now_ms - 60_000).max(60_000));
                            // A connection made again may have missed changes: ask for the feed too.
                            if subscribed.as_ref().is_some_and(|(on, _)| *on != id) {
                                feed_at = Instant::now();
                            }
                            subscribed = Some((id, Instant::now() + Duration::from_millis(left.unwrap_or(1_800_000) as u64)));
                        }
                        Err(e) => {
                            eprintln!("listening activity subscribe: {e:#}");
                            subscribe_at = Instant::now() + Duration::from_secs(60);
                        }
                    }
                }
                let pushed: Vec<Activity> = sp
                    .take_listening_pushes()
                    .iter()
                    .filter_map(|p| Activity::new(p.get("username")?.as_str()?, p.get("activity")?))
                    .collect();
                if !pushed.is_empty() {
                    let (s, a) = (sp.clone(), api.clone());
                    let got = cx.background_executor().spawn(async move { describe(&s, &a, pushed, &known) }).await;
                    let _ = this.update(cx, |this, cx| this.merge_friends(got, cx));
                }
                ticks += 1;
                if open && ticks % 30 == 0 {
                    // Keeps the "x min" times current.
                    let _ = this.update(cx, |_, cx| cx.notify());
                }
                cx.background_executor().timer(Duration::from_secs(2)).await;
            }
        })
        .detach();
    }

    /// The whole feed, as the service gave it.
    fn set_friends(&mut self, list: Vec<serde_json::Value>, cx: &mut Context<Self>) {
        self.friends_raw = list;
        self.friends = Some(parse_friends(&serde_json::json!({ "friends": self.friends_raw })));
        save_friend_cache(&self.friends_raw);
        cx.notify();
    }

    /// A friend's pushed activity replaces what they had (or adds them).
    fn merge_friends(&mut self, list: Vec<serde_json::Value>, cx: &mut Context<Self>) {
        let uri = |f: &serde_json::Value| f.pointer("/user/uri").and_then(|u| u.as_str()).unwrap_or_default().to_string();
        for f in list {
            let u = uri(&f);
            match self.friends_raw.iter_mut().find(|g| uri(g) == u) {
                Some(g) => *g = f,
                None => self.friends_raw.push(f),
            }
        }
        self.friends = Some(parse_friends(&serde_json::json!({ "friends": self.friends_raw })));
        save_friend_cache(&self.friends_raw);
        cx.notify();
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
                            .tooltip(native_tip(f.name.clone()))
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
                                        this.player(Play::ContextAt(ctx.clone(), track.clone()), cx);
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
                                    // Debug: SPOTIRUST_FRIEND_NOW=1 shows the first friend as listening now (captures).
                                    .child(if f.playing || (i == 0 && std::env::var_os("SPOTIRUST_FRIEND_NOW").is_some()) {
                                        // Listening now: the client's 12px animated equaliser.
                                        div()
                                            .id(gpui::ElementId::Name(format!("friend-now-{i}").into()))
                                            .h(px(16.8))
                                            .flex()
                                            .items_center()
                                            .tooltip(native_tip("now".to_string()))
                                            .child(img("images/equaliser-animated-green.gif").size(px(12.)))
                                            .into_any_element()
                                    } else {
                                        div()
                                            .text_size(px(12.))
                                            .line_height(px(16.8))
                                            .text_color(rgba(0xFFFFFFB3))
                                            .child(ago(f.timestamp))
                                            .into_any_element()
                                    }),
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
                                    .child(link(format!("friend-ctx-{i}"), f.context.clone(), f.context_link.clone(), cx)),
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
                            // Its X closes the panel (the header button switches back to Now Playing).
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.right_panel = RightPanel::None;
                                cx.notify();
                            }))
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
