//! Turning a polled MPRIS snapshot into state and the changes to publish.
//!
//! Pure functions, no D-Bus: the controller reads a [`MprisSnapshot`] from the
//! bus and lets [`reconcile`] decide what changed.

use crate::data::{LoopMode, PlaybackState, PlayerCapability, PlayerState, Song};

/// What an MPRIS player says it can do (`Can*` properties), plus whether it
/// exposes `Shuffle` and `LoopStatus` at all.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MprisFlags {
    pub can_control: bool,
    pub can_play: bool,
    pub can_pause: bool,
    pub can_go_next: bool,
    pub can_go_previous: bool,
    pub can_seek: bool,
    pub has_shuffle: bool,
    pub has_loop: bool,
}

/// One poll's worth of an MPRIS player's properties.
#[derive(Debug, Clone, Default)]
pub struct MprisSnapshot {
    pub status: Option<String>,
    pub shuffle: Option<bool>,
    pub loop_status: Option<String>,
    pub position_us: Option<i64>,
    pub flags: MprisFlags,
    pub song: Option<Song>,
    /// `CanControl` could not be read, so `flags` are not to be trusted.
    pub flags_failed: bool,
    /// `Metadata` could not be read, so `song` says nothing about the song.
    pub metadata_failed: bool,
}

/// The state after a poll, and which parts of it changed.
#[derive(Debug, Clone)]
pub struct Reconciled {
    pub state: PlaybackState,
    pub shuffle: bool,
    pub loop_mode: LoopMode,
    pub position: Option<f64>,
    pub state_changed: bool,
    pub shuffle_changed: bool,
    pub loop_changed: bool,
    pub flags: MprisFlags,
    pub flags_changed: bool,
    pub song: Option<Song>,
}

/// The capabilities a player with these flags has.
pub fn capabilities_from_flags(flags: &MprisFlags) -> Vec<PlayerCapability> {
    if !flags.can_control {
        return Vec::new();
    }
    let mut caps = Vec::new();
    if flags.can_play {
        caps.push(PlayerCapability::Play);
    }
    if flags.can_pause {
        caps.push(PlayerCapability::Pause);
    }
    caps.push(PlayerCapability::Stop);
    if flags.can_go_next {
        caps.push(PlayerCapability::Next);
    }
    if flags.can_go_previous {
        caps.push(PlayerCapability::Previous);
    }
    if flags.can_seek {
        caps.push(PlayerCapability::Seek);
    }
    caps.push(PlayerCapability::Position);
    if flags.has_shuffle {
        caps.push(PlayerCapability::Shuffle);
    }
    if flags.has_loop {
        caps.push(PlayerCapability::Loop);
    }
    caps
}

fn playback_state(status: Option<&str>) -> PlaybackState {
    match status {
        Some("Playing") => PlaybackState::Playing,
        Some("Paused") => PlaybackState::Paused,
        Some("Stopped") => PlaybackState::Stopped,
        _ => PlaybackState::Unknown,
    }
}

fn loop_mode(status: &str) -> LoopMode {
    match status {
        "Track" => LoopMode::Track,
        "Playlist" => LoopMode::Playlist,
        _ => LoopMode::None,
    }
}

