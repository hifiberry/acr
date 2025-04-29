use crate::players::PlayerController;
use crate::players::PlayerStateListener;
use crate::data::{PlayerCommand, PlayerCapability, Song, LoopMode, PlayerState, PlayerEvent, PlayerSource};
use crate::players::{create_player_from_json, PlayerCreationError};
use serde_json::Value;
use std::sync::{Arc, RwLock, Weak};
use std::any::Any;
use log::{debug, warn, error};

/// A simple AudioController that manages multiple PlayerController instances
#[derive(Clone)]
pub struct AudioController {
    /// List of player controllers
    controllers: Vec<Arc<RwLock<Box<dyn PlayerController + Send + Sync>>>>,
    
    /// Index of the active player controller in the list
    active_index: Option<usize>,
    
    /// List of state listeners registered with this controller
    listeners: Arc<RwLock<Vec<Weak<dyn PlayerStateListener>>>>,
    
    /// Self-reference for registering with players
    /// This is wrapped in Option because it's initialized after construction
    self_ref: Arc<RwLock<Option<Weak<AudioController>>>>,
}

// Implement PlayerController for AudioController
impl PlayerController for AudioController {
    fn get_capabilities(&self) -> Vec<PlayerCapability> {
        if let Some(idx) = self.active_index {
            if let Ok(controller) = self.controllers[idx].read() {
                return controller.get_capabilities();
            }
        }
        Vec::new() // Return empty capabilities if no active controller
    }
    
    fn get_song(&self) -> Option<Song> {
        if let Some(idx) = self.active_index {
            if let Ok(controller) = self.controllers[idx].read() {
                return controller.get_song();
            }
        }
        None // Return None if no active controller
    }
    
    fn get_loop_mode(&self) -> LoopMode {
        if let Some(idx) = self.active_index {
            if let Ok(controller) = self.controllers[idx].read() {
                return controller.get_loop_mode();
            }
        }
        LoopMode::None // Default loop mode if no active controller
    }
    
    fn get_player_state(&self) -> PlayerState {
        if let Some(idx) = self.active_index {
            if let Ok(controller) = self.controllers[idx].read() {
                return controller.get_player_state();
            }
        }
        PlayerState::Stopped // Default state if no active controller
    }
    
    fn get_player_name(&self) -> String {
        if let Some(idx) = self.active_index {
            if let Ok(controller) = self.controllers[idx].read() {
                return controller.get_player_name();
            }
        }
        "audiocontroller".to_string() // Default name if no active controller
    }
    
    fn get_player_id(&self) -> String {
        if let Some(idx) = self.active_index {
            if let Ok(controller) = self.controllers[idx].read() {
                return controller.get_player_id();
            }
        }
        "none".to_string() // Default ID if no active controller
    }
    
    fn send_command(&self, command: PlayerCommand) -> bool {
        if let Some(idx) = self.active_index {
            if let Ok(controller) = self.controllers[idx].read() {
                return controller.send_command(command);
            }
        }
        false // Return false if no active controller
    }
    
    fn register_state_listener(&mut self, listener: Weak<dyn crate::players::PlayerStateListener>) -> bool {
        if let Ok(mut listeners) = self.listeners.write() {
            listeners.push(listener);
            true
        } else {
            false
        }
    }
    
    fn unregister_state_listener(&mut self, listener: &Arc<dyn crate::players::PlayerStateListener>) -> bool {
        if let Ok(mut listeners) = self.listeners.write() {
            let original_len = listeners.len();
            listeners.retain(|weak_ref| weak_ref.upgrade().map_or(false, |l| !Arc::ptr_eq(&l, listener)));
            original_len != listeners.len()
        } else {
            false
        }
    }
    
    fn as_any(&self) -> &dyn Any {
        self
    }
    
    fn start(&self) -> bool {
        if let Some(idx) = self.active_index {
            if let Ok(controller) = self.controllers[idx].read() {
                return controller.start();
            }
        }
        false // Return false if no active controller
    }
    
    fn stop(&self) -> bool {
        if let Some(idx) = self.active_index {
            if let Ok(controller) = self.controllers[idx].read() {
                return controller.stop();
            }
        }
        false // Return false if no active controller
    }
}

