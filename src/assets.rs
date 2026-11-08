use std::borrow::Cow;

use anyhow::Result;
use gpui::{AssetSource, SharedString};

/// Everything the UI needs is compiled into the binary.
pub struct Assets;

macro_rules! embedded {
    ($($path:literal),* $(,)?) => {
        const FILES: &[(&str, &[u8])] = &[
            $(($path, include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/", $path)))),*
        ];
    };
}

embedded!(
    "icons/more.svg",
    "icons/win_min.svg",
    "icons/win_max.svg",
    "icons/win_close.svg",
    "icons/external.svg",
    "icons/mark_bars.svg",
    "icons/back.svg", "icons/forward.svg", "icons/home_filled.svg", "icons/home.svg",
    "icons/search.svg", "icons/browse.svg", "icons/bell.svg", "icons/friends.svg",
    "icons/plus.svg", "icons/expand.svg", "icons/list.svg", "icons/note.svg", "icons/heart.svg",
    "icons/pin.svg", "icons/shuffle.svg", "icons/prev.svg", "icons/next.svg", "icons/play.svg",
    "icons/pause.svg", "icons/repeat.svg", "icons/add_circle.svg", "icons/check_circle.svg",
    "icons/mic.svg", "icons/queue.svg", "icons/device.svg", "icons/volume.svg",
    "icons/volume_mute.svg", "icons/pip.svg", "icons/fullscreen.svg", "icons/speaker.svg",
    "icons/collapse_library.svg", "icons/lib_search.svg", "icons/mixed.svg", "icons/play24.svg",
    "icons/pause24.svg", "icons/add_circle24.svg", "icons/check_circle24.svg", "icons/hide_panel.svg", "icons/expand_panel.svg",
    "icons/pl_invite12.svg", "icons/pl_explore.svg", "icons/pl_shuffle24.svg", "icons/pl_download24.svg",
    "icons/pl_adduser24.svg", "icons/pl_more24.svg", "icons/pl_search16.svg", "icons/pl_list16.svg",
    "icons/pl_plus16.svg", "icons/pl_mix16.svg", "icons/pl_pencil16.svg", "icons/pl_clock16.svg",
    "icons/pl_columns16.svg", "icons/pl_play24b.svg", "icons/pl_explore_frame.svg", "icons/caret_down16.svg", "icons/verified24.svg", "icons/gear24.svg", "icons/check16.svg", "icons/create_playlist24.svg", "icons/create_mixed24.svg", "icons/create_folder24.svg", "icons/create_jam24.svg", "icons/create_blend.svg", "icons/lib_collapse16.svg", "icons/arrow_down16.svg", "icons/arrow_up16.svg", "icons/view_compact_list16.svg", "icons/view_list16.svg", "icons/view_compact_grid16.svg", "icons/view_grid16.svg", "icons/external16.svg", "icons/sb_up.svg", "icons/sb_down.svg", "icons/jam16.svg", "icons/album16.svg", "icons/track16.svg", "icons/play_tri.svg", "icons/close16.svg", "icons/playlist16.svg", "icons/friends_active.svg", "icons/browse_filled.svg", "icons/ctx_follow.svg", "icons/ctx_don_t_play_this_artist.svg", "icons/ctx_go_to_artist_radio.svg", "icons/ctx_share.svg", "icons/ctx_arrow.svg", "icons/ctx_add_to_your_library.svg", "icons/ctx_add_to_queue.svg", "icons/ctx_start_a_jam.svg", "icons/ctx_turn_on_mix.svg", "icons/ctx_download.svg", "icons/ctx_exclude_from_your_taste_profile.svg", "icons/ctx_add_to_folder.svg", "icons/ctx_add_to_other_playlist.svg", "icons/ctx_unpin_playlist.svg", "icons/ctx_edit_details.svg", "icons/ctx_delete.svg", "icons/ctx_create_playlist.svg", "icons/ctx_create_folder.svg", "icons/ctx_make_public.svg", "icons/ctx_invite_collaborators.svg", "icons/ctx_move_to_folder.svg", "icons/ctx_add_to_playlist.svg", "icons/ctx_remove_from_this_playlist.svg", "icons/ctx_sleep_timer.svg", "icons/ctx_go_to_song_radio.svg", "icons/ctx_go_to_artist.svg", "icons/ctx_go_to_album.svg", "icons/ctx_view_credits.svg", "icons/ctx_hide_in_this_album.svg", "icons/ctx_copy_link.svg", "icons/ctx_embed.svg",
    "icons/copy_link.svg", "icons/library.svg", "icons/verified.svg", "icons/chevron_left.svg", "icons/chevron_right.svg",
    "fonts/Figtree-400.ttf",
    "fonts/Figtree-700.ttf",
);

impl Assets {
    pub fn get(path: &str) -> Option<&'static [u8]> {
        FILES.iter().find(|(p, _)| *p == path).map(|(_, b)| *b)
    }

    pub fn fonts() -> Vec<Cow<'static, [u8]>> {
        FILES
            .iter()
            .filter(|(p, _)| p.starts_with("fonts/"))
            .map(|(_, b)| Cow::Borrowed(*b))
            .collect()
    }
}

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        Ok(Self::get(path).map(Cow::Borrowed))
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        Ok(FILES
            .iter()
            .filter(|(p, _)| p.starts_with(path))
            .map(|(p, _)| SharedString::from(*p))
            .collect())
    }
}
