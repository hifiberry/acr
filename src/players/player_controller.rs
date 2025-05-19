use crate::data::{PlayerCapability, PlayerCapabilitySet, Song, Track, LoopMode, PlaybackState, PlayerCommand, PlayerEvent, PlayerSource, PlayerState, PlayerUpdate};
use crate::data::library::LibraryInterface;
use std::sync::{Arc, Weak, RwLock};
use std::any::Any;
use std::time::SystemTime;
use log::{debug, trace, warn};

/// Trait for objects that listener to PlayerController state changes
pub trait PlayerStateListener: Send + Sync {
    /// Called when any player event occurs
    /// 
    /// # Arguments
    /// 
    /// * `event` - The event that occurred
    fn on_event(&self, event: PlayerEvent);
    
    /// Convert to Any for dynamic casting
    fn as_any(&self) -> &dyn Any;
}

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
    
    /// Register a state listener to be notified of state changes
    /// 
    /// # Arguments
    /// 
    /// * `listener` - The listener to register
    /// 
    /// # Returns
    /// 
    /// `true` if the listener was successfully registered, `false` otherwise
    fn register_state_listener(&mut self, listener: Weak<dyn PlayerStateListener>) -> bool;
    
    /// Unregister a previously registered state listener
    /// 
    /// # Arguments
    /// 
    /// * `listener` - The listener to unregister
    /// 
    /// # Returns
    /// 
    /// `true` if the listener was successfully unregistered, `false` if it wasn't registered
    fn unregister_state_listener(&mut self, listener: &Arc<dyn PlayerStateListener>) -> bool;
    
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
}

/// Base implementation of PlayerController that handles state listener management
/// 
/// This struct provides common functionality for managing state listeners that
/// can be used by concrete player implementations.
#[derive(Clone)]
pub struct BasePlayerController {
    /// List of state listeners registered with this controller
    listeners: Arc<RwLock<Vec<Weak<dyn PlayerStateListener>>>>,
    
    /// Current capabilities of the player
    capabilities: Arc<RwLock<PlayerCapabilitySet>>,
    
    /// Player name identifier (e.g., "mpd", "null")
    player_name: Arc<RwLock<String>>,
    
    /// Player unique ID (e.g., "hostname:port" for MPD)
    player_id: Arc<RwLock<String>>,
    
    /// Player state
    player_state: Arc<RwLock<PlayerState>>,
}

impl BasePlayerController {
    /// Create a new BasePlayerController with no listeners
    pub fn new() -> Self {
        debug!("Creating new BasePlayerController");
        Self {
            listeners: Arc::new(RwLock::new(Vec::new())),
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
            listeners: Arc::new(RwLock::new(Vec::new())),
            capabilities: Arc::new(RwLock::new(PlayerCapabilitySet::empty())),
            player_name: Arc::new(RwLock::new(name.to_string())),
            player_id: Arc::new(RwLock::new(id.to_string())),
            player_state: Arc::new(RwLock::new(PlayerState::new())),
        }
    }
    
    /// Set the player name
    pub fn set_player_name(&self, name: &str) {
        if let Ok(mut player_name) = self.player_name.write() {
            *player_name = name.to_string();
            debug!("Player name set to '{}'", name);
        } else {
            warn!("Failed to acquire write lock when setting player name");
        }
    }
    
    /// Set the player ID
    pub fn set_player_id(&self, id: &str) {
        if let Ok(mut player_id) = self.player_id.write() {
            *player_id = id.to_string();
            debug!("Player ID set to '{}'", id);
        } else {
            warn!("Failed to acquire write lock when setting player ID");
        }
    }
    
    /// Get the player name
    pub fn get_player_name(&self) -> String {
        if let Ok(player_name) = self.player_name.read() {
            player_name.clone()
        } else {
            warn!("Failed to acquire read lock for player name");
            "unknown".to_string()
        }
    }
    
    /// Get the player ID
    pub fn get_player_id(&self) -> String {
        if let Ok(player_id) = self.player_id.read() {
            player_id.clone()
        } else {
            warn!("Failed to acquire read lock for player ID");
            "unknown".to_string()
        }
    }
    
