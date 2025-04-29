/// Class representing metadata for a media player
use std::collections::HashMap;
use serde::{Serialize, Deserialize};

// Update the import path for PlayerState since it's now in the same module
use super::player_state::PlayerState;
use super::capabilities::PlayerCapability;
use super::loop_mode::LoopMode;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Player {
    pub name: String, // Name of the player (required)
    
    #[serde(skip_serializing_if = "Option::is_none")]
    pub player_id: Option<String>, // Unique identifier for the player
    
    #[serde(skip_serializing_if = "Option::is_none")]
    pub type_: Option<String>, // Type of player (e.g., "mpd", "spotify", "bluetooth")
    
    #[serde(default)]
    pub state: PlayerState, // Current state (e.g., "playing", "paused", "stopped")
    
    #[serde(skip_serializing_if = "Option::is_none")]
    pub volume: Option<i32>, // Current volume level (0-100)
    
    #[serde(skip_serializing_if = "Option::is_none")]
    pub muted: Option<bool>, // Whether the player is muted
    
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capabilities: Option<Vec<PlayerCapability>>, // Player capabilities
    
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active: Option<bool>, // Whether this player is the currently active one
    
    #[serde(skip_serializing_if = "Option::is_none")]
    pub position: Option<f64>, // Current playback position in seconds
    
    #[serde(default)]
    pub loop_mode: LoopMode, // Loop mode (None, Track, Playlist)
    
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shuffle: Option<bool>, // Whether shuffle is enabled
    
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    pub metadata: HashMap<String, serde_json::Value>,
}

impl Player {
    /// Create a new Player with the given name and default values for other fields
    pub fn new(name: String) -> Self {
        Self {
            name,
            player_id: None,
            type_: None,
            state: PlayerState::default(),
            volume: None,
            muted: None,
            capabilities: None,
            active: None,
            position: None,
            loop_mode: LoopMode::default(),
            shuffle: None,
            metadata: HashMap::new(),
        }
    }

    /// Add a capability to the player
    pub fn add_capability(&mut self, capability: PlayerCapability) {
        let caps = self.capabilities.get_or_insert_with(Vec::new);
        if !caps.contains(&capability) {
            caps.push(capability);
        }
    }

    /// Check if the player has a specific capability
    pub fn has_capability(&self, capability: PlayerCapability) -> bool {
        if let Some(caps) = &self.capabilities {
            caps.contains(&capability)
        } else {
            false
        }
    }
}
// The to_json method is now provided by the Serializable trait
// which is automatically implemented for all types that implement Serialize