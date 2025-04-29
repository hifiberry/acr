use crate::data::{PlayerCapability, Song, LoopMode, PlayerState, PlayerCommand, PlayerEvent, PlayerSource};
use std::sync::{Arc, Weak, RwLock};
use std::any::Any;
use log::{debug, trace, warn};

/// Trait for objects that listen to PlayerController state changes
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
    /// Returns a vector of capabilities supported by this player
    fn get_capabilities(&self) -> Vec<PlayerCapability>;
    
    /// Get the current song being played
    /// 
    /// Returns the current song, or None if no song is playing
    fn get_song(&self) -> Option<Song>;
    
    /// Get the current loop mode setting
    /// 
    /// Returns the current loop mode of the player
    fn get_loop_mode(&self) -> LoopMode;
    
    /// Get the current player state
    /// 
    /// Returns the current state of the player (playing, paused, stopped, etc.)
    fn get_player_state(&self) -> PlayerState;
    
    /// Get the name of this player controller
    /// 
    /// Returns a string identifier for this type of player (e.g., "mpd", "null")
    fn get_player_name(&self) -> String;
    
    /// Get a unique identifier for this player instance
    /// 
    /// Returns a string that uniquely identifies this player instance
    fn get_player_id(&self) -> String;
    
    /// Send a command to the player
    /// 
    /// # Arguments
    /// 
    /// * `command` - The command to send to the player
    /// 
    /// # Returns
    /// 
    /// `true` if the command was successfully processed, `false` otherwise
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
    capabilities: Arc<RwLock<Vec<PlayerCapability>>>,
    
    /// Player name identifier (e.g., "mpd", "null")
    player_name: Arc<RwLock<String>>,
    
    /// Player unique ID (e.g., "hostname:port" for MPD)
    player_id: Arc<RwLock<String>>,
}

impl BasePlayerController {
    /// Create a new BasePlayerController with no listeners
    pub fn new() -> Self {
        debug!("Creating new BasePlayerController");
        Self {
            listeners: Arc::new(RwLock::new(Vec::new())),
            capabilities: Arc::new(RwLock::new(Vec::new())),
            player_name: Arc::new(RwLock::new("unknown".to_string())),
            player_id: Arc::new(RwLock::new("unknown".to_string())),
        }
    }
    
    /// Initialize the controller with player name and ID
    pub fn with_player_info(name: &str, id: &str) -> Self {
        debug!("Creating BasePlayerController with name='{}', id='{}'", name, id);
        Self {
            listeners: Arc::new(RwLock::new(Vec::new())),
            capabilities: Arc::new(RwLock::new(Vec::new())),
            player_name: Arc::new(RwLock::new(name.to_string())),
            player_id: Arc::new(RwLock::new(id.to_string())),
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
    pub fn get_capabilities(&self) -> Vec<PlayerCapability> {
        if let Ok(caps) = self.capabilities.read() {
            caps.clone()
        } else {
            warn!("Failed to acquire read lock for capabilities");
            Vec::new()
        }
    }
    
    /// Set multiple capabilities at once
    /// 
    /// Replaces all current capabilities with the provided ones
    /// When auto_notify is true, listeners will be notified of changes automatically
    /// Returns true if the capabilities were changed
    pub fn set_capabilities(&self, capabilities: Vec<PlayerCapability>, auto_notify: bool) -> bool {
        debug!("Setting all capabilities to a list of {} capabilities", capabilities.len());
        
        let mut changed = false;
        
        // Update stored capabilities
        if let Ok(mut caps) = self.capabilities.write() {
            // Check if there's any difference between current and new capabilities
            if caps.len() != capabilities.len() || 
               !capabilities.iter().all(|cap| caps.contains(cap)) ||
               !caps.iter().all(|cap| capabilities.contains(cap)) {
                
                // Replace with new capabilities
                *caps = capabilities.clone();
                debug!("Updated capabilities to {} items", caps.len());
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
            let had_capability = caps.contains(&capability);
            
            if enabled && !had_capability {
                // Add capability
                caps.push(capability.clone());
                debug!("Added capability {:?}, now have {} capabilities", capability, caps.len());
                changed = true;
            } else if !enabled && had_capability {
                // Remove capability
                caps.retain(|c| *c != capability);
                debug!("Removed capability {:?}, now have {} capabilities", capability, caps.len());
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
    }

    /// Notify all registered listeners that the player state has changed
    pub fn notify_state_changed(&self, state: PlayerState) {
        let player_name = self.get_player_name();
        let player_id = self.get_player_id();
        
        debug!("Notifying listeners of state change: {}", state);
        self.prune_dead_listeners();
        
        let source = PlayerSource::new(player_name, player_id);
        
        let event = PlayerEvent::StateChanged {
            source,
            state,
        };
        
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
    }

    /// Notify all listeners that the song has changed
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
    }

    /// Notify all registered listeners that the loop mode has changed
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
        
        if let Ok(listeners) = self.listeners.read() {
            debug!("Notifying {} listeners of loop mode change", listeners.len());
            for listener_weak in listeners.iter() {
                if let Some(listener) = listener_weak.upgrade() {
                    trace!("Notifying listener of loop mode change");
                    listener.on_event(event.clone());
                }
            }
        } else {
            warn!("Failed to acquire read lock for listeners when notifying loop mode change");
        }
    }

    /// Notify all listeners that the capabilities have changed
    pub fn notify_capabilities_changed(&self, capabilities: &[PlayerCapability]) {
        let player_name = self.get_player_name();
        let player_id = self.get_player_id();
        
        debug!("Notifying listeners of capabilities change");
        self.prune_dead_listeners();
        
        // Store the capabilities internally
        if let Ok(mut caps) = self.capabilities.write() {
            *caps = capabilities.to_vec();
            debug!("Updated to {} capabilities", caps.len());
        } else {
            warn!("Failed to acquire write lock when updating capabilities");
        }
        
        // Create a copied vector for each listener
        let capabilities_vec = capabilities.to_vec();
        
        let source = PlayerSource::new(player_name, player_id);
        
        let event = PlayerEvent::CapabilitiesChanged {
            source,
            capabilities: capabilities_vec,
        };
        
        if let Ok(listeners) = self.listeners.read() {
            debug!("Notifying {} listeners of capabilities change", listeners.len());
            for listener_weak in listeners.iter() {
                if let Some(listener) = listener_weak.upgrade() {
                    trace!("Notifying listener of capabilities change");
                    listener.on_event(event.clone());
                }
            }
        } else {
            warn!("Failed to acquire read lock for listeners when notifying capabilities change");
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
}