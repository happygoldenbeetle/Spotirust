//! Home feed model, parsed from the pathfinder `home` response.

use serde_json::Value;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    Playlist,
    Album,
    Artist,
    /// A user profile (round art, no play button).
    User,
    /// An audiobook author (round art, a book when there's no picture, no play button).
    Author,
    LikedSongs,
    Other,
}

#[derive(Clone, Debug)]
pub struct Item {
    pub uri: String,
    pub kind: Kind,
    pub name: String,
    pub subtitle: String,
    pub image: Option<String>,
    /// `extractedColors.colorDark`, drives the Home header tint on hover.
    pub color: Option<u32>,
    /// Spotify-made mixes/stations carry their name in the artwork; the card hides the title.
    pub hide_title: bool,
}

#[derive(Clone, Debug)]
pub struct Shelf {
    pub title: String,
    pub subtitle: Option<String>,
    pub items: Vec<Item>,
}

#[derive(Clone, Debug, Default)]
pub struct HomeFeed {
    pub greeting: String,
    pub chips: Vec<(String, String)>, // (id, label)
    pub shortcuts: Vec<Item>,
    pub shelves: Vec<Shelf>,
}

fn s<'a>(v: &'a Value, path: &[&str]) -> Option<&'a str> {
    let mut cur = v;
    for p in path {
        cur = cur.get(p)?;
    }
    cur.as_str()
}

fn label(v: &Value) -> Option<String> {
    s(v, &["transformedLabel"]).or_else(|| s(v, &["text"])).map(str::to_owned)
}

/// Picks a source at least `min` px wide (falls back to the largest / first).
fn pick_source(sources: &Value, min: u64) -> Option<String> {
    let arr = sources.as_array()?;
    let width = |x: &Value| x.get("width").and_then(Value::as_u64);
    arr.iter()
        .filter(|x| width(x).map_or(true, |w| w >= min))
        .min_by_key(|x| width(x).unwrap_or(u64::MAX))
        .or_else(|| arr.first())
        .and_then(|x| x.get("url")?.as_str().map(str::to_owned))
}

fn hex(v: Option<&str>) -> Option<u32> {
    u32::from_str_radix(v?.trim_start_matches('#'), 16).ok()
}

/// Playlist descriptions are HTML-ish (links, entities).
pub fn plain_text(text: &str) -> String {
    plain(text)
}

fn plain(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_tag = false;
    for ch in text.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            c if !in_tag => out.push(c),
            _ => {}
        }
    }
    out.replace("&amp;", "&").replace("&quot;", "\"").replace("&#x27;", "'").replace("&#39;", "'")
        .replace("&lt;", "<").replace("&gt;", ">")
}

fn item(entry: &Value) -> Option<Item> {
    let uri = entry.get("uri")?.as_str()?.to_string();
    let content = entry.get("content")?;
    let data = content.get("data").unwrap_or(&Value::Null);
    let typename = content.get("__typename").and_then(Value::as_str).unwrap_or("");

    if uri.ends_with(":collection") {
        return Some(Item {
            uri,
            kind: Kind::LikedSongs,
            name: "Liked Songs".into(),
            subtitle: String::new(),
            image: None,
            // The reference's header background-color while Liked Songs is hovered.
            color: Some(0x7860E8),
            hide_title: false,
        });
    }

    match typename {
        "PlaylistResponseWrapper" => {
            let image = data.pointer("/images/items/0");
            let format = s(data, &["format"]).unwrap_or("");
            Some(Item {
                uri,
                kind: Kind::Playlist,
                name: s(data, &["name"])?.to_string(),
                subtitle: plain(s(data, &["description"]).unwrap_or("")),
                image: image.and_then(|i| pick_source(i.get("sources")?, 300)),
                color: hex(image.and_then(|i| s(i, &["extractedColors", "colorDark", "hex"]))),
                hide_title: matches!(format, "inspiredby-mix" | "daily-mix" | "artist-mix-reader" | "topic-mix"),
            })
        }
        "AlbumResponseWrapper" => {
            let artists = data
                .pointer("/artists/items")
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(|x| s(x, &["profile", "name"])).collect::<Vec<_>>().join(", "))
                .unwrap_or_default();
            let cover = data.get("coverArt");
            Some(Item {
                uri,
                kind: Kind::Album,
                name: s(data, &["name"])?.to_string(),
                subtitle: artists,
                image: cover.and_then(|c| pick_source(c.get("sources")?, 300)),
                color: hex(cover.and_then(|c| s(c, &["extractedColors", "colorDark", "hex"]))),
                hide_title: false,
            })
        }
        "ArtistResponseWrapper" => {
            let avatar = data.pointer("/visuals/avatarImage");
            Some(Item {
                uri,
                kind: Kind::Artist,
                name: s(data, &["profile", "name"])?.to_string(),
                subtitle: "Artist".into(),
                image: avatar.and_then(|a| pick_source(a.get("sources")?, 300)),
                color: hex(avatar.and_then(|a| s(a, &["extractedColors", "colorDark", "hex"]))),
                hide_title: false,
            })
        }
        _ => s(data, &["name"]).map(|name| Item {
            uri,
            kind: Kind::Other,
            name: name.to_string(),
            subtitle: String::new(),
            image: None,
            color: None,
            hide_title: false,
        }),
    }
}

pub fn parse(v: &Value) -> HomeFeed {
    let home = v.pointer("/data/home").unwrap_or(&Value::Null);
    let mut feed = HomeFeed {
        greeting: home.get("greeting").and_then(label).unwrap_or_default(),
        chips: vec![("".into(), "All".into())],
        ..Default::default()
    };
    if let Some(chips) = home.get("homeChips").and_then(Value::as_array) {
        for c in chips {
            if let (Some(id), Some(l)) = (s(c, &["id"]), c.get("label").and_then(label)) {
                feed.chips.push((id.to_string(), l));
            }
        }
    }
    let sections = home.pointer("/sectionContainer/sections/items").and_then(Value::as_array);
    for sec in sections.into_iter().flatten() {
        let typename = s(sec, &["data", "__typename"]).unwrap_or("");
        let items: Vec<Item> = sec
            .pointer("/sectionItems/items")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(item).collect())
            .unwrap_or_default();
        if typename == "HomeShortsSectionData" {
            feed.shortcuts = items;
            continue;
        }
        let Some(mut title) = sec.pointer("/data/title").and_then(label) else { continue };
        if items.is_empty() {
            continue;
        }
        let mut subtitle = sec.pointer("/data/subtitle").and_then(label).filter(|s| !s.is_empty());
        // "Made For {0}": the client shows the prefix as the small line and the name as the title.
        if let Some(base) = s(sec, &["data", "title", "translatedBaseText"]) {
            if let Some(prefix) = base.strip_suffix("{0}").map(str::trim).filter(|p| !p.is_empty()) {
                if let Some(name) = title.strip_prefix(prefix) {
                    title = name.trim().to_string();
                    subtitle = Some(prefix.to_string());
                }
            }
        }
        feed.shelves.push(Shelf { title, subtitle, items });
    }
    feed
}