/// Reconcile a poll against the previous state. `snap` is `None` when the
/// player is not on the bus, which reads as stopped with no song.
pub fn reconcile(
    prev: &PlayerState,
    prev_flags: Option<MprisFlags>,
    snap: Option<&MprisSnapshot>,
    clear_song_when_stopped: bool,
) -> Reconciled {
    let (state, shuffle, loop_mode, position, flags, song) = match snap {
        None => (PlaybackState::Stopped, prev.shuffle, prev.loop_mode, None, MprisFlags::default(), None),
        Some(s) => {
            let state = playback_state(s.status.as_deref());
            let song = if clear_song_when_stopped && state == PlaybackState::Stopped {
                None
            } else {
                s.song.clone()
            };
            (
                state,
                s.shuffle.unwrap_or(false),
                s.loop_status.as_deref().map(loop_mode).unwrap_or(prev.loop_mode),
                s.position_us.map(|us| us as f64 / 1_000_000.0),
                s.flags,
                song,
            )
        }
    };
    Reconciled {
        state,
        shuffle,
        loop_mode,
        position,
        state_changed: state != prev.state,
        shuffle_changed: shuffle != prev.shuffle,
        loop_changed: loop_mode != prev.loop_mode,
        flags,
        flags_changed: prev_flags != Some(flags),
        song,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(status: &str) -> MprisSnapshot {
        MprisSnapshot { status: Some(status.to_string()), ..Default::default() }
    }

    fn song(title: &str) -> Song {
        Song { title: Some(title.to_string()), ..Default::default() }
    }

    fn state(s: PlaybackState) -> PlayerState {
        let mut p = PlayerState::new();
        p.state = s;
        p
    }

    fn shairport_flags() -> MprisFlags {
        MprisFlags {
            can_control: true, can_play: true, can_pause: true, can_go_next: true,
            can_go_previous: true, can_seek: false, has_shuffle: true, has_loop: true,
        }
    }

    #[test]
    fn absent_player_is_stopped_without_song() {
        let r = reconcile(&state(PlaybackState::Playing), Some(shairport_flags()), None, false);
        assert_eq!(r.state, PlaybackState::Stopped);
        assert!(r.state_changed);
        assert!(r.song.is_none());
        assert_eq!(r.flags, MprisFlags::default());
    }

    #[test]
    fn status_strings_map() {
        let prev = state(PlaybackState::Unknown);
        for (s, expected) in [
            ("Playing", PlaybackState::Playing),
            ("Paused", PlaybackState::Paused),
            ("Stopped", PlaybackState::Stopped),
            ("Bogus", PlaybackState::Unknown),
        ] {
            assert_eq!(reconcile(&prev, None, Some(&snap(s)), false).state, expected);
        }
    }

    #[test]
    fn shuffle_and_loop_changes_are_flagged() {
        let prev = state(PlaybackState::Playing);
        let mut s = snap("Playing");
        s.shuffle = Some(true);
        s.loop_status = Some("Track".to_string());
        let r = reconcile(&prev, None, Some(&s), false);
        assert!(r.shuffle_changed);
        assert!(r.loop_changed);
        assert!(r.shuffle);
        assert_eq!(r.loop_mode, LoopMode::Track);

        s.loop_status = Some("Sideways".to_string());
        assert_eq!(reconcile(&prev, None, Some(&s), false).loop_mode, LoopMode::None);
    }

    #[test]
    fn unchanged_snapshot_flags_nothing() {
        let prev = state(PlaybackState::Playing);
        let mut s = snap("Playing");
        s.shuffle = Some(false);
        s.loop_status = Some("None".to_string());
        let r = reconcile(&prev, Some(MprisFlags::default()), Some(&s), false);
        assert!(!r.state_changed);
        assert!(!r.shuffle_changed);
        assert!(!r.loop_changed);
        assert!(!r.flags_changed);
    }

    #[test]
    fn stopped_clears_song_when_configured() {
        let prev = state(PlaybackState::Playing);
        let mut s = snap("Stopped");
        s.song = Some(song("Old"));
        assert!(reconcile(&prev, None, Some(&s), true).song.is_none());
        assert!(reconcile(&prev, None, Some(&s), false).song.is_some());
    }

    #[test]
    fn position_converts_microseconds() {
        let mut s = snap("Playing");
        s.position_us = Some(2_500_000);
        let r = reconcile(&state(PlaybackState::Playing), None, Some(&s), false);
        assert_eq!(r.position, Some(2.5));
    }

    #[test]
    fn flags_changed_only_on_difference() {
        let mut s = snap("Playing");
        s.flags = shairport_flags();
        let prev = state(PlaybackState::Playing);
        assert!(!reconcile(&prev, Some(shairport_flags()), Some(&s), false).flags_changed);
        assert!(reconcile(&prev, None, Some(&s), false).flags_changed);
    }

    #[test]
    fn shairport_flags_give_expected_capabilities() {
        use PlayerCapability::*;
        assert_eq!(
            capabilities_from_flags(&shairport_flags()),
            vec![Play, Pause, Stop, Next, Previous, Position, Shuffle, Loop]
        );
        let no_control = MprisFlags { can_control: false, ..shairport_flags() };
        assert!(capabilities_from_flags(&no_control).is_empty());
    }

    #[test]
    fn poll_picks_up_player_that_appears_later() {
        let absent = reconcile(&state(PlaybackState::Playing), None, None, false);
        let mut prev = state(absent.state);
        prev.shuffle = absent.shuffle;
        prev.loop_mode = absent.loop_mode;
        let mut s = snap("Playing");
        s.flags = shairport_flags();
        let r = reconcile(&prev, Some(absent.flags), Some(&s), false);
        assert!(r.state_changed);
        assert_eq!(r.state, PlaybackState::Playing);
        assert!(r.flags_changed);
    }
}
