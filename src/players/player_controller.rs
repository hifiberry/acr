use crate::data::{PlayerCapability, PlayerCapabilitySet, Song, Track, LoopMode, PlaybackState, PlayerCommand, PlayerEvent, PlayerSource, PlayerState, PlayerUpdate};
use crate::data::library::LibraryInterface;
use std::sync::Arc;
use parking_lot::RwLock;
use std::any::Any;
use std::time::SystemTime;
use log::debug;

/// PlayerController trait - abstract interface for player implementations
/// 
/// This trait defines the core functionality that any player implementation must provide.
/// It serves as an abstraction layer for different media player backends.
pub trait PlayerController: Send + Sync {
    /// Get the capabilities of the player
    /// 
    /// Returns a PlayerCapabilitySet with the capabilities supported by this player
    fn get_capabilities(&self) -> PlayerCapabilitySet;
    
    /// Get the current song being played
    /// 
    /// Returns the current song, or None if no song is playing
    fn get_song(&self) -> Option<Song>;

    /// Get the queue of songs
    /// 
    /// Returns a vector of songs in the queue (can be empty if no songs are queued)
    /// If the player does not support queues, this will return an empty vector
    fn get_queue(&self) -> Vec<Track>;
    
    /// Get the current loop mode setting
    /// 
    /// Returns the current loop mode of the player
    fn get_loop_mode(&self) -> LoopMode;
    
    /// Get the current player state
    /// 
    /// Returns the current state of the player (playing, paused, stopped, etc.)
    fn get_playback_state(&self) -> PlaybackState;
    
    /// Get the current playback position in seconds
    ///
    /// Returns the current position as seconds from the start of the track, or None if position is unknown
    fn get_position(&self) -> Option<f64>;
    
    /// Get whether shuffle is enabled
    /// 
    /// Returns true if shuffle is enabled, false otherwise
    fn get_shuffle(&self) -> bool;
    
    /// Get the name of this player controller
    /// 
    /// Returns a string identifier for this type of player (e.g., "mpd", "null")
    fn get_player_name(&self) -> String;
    
    /// Get a unique identifier for this player instance
    /// 
    /// Returns a string that uniquely identifies this player instance
    fn get_player_id(&self) -> String;
    
    /// Get the aliases for this player
    /// 
    /// Returns a vector of string aliases that can be used to identify this player type
    /// Default implementation returns just the player name
    fn get_aliases(&self) -> Vec<String> {
        vec![self.get_player_name()]
    }
    
    /// Get the last time this player was seen active
    /// 
    /// Returns the timestamp when the player was last seen, or None if not tracked
    fn get_last_seen(&self) -> Option<SystemTime>;
    
    /// Send a command to the player
    /// 
    /// # Arguments
    /// 
    /// * `command` - The command to send to the player
    /// 
    /// # Returns
    /// 
    /// Return s`true` if the command was successfully processed, `false` otherwise
    fn send_command(&self, command: PlayerCommand) -> bool;
    
    /// Downcasts the player controller to a concrete type via Any
    /// 
    /// This allows accessing implementation-specific functionality when needed.
    fn as_any(&self) -> &dyn Any;
    
    /// Starts the player controller
    /// 
    /// This initializes any background threads and connections needed for the player to operate.
    /// Returns true if the player was successfully started, false otherwise.
    fn start(&self) -> bool;
    
    /// Stops the player controller
    /// 
    /// This cleans up any resources used by the player, including stopping background threads
    /// and closing connections. Returns true if the player was successfully stopped, false otherwise.
    fn stop(&self) -> bool;

    /// Receive an update. This could be a song change,
    /// position change, random/loop mode change, etc.
    ///
    /// # Arguments
    ///
    /// * `update` - The player update
    ///
    /// # Returns
    ///
    /// `true` if the update was successfully processed, `false` otherwise
    fn receive_update(&self, update: PlayerUpdate) -> bool {
        // Default implementation does nothing and returns true
        // Player implementations should override this if they support receiving updates
        debug!("Player {} received update {:?}, but does not implement receive_update", self.get_player_name(), update);
        true
    }

    /// Get the library interface for this player, if available
    /// 
    /// Returns a library interface that can be used to query albums, artists, and tracks,
    /// or None if the player does not support library functionality.
    fn get_library(&self) -> Option<Box<dyn LibraryInterface>> {
        None  // Default implementation returns None
    }
    