    /// Get the current capabilities
    pub fn get_capabilities(&self) -> PlayerCapabilitySet {
        if let Ok(caps) = self.capabilities.read() {
            *caps
        } else {
            warn!("Failed to acquire read lock for capabilities");
            PlayerCapabilitySet::empty()
        }
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
        if let Ok(mut caps) = self.capabilities.write() {
            // Check if there's any difference
            if *caps != capabilities {
                // Replace with new capabilities
                *caps = capabilities;
                debug!("Updated capabilities");
                changed = true;
            } else {
                debug!("Capabilities unchanged, not updating");
            }
        } else {
            warn!("Failed to acquire write lock when setting capabilities");
            return false;
        }
        
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
        if let Ok(mut caps) = self.capabilities.write() {
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
        } else {
            warn!("Failed to acquire write lock when setting capability");
            return false;
        }
        
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
        
        debug!("Notifying listeners of state change: {}", state);
        self.prune_dead_listeners();
        
        let source = PlayerSource::new(player_name, player_id);
        
        let event = PlayerEvent::StateChanged {
            source,
            state,
        };
        
        // Publish to the global event bus
        debug!("Publishing state change event to the global event bus");
        crate::audiocontrol::eventbus::EventBus::instance().publish(event.clone());
        
        if let Ok(listeners) = self.listeners.read() {
            debug!("Notifying {} listeners of state change", listeners.len());
            for listener_weak in listeners.iter() {
                if let Some(listener) = listener_weak.upgrade() {
                    trace!("Notifying listener of state change");
                    listener.on_event(event.clone());
                }
            }
        } else {
            warn!("Failed to acquire read lock for listeners when notifying state change");
        }
    }    /// Notify all listeners that the song has changed
    pub fn notify_song_changed(&self, song: Option<&Song>) {
        let player_name = self.get_player_name();
        let player_id = self.get_player_id();
        
        debug!("Notifying listeners of song change");
        self.prune_dead_listeners();
        
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
        
        if let Ok(listeners) = self.listeners.read() {
            debug!("Notifying {} listeners of song change", listeners.len());
            for listener_weak in listeners.iter() {
                if let Some(listener) = listener_weak.upgrade() {
                    trace!("Notifying listener of song change");
                    listener.on_event(event.clone());
                }
            }
        } else {
            warn!("Failed to acquire read lock for listeners when notifying song change");
        }
    }    /// Notify all registered listeners that the loop mode has changed
    pub fn notify_loop_mode_changed(&self, mode: LoopMode) {
        let player_name = self.get_player_name();
        let player_id = self.get_player_id();
        
        debug!("Notifying listeners of loop mode change: {}", mode);
        self.prune_dead_listeners();
        
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
        self.prune_dead_listeners();
        
        let source = PlayerSource::new(player_name, player_id);
        
        let event = PlayerEvent::RandomChanged {
            source,
            enabled,
        };
        
        // Publish to the global event bus
        debug!("Publishing random mode change event to the global event bus");
        crate::audiocontrol::eventbus::EventBus::instance().publish(event.clone());
        
        if let Ok(listeners) = self.listeners.read() {
            debug!("Notifying {} listeners of random mode change", listeners.len());
            for listener_weak in listeners.iter() {
                if let Some(listener) = listener_weak.upgrade() {
                    trace!("Notifying listener of random mode change");
                    listener.on_event(event.clone());
                }
            }
        } else {
            warn!("Failed to acquire read lock for listeners when notifying random mode change");
        }
    }    /// Notify all listeners that the capabilities have changed
    pub fn notify_capabilities_changed(&self, capabilities: &PlayerCapabilitySet) {
        let player_name = self.get_player_name();
        let player_id = self.get_player_id();
        
        debug!("Notifying listeners of capabilities change");
        self.prune_dead_listeners();
        
        // Store the capabilities internally
        if let Ok(mut caps) = self.capabilities.write() {
            *caps = *capabilities;
            debug!("Updated capabilities");
        } else {
            warn!("Failed to acquire write lock when updating capabilities");
        }
        
        let source = PlayerSource::new(player_name, player_id);
        
        let event = PlayerEvent::CapabilitiesChanged {
            source,
            capabilities: *capabilities,
        };
        
        // Publish to the global event bus
        debug!("Publishing capabilities change event to the global event bus");
        crate::audiocontrol::eventbus::EventBus::instance().publish(event.clone());
        
        // do not notify listeners anymore

    }    /// Notify all registered listeners that the player position has changed
    pub fn notify_position_changed(&self, position: f64) {
        let player_name = self.get_player_name();
        let player_id = self.get_player_id();
        
        debug!("Notifying listeners of position change: {:.1}s", position);
        self.prune_dead_listeners();
        
        let source = PlayerSource::new(player_name, player_id);
        
        let event = PlayerEvent::PositionChanged {
            source,
            position,
        };
        
        // Publish to the global event bus
        debug!("Publishing position change event to the global event bus");
        crate::audiocontrol::eventbus::EventBus::instance().publish(event.clone());
        
        // do not notifiy listeners anymore
    }

    /// Create a PlayerSource object for the current player
    pub fn create_player_source(&self) -> PlayerSource {
        PlayerSource::new(self.get_player_name(), self.get_player_id())
    }    /// Broadcast an event to all registered listeners
    pub fn broadcast_event(&self, event: PlayerEvent) {
        self.prune_dead_listeners();
        
        // Note: We're intentionally not publishing to the event bus here
        // since this method is called by other notify_ methods that already publish to the event bus.
        // If broadcast_event is called directly (outside of a notify_ method), the caller should
        // handle publishing to the event bus if needed.
        
        if let Ok(listeners) = self.listeners.read() {
            debug!("Broadcasting event to {} listeners: {:?}", listeners.len(), event);
            for listener_weak in listeners.iter() {
                if let Some(listener) = listener_weak.upgrade() {
                    trace!("Notifying listener of event");
                    listener.on_event(event.clone());
                }
            }
        } else {
            warn!("Failed to acquire read lock for listeners when broadcasting event");
        }
    }/// Notify listeners that the database is being updated
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
        
