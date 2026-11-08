//! Your Library, parsed from the pathfinder `libraryV3` response (Recents order, pins, types).

use serde_json::Value;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LibKind {
    LikedSongs,
    Playlist,
    Album,
    Artist,
    Folder,
    Show,
}

#[derive(Clone, Debug)]
pub struct LibItem {
    pub uri: String,
    pub kind: LibKind,
    pub name: String,
    /// Text after the type, e.g. the owner in "Playlist • abdullah".
    pub owner: String,
    pub image: Option<String>,
    pub pinned: bool,
    /// "Mixed playlist" (MIX_STATE extension), filled in separately.
    pub mixed: bool,
}

impl LibItem {
    /// The row's second line, exactly as the client words it.
    pub fn type_label(&self) -> &'static str {
        match self.kind {
            LibKind::LikedSongs | LibKind::Playlist if self.mixed => "Mixed playlist",
            LibKind::LikedSongs | LibKind::Playlist => "Playlist",
            LibKind::Album => "Album",
            LibKind::Artist => "Artist",
            LibKind::Folder => "Folder",
            LibKind::Show => "Podcast",
        }
    }
}

fn s<'a>(v: &'a Value, ptr: &str) -> Option<&'a str> {
    v.pointer(ptr)?.as_str()
}

/// Smallest source at least `min` px wide; sources without a width count as large.
fn pick(sources: Option<&Value>, min: u64) -> Option<String> {
    let arr = sources?.as_array()?;
    let w = |x: &Value| x.get("width").and_then(Value::as_u64).unwrap_or(u64::MAX);
    arr.iter()
        .filter(|x| w(x) >= min)
        .min_by_key(|x| w(x))
        .or_else(|| arr.first())
        .and_then(|x| x.get("url")?.as_str().map(str::to_owned))
}

pub fn parse(v: &Value, me_name: &str) -> Vec<LibItem> {
    let items = v.pointer("/data/me/libraryV3/items").and_then(Value::as_array);
    let mut out = Vec::new();
    for it in items.into_iter().flatten() {
        let pinned = it.get("pinned").and_then(Value::as_bool).unwrap_or(false);
        let wrapper = it.get("item").unwrap_or(&Value::Null);
        let uri = s(wrapper, "/_uri").unwrap_or_default().to_string();
        let d = wrapper.get("data").unwrap_or(&Value::Null);
        let typename = s(d, "/__typename").unwrap_or("");
        let name = s(d, "/name").unwrap_or_default().to_string();
        let item = match typename {
            "PseudoPlaylist" if uri.ends_with(":collection:tracks") => LibItem {
                uri,
                kind: LibKind::LikedSongs,
                name,
                owner: me_name.to_string(),
                image: pick(d.pointer("/image/sources"), 120),
                pinned,
                mixed: false,
            },
            "Playlist" => LibItem {
                uri,
                kind: LibKind::Playlist,
                name,
                owner: s(d, "/ownerV2/data/name").unwrap_or_default().to_string(),
                image: pick(d.pointer("/images/items/0/sources"), 120),
                pinned,
                mixed: false,
            },
            "Album" => LibItem {
                uri,
                kind: LibKind::Album,
                name,
                owner: d
                    .pointer("/artists/items")
                    .and_then(Value::as_array)
                    .map(|a| a.iter().filter_map(|x| s(x, "/profile/name")).collect::<Vec<_>>().join(", "))
                    .unwrap_or_default(),
                image: pick(d.pointer("/coverArt/sources"), 120),
                pinned,
                mixed: false,
            },
            "Artist" => LibItem {
                uri,
                kind: LibKind::Artist,
                name: s(d, "/profile/name").unwrap_or_default().to_string(),
                owner: String::new(),
                image: pick(d.pointer("/visuals/avatarImage/sources"), 120),
                pinned,
                mixed: false,
            },
            "Folder" => LibItem {
                uri,
                kind: LibKind::Folder,
                name,
                owner: String::new(),
                image: None,
                pinned,
                mixed: false,
            },
            "Podcast" => LibItem {
                uri,
                kind: LibKind::Show,
                name,
                owner: s(d, "/publisher/name").unwrap_or_default().to_string(),
                image: pick(d.pointer("/coverArt/sources"), 120),
                pinned,
                mixed: false,
            },
            _ => continue,
        };
        out.push(item);
    }
    out
}
