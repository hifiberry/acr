use crate::data::{PlayerState, Song, LoopMode, PlayerCapability};
use crate::players::player_controller::PlayerStateListener;
use std::sync::{Arc, Weak, RwLock};
use log::{debug, trace, warn};

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
}

impl BasePlayerController {
    /// Create a new BasePlayerController with no listeners
    pub fn new() -> Self {
        debug!("Creating new BasePlayerController");
        Self {
            listeners: Arc::new(RwLock::new(Vec::new())),
            capabilities: Arc::new(RwLock::new(Vec::new())),
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
        debug!("Notifying listeners of state change: {}", state);
        self.prune_dead_listeners();
        if let Ok(listeners) = self.listeners.read() {
            debug!("Notifying {} listeners of state change", listeners.len());
            for listener_weak in listeners.iter() {
                if let Some(listener) = listener_weak.upgrade() {
                    trace!("Notifying listener of state change");
                    listener.on_state_changed(state);
                }
            }
        } else {
            warn!("Failed to acquire read lock for listeners when notifying state change");
        }
    }

    /// Notify all listeners that the song has changed
    pub fn notify_song_changed(&self, song: Option<&Song>) {
        debug!("Notifying listeners of song change");
        self.prune_dead_listeners();
        
        // Create a cloned version of the song to pass to listeners
        let song_copy = song.cloned();
        
        if let Ok(listeners) = self.listeners.read() {
            debug!("Notifying {} listeners of song change", listeners.len());
            for listener_weak in listeners.iter() {
                if let Some(listener) = listener_weak.upgrade() {
                    trace!("Notifying listener of song change");
                    listener.on_song_changed(song_copy.clone());
                }
            }
        } else {
            warn!("Failed to acquire read lock for listeners when notifying song change");
        }
    }

    /// Notify all registered listeners that the loop mode has changed
    pub fn notify_loop_mode_changed(&self, mode: LoopMode) {
        debug!("Notifying listeners of loop mode change: {}", mode);
        self.prune_dead_listeners();
        if let Ok(listeners) = self.listeners.read() {
            debug!("Notifying {} listeners of loop mode change", listeners.len());
            for listener_weak in listeners.iter() {
                if let Some(listener) = listener_weak.upgrade() {
                    trace!("Notifying listener of loop mode change");
                    listener.on_loop_mode_changed(mode);
                }
            }
        } else {
            warn!("Failed to acquire read lock for listeners when notifying loop mode change");
        }
    }

    /// Notify all listeners that the capabilities have changed
    pub fn notify_capabilities_changed(&self, capabilities: &[PlayerCapability]) {
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
        
        if let Ok(listeners) = self.listeners.read() {
            debug!("Notifying {} listeners of capabilities change", listeners.len());
            for listener_weak in listeners.iter() {
                if let Some(listener) = listener_weak.upgrade() {
                    trace!("Notifying listener of capabilities change");
                    listener.on_capabilities_changed(capabilities_vec.clone());
                }
            }
        } else {
            warn!("Failed to acquire read lock for listeners when notifying capabilities change");
        }
    }

    /// Register a state listener to be notified of state changes
    pub fn register_state_listener(&self, listener: Weak<dyn PlayerStateListener>) -> bool {
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
    pub fn unregister_state_listener(&self, listener: &Arc<dyn PlayerStateListener>) -> bool {
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
}