// Implement PlayerStateListener for AudioController
impl PlayerStateListener for AudioController {
    fn on_event(&self, event: PlayerEvent) {
        match event {
            PlayerEvent::StateChanged { source, state } => {
                // Check if the event is from the active player
                if self.is_active_player(&source.player_name, &source.player_id) {
                    debug!("AudioController forwarding state change from active player {}: {}", source.player_id, state);
                    self.forward_state_changed(source.player_name, source.player_id, state);
                } else {
                    debug!("AudioController ignoring state change from inactive player {}", source.player_id);
                }
            },
            PlayerEvent::SongChanged { source, song } => {
                // Check if the event is from the active player
                if self.is_active_player(&source.player_name, &source.player_id) {
                    let song_title = song.as_ref().map_or("None".to_string(), |s| s.title.as_deref().unwrap_or("Unknown").to_string());
                    debug!("AudioController forwarding song change from active player {}: {}", source.player_id, song_title);
                    self.forward_song_changed(source.player_name, source.player_id, song);
                } else {
                    debug!("AudioController ignoring song change from inactive player {}", source.player_id);
                }
            },
            PlayerEvent::LoopModeChanged { source, mode } => {
                // Check if the event is from the active player
                if self.is_active_player(&source.player_name, &source.player_id) {
                    debug!("AudioController forwarding loop mode change from active player {}: {}", source.player_id, mode);
                    self.forward_loop_mode_changed(source.player_name, source.player_id, mode);
                } else {
                    debug!("AudioController ignoring loop mode change from inactive player {}", source.player_id);
                }
            },
            PlayerEvent::CapabilitiesChanged { source, capabilities } => {
                // Check if the event is from the active player
                if self.is_active_player(&source.player_name, &source.player_id) {
                    debug!("AudioController forwarding capabilities change from active player {}", source.player_id);
                    self.forward_capabilities_changed(source.player_name, source.player_id, capabilities);
                } else {
                    debug!("AudioController ignoring capabilities change from inactive player {}", source.player_id);
                }
            },
        }
    }
    
    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl AudioController {
    /// Create a new AudioController with no controllers
    pub fn new() -> Self {
        Self {
            controllers: Vec::new(),
            active_index: None,
            listeners: Arc::new(RwLock::new(Vec::new())),
            self_ref: Arc::new(RwLock::new(None)),
        }
    }
    
    /// Initialize the controller with a strong reference to itself
    pub fn initialize(controller: &Arc<AudioController>) {
        let weak_ref = Arc::downgrade(controller);
        if let Ok(mut self_ref) = controller.self_ref.write() {
            *self_ref = Some(weak_ref);
            debug!("AudioController self-reference initialized");
        } else {
            warn!("Failed to initialize AudioController self-reference");
        }
    }
    
    /// Add a player controller to the list
    /// 
    /// If this is the first controller added, it becomes the active controller.
    pub fn add_controller(&mut self, controller: Box<dyn PlayerController + Send + Sync>) -> usize {
        // Check if we have a self reference for listener registration
        let self_weak = if let Ok(self_ref) = self.self_ref.read() {
            if let Some(weak_ref) = self_ref.as_ref() {
                weak_ref.clone() as Weak<dyn PlayerStateListener>
            } else {
                warn!("AudioController self-reference not initialized, cannot register as listener");
                // Continue without registering as listener
                let controller = controller;
                let controller = Arc::new(RwLock::new(controller));
                self.controllers.push(controller);
                
                if self.controllers.len() == 1 {
                    self.active_index = Some(0);
                }
                
                return self.controllers.len() - 1;
            }
        } else {
            warn!("Failed to acquire read lock for self-reference, cannot register as listener");
            // Continue without registering as listener
            let controller = controller;
            let controller = Arc::new(RwLock::new(controller));
            self.controllers.push(controller);
            
            if self.controllers.len() == 1 {
                self.active_index = Some(0);
            }
            
            return self.controllers.len() - 1;
        };
        
        // Register self as listener
        let mut controller = controller;
        if controller.register_state_listener(self_weak) {
            debug!("AudioController registered as listener to player");
        } else {
            warn!("Failed to register AudioController as listener to player");
        }
        
        // Wrap in Arc+RwLock and store
        let controller = Arc::new(RwLock::new(controller));
        self.controllers.push(controller);
        
        // If this is the first controller, make it active
        if self.controllers.len() == 1 {
            self.active_index = Some(0);
        }
        
        // Return the index of the added controller
        self.controllers.len() - 1
    }
    
    /// Remove a player controller from the list by index
    /// 
    /// If the removed controller was active, the active_index is reset to None.
    /// Returns true if a controller was removed, false if the index was invalid.
    pub fn remove_controller(&mut self, index: usize) -> bool {
        if index >= self.controllers.len() {
            return false;
        }
        
        self.controllers.remove(index);
        
        // If the active controller was removed, update active_index
        if let Some(active_idx) = self.active_index {
            if active_idx == index {
                // The active controller was removed
                self.active_index = None;
            } else if active_idx > index {
                // The active controller index needs to be adjusted
                self.active_index = Some(active_idx - 1);
            }
        }
        
        true
    }
    
    /// Get the list of controllers
    pub fn list_controllers(&self) -> Vec<Arc<RwLock<Box<dyn PlayerController + Send + Sync>>>> {
        self.controllers.clone()
    }
    
    /// Set the active controller by index
    /// 
    /// Returns true if the active controller was changed, false if the index was invalid.
    /// When the active controller changes, immediately notifies listeners about the 
    /// new active controller's state, song, and capabilities.
    pub fn set_active_controller(&mut self, index: usize) -> bool {
        if index >= self.controllers.len() {
            return false;
        }
        
        // Check if this is actually a change
        if Some(index) == self.active_index {
            debug!("Active controller already set to index {}", index);
            return true;
        }
        
        // Set the new active index
        debug!("Changing active controller to index {}", index);
        self.active_index = Some(index);
        
        // Get current state of the new active player and notify listeners
        if let Ok(controller) = self.controllers[index].read() {
            let player_name = controller.get_player_name();
            let player_id = controller.get_player_id();
            
            // Notify about current state
            let state = controller.get_player_state();
            debug!("Notifying about state of new active controller: {}", state);
            self.forward_state_changed(player_name.clone(), player_id.clone(), state);
            
            // Notify about current song
            let song = controller.get_song();
            debug!("Notifying about song of new active controller");
            self.forward_song_changed(player_name.clone(), player_id.clone(), song);
            
            // Notify about current loop mode
            let loop_mode = controller.get_loop_mode();
            debug!("Notifying about loop mode of new active controller: {}", loop_mode);
            self.forward_loop_mode_changed(player_name.clone(), player_id.clone(), loop_mode);
            
            // Notify about current capabilities
            let capabilities = controller.get_capabilities();
            debug!("Notifying about {} capabilities of new active controller", capabilities.len());
            self.forward_capabilities_changed(player_name, player_id, capabilities);
        }
        
        true
    }
    
    /// Get the currently active controller, if any
    pub fn get_active_controller(&self) -> Option<Arc<RwLock<Box<dyn PlayerController + Send + Sync>>>> {
        self.active_index.map(|idx| self.controllers[idx].clone())
    }
    
    /// Send a command to the active player controller
    /// 
    /// Returns true if the command was sent successfully, false if there is no active controller.
    pub fn send_command(&self, command: PlayerCommand) -> bool {
        if let Some(idx) = self.active_index {
            if let Ok(controller) = self.controllers[idx].read() {
                return controller.send_command(command);
            }
        }
        false
    }
    
    /// Send a command to all inactive player controllers
    /// 
    /// Returns the number of controllers that successfully processed the command.
    pub fn send_command_to_inactives(&self, command: PlayerCommand) -> usize {
        let mut success_count = 0;
        
        for (idx, controller) in self.controllers.iter().enumerate() {
            // Skip the active controller
            if Some(idx) == self.active_index {
                continue;
            }
            
            // Send command to this inactive controller
            if let Ok(controller) = controller.read() {
                if controller.send_command(command.clone()) {
                    success_count += 1;
                }
            }
        }
        
        success_count
    }

    /// Create a new AudioController from a JSON array of player configurations
    /// 
    /// Each element in the array should be a valid configuration for create_player_from_json
    /// Returns a Result with the new AudioController or an error if any player creation failed
    pub fn from_json(config: &Value) -> Result<Arc<AudioController>, PlayerCreationError> {
        if !config.is_array() {
            return Err(PlayerCreationError::ParseError("Expected a JSON array".to_string()));
        }

        // Create controller without players first
        let controller = Arc::new(AudioController::new());
        
        // Initialize the self-reference
        AudioController::initialize(&controller);
        
        let config_array = config.as_array().unwrap();
        debug!("Creating AudioController from JSON array with {} elements", config_array.len());
        
        // Get a mutable reference to add players
        let controller_ref = unsafe { &mut *(Arc::as_ptr(&controller) as *mut AudioController) };
        
        for (idx, player_config) in config_array.iter().enumerate() {
            match create_player_from_json(player_config) {
                Ok(player) => {
                    debug!("Successfully created player {} from JSON configuration", idx);
                    // Use add_controller to ensure the AudioController registers itself as a listener
                    controller_ref.add_controller(player);
                },
                Err(e) => {
                    error!("Failed to create player {}: {}", idx, e);
                    return Err(e);
                }
            }
        }
        
        if controller_ref.controllers.is_empty() {
            warn!("No valid player controllers found in configuration");
        }
        
        Ok(controller)
    }

    /// Check if the given player name and ID match the active player
    fn is_active_player(&self, player_name: &str, player_id: &str) -> bool {
        if let Some(idx) = self.active_index {
            if let Ok(controller) = self.controllers[idx].read() {
                return controller.get_player_name() == player_name && 
                       controller.get_player_id() == player_id;
            }
        }
        false
    }
    
    /// Forward state changed event to all registered listeners
    fn forward_state_changed(&self, player_name: String, player_id: String, state: PlayerState) {
        // Prune dead listeners
        self.prune_dead_listeners();
        
        let source = PlayerSource::new(player_name, player_id);
        
        let event = PlayerEvent::StateChanged {
            source,
            state,
        };
        
        // Forward the event to all active listeners
        if let Ok(listeners) = self.listeners.read() {
            for listener_weak in listeners.iter() {
                if let Some(listener) = listener_weak.upgrade() {
                    listener.on_event(event.clone());
                }
            }
        } else {
            warn!("Failed to acquire read lock for listeners when forwarding state change");
        }
    }
    
    /// Forward song changed event to all registered listeners
    fn forward_song_changed(&self, player_name: String, player_id: String, song: Option<Song>) {
        // Prune dead listeners
        self.prune_dead_listeners();
        
        let source = PlayerSource::new(player_name, player_id);
        
        let event = PlayerEvent::SongChanged {
            source,
            song,
        };
        
        // Forward the event to all active listeners
        if let Ok(listeners) = self.listeners.read() {
            for listener_weak in listeners.iter() {
                if let Some(listener) = listener_weak.upgrade() {
                    listener.on_event(event.clone());
                }
            }
        } else {
            warn!("Failed to acquire read lock for listeners when forwarding song change");
        }
    }
    
    /// Forward loop mode changed event to all registered listeners
    fn forward_loop_mode_changed(&self, player_name: String, player_id: String, mode: LoopMode) {
        // Prune dead listeners
        self.prune_dead_listeners();
        
        let source = PlayerSource::new(player_name, player_id);
        
        let event = PlayerEvent::LoopModeChanged {
            source,
            mode,
        };
        
        // Forward the event to all active listeners
        if let Ok(listeners) = self.listeners.read() {
            for listener_weak in listeners.iter() {
                if let Some(listener) = listener_weak.upgrade() {
                    listener.on_event(event.clone());
                }
            }
        } else {
            warn!("Failed to acquire read lock for listeners when forwarding loop mode change");
        }
    }
    
    /// Forward capabilities changed event to all registered listeners
    fn forward_capabilities_changed(&self, player_name: String, player_id: String, capabilities: Vec<PlayerCapability>) {
        // Prune dead listeners
        self.prune_dead_listeners();
        
        let source = PlayerSource::new(player_name, player_id);
        
        let event = PlayerEvent::CapabilitiesChanged {
            source,
            capabilities,
        };
        
        // Forward the event to all active listeners
        if let Ok(listeners) = self.listeners.read() {
            for listener_weak in listeners.iter() {
                if let Some(listener) = listener_weak.upgrade() {
                    listener.on_event(event.clone());
                }
            }
        } else {
            warn!("Failed to acquire read lock for listeners when forwarding capabilities change");
        }
    }
    
    /// Remove any dead (dropped) listeners
    fn prune_dead_listeners(&self) {
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

#[cfg(test)]
mod tests {
    // Add tests here later
}