    /// Check if this player offers library functionality
    /// 
    /// Returns true if the player has a library interface, false otherwise
    /// This is a convenience method that checks if get_library() would return Some
    fn has_library(&self) -> bool {
        // Since get_library consumes resources to create the Box, we just want to check
        // if the player has the capability rather than actually creating the library interface
        self.get_library().is_some()
    }

    /// Get a list of metadata keys available for this player
    /// 
    /// Returns a list of metadata keys that can be queried
    /// via get_metadata_value(). Default implementation returns an empty vector.
    fn get_meta_keys(&self) -> Vec<String> {
        vec![]
    }
    
    /// Get a specific metadata value as string
    /// 
    /// # Arguments
    /// 
    /// * `key` - The metadata key to retrieve
    /// 
    /// # Returns
    /// 
    /// The metadata value as a string, or None if the key is not found
    /// or the player doesn't support metadata
    fn get_metadata_value(&self, _key: &str) -> Option<String> {
        None
    }
    
    /// Get all metadata as a HashMap with JSON values
    /// 
    /// # Returns
    /// 
    /// All metadata for the player as a HashMap with JSON values, 
    /// or None if the player doesn't support metadata
    fn get_metadata(&self) -> Option<std::collections::HashMap<String, serde_json::Value>> {
        // Convert string metadata to JSON values
        let mut result = std::collections::HashMap::new();
        
        // Add each meta key to the result
        for key in self.get_meta_keys() {
            if let Some(value) = self.get_metadata_value(&key) {
                // Try to parse as JSON, fall back to string value
                match serde_json::from_str(&value) {
                    Ok(json_value) => {
                        result.insert(key, json_value);
                    },
                    Err(_) => {
                        // Use string value
                        result.insert(key, serde_json::Value::String(value));
                    }
                }
            }
        }
        
        if result.is_empty() {
            None
        } else {
            Some(result)
        }
    }
    
    /// Check if this player supports metadata
    /// 
    /// Returns true if the player provides metadata functionality
    fn has_metadata(&self) -> bool {
        !self.get_meta_keys().is_empty()
    }
    
    /// Check if this player supports API events
    /// 
    /// Returns true if the player can process API events, false otherwise
    fn supports_api_events(&self) -> bool {
        false
    }
    
    /// Process an API event
    /// 
    /// # Arguments
    /// 
    /// * `event_data` - The event data to process
    /// 
    /// # Returns
    /// 
    /// `true` if the event was successfully processed, `false` otherwise
    fn process_api_event(&self, _event_data: &serde_json::Value) -> bool {
        false
    }
}

/// Base implementation of PlayerController that handles state listener management
/// 
/// This struct provides common functionality for managing state listeners that
/// can be used by concrete player implementations.
#[derive(Clone)]
pub struct BasePlayerController {
    /// Current capabilities of the player
    capabilities: Arc<RwLock<PlayerCapabilitySet>>,
    
    /// Player name identifier (e.g., "mpd", "null")
    player_name: Arc<RwLock<String>>,
    
    /// Player unique ID (e.g., "hostname:port" for MPD)
    player_id: Arc<RwLock<String>>,
    
    /// Player state
    player_state: Arc<RwLock<PlayerState>>,
}

impl Default for BasePlayerController {
    fn default() -> Self {
        Self::new()
    }
}

impl BasePlayerController {
    /// Create a new BasePlayerController with no listeners
    pub fn new() -> Self {
        debug!("Creating new BasePlayerController");
        Self {
            capabilities: Arc::new(RwLock::new(PlayerCapabilitySet::empty())),
            player_name: Arc::new(RwLock::new("unknown".to_string())),
            player_id: Arc::new(RwLock::new("unknown".to_string())),
            player_state: Arc::new(RwLock::new(PlayerState::new())),
        }
    }
    
    /// Initialize the controller with player name and ID
    pub fn with_player_info(name: &str, id: &str) -> Self {
        debug!("Creating BasePlayerController with name='{}', id='{}'", name, id);
        Self {
            capabilities: Arc::new(RwLock::new(PlayerCapabilitySet::empty())),
            player_name: Arc::new(RwLock::new(name.to_string())),
            player_id: Arc::new(RwLock::new(id.to_string())),
            player_state: Arc::new(RwLock::new(PlayerState::new())),
        }
    }
    
    /// Set the player name
    pub fn set_player_name(&self, name: &str) {
        *self.player_name.write() = name.to_string();
        debug!("Player name set to '{}'", name);
    }
    
    /// Set the player ID
    pub fn set_player_id(&self, id: &str) {
        *self.player_id.write() = id.to_string();
        debug!("Player ID set to '{}'", id);
    }
    
