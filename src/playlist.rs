//! Playlist page data, parsed from pathfinder `fetchPlaylist` (the query the client runs).

use serde_json::Value;

#[derive(Clone, Copy, PartialEq)]
pub enum PageKind {
    Playlist,
    Album,
    Liked,
}

#[derive(Clone)]
pub struct PlaylistPage {
    pub kind: PageKind,
    pub uri: String,
    pub name: String,
    pub description: String,
    /// "Public Playlist" / "Private Playlist".
    pub label: &'static str,
    pub owner: String,
    pub owner_avatar: Option<String>,
    pub image: Option<String>,
    pub can_edit: bool,
    pub colors: HeaderColors,
    pub total: usize,
    pub tracks: Vec<Track>,
    /// Album: release year after the artist ("• 2012").
    pub year: Option<String>,
    /// Album footer: "November 22, 2012" and the copyright lines.
    pub release_date: Option<String>,
    pub copyrights: Vec<String>,
    /// The owner / artist link in the owner line.
    pub owner_uri: String,
    /// Album: "More by <artist>" releases.
    pub more_by: Vec<crate::home::Item>,
    /// Album: other editions ("1 more release").
    pub more_releases: usize,
    /// Liked Songs: the filter chips (`content-filter/v1/liked-songs`).
    pub filters: Vec<String>,
}

/// From `visualIdentity.squareCoverImage.extractedColorSet`.
#[derive(Clone, Copy)]
pub struct HeaderColors {
    /// Gradient start at the top of the header (`minContrast.backgroundBase`).
    pub top: u32,
    /// Header base and the body tint (`higherContrast.backgroundBase`).
    pub base: u32,
    /// "• 21 songs, 1 hr 2 min" (`higherContrast.textSubdued`).
    pub subdued: u32,
}

impl Default for HeaderColors {
    fn default() -> Self {
        Self { top: 0x535353, base: 0x535353, subdued: 0xB3B3B3 }
    }
}

#[derive(Clone)]
pub struct Track {
    pub uid: String,
    pub uri: String,
    pub name: String,
    pub artists: Vec<(String, String)>,
    pub album: String,
    pub album_uri: String,
    /// 64px album cover.
    pub image: Option<String>,
    pub duration_ms: u64,
    pub explicit: bool,
    pub playable: bool,
    /// Seconds since the Unix epoch.
    pub added_at: Option<i64>,
    /// Album pages number rows by track number and split them by disc.
    pub disc: u32,
    pub number: u32,
    /// Play count (discography tracklists show it), when known.
    pub plays: Option<u64>,
}

fn color(v: Option<&Value>) -> Option<u32> {
    let v = v?;
    let ch = |n: &str| v.get(n).and_then(Value::as_u64).map(|x| x as u32);
    Some(ch("red")? << 16 | ch("green")? << 8 | ch("blue")?)
}

fn smallest_source(sources: Option<&Value>) -> Option<String> {
    let arr = sources?.as_array()?;
    arr.iter()
        .min_by_key(|s| s.get("width").and_then(Value::as_u64).unwrap_or(u64::MAX))
        .and_then(|s| s.get("url")?.as_str())
        .map(str::to_owned)
}

fn largest_source(sources: Option<&Value>) -> Option<String> {
    let arr = sources?.as_array()?;
    arr.iter()
        .max_by_key(|s| s.get("width").and_then(Value::as_u64).unwrap_or(0))
        .and_then(|s| s.get("url")?.as_str())
        .map(str::to_owned)
}

fn str_at<'a>(v: &'a Value, ptr: &str) -> &'a str {
    v.pointer(ptr).and_then(Value::as_str).unwrap_or_default()
}

/// Header colours from `visualIdentity` (same shape on playlists and albums).
fn header_colors(p: &Value) -> HeaderColors {
    p.pointer("/visualIdentity/squareCoverImage/extractedColorSet")
        .and_then(|s| {
            Some(HeaderColors {
                top: color(s.pointer("/minContrast/backgroundBase"))?,
                base: color(s.pointer("/higherContrast/backgroundBase"))?,
                subdued: color(s.pointer("/higherContrast/textSubdued"))?,
            })
        })
        .unwrap_or_default()
}

