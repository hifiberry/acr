use crate::players::PlayerController;
use crate::data::{PlayerCommand, PlayerCapability, Song, LoopMode, PlayerState};
use crate::players::{create_player_from_json, PlayerCreationError};
use std::sync::{Arc, RwLock, Weak};
use std::any::Any;
use log::{debug, warn, error};
use serde_json::Value;

/// A simple AudioController that manages multiple PlayerController instances
pub struct AudioController {
    /// List of player controllers
    controllers: Vec<Arc<RwLock<Box<dyn PlayerController + Send + Sync>>>>,
    
    /// Index of the active player controller in the list
    active_index: Option<usize>,
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
        if let Some(idx) = self.active_index {
            if let Ok(mut controller) = self.controllers[idx].write() {
                return controller.register_state_listener(listener);
            }
        }
        false // Return false if no active controller
    }
    
    fn unregister_state_listener(&mut self, listener: &Arc<dyn crate::players::PlayerStateListener>) -> bool {
        if let Some(idx) = self.active_index {
            if let Ok(mut controller) = self.controllers[idx].write() {
                return controller.unregister_state_listener(listener);
            }
        }
        false // Return false if no active controller
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

impl AudioController {
    /// Create a new AudioController with no controllers
    pub fn new() -> Self {
        Self {
            controllers: Vec::new(),
            active_index: None,
        }
    }
    
    /// Add a player controller to the list
    /// 
    /// If this is the first controller added, it becomes the active controller.
    pub fn add_controller(&mut self, controller: Box<dyn PlayerController + Send + Sync>) -> usize {
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
    pub fn set_active_controller(&mut self, index: usize) -> bool {
        if index >= self.controllers.len() {
            return false;
        }
        
        self.active_index = Some(index);
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
    pub fn from_json(config: &Value) -> Result<Self, PlayerCreationError> {
        if !config.is_array() {
            return Err(PlayerCreationError::ParseError("Expected a JSON array".to_string()));
        }

        let mut controller = AudioController::new();
        
        let config_array = config.as_array().unwrap();
        debug!("Creating AudioController from JSON array with {} elements", config_array.len());
        
        for (idx, player_config) in config_array.iter().enumerate() {
            match create_player_from_json(player_config) {
                Ok(player) => {
                    debug!("Successfully created player {} from JSON configuration", idx);
                    controller.add_controller(player);
                },
                Err(e) => {
                    error!("Failed to create player {}: {}", idx, e);
                    return Err(e);
                }
            }
        }
        
        if controller.controllers.is_empty() {
            warn!("No valid player controllers found in configuration");
        }
        
        Ok(controller)
    }
}

#[cfg(test)]
mod tests {
    // Add tests here later
}