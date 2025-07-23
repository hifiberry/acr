use crate::data::{PlaybackState, Song, LoopMode, PlayerCapabilitySet};
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
    
    /// Song information has been updated (e.g., cover art, metadata)
    // in this event, song title and artist are not updated, they
    // are only present to check in the UI if the song is the same
    // as the one currently playing
    // all other fields are optional. if a field is None, it means
    // that the field is not updated
    // only updated fields are populated
    SongInformationUpdate {
        source: PlayerSource,
        song: Song,
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

    /// Active player has changed
    ActivePlayerChanged {
        source: PlayerSource,
        player_id: String,
    },

    /// Volume control has changed (system-wide event)
    VolumeChanged {
        /// Name of the volume control that changed
        control_name: String,
        /// Display name of the control
        display_name: String,
        /// New volume percentage (0-100)
        percentage: f64,
        /// New volume in decibels (if supported)
        decibels: Option<f64>,
        /// Raw control value (implementation specific)
        raw_value: Option<i64>,
    },

}

impl PlayerEvent {
    /// Get the player source associated with this event (if any)
    pub fn source(&self) -> Option<&PlayerSource> {
        match self {
            PlayerEvent::StateChanged { source, .. } => Some(source),
            PlayerEvent::SongChanged { source, .. } => Some(source),
            PlayerEvent::LoopModeChanged { source, .. } => Some(source),
            PlayerEvent::RandomChanged { source, .. } => Some(source),
            PlayerEvent::CapabilitiesChanged { source, .. } => Some(source),
            PlayerEvent::PositionChanged { source, .. } => Some(source),
            PlayerEvent::DatabaseUpdating { source, .. } => Some(source),
            PlayerEvent::QueueChanged { source } => Some(source),
            PlayerEvent::SongInformationUpdate { source, .. } => Some(source),
            PlayerEvent::ActivePlayerChanged { source, .. } => Some(source),
            PlayerEvent::VolumeChanged { .. } => None, // Volume events are system-wide
        }
    }
    
    /// Get the player name associated with this event (if any)
    pub fn player_name(&self) -> Option<&str> {
        self.source().map(|s| s.player_name())
    }
    
    /// Get the player ID associated with this event (if any)
    pub fn player_id(&self) -> Option<&str> {
        self.source().map(|s| s.player_id())
    }
    
    /// Get the event type as a string
    pub fn event_type(&self) -> &'static str {
        match self {
            PlayerEvent::StateChanged { .. } => "state_changed",
            PlayerEvent::SongChanged { .. } => "song_changed",
            PlayerEvent::LoopModeChanged { .. } => "loop_mode_changed",
            PlayerEvent::RandomChanged { .. } => "random_changed",
            PlayerEvent::CapabilitiesChanged { .. } => "capabilities_changed",
            PlayerEvent::PositionChanged { .. } => "position_changed",
            PlayerEvent::DatabaseUpdating { .. } => "database_updating",
            PlayerEvent::QueueChanged { .. } => "queue_changed",
            PlayerEvent::SongInformationUpdate { .. } => "song_information_update",
            PlayerEvent::ActivePlayerChanged { .. } => "active_player_changed",
            PlayerEvent::VolumeChanged { .. } => "volume_changed",
        }
    }
}

impl fmt::Display for PlayerEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PlayerEvent::StateChanged { source, state } => {
                write!(f, "Player {} state changed to {}", source, state)
            }
            PlayerEvent::SongChanged { source, song } => {
                if let Some(s) = song {
                    write!(f, "Player {} song changed to '{}'", source, s)
                } else {
                    write!(f, "Player {} song changed to None", source)
                }
            }
            PlayerEvent::LoopModeChanged { source, mode } => {
                write!(f, "Player {} loop mode changed to {}", source, mode)
            }
            PlayerEvent::RandomChanged { source, enabled } => {
                write!(f, "Player {} random mode changed to {}", source, if *enabled { "enabled" } else { "disabled" })
            }
            PlayerEvent::CapabilitiesChanged { source, capabilities } => {
                write!(f, "Player {} capabilities changed: {:?}", source, capabilities) // Using Debug for capabilities
            }
            PlayerEvent::PositionChanged { source, position } => {
                write!(f, "Player {} position changed to {:.2}s", source, position)
            }
            PlayerEvent::DatabaseUpdating { source, artist, album, song, percentage } => {
                let mut details = String::new();
                if let Some(p) = percentage {
                    details.push_str(&format!("{}% ", p));
                }
                if let Some(s_artist) = artist {
                    details.push_str(&format!("Artist: {} ", s_artist));
                }
                if let Some(s_album) = album {
                    details.push_str(&format!("Album: {} ", s_album));
                }
                if let Some(s_song) = song {
                    details.push_str(&format!("Song: {} ", s_song));
                }
                write!(f, "Player {} database updating {}", source, details.trim())
            }            PlayerEvent::QueueChanged { source } => {
                write!(f, "Player {} queue changed", source)
            }
            PlayerEvent::SongInformationUpdate { source, song } => {
                write!(f, "Player {} song information updated for '{}'", source, song)
            }
            PlayerEvent::ActivePlayerChanged { source, player_id } => {
                write!(f, "Active player changed to {} (ID: {})", source, player_id)
            }
            PlayerEvent::VolumeChanged { control_name, percentage, decibels, .. } => {
                if let Some(db) = decibels {
                    write!(f, "Volume control '{}' changed to {:.1}% ({:.1}dB)", control_name, percentage, db)
                } else {
                    write!(f, "Volume control '{}' changed to {:.1}%", control_name, percentage)
                }
            }
        }
    }
}