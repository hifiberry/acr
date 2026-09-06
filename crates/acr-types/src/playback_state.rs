use serde::{Serialize, Deserialize};
use strum_macros::EnumString;

/// Player state enumeration defining possible states a player can be in
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, EnumString)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum PlaybackState {
    /// Player is actively playing media
    #[serde(rename = "playing")]
    Playing,
    /// Playback is paused
    #[serde(rename = "paused")]
    Paused,
    /// Playback is stopped
    #[serde(rename = "stopped")]
    Stopped,
    /// Player process has been killed or crashed
    #[serde(rename = "killed")]
    Killed,
    /// Player is disconnected or not available
    #[serde(rename = "disconnected")]
    Disconnected,
    /// Player state cannot be determined
    #[serde(rename = "unknown")]
    #[default]
    Unknown,
}


impl std::fmt::Display for PlaybackState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Return the value as a string for backwards compatibility
        match self {
            PlaybackState::Playing => write!(f, "playing"),
            PlaybackState::Paused => write!(f, "paused"),
            PlaybackState::Stopped => write!(f, "stopped"),
            PlaybackState::Killed => write!(f, "killed"),
            PlaybackState::Disconnected => write!(f, "disconnected"),
            PlaybackState::Unknown => write!(f, "unknown"),
        }
    }
}
