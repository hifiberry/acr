/// Loop mode enumeration for playback
use serde::{Serialize, Deserialize};
use strum_macros::EnumString;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, EnumString)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum LoopMode {
    /// No loop
    #[serde(rename = "no")]
    #[default]
    None,
    /// Loop current track/song
    #[serde(rename = "song")]
    Track,
    /// Loop entire playlist
    #[serde(rename = "playlist")]
    Playlist,
}


impl std::fmt::Display for LoopMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Return the value as a string for backwards compatibility
        match self {
            LoopMode::None => write!(f, "no"),
            LoopMode::Track => write!(f, "song"),
            LoopMode::Playlist => write!(f, "playlist"),
        }
    }
}