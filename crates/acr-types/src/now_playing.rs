use crate::{PlaybackState, PlayerSource, Song};

/// What the metadata side learns about playback, in the order it happens.
#[derive(Debug, Clone, PartialEq)]
pub enum NowPlayingEvent {
    SongChanged {
        source: PlayerSource,
        song: Option<Song>,
    },
    StateChanged {
        source: PlayerSource,
        state: PlaybackState,
    },
}

/// Where an enrichment result goes. The player daemon implements it with
/// `AudioController::apply_song_information`; Phase 1 implements it over HTTP.
pub trait SongInformationSink: Send + Sync {
    /// Returns whether the stored song changed.
    fn apply(&self, source: &PlayerSource, partial: &Song) -> bool;
}

/// What the active player is doing right now, asked rather than awaited.
///
/// A worker that measures how long a track has been playing cannot rely on
/// having seen every `StateChanged`: one that never arrives leaves its idea of
/// the state wrong for as long as the track lasts, and the scrobble that
/// depends on it then either never happens or happens against a paused
/// player. The Last.fm worker has always reconciled against the player
/// periodically for that reason, so the channel alone is not enough to carry
/// it. The player daemon implements this with
/// `AudioController::get_playback_state`; Phase 1 implements it over HTTP.
pub trait PlaybackStateSource: Send + Sync {
    /// The state of the active player, as the player itself reports it.
    fn playback_state(&self) -> PlaybackState;
}
