use serde::{Serialize, Deserialize};
use std::fmt; // Added for Display

/// Identifies the source of a player event
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlayerSource {
    /// String identifier for the player type (e.g., "mpd", "spotify")
    pub player_name: String,

    /// Unique identifier for the player instance
    pub player_id: String,
}

impl PlayerSource {
    /// Create a new PlayerSource
    pub fn new(player_name: String, player_id: String) -> Self {
        Self { player_name, player_id }
    }

    /// Get the player name
    pub fn player_name(&self) -> &str {
        &self.player_name
    }

    /// Get the player ID
    pub fn player_id(&self) -> &str {
        &self.player_id
    }
}

impl fmt::Display for PlayerSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} ({})", self.player_name, self.player_id)
    }
}