/// Header + the first page of items.
pub fn parse(v: &Value) -> Option<PlaylistPage> {
    let p = v.pointer("/data/playlistV2")?;
    if p.get("__typename").and_then(Value::as_str) != Some("Playlist") {
        return None;
    }
    let colors = header_colors(p);
    let label = match p.get("basePermission").and_then(Value::as_str) {
        Some("BLOCKED") => "Private Playlist",
        _ => "Public Playlist",
    };
    Some(PlaylistPage {
        kind: PageKind::Playlist,
        year: None,
        release_date: None,
        copyrights: Vec::new(),
        owner_uri: str_at(p, "/ownerV2/data/uri").to_owned(),
        more_by: Vec::new(),
        more_releases: 0,
        filters: Vec::new(),
        uri: str_at(p, "/uri").to_owned(),
        name: str_at(p, "/name").to_owned(),
        description: str_at(p, "/description").to_owned(),
        label,
        owner: str_at(p, "/ownerV2/data/name").to_owned(),
        owner_avatar: smallest_source(p.pointer("/ownerV2/data/avatar/sources")),
        image: largest_source(p.pointer("/images/items/0/sources")),
        can_edit: p.pointer("/currentUserCapabilities/canEditItems").and_then(Value::as_bool).unwrap_or(false),
        colors,
        total: p.pointer("/content/totalCount").and_then(Value::as_u64).unwrap_or(0) as usize,
        tracks: parse_items(v),
    })
}

/// Track rows of one `fetchPlaylist` page (episodes and unavailable items are skipped).
pub fn parse_items(v: &Value) -> Vec<Track> {
    let Some(items) = v.pointer("/data/playlistV2/content/items").and_then(Value::as_array) else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|it| {
            let t = it.pointer("/itemV2/data")?;
            if t.get("__typename")?.as_str()? != "Track" {
                return None;
            }
            let artists = t
                .pointer("/artists/items")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .map(|x| (str_at(x, "/profile/name").to_owned(), str_at(x, "/uri").to_owned()))
                        .collect()
                })
                .unwrap_or_default();
            Some(Track {
                uid: str_at(it, "/uid").to_owned(),
                uri: str_at(t, "/uri").to_owned(),
                name: str_at(t, "/name").to_owned(),
                artists,
                album: str_at(t, "/albumOfTrack/name").to_owned(),
                album_uri: str_at(t, "/albumOfTrack/uri").to_owned(),
                image: smallest_source(t.pointer("/albumOfTrack/coverArt/sources")),
                duration_ms: t.pointer("/trackDuration/totalMilliseconds").and_then(Value::as_u64).unwrap_or(0),
                explicit: str_at(t, "/contentRating/label") == "EXPLICIT",
                playable: t.pointer("/playability/playable").and_then(Value::as_bool).unwrap_or(true),
                added_at: it.pointer("/addedAt/isoString").and_then(Value::as_str).and_then(parse_iso),
                disc: 1,
                number: 0,
                plays: None,
            })
        })
        .collect()
}

/// "1 hr 2 min" / "about 5 hr" / "42 min 7 sec", as the header shows the total length.
pub fn total_duration(ms: u64) -> String {
    let secs = ms / 1000;
    let (h, m, s) = (secs / 3600, secs / 60 % 60, secs % 60);
    if h > 0 {
        format!("{h} hr {m} min")
    } else {
        format!("{m} min {s} sec")
    }
}

/// "Feb 14, 2025", or "N days ago" style for the last month, like the Date added column.
pub fn date_added(at: i64, now: i64) -> String {
    let d = (now - at).max(0);
    let plural = |n: i64, unit: &str| format!("{n} {unit}{} ago", if n == 1 { "" } else { "s" });
    if d < 60 {
        plural(d, "second")
    } else if d < 3600 {
        plural(d / 60, "minute")
    } else if d < 86_400 {
        plural(d / 3600, "hour")
    } else if d < 7 * 86_400 {
        plural(d / 86_400, "day")
    } else if d < 28 * 86_400 {
        plural(d / (7 * 86_400), "week")
    } else {
        let (y, mo, day) = civil(at.div_euclid(86_400));
        const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
        format!("{} {day}, {y}", MONTHS[(mo - 1) as usize])
    }
}

/// `2025-02-14T17:38:42Z` -> Unix seconds.
fn parse_iso(s: &str) -> Option<i64> {
    let n = |r: std::ops::Range<usize>| s.get(r)?.parse::<i64>().ok();
    let (y, mo, d) = (n(0..4)?, n(5..7)?, n(8..10)?);
    let (h, mi, se) = (n(11..13).unwrap_or(0), n(14..16).unwrap_or(0), n(17..19).unwrap_or(0));
    Some(days_from_civil(y, mo, d) * 86_400 + h * 3600 + mi * 60 + se)
}

// Howard Hinnant's civil date algorithms.
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn civil(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + if m <= 2 { 1 } else { 0 }, m, d)
}