    /// Get the player name
    pub fn get_player_name(&self) -> String {
        self.player_name.read().clone()
    }
    
    /// Get the player ID
    pub fn get_player_id(&self) -> String {
        self.player_id.read().clone()
    }
    
    /// Get the current capabilities
    pub fn get_capabilities(&self) -> PlayerCapabilitySet {
        *self.capabilities.read()
    }
    
    /// Set multiple capabilities at once using a PlayerCapabilitySet
    /// 
    /// Replaces all current capabilities with the provided ones
    /// When auto_notify is true, listeners will be notified of changes automatically
    /// Returns true if the capabilities were changed
    pub fn set_capabilities_set(&self, capabilities: PlayerCapabilitySet, auto_notify: bool) -> bool {
        debug!("Setting all capabilities to a new capability set");
        
        let mut changed = false;
        
        // Update stored capabilities
        let mut caps = self.capabilities.write();
        // Check if there's any difference
        if *caps != capabilities {
            // Replace with new capabilities
            *caps = capabilities;
            debug!("Updated capabilities");
            changed = true;
        } else {
            debug!("Capabilities unchanged, not updating");
        }
        drop(caps);
        
        // If capabilities changed and auto_notify is true, notify listeners
        if changed && auto_notify {
            self.notify_capabilities_changed(&capabilities);
        }
        
        changed
    }
    
    /// Set multiple capabilities at once using a Vec of PlayerCapability
    /// 
    /// Replaces all current capabilities with the provided ones
    /// When auto_notify is true, listeners will be notified of changes automatically
    /// Returns true if the capabilities were changed
    pub fn set_capabilities(&self, capabilities: Vec<PlayerCapability>, auto_notify: bool) -> bool {
        debug!("Setting all capabilities to a list of {} capabilities", capabilities.len());
        
        let new_set = PlayerCapabilitySet::from_slice(&capabilities);
        self.set_capabilities_set(new_set, auto_notify)
    }

    /// Set a capability as enabled or disabled
    /// 
    /// If enabled is true, adds the capability if not already present
    /// If enabled is false, removes the capability if present
    /// When auto_notify is true, listeners will be notified of changes automatically
    /// Returns true if the capabilities were changed
    pub fn set_capability(&self, capability: PlayerCapability, enabled: bool, auto_notify: bool) -> bool {
        debug!("Setting capability {:?} to {}", capability, enabled);
        
        let mut changed = false;
        
        // Update stored capabilities
        let mut caps = self.capabilities.write();
        let had_capability = caps.has_capability(capability);

        if enabled && !had_capability {
            // Add capability
            caps.add_capability(capability);
            debug!("Added capability {:?}", capability);
            changed = true;
        } else if !enabled && had_capability {
            // Remove capability
            caps.remove_capability(capability);
            debug!("Removed capability {:?}", capability);
            changed = true;
        }
        drop(caps);
        
        // If capabilities changed and auto_notify is true, notify listeners
        if changed && auto_notify {
            let current_caps = self.get_capabilities();
            self.notify_capabilities_changed(&current_caps);
        }
        
        changed
    }    /// Notify all registered listeners that the player state has changed
    pub fn notify_state_changed(&self, state: PlaybackState) {
        let player_name = self.get_player_name();
        let player_id = self.get_player_id();
        
        let source = PlayerSource::new(player_name, player_id);
        
        let event = PlayerEvent::StateChanged {
            source,
            state,
        };
        
        // Publish to the global event bus
        debug!("Publishing state change event to the global event bus");
        crate::audiocontrol::eventbus::EventBus::instance().publish(event.clone());
        
    }    
    
    /// Notify all listeners that the song has changed
    pub fn notify_song_changed(&self, song: Option<&Song>) {
        let player_name = self.get_player_name();
        let player_id = self.get_player_id();
        
        // Create a cloned version of the song to pass to listeners
        let song_copy = song.cloned();
        
        let source = PlayerSource::new(player_name, player_id);
        
        let event = PlayerEvent::SongChanged {
            source,
            song: song_copy,
        };
        
        // Publish to the global event bus
        debug!("Publishing song change event to the global event bus");
        crate::audiocontrol::eventbus::EventBus::instance().publish(event.clone());
        
    }    
    
