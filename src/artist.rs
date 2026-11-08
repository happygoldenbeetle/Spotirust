//! Artist page data, parsed from pathfinder `queryArtistOverview` (the query the client runs).

use serde_json::Value;

use crate::{
    home::{Item, Kind},
    playlist::Track,
};

#[derive(Clone)]
pub struct ArtistPage {
    pub uri: String,
    pub name: String,
    pub verified: bool,
    pub monthly_listeners: u64,
    /// Wide header image (the client loads the `0194` rendition).
    pub header_image: Option<String>,
    pub avatar: Option<String>,
    /// `wideFullBleedImage` `higherContrast.backgroundBase`: action-bar tint and overscroll colour.
    pub color: u32,
    pub top_tracks: Vec<Track>,
    pub pinned: Option<Pinned>,
    /// Discography filters: popular releases (latest first), albums, singles and EPs, compilations.
    pub discography: [Vec<Item>; 4],
    pub featuring: Vec<Item>,
    pub concerts: Vec<Concert>,
    pub about_image: Option<String>,
    pub biography: String,
    pub discovered_on: Vec<Item>,
    pub related: Vec<Item>,
    pub appears_on: Vec<Item>,
    pub saved: bool,
}

#[derive(Clone)]
pub struct Pinned {
    pub comment: String,
    pub title: String,
    /// "Single", "Playlist", ...: the type part of the client's "Single • New Release".
    pub subtitle: String,
    pub uri: String,
    pub image: Option<String>,
}

#[derive(Clone)]
pub struct Concert {
    pub uri: String,
    pub title: String,
    pub city: String,
    pub venue: String,
    /// `2026-11-22T18:00+05:30`
    pub start: String,
}

fn str_at<'a>(v: &'a Value, ptr: &str) -> &'a str {
    v.pointer(ptr).and_then(Value::as_str).unwrap_or_default()
}

fn rgb(v: Option<&Value>) -> Option<u32> {
    let v = v?;
    let ch = |n: &str| v.get(n).and_then(Value::as_u64).map(|x| x as u32);
    Some(ch("red")? << 16 | ch("green")? << 8 | ch("blue")?)
}

/// The source closest to 300px wide (unknown widths count as the first listed).
fn source_300(sources: Option<&Value>) -> Option<String> {
    let arr = sources?.as_array()?;
    arr.iter()
        .find(|s| s.get("width").and_then(Value::as_u64) == Some(300))
        .or_else(|| arr.iter().find(|s| str_at(s, "/url").contains("ab67616d00001e02")))
        .or_else(|| arr.iter().max_by_key(|s| s.get("width").and_then(Value::as_u64).unwrap_or(0)))
        .and_then(|s| s.get("url")?.as_str())
        .map(str::to_owned)
}

fn type_label(t: &str) -> &'static str {
    match t {
        "SINGLE" => "Single",
        "EP" => "EP",
        "COMPILATION" => "Compilation",
        _ => "Album",
    }
}

/// A release card: "2023 • Single" (or "Latest Release • Single").
fn release(r: &Value, latest: bool) -> Item {
    let year = r.pointer("/date/year").and_then(Value::as_u64).map(|y| y.to_string()).unwrap_or_default();
    let kind = type_label(str_at(r, "/type"));
    Item {
        uri: str_at(r, "/uri").to_owned(),
        kind: Kind::Album,
        name: str_at(r, "/name").to_owned(),
        subtitle: format!("{} \u{2022} {kind}", if latest { "Latest Release" } else { &year }),
        image: source_300(r.pointer("/coverArt/sources")),
        color: None,
        hide_title: false,
    }
}

fn releases(v: Option<&Value>) -> Vec<Item> {
    v.and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|x| x.pointer("/releases/items/0")).map(|r| release(r, false)).collect())
        .unwrap_or_default()
}

fn playlist_item(d: &Value) -> Option<Item> {
    if d.get("__typename").and_then(Value::as_str) != Some("Playlist") {
        return None;
    }
    Some(Item {
        uri: str_at(d, "/uri").to_owned(),
        kind: Kind::Playlist,
        name: str_at(d, "/name").to_owned(),
        subtitle: str_at(d, "/description").to_owned(),
        image: d
            .pointer("/images/items/0/sources")
            .and_then(Value::as_array)
            .and_then(|s| s.first())
            .and_then(|s| s.get("url")?.as_str())
            .map(str::to_owned),
        color: None,
        hide_title: false,
    })
}

fn artist_item(a: &Value) -> Item {
    Item {
        uri: str_at(a, "/uri").to_owned(),
        kind: Kind::Artist,
        name: str_at(a, "/profile/name").to_owned(),
        subtitle: "Artist".into(),
        image: a
            .pointer("/visuals/avatarImage/sources")
            .and_then(Value::as_array)
            .and_then(|s| s.iter().find(|x| x.get("width").and_then(Value::as_u64) == Some(320)).or(s.first()))
            .and_then(|s| s.get("url")?.as_str())
            .map(str::to_owned),
        color: None,
        hide_title: false,
    }
}

/// Swaps the size code of an `/image/<prefix 12><code 4><id>` URL.
fn rendition(url: &str, code: &str) -> String {
    match url.find("/image/") {
        Some(p) if url.len() >= p + 7 + 16 => format!("{}{code}{}", &url[..p + 7 + 12], &url[p + 7 + 16..]),
        _ => url.to_string(),
    }
}

