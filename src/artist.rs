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
    /// "#N in the world" badge on the About card (0 = none).
    pub world_rank: u64,
    /// `goods.concerts.totalCount` (the "View all upcoming concerts (N)" count).
    pub concerts_total: usize,
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
    /// What the About dialog shows (also built from `queryNpvArtist` for the Now Playing view).
    pub about: About,
}

/// An artist's About dialog: gallery, stats, top cities and the biography (with its links).
#[derive(Clone, Default)]
pub struct About {
    pub uri: String,
    pub name: String,
    pub verified: bool,
    pub registered: bool,
    pub followers: u64,
    pub monthly_listeners: u64,
    /// Gallery images, largest rendition each (the About card shows the first).
    pub gallery: Vec<String>,
    /// Raw biography: paragraphs split by blank lines, `<a href="spotify:...">name</a>` links.
    pub bio: String,
    /// ("London, GB", listeners), most listeners first.
    pub cities: Vec<(String, u64)>,
}

impl About {
    /// The biography as plain text on one line (the cards' 3-line previews).
    pub fn bio_preview(&self) -> String {
        crate::home::plain_text(&self.bio).split_whitespace().collect::<Vec<_>>().join(" ")
    }
}

/// One credit line in the Now Playing view / Credits dialog.
#[derive(Clone)]
pub struct Contributor {
    pub name: String,
    /// `spotify:artist:...` when the contributor has a profile (Follow button, link).
    pub uri: Option<String>,
    /// "Main Artist", "Composer", ...
    pub role: String,
    /// "Artist", "Composition & Lyrics", "Production & Engineering", ...
    pub group: String,
}

/// `queryNpvArtist` for the playing track: its first artist and the track's credits.
#[derive(Clone)]
pub struct NpvInfo {
    pub track: String,
    pub about: About,
    pub contributors: Vec<Contributor>,
    /// Labels / sources ("Ample Play Recordings").
    pub sources: Vec<String>,
}

pub fn parse_about(a: &Value) -> About {
    let gallery = a
        .pointer("/visuals/gallery/items")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|it| {
                    it.get("sources")?
                        .as_array()?
                        .iter()
                        .max_by_key(|x| x.get("width").and_then(Value::as_u64).unwrap_or(0))
                        .and_then(|s| s.get("url")?.as_str())
                        .map(str::to_owned)
                })
                .collect()
        })
        .unwrap_or_default();
    let cities = a
        .pointer("/stats/topCities/items")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .map(|c| {
                    (
                        format!("{}, {}", str_at(c, "/city"), str_at(c, "/country")),
                        c.get("numberOfListeners").and_then(Value::as_u64).unwrap_or(0),
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    About {
        uri: str_at(a, "/uri").to_owned(),
        name: str_at(a, "/profile/name").to_owned(),
        verified: a.pointer("/onPlatformReputationTrait/verification/isVerified").and_then(Value::as_bool).unwrap_or(false),
        registered: a.pointer("/onPlatformReputationTrait/verification/isRegistered").and_then(Value::as_bool).unwrap_or(false),
        followers: a.pointer("/stats/followers").and_then(Value::as_u64).unwrap_or(0),
        monthly_listeners: a.pointer("/stats/monthlyListeners").and_then(Value::as_u64).unwrap_or(0),
        gallery,
        bio: str_at(a, "/profile/biography/text").to_owned(),
        cities,
    }
}

pub fn parse_npv(v: &Value, track: &str) -> Option<NpvInfo> {
    let a = v.pointer("/data/artistUnion")?;
    let t = v.pointer("/data/trackUnion");
    let contributors = t
        .and_then(|t| t.pointer("/creditsTrait/contributors/items"))
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .map(|c| Contributor {
                    name: str_at(c, "/name").to_owned(),
                    uri: c.get("uri").and_then(Value::as_str).filter(|u| !u.is_empty()).map(str::to_owned),
                    role: str_at(c, "/role").to_owned(),
                    group: str_at(c, "/roleGroup/name").to_owned(),
                })
                .collect()
        })
        .unwrap_or_default();
    let sources = t
        .and_then(|t| t.pointer("/creditsTrait/sources/items"))
        .and_then(Value::as_array)
        .map(|items| items.iter().map(|s| str_at(s, "/name").to_owned()).filter(|s| !s.is_empty()).collect())
        .unwrap_or_default();
    Some(NpvInfo { track: track.to_owned(), about: parse_about(a), contributors, sources })
}

/// The biography's paragraphs: each as plain text plus its links (byte range in that text, uri).
pub fn bio_paragraphs(raw: &str) -> Vec<(String, Vec<(std::ops::Range<usize>, String)>)> {
    let decode = |s: &str| {
        s.replace("&amp;", "&").replace("&quot;", "\"").replace("&#x27;", "'").replace("&#39;", "'").replace("&lt;", "<").replace("&gt;", ">")
    };
    let mut out = Vec::new();
    for para in raw.split("\n\n").map(str::trim).filter(|p| !p.is_empty()) {
        let mut text = String::new();
        let mut links = Vec::new();
        let mut rest = para;
        while let Some(i) = rest.find("<a ") {
            text.push_str(&decode(&rest[..i]));
            let tag = &rest[i..];
            let Some(close) = tag.find('>') else { break };
            let href = tag[..close].split("href=\"").nth(1).and_then(|h| h.split('"').next()).unwrap_or_default().to_owned();
            let after = &tag[close + 1..];
            let end = after.find("</a>").unwrap_or(after.len());
            let label = decode(&after[..end]);
            let start = text.len();
            text.push_str(&label);
            if !href.is_empty() {
                links.push((start..text.len(), href));
            }
            rest = after.get(end + 4..).unwrap_or_default();
        }
        text.push_str(&crate::home::plain_text(rest));
        out.push((text.replace('\n', " "), links));
    }
    out
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
                        plays: None,
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
        world_rank: a.pointer("/stats/worldRank").and_then(Value::as_u64).unwrap_or(0),
        concerts_total: a.pointer("/goods/concerts/totalCount").and_then(Value::as_u64).unwrap_or(0) as usize,
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
        about: parse_about(a),
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
