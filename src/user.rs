//! User profile page data: `user-profile-view/v3` (profile, followers, following) plus the
//! signed-in user's "Top artists this month" (`userTopContent`).

use crate::home::{Item, Kind};
use serde_json::Value;

#[derive(Clone, Debug)]
pub struct UserPage {
    pub uri: String,
    pub name: String,
    pub image: Option<String>,
    pub followers: u64,
    pub following: u64,
    pub public_playlists: u64,
    pub is_current_user: bool,
    /// Header colour (`fetchExtractedColors` for the avatar), 0xRRGGBB.
    pub color: u32,
    pub top_artists: Vec<Item>,
    pub playlists: Vec<Item>,
    pub follower_list: Vec<Item>,
    pub following_list: Vec<Item>,
}

fn s(v: &Value, k: &str) -> String {
    v.get(k).and_then(Value::as_str).unwrap_or_default().to_string()
}

fn profile_items(v: &Value) -> Vec<Item> {
    v.get("profiles")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|p| {
            let uri = s(p, "uri");
            // artists can appear in "Following"
            let artist = uri.starts_with("spotify:artist:");
            Item {
                uri,
                kind: if artist { Kind::Artist } else { Kind::User },
                name: s(p, "name"),
                subtitle: if artist { "Artist".into() } else { "Profile".into() },
                image: p.get("image_url").and_then(Value::as_str).filter(|u| !u.is_empty()).map(str::to_string),
                color: None,
                hide_title: false,
            }
        })
        .collect()
}

pub fn parse(profile: &Value, followers: &Value, following: &Value, top: Option<&Value>, color: Option<u32>) -> Option<UserPage> {
    let uri = s(profile, "uri");
    if uri.is_empty() {
        return None;
    }
    let playlists = profile
        .get("public_playlists")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|p| Item {
            uri: s(p, "uri"),
            kind: Kind::Playlist,
            name: s(p, "name"),
            subtitle: format!("By {}", s(p, "owner_name")),
            image: p.get("image_url").and_then(Value::as_str).map(str::to_string),
            color: None,
            hide_title: false,
        })
        .collect();
    let top_artists = top
        .and_then(|t| t.pointer("/data/me/profile/topArtists/items"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|e| {
            let a = e.get("data")?;
            (a.get("__typename")?.as_str()? == "Artist").then_some(())?;
            let sources = a.pointer("/visuals/avatarImage/sources")?.as_array()?;
            // the card loads the 320 rendition
            let image = sources
                .iter()
                .min_by_key(|s| (s.get("width").and_then(Value::as_u64).unwrap_or(0) as i64 - 320).abs())
                .and_then(|s| s.get("url")?.as_str())
                .map(str::to_string);
            Some(Item {
                uri: s(a, "uri"),
                kind: Kind::Artist,
                name: a.pointer("/profile/name")?.as_str()?.to_string(),
                subtitle: "Artist".into(),
                image,
                color: None,
                hide_title: false,
            })
        })
        .collect();
    Some(UserPage {
        uri,
        name: s(profile, "name"),
        image: profile.get("image_url").and_then(Value::as_str).filter(|u| !u.is_empty()).map(str::to_string),
        followers: profile.get("followers_count").and_then(Value::as_u64).unwrap_or(0),
        following: profile.get("following_count").and_then(Value::as_u64).unwrap_or(0),
        public_playlists: profile.get("total_public_playlists_count").and_then(Value::as_u64).unwrap_or(0),
        is_current_user: profile.get("is_current_user").and_then(Value::as_bool).unwrap_or(false),
        color: color.unwrap_or(0x535353),
        top_artists,
        playlists,
        follower_list: profile_items(followers),
        following_list: profile_items(following),
    })
}

/// `fetchExtractedColors` → the first image's `colorDark` as 0xRRGGBB.
pub fn extracted_color(v: &Value) -> Option<u32> {
    let hex = v.pointer("/data/extractedColors/0/colorDark/hex")?.as_str()?;
    u32::from_str_radix(hex.trim_start_matches('#'), 16).ok()
}
