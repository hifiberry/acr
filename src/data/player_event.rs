use crate::data::{PlaybackState, Song, LoopMode, PlayerCapabilitySet};
use serde::{Serialize, Deserialize};

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

/// Represents different events that can occur in a player
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PlayerEvent {
    /// Player state has changed (playing, paused, stopped, etc.)
    StateChanged {
        source: PlayerSource,
        state: PlaybackState,
    },
    
    /// Current song has changed
    SongChanged {
        source: PlayerSource,
        song: Option<Song>,
    },
    
    /// Loop mode has changed
    LoopModeChanged {
        source: PlayerSource,
        mode: LoopMode,
    },
    
    /// Shuffle/random mode has changed
    RandomChanged {
        source: PlayerSource,
        enabled: bool,
    },
    
    /// Player capabilities have changed
    CapabilitiesChanged {
        source: PlayerSource,
        capabilities: PlayerCapabilitySet,
    },
    
    /// Playback position has changed
    PositionChanged {
        source: PlayerSource,
        position: f64,
    },

    /// Database is being updated
    DatabaseUpdating {
        source: PlayerSource,
        artist: Option<String>,
        album: Option<String>,
        song: Option<String>,
        percentage: Option<f32>,
    },

    /// Queue content has changed
    QueueChanged {
        source: PlayerSource,
    },
}

impl PlayerEvent {
    /// Get the player source associated with this event
    pub fn source(&self) -> &PlayerSource {
        match self {
            PlayerEvent::StateChanged { source, .. } => source,
            PlayerEvent::SongChanged { source, .. } => source,
            PlayerEvent::LoopModeChanged { source, .. } => source,
            PlayerEvent::RandomChanged { source, .. } => source,
            PlayerEvent::CapabilitiesChanged { source, .. } => source,
            PlayerEvent::PositionChanged { source, .. } => source,
            PlayerEvent::DatabaseUpdating { source, .. } => source,
            PlayerEvent::QueueChanged { source } => source,
        }
    }
    
    /// Get the player name associated with this event
    pub fn player_name(&self) -> &str {
        self.source().player_name()
    }
    
    /// Get the player ID associated with this event
    pub fn player_id(&self) -> &str {
        self.source().player_id()
    }
}