/// Player state enumeration defining possible states a player can be in
use serde::{Serialize, Deserialize};
use strum_macros::EnumString;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, EnumString)]
#[serde(rename_all = "lowercase")]
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
    /// Player state cannot be determined
    #[serde(rename = "unknown")]
    Unknown,
}

impl Default for PlaybackState {
    fn default() -> Self {
        PlaybackState::Unknown
    }
}

impl std::fmt::Display for PlaybackState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Return the value as a string for backwards compatibility
        match self {
            PlaybackState::Playing => write!(f, "playing"),
            PlaybackState::Paused => write!(f, "paused"),
            PlaybackState::Stopped => write!(f, "stopped"),
            PlaybackState::Unknown => write!(f, "unknown"),
        }
    }
}