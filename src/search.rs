//! Search model, parsed from pathfinder's `searchSuggestions`, `searchTopResultsList`,
//! `search<Kind>` and `recentSearches` responses (they share one hit shape).

use serde_json::Value;

use crate::home::{Item, Kind};
use crate::playlist::Track;

/// A filter chip of the results page (`/search/<query>/<path>`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Filter {
    All,
    Tracks,
    Albums,
    Artists,
    Playlists,
    Users,
    Audiobooks,
    Authors,
}

impl Filter {
    pub fn label(self) -> &'static str {
        match self {
            Self::All => "All",
            Self::Tracks => "Songs",
            Self::Albums => "Albums",
            Self::Artists => "Artists",
            Self::Playlists => "Playlists",
            Self::Users => "Profiles",
            Self::Audiobooks => "Audiobooks",
            Self::Authors => "Authors",
        }
    }

    /// The route segment, which is also the key of `Spotify::search_kind`.
    pub fn path(self) -> &'static str {
        match self {
            Self::All => "",
            Self::Tracks => "tracks",
            Self::Albums => "albums",
            Self::Artists => "artists",
            Self::Playlists => "playlists",
            Self::Users => "users",
            Self::Audiobooks => "audiobooks",
            Self::Authors => "authors",
        }
    }

    /// Where the response keeps this filter's list.
    fn field(self) -> &'static str {
        match self {
            Self::All => "topResultsV2",
            Self::Tracks => "tracksV2",
            Self::Albums => "albumsV2",
            Self::Artists => "artists",
            Self::Playlists => "playlists",
            Self::Users => "users",
            Self::Audiobooks => "audiobooks",
            Self::Authors => "authors",
        }
    }

    /// The filter a route segment names ("All" for anything else).
    pub fn from_path(path: &str) -> Self {
        [Self::Tracks, Self::Albums, Self::Artists, Self::Playlists, Self::Users, Self::Audiobooks, Self::Authors]
            .into_iter()
            .find(|f| f.path() == path)
            .unwrap_or(Self::All)
    }

    fn from_chip(type_name: &str) -> Option<Self> {
        Some(match type_name {
            "TRACKS" => Self::Tracks,
            "ALBUMS" => Self::Albums,
            "ARTISTS" => Self::Artists,
            "PLAYLISTS" => Self::Playlists,
            "USERS" => Self::Users,
            _ => return None,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum HitKind {
    /// A completed query (`spotify:search:<words>`).
    Query,
    Track,
    Album,
    Artist,
    Playlist,
    User,
    Audiobook,
    Author,
    /// Podcasts and episodes: listed, but they have no page here yet.
    Other,
}

#[derive(Clone, Debug)]
pub struct Hit {
    pub kind: HitKind,
    pub uri: String,
    pub name: String,
    /// (width, url); width 0 when the service doesn't say.
    pub images: Vec<(u32, String)>,
    pub explicit: bool,
    pub verified: bool,
    /// The word that starts the second line: "Song", "Album", "Single", "Artist", …
    pub label: &'static str,
    /// Who follows it on that line: artists (links), a playlist's owner, an audiobook's authors.
    pub people: Vec<(String, String)>,
    /// Songs: (album name, album uri).
    pub album: Option<(String, String)>,
    pub duration_ms: u64,
    pub year: Option<u32>,
    pub playable: bool,
}

impl Hit {
    /// The smallest image at least `min` px wide (else the largest there is).
    pub fn image(&self, min: u32) -> Option<String> {
        let fits = self.images.iter().filter(|(w, _)| *w >= min).min_by_key(|(w, _)| *w);
        fits.or_else(|| self.images.iter().max_by_key(|(w, _)| *w)).map(|(_, u)| u.clone())
    }

    pub fn round(&self) -> bool {
        matches!(self.kind, HitKind::Artist | HitKind::User | HitKind::Author)
    }

    /// A playback context of its own (albums, playlists, artists).
    pub fn context(&self) -> bool {
        matches!(self.kind, HitKind::Album | HitKind::Playlist | HitKind::Artist)
    }

    /// The tracklist row of a song hit.
    pub fn track(&self) -> Track {
        let (album, album_uri) = self.album.clone().unwrap_or_default();
        Track {
            uid: String::new(),
            uri: self.uri.clone(),
            name: self.name.clone(),
            artists: self.people.clone(),
            album,
            album_uri,
            image: self.image(64),
            duration_ms: self.duration_ms,
            explicit: self.explicit,
            playable: self.playable,
            added_at: None,
            disc: 0,
            number: 0,
        }
    }

    /// The card of a hit on a grid page or in a shelf.
    pub fn card(&self) -> Item {
        let names = || self.people.iter().map(|p| p.0.as_str()).collect::<Vec<_>>().join(", ");
        let (kind, subtitle) = match self.kind {
            HitKind::Album => (
                Kind::Album,
                match self.year {
                    Some(y) => format!("{y} \u{2022} {}", names()),
                    None => names(),
                },
            ),
            HitKind::Playlist => (Kind::Playlist, format!("By {}", names())),
            HitKind::Artist => (Kind::Artist, "Artist".to_string()),
            HitKind::User => (Kind::User, "Profile".to_string()),
            HitKind::Author => (Kind::Author, "Author".to_string()),
            HitKind::Audiobook => (Kind::Other, names()),
            _ => (Kind::Other, self.label.to_string()),
        };
        Item { uri: self.uri.clone(), kind, name: self.name.clone(), subtitle, image: self.image(300), color: None, hide_title: false }
    }
}

/// One entry of the "All" page, in the order the service sends them.
#[derive(Clone, Debug)]
pub enum Entry {
    Hit(Hit),
    /// A shelf ("Featuring Kendrick Lamar").
    Section { title: String, hits: Vec<Hit> },
}

#[derive(Clone, Debug, Default)]
pub struct Results {
    /// Filter chips after "All" (the "All" page says which exist).
    pub chips: Vec<Filter>,
    pub entries: Vec<Entry>,
    pub total: usize,
    /// Offset of the next page, while there is one.
    pub next: Option<u32>,
}

impl Results {
    pub fn hits(&self) -> impl Iterator<Item = &Hit> {
        self.entries.iter().filter_map(|e| match e {
            Entry::Hit(h) => Some(h),
            Entry::Section { .. } => None,
        })
    }
}

fn text(v: &Value, pointer: &str) -> String {
    v.pointer(pointer).and_then(Value::as_str).unwrap_or_default().to_string()
}

fn sources(v: Option<&Value>) -> Vec<(u32, String)> {
    let Some(list) = v.and_then(Value::as_array) else { return Vec::new() };
    list.iter()
        .filter_map(|s| {
            let url = s.get("url")?.as_str()?.to_string();
            let width = s.get("width").or_else(|| s.get("maxWidth")).and_then(Value::as_u64).unwrap_or(0) as u32;
            Some((width, url))
        })
        .collect()
}

fn artists(v: &Value) -> Vec<(String, String)> {
    v.pointer("/artists/items")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|a| (text(a, "/profile/name"), text(a, "/uri")))
        .filter(|a| !a.0.is_empty())
        .collect()
}

fn playable(d: &Value) -> bool {
    d.pointer("/playability/playable").and_then(Value::as_bool).unwrap_or(true)
}

/// A hit from its `…ResponseWrapper` (or a completion entity).
fn hit(wrapper: &Value) -> Option<Hit> {
    let d = wrapper.get("data")?;
    let mut h = Hit {
        kind: HitKind::Other,
        uri: text(d, "/uri"),
        name: text(d, "/name"),
        images: Vec::new(),
        explicit: false,
        verified: false,
        label: "",
        people: Vec::new(),
        album: None,
        duration_ms: 0,
        year: None,
        playable: true,
    };
    let type_name = d.get("__typename").and_then(Value::as_str).unwrap_or_default();
    match (wrapper.get("__typename").and_then(Value::as_str).unwrap_or_default(), type_name) {
        ("SearchAutoCompleteEntity", _) => {
            h.kind = HitKind::Query;
            h.name = text(d, "/text");
        }
        (_, "Track") => {
            h.kind = HitKind::Track;
            h.label = "Song";
            h.images = sources(d.pointer("/albumOfTrack/coverArt/sources"));
            h.explicit = text(d, "/contentRating/label") == "EXPLICIT";
            h.people = artists(d);
            h.album = Some((text(d, "/albumOfTrack/name"), text(d, "/albumOfTrack/uri")));
            h.duration_ms = d.pointer("/duration/totalMilliseconds").and_then(Value::as_u64).unwrap_or(0);
            h.playable = playable(d);
        }
        (_, "Album") => {
            h.kind = HitKind::Album;
            h.label = match text(d, "/type").as_str() {
                "SINGLE" => "Single",
                "EP" => "EP",
                "COMPILATION" => "Compilation",
                _ => "Album",
            };
            h.images = sources(d.pointer("/coverArt/sources"));
            h.people = artists(d);
            h.year = d.pointer("/date/year").and_then(Value::as_u64).map(|y| y as u32);
            h.playable = playable(d);
        }
        (_, "Artist") => {
            h.kind = HitKind::Artist;
            h.label = "Artist";
            h.name = text(d, "/profile/name");
            h.images = sources(d.pointer("/visuals/avatarImage/sources"));
            h.verified = d.pointer("/onPlatformReputationTrait/verification/isVerified").and_then(Value::as_bool).unwrap_or(false)
                || d.pointer("/profile/verified").and_then(Value::as_bool).unwrap_or(false);
        }
        (_, "Playlist") => {
            h.kind = HitKind::Playlist;
            h.label = "Playlist";
            h.images = sources(d.pointer("/images/items/0/sources"));
            let owner = (text(d, "/ownerV2/data/name"), text(d, "/ownerV2/data/uri"));
            if !owner.0.is_empty() {
                h.people.push(owner);
            }
        }
        (_, "User") => {
            h.kind = HitKind::User;
            h.label = "Profile";
            h.name = [text(d, "/displayName"), text(d, "/name"), text(d, "/username")].into_iter().find(|n| !n.is_empty()).unwrap_or_default();
            h.images = sources(d.pointer("/avatar/sources"));
        }
        (_, "Audiobook") => {
            h.kind = HitKind::Audiobook;
            h.label = "Audiobook";
            h.images = sources(d.pointer("/coverArt/sources"));
            h.people = d
                .get("authorsV2")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .map(|a| (text(a, "/name"), text(a, "/uri")))
                .filter(|a| !a.0.is_empty())
                .collect();
        }
        (_, "AuthorV2") => {
            h.kind = HitKind::Author;
            h.label = "Author";
            h.images = sources(
                d.pointer("/visualIdentity/squareCoverImage/image/data/sources")
                    .or_else(|| d.pointer("/avatar/sources"))
                    .or_else(|| d.pointer("/visuals/avatarImage/sources")),
            );
        }
        (_, "Podcast") => {
            h.label = "Podcast";
            h.images = sources(d.pointer("/coverArt/sources"));
            let publisher = text(d, "/publisher/name");
            if !publisher.is_empty() {
                h.people.push((publisher, String::new()));
            }
        }
        (_, "Episode") => {
            h.label = "Episode";
            h.images = sources(d.pointer("/coverArt/sources"));
            let show = (text(d, "/podcastV2/data/name"), text(d, "/podcastV2/data/uri"));
            if !show.0.is_empty() {
                h.people.push(show);
            }
        }
        _ => return None,
    }
    (!h.name.is_empty() && !h.uri.is_empty()).then_some(h)
}

/// Hits with a page (or a search) here: audiobooks, authors, podcasts and episodes have none yet,
/// so they're left out everywhere.
fn shown(wrapper: &Value) -> Option<Hit> {
    hit(wrapper).filter(|h| !matches!(h.kind, HitKind::Audiobook | HitKind::Author | HitKind::Other))
}

fn entry(item: &Value) -> Option<Entry> {
    // Top results wrap each hit as `{ item, matchedFields }`; the filter pages mostly don't.
    let wrapper = item.get("item").filter(|w| w.get("data").is_some()).unwrap_or(item);
    if wrapper.get("__typename").and_then(Value::as_str) == Some("SearchSectionEntity") {
        let d = wrapper.get("data")?;
        let hits: Vec<Hit> = d.get("items").and_then(Value::as_array).into_iter().flatten().filter_map(shown).collect();
        let title = text(d, "/title/transformedLabel");
        return (!hits.is_empty()).then_some(Entry::Section { title, hits });
    }
    shown(wrapper).map(Entry::Hit)
}

/// The dropdown's list while typing.
pub fn parse_suggestions(v: &Value) -> Vec<Hit> {
    let items = v.pointer("/data/searchV2/topResultsV2/itemsV2").and_then(Value::as_array);
    items.into_iter().flatten().filter_map(|i| shown(i.get("item")?)).collect()
}

pub fn parse_recents(v: &Value) -> Vec<Hit> {
    let items = v.pointer("/data/recentSearches/recentSearchesItems/items").and_then(Value::as_array);
    items.into_iter().flatten().filter_map(shown).collect()
}

/// A page of results for `filter` (the "All" page also carries the chips).
pub fn parse_results(filter: Filter, v: &Value) -> Results {
    let Some(s) = v.pointer("/data/searchV2") else { return Results::default() };
    let list = s.get(filter.field());
    let items = list.and_then(|l| l.get("itemsV2").or_else(|| l.get("items"))).and_then(Value::as_array);
    let entries: Vec<Entry> = items.into_iter().flatten().filter_map(entry).collect();
    let chips = s
        .pointer("/chipOrder/items")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|c| Filter::from_chip(c.get("typeName")?.as_str()?))
        .collect();
    let total = list.and_then(|l| l.get("totalCount")).and_then(Value::as_u64).unwrap_or(entries.len() as u64) as usize;
    let next = list.and_then(|l| l.pointer("/pagingInfo/nextOffset")).and_then(Value::as_u64).map(|n| n as u32);
    Results { chips, entries, total, next }
}
