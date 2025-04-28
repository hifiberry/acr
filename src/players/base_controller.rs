use crate::data::{PlayerState, Song, LoopMode, PlayerCapability};
use crate::players::player_controller::PlayerStateListener;
use std::sync::{Arc, Weak, RwLock};

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
        Self {
            listeners: RwLock::new(Vec::new()),
        }
    }

    /// Notify all registered listeners that the player state has changed
    pub fn notify_state_changed(&self, state: PlayerState) {
        self.prune_dead_listeners();
        if let Ok(listeners) = self.listeners.read() {
            for listener_weak in listeners.iter() {
                if let Some(listener) = listener_weak.upgrade() {
                    listener.on_state_changed(state);
                }
            }
        }
    }

    /// Notify all registered listeners that the current song has changed
    pub fn notify_song_changed(&self, song: Option<&Song>) {
        self.prune_dead_listeners();
        if let Ok(listeners) = self.listeners.read() {
            for listener_weak in listeners.iter() {
                if let Some(listener) = listener_weak.upgrade() {
                    listener.on_song_changed(song);
                }
            }
        }
    }

    /// Notify all registered listeners that the loop mode has changed
    pub fn notify_loop_mode_changed(&self, mode: LoopMode) {
        self.prune_dead_listeners();
        if let Ok(listeners) = self.listeners.read() {
            for listener_weak in listeners.iter() {
                if let Some(listener) = listener_weak.upgrade() {
                    listener.on_loop_mode_changed(mode);
                }
            }
        }
    }

    /// Notify all registered listeners that the capabilities have changed
    pub fn notify_capabilities_changed(&self, capabilities: &[PlayerCapability]) {
        self.prune_dead_listeners();
        if let Ok(listeners) = self.listeners.read() {
            for listener_weak in listeners.iter() {
                if let Some(listener) = listener_weak.upgrade() {
                    listener.on_capabilities_changed(capabilities);
                }
            }
        }
    }

    /// Register a state listener to be notified of state changes
    pub fn register_listener(&self, listener: Weak<dyn PlayerStateListener>) -> bool {
        if let Ok(mut listeners) = self.listeners.write() {
            // Check for duplicates before adding
            for existing in listeners.iter() {
                if let (Some(new), Some(old)) = (listener.upgrade(), existing.upgrade()) {
                    // Compare pointers to check if they're the same object
                    if Arc::ptr_eq(&new, &old) {
                        return false;
                    }
                }
            }
            listeners.push(listener);
            return true;
        }
        false
    }

    /// Unregister a previously registered state listener
    pub fn unregister_listener(&self, listener: &Arc<dyn PlayerStateListener>) -> bool {
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
            return listeners.len() < original_len;
        }
        false
    }

    /// Remove any dead (dropped) listeners
    fn prune_dead_listeners(&self) {
        if let Ok(mut listeners) = self.listeners.write() {
            listeners.retain(|weak_ref| weak_ref.upgrade().is_some());
        }
    }
}