        // Broadcast to registered listeners
        self.broadcast_event(event);
    }    /// Notify listeners that the player's queue has changed
    pub fn notify_queue_changed(&self) {
        let event = PlayerEvent::QueueChanged {
            source: self.create_player_source(),
        };
        
        // Publish to the global event bus
        debug!("Publishing queue changed event to the global event bus");
        crate::audiocontrol::eventbus::EventBus::instance().publish(event.clone());
        
        // Broadcast to registered listeners
        self.broadcast_event(event);
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
        
        // Broadcast to registered listeners
        self.broadcast_event(event);
    }

    /// Get the last time this player was seen active
    pub fn get_last_seen(&self) -> Option<SystemTime> {
        if let Ok(state) = self.player_state.read() {
            state.last_seen
        } else {
            warn!("Failed to acquire read lock for player state");
            None
        }
    }

    /// Register a state listener to be notified of state changes
    pub fn register_listener(&self, listener: Weak<dyn PlayerStateListener>) -> bool {
        debug!("Attempting to register a new listener");
        if let Ok(mut listeners) = self.listeners.write() {
            // Check for duplicates before adding
            for existing in listeners.iter() {
                if let (Some(new), Some(old)) = (listener.upgrade(), existing.upgrade()) {
                    // Compare pointers to check if they're the same object
                    if Arc::ptr_eq(&new, &old) {
                        debug!("Listener already registered, skipping");
                        return false;
                    }
                }
            }
            listeners.push(listener);
            debug!("Listener successfully registered, total listeners: {}", listeners.len());
            return true;
        }
        warn!("Failed to acquire write lock when registering listener");
        false
    }

    /// Unregister a previously registered state listener
    pub fn unregister_listener(&self, listener: &Arc<dyn PlayerStateListener>) -> bool {
        debug!("Attempting to unregister a listener");
        if let Ok(mut listeners) = self.listeners.write() {
            let original_len = listeners.len();
            // Remove all weak references that point to the same object or are dead
            listeners.retain(|weak_ref| {
                if let Some(target) = weak_ref.upgrade() {
                    !Arc::ptr_eq(&target, listener)
                } else {
                    false // Remove dead weak references
                }
            });
            let removed = listeners.len() < original_len;
            if removed {
                debug!("Listener successfully unregistered, remaining listeners: {}", listeners.len());
            } else {
                debug!("Listener not found for unregistration");
            }
            return removed;
        }
        warn!("Failed to acquire write lock when unregistering listener");
        false
    }

    /// Remove any dead (dropped) listeners
    fn prune_dead_listeners(&self) {
        trace!("Pruning dead listeners");
        if let Ok(mut listeners) = self.listeners.write() {
            let original_len = listeners.len();
            listeners.retain(|weak_ref| weak_ref.upgrade().is_some());
            let removed = original_len - listeners.len();
            if removed > 0 {
                debug!("Pruned {} dead listeners, remaining: {}", removed, listeners.len());
            }
        } else {
            warn!("Failed to acquire write lock when pruning dead listeners");
        }
    }

    /// Register a state listener to be notified of state changes
    /// This is an alias for register_listener to match the PlayerController trait
    pub fn register_state_listener(&mut self, listener: Weak<dyn PlayerStateListener>) -> bool {
        self.register_listener(listener)
    }

    /// Unregister a previously registered state listener
    /// This is an alias for unregister_listener to match the PlayerController trait
    pub fn unregister_state_listener(&mut self, listener: &Arc<dyn PlayerStateListener>) -> bool {
        self.unregister_listener(listener)
    }
    
    /// Update the last_seen timestamp for this player
    /// 
    /// This should be called by player implementations whenever they are accessed
    /// or when they update their status to indicate that the player is still active.
    pub fn alive(&self) {
        if let Ok(mut state) = self.player_state.write() {
            state.last_seen = Some(SystemTime::now());
            debug!("Updated last_seen timestamp for player {}:{}", 
                  self.get_player_name(), self.get_player_id());
        } else {
            warn!("Failed to acquire write lock for updating last_seen timestamp");
        }
    }

    /// Get the current playback position
    /// Implementation for the PlayerController trait
    pub fn get_position(&self) -> Option<f64> {
        if let Ok(state) = self.player_state.read() {
            state.position
        } else {
            warn!("Failed to acquire read lock for player state");
            None
        }
    }
}