use crate::data::{Song, PlaybackState, LoopMode};
use serde::{Serialize, Deserialize};

/// Enum representing updates that can be sent to a player controller.
/// These are typically informational updates about changes that might have
/// originated from another source or need to be synchronized with the player.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum PlayerUpdate {
    /// Indicates that the current song may have changed.
    SongChanged(Option<Song>),
    /// Indicates that the playback position may have changed.
    PositionChanged(Option<f64>),
    /// Indicates that the playback state may have changed.
    StateChanged(PlaybackState),
    /// Indicates that the loop mode may have changed.
    LoopModeChanged(LoopMode),
    /// Indicates that the shuffle/random mode may have changed.
    ShuffleChanged(bool),
    // Potentially add other updates like VolumeChanged, MuteChanged if needed.
}
