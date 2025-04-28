use crate::players::PlayerController;
use crate::data::PlayerCommand;
use std::sync::{Arc, RwLock};

/// A simple AudioController that manages multiple PlayerController instances
pub struct AudioController {
    /// List of player controllers
    controllers: Vec<Arc<RwLock<Box<dyn PlayerController + Send + Sync>>>>,
    
    /// Index of the active player controller in the list
    active_index: Option<usize>,
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
}

#[cfg(test)]
mod tests {
    // Add tests here later
}