pub fn now_unix() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

const MONTHS_LONG: [&str; 12] = [
    "January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December",
];

fn artists_of(v: Option<&Value>) -> Vec<(String, String)> {
    v.and_then(Value::as_array)
        .map(|a| a.iter().map(|x| (str_at(x, "/profile/name").to_owned(), str_at(x, "/uri").to_owned())).collect())
        .unwrap_or_default()
}

/// `getAlbum`: header, artist, colours, footer and the first page of tracks.
pub fn parse_album(v: &Value) -> Option<PlaylistPage> {
    let a = v.pointer("/data/albumUnion")?;
    if a.get("__typename").and_then(Value::as_str) != Some("Album") {
        return None;
    }
    let artist = a.pointer("/artists/items/0");
    let label = match a.get("type").and_then(Value::as_str) {
        Some("SINGLE") => "Single",
        Some("EP") => "EP",
        Some("COMPILATION") => "Compilation",
        _ => "Album",
    };
    let date = a.pointer("/date/isoString").and_then(Value::as_str).and_then(parse_iso);
    let precision = str_at(a, "/date/precision");
    let year = a
        .pointer("/date/year")
        .and_then(Value::as_u64)
        .map(|y| y.to_string())
        .or_else(|| date.map(|d| civil(d.div_euclid(86_400)).0.to_string()));
    let release_date = date.map(|d| {
        let (y, m, day) = civil(d.div_euclid(86_400));
        match precision {
            "YEAR" => y.to_string(),
            _ => format!("{} {day}, {y}", MONTHS_LONG[(m - 1) as usize]),
        }
    });
    let copyrights = a
        .pointer("/copyright/items")
        .and_then(Value::as_array)
        .map(|c| {
            c.iter()
                .map(|x| {
                    let text = str_at(x, "/text");
                    match str_at(x, "/type") {
                        "P" if !text.starts_with('\u{2117}') => format!("\u{2117} {text}"),
                        "C" if !text.starts_with('\u{a9}') => format!("\u{a9} {text}"),
                        _ => text.to_owned(),
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    let album_uri = str_at(a, "/uri");
    let more_by = a
        .pointer("/moreAlbumsByArtist/items/0/discography/popularReleasesAlbums/items")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter(|x| str_at(x, "/uri") != album_uri)
                .map(|x| crate::home::Item {
                    uri: str_at(x, "/uri").to_owned(),
                    kind: crate::home::Kind::Album,
                    name: str_at(x, "/name").to_owned(),
                    subtitle: x.pointer("/date/year").and_then(Value::as_u64).map(|y| y.to_string()).unwrap_or_default(),
                    image: x
                        .pointer("/coverArt/sources")
                        .and_then(Value::as_array)
                        .and_then(|s| s.iter().find(|i| i.get("width").and_then(Value::as_u64) == Some(300)).or(s.first()))
                        .and_then(|i| i.get("url")?.as_str())
                        .map(str::to_owned),
                    color: None,
                    hide_title: false,
                })
                .collect()
        })
        .unwrap_or_default();
    let colors = header_colors(a);
    Some(PlaylistPage {
        kind: PageKind::Album,
        uri: album_uri.to_owned(),
        name: str_at(a, "/name").to_owned(),
        description: String::new(),
        label,
        owner: artist.map(|x| str_at(x, "/profile/name").to_owned()).unwrap_or_default(),
        owner_avatar: artist.and_then(|x| smallest_source(x.pointer("/visuals/avatarImage/sources"))),
        owner_uri: artist.map(|x| str_at(x, "/uri").to_owned()).unwrap_or_default(),
        image: largest_source(a.pointer("/coverArt/sources")),
        can_edit: false,
        colors,
        total: a
            .pointer("/tracksV2/totalCount")
            .or(a.pointer("/tracks/totalCount"))
            .and_then(Value::as_u64)
            .unwrap_or(0) as usize,
        tracks: parse_album_items(v),
        year,
        release_date,
        copyrights,
        more_by,
        more_releases: a.pointer("/releases/totalCount").and_then(Value::as_u64).unwrap_or(0) as usize,
        filters: Vec::new(),
    })
}

/// Track rows of one `getAlbum` page.
pub fn parse_album_items(v: &Value) -> Vec<Track> {
    let Some(a) = v.pointer("/data/albumUnion") else { return Vec::new() };
    let image = smallest_source(a.pointer("/coverArt/sources"));
    let (album, album_uri) = (str_at(a, "/name").to_owned(), str_at(a, "/uri").to_owned());
    let Some(items) = a.pointer("/tracksV2/items").or(a.pointer("/tracks/items")).and_then(Value::as_array) else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|it| {
            let t = it.get("track")?;
            Some(Track {
                uid: str_at(it, "/uid").to_owned(),
                uri: str_at(t, "/uri").to_owned(),
                name: str_at(t, "/name").to_owned(),
                artists: artists_of(t.pointer("/artists/items")),
                album: album.clone(),
                album_uri: album_uri.clone(),
                image: image.clone(),
                duration_ms: t.pointer("/duration/totalMilliseconds").and_then(Value::as_u64).unwrap_or(0),
                explicit: str_at(t, "/contentRating/label") == "EXPLICIT",
                playable: t.pointer("/playability/playable").and_then(Value::as_bool).unwrap_or(true),
                added_at: None,
                disc: t.get("discNumber").and_then(Value::as_u64).unwrap_or(1) as u32,
                number: t.get("trackNumber").and_then(Value::as_u64).unwrap_or(0) as u32,
                plays: None,
            })
        })
        .collect()
}

/// Liked Songs from the Web API's saved tracks (newest first), all pages.
pub fn liked_page(api: &crate::api::Api, me: &crate::api::Me) -> anyhow::Result<PlaylistPage> {
    let mut tracks = Vec::new();
    let mut total = 0;
    let mut next = Some("/me/tracks?limit=50".to_string());
    while let Some(url) = next {
        let page: Value = api.get(&url)?;
        total = page.get("total").and_then(Value::as_u64).unwrap_or(0) as usize;
        for it in page.get("items").and_then(Value::as_array).into_iter().flatten() {
            let Some(t) = it.get("track") else { continue };
            let image = t
                .pointer("/album/images")
                .and_then(Value::as_array)
                .and_then(|i| i.iter().min_by_key(|x| x.get("width").and_then(Value::as_u64).unwrap_or(u64::MAX)))
                .and_then(|x| x.get("url")?.as_str())
                .map(str::to_owned);
            tracks.push(Track {
                uid: String::new(),
                uri: str_at(t, "/uri").to_owned(),
                name: str_at(t, "/name").to_owned(),
                artists: t
                    .get("artists")
                    .and_then(Value::as_array)
                    .map(|a| a.iter().map(|x| (str_at(x, "/name").to_owned(), str_at(x, "/uri").to_owned())).collect())
                    .unwrap_or_default(),
                album: str_at(t, "/album/name").to_owned(),
                album_uri: str_at(t, "/album/uri").to_owned(),
                image,
                duration_ms: t.get("duration_ms").and_then(Value::as_u64).unwrap_or(0),
                explicit: t.get("explicit").and_then(Value::as_bool).unwrap_or(false),
                playable: t.get("is_playable").and_then(Value::as_bool).unwrap_or(true),
                added_at: it.get("added_at").and_then(Value::as_str).and_then(parse_iso),
                disc: 1,
                number: 0,
                plays: None,
            });
        }
        next = page.get("next").and_then(Value::as_str).map(str::to_owned);
    }
    let me_uri = format!("spotify:user:{}", me.id);
    Ok(PlaylistPage {
        kind: PageKind::Liked,
        uri: format!("{me_uri}:collection"),
        name: "Liked Songs".into(),
        description: String::new(),
        label: "Playlist",
        owner: me.display_name.clone().unwrap_or_else(|| me.id.clone()),
        owner_avatar: me.images.iter().min_by_key(|i| i.width.unwrap_or(u32::MAX)).map(|i| i.url.clone()),
        owner_uri: me_uri,
        image: None,
        can_edit: false,
        colors: HeaderColors { top: 0x5038A0, base: 0x5038A0, subdued: 0xB3B3B3 },
        total,
        tracks,
        year: None,
        release_date: None,
        copyrights: Vec::new(),
        more_by: Vec::new(),
        more_releases: 0,
        filters: Vec::new(),
    })
}

/// Chip labels from `content-filter/v1/liked-songs`: every `title` in the filter list, in order.
pub fn parse_filters(v: &Value) -> Vec<String> {
    fn walk(v: &Value, out: &mut Vec<String>) {
        match v {
            Value::Object(m) => {
                if let Some(t) = m.get("title").and_then(Value::as_str) {
                    out.push(t.to_owned());
                }
                m.values().for_each(|x| walk(x, out));
            }
            Value::Array(a) => a.iter().for_each(|x| walk(x, out)),
            _ => {}
        }
    }
    let mut out = Vec::new();
    walk(v, &mut out);
    out
}
