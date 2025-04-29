/// Class representing metadata for a media player
use std::collections::HashMap;
use serde::{Serialize, Deserialize};
use strum_macros::EnumString;

use super::capabilities::{PlayerCapability, PlayerCapabilitySet};
use super::loop_mode::LoopMode;

/// Player state enumeration defining possible states a player can be in
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
    /// Player process has been killed or crashed
    #[serde(rename = "killed")]
    Killed,
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
            PlaybackState::Killed => write!(f, "killed"),
            PlaybackState::Unknown => write!(f, "unknown"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Player {
    pub name: String, // Name of the player (required)
    
    #[serde(skip_serializing_if = "Option::is_none")]
    pub player_id: Option<String>, // Unique identifier for the player
    
    #[serde(skip_serializing_if = "Option::is_none")]
    pub type_: Option<String>, // Type of player (e.g., "mpd", "spotify", "bluetooth")
    
    #[serde(default)]
    pub state: PlaybackState, // Current state (e.g., "playing", "paused", "stopped")
    
    #[serde(skip_serializing_if = "Option::is_none")]
    pub volume: Option<i32>, // Current volume level (0-100)
    
    #[serde(skip_serializing_if = "Option::is_none")]
    pub muted: Option<bool>, // Whether the player is muted
    
    #[serde(default, skip_serializing_if = "PlayerCapabilitySet::is_empty")]
    pub capabilities: PlayerCapabilitySet, // Player capabilities using bitflags
    
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
            state: PlaybackState::default(),
            volume: None,
            muted: None,
            capabilities: PlayerCapabilitySet::empty(),
            active: None,
            position: None,
            loop_mode: LoopMode::default(),
            shuffle: None,
            metadata: HashMap::new(),
        }
    }

    /// Add a capability to the player
    pub fn add_capability(&mut self, capability: PlayerCapability) {
        self.capabilities.add_capability(capability);
    }

    /// Check if the player has a specific capability
    pub fn has_capability(&self, capability: PlayerCapability) -> bool {
        self.capabilities.has_capability(capability)
    }
    
    /// Remove a capability from the player
    pub fn remove_capability(&mut self, capability: PlayerCapability) {
        self.capabilities.remove_capability(capability);
    }
    
    /// Get all capabilities as a vector (for compatibility with existing code)
    pub fn get_capabilities_vec(&self) -> Vec<PlayerCapability> {
        self.capabilities.to_vec()
    }
    
    /// Set multiple capabilities at once from a slice
    pub fn set_capabilities(&mut self, capabilities: &[PlayerCapability]) {
        self.capabilities = PlayerCapabilitySet::from_slice(capabilities);
    }
    
    /// Check if the player has any capability
    pub fn has_any_capability(&self) -> bool {
        !self.capabilities.is_empty()
    }
}