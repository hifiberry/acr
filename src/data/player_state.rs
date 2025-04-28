/// Player state enumeration defining possible states a player can be in
use serde::{Serialize, Deserialize};
use strum_macros::EnumString;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, EnumString)]
#[serde(rename_all = "lowercase")]
pub enum PlayerState {
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
    /// Player state cannot be determined
    #[serde(rename = "unknown")]
    Unknown,
}

impl Default for PlayerState {
    fn default() -> Self {
        PlayerState::Unknown
    }
}

impl std::fmt::Display for PlayerState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Return the value as a string for backwards compatibility
        match self {
            PlayerState::Playing => write!(f, "playing"),
            PlayerState::Paused => write!(f, "paused"),
            PlayerState::Stopped => write!(f, "stopped"),
            PlayerState::Killed => write!(f, "killed"),
            PlayerState::Unknown => write!(f, "unknown"),
        }
    }
}