    /// Notify all registered listeners that the loop mode has changed
    pub fn notify_loop_mode_changed(&self, mode: LoopMode) {
        let player_name = self.get_player_name();
        let player_id = self.get_player_id();
        
        debug!("Notifying listeners of loop mode change: {}", mode);

        let source = PlayerSource::new(player_name, player_id);
        
        let event = PlayerEvent::LoopModeChanged {
            source,
            mode,
        };
        
        // Publish to the global event bus
        debug!("Publishing loop mode change event to the global event bus");
        crate::audiocontrol::eventbus::EventBus::instance().publish(event.clone());
        
        // do not notify listeners anymore
        
    }    /// Notify all registered listeners that the random mode has changed
    pub fn notify_random_changed(&self, enabled: bool) {
        let player_name = self.get_player_name();
        let player_id = self.get_player_id();
        
        debug!("Notifying listeners of random mode change: {}", enabled);

        let source = PlayerSource::new(player_name, player_id);
        
        let event = PlayerEvent::RandomChanged {
            source,
            enabled,
        };
        
        // Publish to the global event bus
        debug!("Publishing random mode change event to the global event bus");
        crate::audiocontrol::eventbus::EventBus::instance().publish(event.clone());
        
    }    
    
    /// Notify all listeners that the capabilities have changed
    pub fn notify_capabilities_changed(&self, capabilities: &PlayerCapabilitySet) {
        let player_name = self.get_player_name();
        let player_id = self.get_player_id();
        
        debug!("Notifying listeners of capabilities change");
        
        // Store the capabilities internally
        let mut caps = self.capabilities.write();
        *caps = *capabilities;
        debug!("Updated capabilities");
        drop(caps);
        
        let source = PlayerSource::new(player_name, player_id);
        
        let event = PlayerEvent::CapabilitiesChanged {
            source,
            capabilities: *capabilities,
        };
        
        // Publish to the global event bus
        debug!("Publishing capabilities change event to the global event bus");
        crate::audiocontrol::eventbus::EventBus::instance().publish(event.clone());
        
    }    
    
    /// Notify all registered listeners that the player position has changed
    pub fn notify_position_changed(&self, position: f64) {
        let player_name = self.get_player_name();
        let player_id = self.get_player_id();
        
        let source = PlayerSource::new(player_name, player_id);
        
        let event = PlayerEvent::PositionChanged {
            source,
            position,
        };
        
        // Publish to the global event bus
        debug!("Publishing position change event to the global event bus");
        crate::audiocontrol::eventbus::EventBus::instance().publish(event.clone());
    }

    /// Create a PlayerSource object for the current player
    pub fn create_player_source(&self) -> PlayerSource {
        PlayerSource::new(self.get_player_name(), self.get_player_id())
    }    
    
    /// Notify listeners that the database is being updated
    pub fn notify_database_update(&self, artist: Option<String>, album: Option<String>,
                                song: Option<String>, percentage: Option<f32>) {
        let event = PlayerEvent::DatabaseUpdating {
            source: self.create_player_source(),
            artist,
            album,
            song,
            percentage,
        };
        
        // Publish to the global event bus
        debug!("Publishing database update event to the global event bus");
        crate::audiocontrol::eventbus::EventBus::instance().publish(event.clone());
        
    }    
    
    /// Notify listeners that the player's queue has changed
    pub fn notify_queue_changed(&self) {
        let event = PlayerEvent::QueueChanged {
            source: self.create_player_source(),
        };
        
        // Publish to the global event bus
        debug!("Publishing queue changed event to the global event bus");
        crate::audiocontrol::eventbus::EventBus::instance().publish(event.clone());
        
    }
    
    /// Notify listeners that the active player has changed
    pub fn notify_active_player_changed(&self, player_id: String) {
        let event = PlayerEvent::ActivePlayerChanged {
            source: self.create_player_source(),
            player_id,
        };
        
        // Publish to the global event bus
        debug!("Publishing active player changed event to the global event bus");
        crate::audiocontrol::eventbus::EventBus::instance().publish(event.clone());
        
    }

    /// Get the last time this player was seen active
    pub fn get_last_seen(&self) -> Option<SystemTime> {
        self.player_state.read().last_seen
    }

    /// Update the last_seen timestamp for this player
    /// 
    /// This should be called by player implementations whenever they are accessed
    /// or when they update their status to indicate that the player is still active.
    pub fn alive(&self) {
        let mut state = self.player_state.write();
        state.last_seen = Some(SystemTime::now());
        debug!("Updated last_seen timestamp for player {}:{}",
              self.get_player_name(), self.get_player_id());
    }

    /// Get the current playback position
    /// Implementation for the PlayerController trait
    pub fn get_position(&self) -> Option<f64> {
        self.player_state.read().position
    }
}