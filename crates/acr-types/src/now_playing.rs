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
