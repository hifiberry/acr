use crate::data::{PlayerState, Song, LoopMode, PlayerCapability};
use crate::players::player_controller::PlayerStateListener;
use std::sync::{Arc, Weak, RwLock};
use log::{debug, trace, warn};

/// Base implementation of PlayerController that handles state listener management
/// 
/// This struct provides common functionality for managing state listeners that
/// can be used by concrete player implementations.
pub struct BasePlayerController {
    /// List of state listeners registered with this controller
    listeners: RwLock<Vec<Weak<dyn PlayerStateListener>>>,
}

impl BasePlayerController {
    /// Create a new BasePlayerController with no listeners
    pub fn new() -> Self {
        debug!("Creating new BasePlayerController");
        Self {
            listeners: RwLock::new(Vec::new()),
        }
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

    /// Notify all registered listeners that the current song has changed
    pub fn notify_song_changed(&self, song: Option<&Song>) {
        debug!("Notifying listeners of song change: {:?}", song.map(|s| s.title.as_deref().unwrap_or("Unknown")));
        self.prune_dead_listeners();
        if let Ok(listeners) = self.listeners.read() {
            debug!("Notifying {} listeners of song change", listeners.len());
            for listener_weak in listeners.iter() {
                if let Some(listener) = listener_weak.upgrade() {
                    trace!("Notifying listener of song change");
                    listener.on_song_changed(song);
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

    /// Notify all registered listeners that the capabilities have changed
    pub fn notify_capabilities_changed(&self, capabilities: &[PlayerCapability]) {
        debug!("Notifying listeners of capabilities change");
        self.prune_dead_listeners();
        if let Ok(listeners) = self.listeners.read() {
            debug!("Notifying {} listeners of capabilities change", listeners.len());
            for listener_weak in listeners.iter() {
                if let Some(listener) = listener_weak.upgrade() {
                    trace!("Notifying listener of capabilities change");
                    listener.on_capabilities_changed(capabilities);
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
}