pub fn parse(v: &Value) -> Option<ArtistPage> {
    let a = v.pointer("/data/artistUnion")?;
    if a.get("__typename").and_then(Value::as_str) != Some("Artist") {
        return None;
    }
    let top_tracks = a
        .pointer("/discography/topTracks/items")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|it| {
                    let t = it.get("track")?;
                    Some(Track {
                        uid: str_at(it, "/uid").to_owned(),
                        uri: str_at(t, "/uri").to_owned(),
                        name: str_at(t, "/name").to_owned(),
                        artists: t
                            .pointer("/artists/items")
                            .and_then(Value::as_array)
                            .map(|a| a.iter().map(|x| (str_at(x, "/profile/name").to_owned(), str_at(x, "/uri").to_owned())).collect())
                            .unwrap_or_default(),
                        album: String::new(),
                        album_uri: str_at(t, "/albumOfTrack/uri").to_owned(),
                        image: source_300(t.pointer("/albumOfTrack/coverArt/sources")),
                        duration_ms: t.pointer("/duration/totalMilliseconds").and_then(Value::as_u64).unwrap_or(0),
                        explicit: str_at(t, "/contentRating/label") == "EXPLICIT",
                        playable: t.pointer("/playability/playable").and_then(Value::as_bool).unwrap_or(true),
                        added_at: None,
                        disc: 1,
                        number: 0,
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    let pinned = a.pointer("/profile/pinnedItem").filter(|p| !p.is_null()).map(|p| Pinned {
        comment: str_at(p, "/comment").to_owned(),
        title: str_at(p, "/title").to_owned(),
        subtitle: str_at(p, "/subtitle").split(" \u{2022} ").next().unwrap_or_default().to_owned(),
        uri: str_at(p, "/uri").to_owned(),
        image: source_300(p.pointer("/thumbnailImage/data/sources"))
            .or_else(|| source_300(p.pointer("/itemV2/data/coverArt/sources"))),
    });
    let mut popular: Vec<Item> = Vec::new();
    if let Some(latest) = a.pointer("/discography/latest").filter(|l| !l.is_null()) {
        popular.push(release(latest, true));
    }
    for r in a.pointer("/discography/popularReleasesAlbums/items").and_then(Value::as_array).into_iter().flatten() {
        if !popular.iter().any(|p| p.uri == str_at(r, "/uri")) {
            popular.push(release(r, false));
        }
    }
    let concerts = a
        .pointer("/goods/concerts/items")
        .and_then(Value::as_array)
        .map(|c| {
            c.iter()
                .filter_map(|x| x.get("data"))
                .map(|d| Concert {
                    uri: str_at(d, "/uri").to_owned(),
                    title: str_at(d, "/title").to_owned(),
                    city: str_at(d, "/location/city").to_owned(),
                    venue: str_at(d, "/location/name").to_owned(),
                    start: str_at(d, "/startDateIsoString").to_owned(),
                })
                .collect()
        })
        .unwrap_or_default();
    let header_image = a
        .pointer("/headerImage/data/sources/0/url")
        .and_then(Value::as_str)
        .map(|u| rendition(u, "0194"));
    Some(ArtistPage {
        uri: str_at(a, "/uri").to_owned(),
        name: str_at(a, "/profile/name").to_owned(),
        verified: a.pointer("/onPlatformReputationTrait/verification/isVerified").and_then(Value::as_bool).unwrap_or(false),
        monthly_listeners: a.pointer("/stats/monthlyListeners").and_then(Value::as_u64).unwrap_or(0),
        header_image,
        avatar: artist_item(a).image,
        color: rgb(a.pointer("/visualIdentity/wideFullBleedImage/extractedColorSet/higherContrast/backgroundBase"))
            .unwrap_or(0x535353),
        top_tracks,
        pinned,
        discography: [
            popular,
            releases(a.pointer("/discography/albums/items")),
            releases(a.pointer("/discography/singles/items")),
            releases(a.pointer("/discography/compilations/items")),
        ],
        featuring: a
            .pointer("/relatedContent/featuringV2/items")
            .and_then(Value::as_array)
            .map(|i| i.iter().filter_map(|x| playlist_item(x.get("data")?)).collect())
            .unwrap_or_default(),
        concerts,
        about_image: a
            .pointer("/visuals/gallery/items/0/sources")
            .and_then(Value::as_array)
            .and_then(|s| s.iter().max_by_key(|x| x.get("width").and_then(Value::as_u64).unwrap_or(0)))
            .and_then(|s| s.get("url")?.as_str())
            .map(str::to_owned),
        biography: crate::home::plain_text(str_at(a, "/profile/biography/text")),
        discovered_on: a
            .pointer("/relatedContent/discoveredOnV2/items")
            .and_then(Value::as_array)
            .map(|i| i.iter().filter_map(|x| playlist_item(x.get("data")?)).collect())
            .unwrap_or_default(),
        related: a
            .pointer("/relatedContent/relatedArtists/items")
            .and_then(Value::as_array)
            .map(|i| i.iter().map(artist_item).collect())
            .unwrap_or_default(),
        appears_on: releases(a.pointer("/relatedContent/appearsOn/items")),
        saved: a.get("saved").and_then(Value::as_bool).unwrap_or(false),
    })
}

/// "14.2M" / "980K" / "1,234": the header's monthly listeners. The header uses the
/// compact form; the About card spells the number out with grouping.
pub fn compact(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1e6).replace(".0M", "M")
    } else if n >= 1_000 {
        format!("{:.1}K", n as f64 / 1e3).replace(".0K", "K")
    } else {
        n.to_string()
    }
}

pub fn grouped(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, ch) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out
}
