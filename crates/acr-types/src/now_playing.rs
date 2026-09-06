use crate::{PlaybackState, PlayerSource, Song};
use serde::Deserialize;

/// The name the Last.fm worker logs under, and the name the player daemon's
/// action-plugin list reports for the `action_plugins` entry that configures
/// it. That list is a shipped API, so the name is fixed even though nothing
/// behind it is a plugin any more.
///
/// It is here rather than with the worker because both sides need it and only
/// one of them may hold it: the worker names itself with it, and the player
/// daemon reports it from `GET /api/plugins/actions` without linking the
/// metadata crate.
pub const LASTFM_WORKER_NAME: &str = "Lastfm";

/// The `action_plugins` entry named `lastfm`: same keys, same default, so an
/// existing configuration file keeps working.
///
/// Shared for the same reason as the name. The player daemon parses the entry
/// to decide whether to report the worker at all — an entry that would not
/// have produced a plugin does not produce a descriptor either — and the
/// metadata worker parses it to configure itself. One definition, so the two
/// cannot come to disagree about what a valid entry is.
#[derive(Debug, Deserialize, Clone)]
pub struct LastfmWorkerConfig {
    pub enabled: bool,
    pub api_key: String,
    pub api_secret: String,
    #[serde(default = "default_scrobble_config")]
    pub scrobble: bool,
}

fn default_scrobble_config() -> bool {
    true
}

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
