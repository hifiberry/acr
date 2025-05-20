use crate::players::PlayerController;
use crate::data::{PlayerCommand, PlayerCapabilitySet, Song, LoopMode, PlaybackState, PlayerEvent, PlayerSource, Track};
use crate::players::{create_player_from_json, PlayerCreationError};
use crate::plugins::ActionPlugin;
use serde_json::Value;
use std::sync::{Arc, RwLock, Weak, Mutex, Once};
use std::any::Any;
use log::{debug, warn, error}; // Ensure warn is imported
use crate::audiocontrol::eventbus::EventBus; // Added for EventBus

// Static singleton instance
static mut AUDIO_CONTROLLER_INSTANCE: Option<Arc<AudioController>> = None;
static AUDIO_CONTROLLER_INIT: Once = Once::new();
static AUDIO_CONTROLLER_MUTEX: Mutex<()> = Mutex::new(());

/// A simple AudioController that manages multiple PlayerController instances
#[derive(Clone)]
pub struct AudioController {
    /// List of player controllers
    controllers: Vec<Arc<RwLock<Box<dyn PlayerController + Send + Sync>>>>,
    
    /// Index of the active player controller in the list
    active_index: Arc<RwLock<usize>>,
    
    /// List of action plugins
    action_plugins: Arc<RwLock<Vec<Box<dyn ActionPlugin + Send + Sync>>>>,
    
    /// Self-reference for registering with players
    /// This is wrapped in Option because it's initialized after construction
    self_ref: Arc<RwLock<Option<Weak<AudioController>>>>,
}

// Implement PlayerController for AudioController
impl PlayerController for AudioController {
    fn get_capabilities(&self) -> PlayerCapabilitySet {
        if let Ok(active_idx) = self.active_index.read() {
            if *active_idx < self.controllers.len() {
                if let Ok(controller) = self.controllers[*active_idx].read() {
                    return controller.get_capabilities();
                }
            }
        }
        PlayerCapabilitySet::empty() // Return empty capabilities if no active controller
    }
    
    fn get_song(&self) -> Option<Song> {
        if let Ok(active_idx) = self.active_index.read() {
            if *active_idx < self.controllers.len() {
                if let Ok(controller) = self.controllers[*active_idx].read() {
                    return controller.get_song();
                }
            }
        }
        None // Return None if no active controller
    }
    
    fn get_loop_mode(&self) -> LoopMode {
        if let Ok(active_idx) = self.active_index.read() {
            if *active_idx < self.controllers.len() {
                if let Ok(controller) = self.controllers[*active_idx].read() {
                    return controller.get_loop_mode();
                }
            }
        }
        LoopMode::None // Default loop mode if no active controller
    }
    
    fn get_playback_state(&self) -> PlaybackState {
        if let Ok(active_idx) = self.active_index.read() {
            if *active_idx < self.controllers.len() {
                if let Ok(controller) = self.controllers[*active_idx].read() {
                    return controller.get_playback_state();
                }
            }
        }
        PlaybackState::Stopped // Default state if no active controller
    }
    
    fn get_position(&self) -> Option<f64> {
        if let Ok(active_idx) = self.active_index.read() {
            if *active_idx < self.controllers.len() {
                if let Ok(controller) = self.controllers[*active_idx].read() {
                    return controller.get_position();
                }
            }
        }
        None // Return None if no active controller
    }
    
    fn get_shuffle(&self) -> bool {
        if let Ok(active_idx) = self.active_index.read() {
            if *active_idx < self.controllers.len() {
                if let Ok(controller) = self.controllers[*active_idx].read() {
                    return controller.get_shuffle();
                }
            }
        }
        false // Default shuffle state if no active controller
    }
    
    fn get_player_name(&self) -> String {
        if let Ok(active_idx) = self.active_index.read() {
            if *active_idx < self.controllers.len() {
                if let Ok(controller) = self.controllers[*active_idx].read() {
                    return controller.get_player_name();
                }
            }
        }
        "audiocontroller".to_string() // Default name if no active controller
    }
    
    fn get_player_id(&self) -> String {
        if let Ok(active_idx) = self.active_index.read() {
            if *active_idx < self.controllers.len() {
                if let Ok(controller) = self.controllers[*active_idx].read() {
                    return controller.get_player_id();
                }
            }
        }
        "none".to_string() // Default ID if no active controller
    }
    
    fn get_last_seen(&self) -> Option<std::time::SystemTime> {
        if let Ok(active_idx) = self.active_index.read() {
            if *active_idx < self.controllers.len() {
                if let Ok(controller) = self.controllers[*active_idx].read() {
                    return controller.get_last_seen();
                }
            }
        }
        None // Return None if no active controller
    }
    
    fn send_command(&self, command: PlayerCommand) -> bool {
        if let Ok(active_idx) = self.active_index.read() {
            if *active_idx < self.controllers.len() {
                debug!("Sending command to active controller [{}]: {}", active_idx, command);
                if let Ok(controller) = self.controllers[*active_idx].read() {
                    return controller.send_command(command);
                }
            }
        }
        false // Return false if no active controller
    }
    
    fn as_any(&self) -> &dyn Any {
        self
    }
    
    fn start(&self) -> bool {
        let mut success = false;
        
        // Start all controllers, not just the active one
        for controller_lock in &self.controllers {
            if let Ok(controller) = controller_lock.read() {
                if controller.start() {
                    success = true;  // If at least one controller starts successfully
                    debug!("Successfully started player controller: {}", controller.get_player_name());
                } else {
                    warn!("Failed to start player controller: {}", controller.get_player_name());
                }
            }
        }
        
        success // Return true if at least one controller started successfully
    }
    
    fn stop(&self) -> bool {
        let mut success = false;
        
        // Stop all controllers, not just the active one
        for controller_lock in &self.controllers {
            if let Ok(controller) = controller_lock.read() {
                if controller.stop() {
                    success = true;  // If at least one controller stops successfully
                    debug!("Successfully stopped player controller: {}", controller.get_player_name());
                } else {
                    warn!("Failed to stop player controller: {}", controller.get_player_name());
                }
            }
        }
        
        success // Return true if at least one controller stopped successfully
    }

    fn get_queue(&self) -> Vec<Track> {
        if let Ok(active_idx) = self.active_index.read() {
            if *active_idx < self.controllers.len() {
                if let Ok(controller) = self.controllers[*active_idx].read() {
                    return controller.get_queue();
                }
            }
        }
        Vec::new() // Return empty vector if no active controller
    }
}

impl AudioController {
    /// Create a new AudioController with no controllers
    pub fn new() -> Self {
        Self {
            controllers: Vec::new(),
            active_index: Arc::new(RwLock::new(0)),
            action_plugins: Arc::new(RwLock::new(Vec::new())),
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

        // Add listener to the global event bus
        let bus = EventBus::instance();
        let (id, receiver) = bus.subscribe_all();
        debug!("AudioController subscribed to global EventBus for logging with ID: {:?}", id);
        bus.spawn_worker(id, receiver, move |event| {
            debug!("[EventBus GLOBAL] Received event: {:?}, doing nothing", event);
        });
    }

    /// Get the singleton instance of AudioController
    pub fn instance() -> Arc<AudioController> {
        // Initialize once with mutex protection for thread safety
        let _guard = AUDIO_CONTROLLER_MUTEX.lock().unwrap();
          unsafe {
            AUDIO_CONTROLLER_INIT.call_once(|| {
                // Create a default instance with empty configuration
                let default_config = serde_json::json!({
                    "players": [],
                    "action_plugins": []
                });
                
                let controller = Self::from_json(&default_config)
                    .expect("Failed to create default AudioController");
                
                AUDIO_CONTROLLER_INSTANCE = Some(controller);
            });
            
            // This is safe because we've initialized it in call_once
            // and we're holding the mutex lock
            match AUDIO_CONTROLLER_INSTANCE {
                Some(ref controller) => controller.clone(),
                None => panic!("AudioController instance is not initialized")
            }
        }
    }
    
    /// Initialize the singleton instance with a specific controller
    pub fn initialize_instance(controller: Arc<AudioController>) -> Result<(), String> {
        unsafe {
            let _guard = AUDIO_CONTROLLER_MUTEX.lock().unwrap();
            if AUDIO_CONTROLLER_INIT.is_completed() {
                return Err("AudioController singleton already initialized".to_string());
            }
            
            AUDIO_CONTROLLER_INSTANCE = Some(controller);
            AUDIO_CONTROLLER_INIT.call_once(|| {});
            Ok(())
        }
    }
    
    /// Reset the singleton instance (mainly for testing)
    #[cfg(test)]
    pub fn reset_instance() {
        unsafe {
            let _guard = AUDIO_CONTROLLER_MUTEX.lock().unwrap();
            AUDIO_CONTROLLER_INSTANCE = None;
        }
    }

    /// Add a player controller to the list
    /// 
    /// If this is the first controller added, it becomes the active controller.
    pub fn add_controller(&mut self, controller: Box<dyn PlayerController + Send + Sync>) -> usize {
        // Check if we have a self reference for listener registration
        let self_weak = if let Ok(self_ref) = self.self_ref.read() {
            if let Some(weak_ref) = self_ref.as_ref() {
                weak_ref.clone() as Weak<dyn PlayerController + Send + Sync>
            } else {
                // Continue without registering as listener
                let controller = controller;
                let controller = Arc::new(RwLock::new(controller));
                self.controllers.push(controller);
                
                if self.controllers.len() == 1 {
                    if let Ok(mut active_idx) = self.active_index.write() {
                        *active_idx = 0;
                    } else {
                        error!("Failed to acquire write lock for active_index");
                    }
                }
                
                return self.controllers.len() - 1;
            }
        } else {
            // Continue without registering as listener
            let controller = controller;
            let controller = Arc::new(RwLock::new(controller));
            self.controllers.push(controller);
            
            if self.controllers.len() == 1 {
                if let Ok(mut active_idx) = self.active_index.write() {
                    *active_idx = 0;
                } else {
                    error!("Failed to acquire write lock for active_index");
                }
            }
            
            return self.controllers.len() - 1;
        };
        
        // Wrap in Arc+RwLock and store
        let controller = Arc::new(RwLock::new(controller));
        self.controllers.push(controller);
        
        // If this is the first controller, make it active
        if self.controllers.len() == 1 {
            if let Ok(mut active_idx) = self.active_index.write() {
                *active_idx = 0;
            } else {
                error!("Failed to acquire write lock for active_index");
            }
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
        if let Ok(mut active_idx) = self.active_index.write() {
            if *active_idx == index {
                // The active controller was removed
                *active_idx = 0;
            } else if *active_idx > index {
                // The active controller index needs to be adjusted
                *active_idx -= 1;
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
        
        // Check if this is actually a change
        if let Ok(active_idx) = self.active_index.read() {
            if index == *active_idx {
                debug!("Active controller already set to index {}", index);
                return true;
            }
        }
        
        // Set the new active index
        if let Ok(mut active_idx) = self.active_index.write() {
            *active_idx = index;
            debug!("Changing active controller to index {}", index);
            true
        } else {
            error!("Failed to acquire write lock for active_index");
            false
        }
    }
    
    /// Get the currently active controller, if any
    pub fn get_active_controller(&self) -> Option<Arc<RwLock<Box<dyn PlayerController + Send + Sync>>>> {
        if let Ok(active_idx) = self.active_index.read() {
            if *active_idx < self.controllers.len() {
                return Some(self.controllers[*active_idx].clone());
            }
        }
        None
    }
    
    /// Send a command to the active player controller
    /// 
    /// Returns true if the command was sent successfully, false if there is no active controller.
    pub fn send_command(&self, command: PlayerCommand) -> bool {
        if let Ok(active_idx) = self.active_index.read() {
            if *active_idx < self.controllers.len() {
                if let Ok(controller) = self.controllers[*active_idx].read() {
                    return controller.send_command(command);
                }
            }
        }
        false
    }
    
    /// Send a command to all inactive player controllers
    /// 
    /// Returns the number of controllers that successfully processed the command.
    pub fn send_command_to_inactives(&self, command: PlayerCommand) -> usize {
        let mut success_count = 0;
        
        let active_idx_value = if let Ok(active_idx) = self.active_index.read() {
            *active_idx
        } else {
            0
        };
        
        for (idx, controller) in self.controllers.iter().enumerate() {
            // Skip the active controller
            if idx == active_idx_value {
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
    /// The JSON configuration can include:
    /// - "players": Array of player configurations
    /// - "action_plugins": Array of action plugin configurations
    /// 
    /// Player configurations can include an "enable" flag which, if set to false,
    /// will cause that player to be skipped without error.
    /// 
    /// Returns a Result with the new AudioController or an error if any player creation failed
    pub fn from_json(config: &Value) -> Result<Arc<AudioController>, PlayerCreationError> {
        // Create controller without players first
        let controller = Arc::new(AudioController::new());
        
        // Initialize the self-reference
        AudioController::initialize(&controller);
        
        // Get a mutable reference to add players
        let controller_ref = unsafe { &mut *(Arc::as_ptr(&controller) as *mut AudioController) };
        
        // Process player configurations if present
        if let Some(players_config) = config.get("players").and_then(|v| v.as_array()) {
            debug!("Creating AudioController players from JSON array with {} elements", players_config.len());
            
            for (idx, player_config) in players_config.iter().enumerate() {
                match create_player_from_json(player_config) {
                    Ok(player) => {
                        debug!("Successfully created player {} from JSON configuration", idx);
                        // Use add_controller to ensure the AudioController registers itself as a listener
                        controller_ref.add_controller(player);
                    },
                    Err(e) => {
                        // Check if this is due to the player being disabled
                        if let PlayerCreationError::ParseError(msg) = &e {
                            if msg.contains("disabled in configuration") {
                                debug!("Skipping disabled player {}: {}", idx, msg);
                                continue; // Skip this player and move on to the next one
                            }
                        }
                        
                        // For any other error, return it
                        error!("Failed to create player {}: {}", idx, e);
                        return Err(e);
                    }
                }
            }
            
            if controller_ref.controllers.is_empty() {
                warn!("No valid player controllers found in configuration");
            }
        } else if let Some(players_config) = config.as_array() {
            // For backward compatibility, check if the top-level config is an array of players
            debug!("Using legacy format - Creating AudioController from JSON array with {} elements", players_config.len());
            
            for (idx, player_config) in players_config.iter().enumerate() {
                match create_player_from_json(player_config) {
                    Ok(player) => {
                        debug!("Successfully created player {} from JSON configuration", idx);
                        controller_ref.add_controller(player);
                    },
                    Err(e) => {
                        // Check if this is due to the player being disabled
                        if let PlayerCreationError::ParseError(msg) = &e {
                            if msg.contains("disabled in configuration") {
                                debug!("Skipping disabled player {}: {}", idx, msg);
                                continue; // Skip this player and move on to the next one
                            }
                        }
                        
                        // For any other error, return it
                        error!("Failed to create player {}: {}", idx, e);
                        return Err(e);
                    }
                }
            }
        }
        
        // Process action plugin configurations if present
        if let Some(plugins_config) = config.get("action_plugins").and_then(|v| v.as_array()) {
            debug!("Creating action plugins from JSON array with {} elements", plugins_config.len());
            
            let factory = crate::plugins::plugin_factory::PluginFactory::new();
            
            for (idx, plugin_config) in plugins_config.iter().enumerate() {
                // Check if this plugin is enabled
                if let Some(enabled) = plugin_config.get("enabled").and_then(Value::as_bool) {
                    if !enabled {
                        debug!("Skipping disabled action plugin at index {}", idx);
                        continue;
                    }
                }
                
                // Convert the plugin config to a string for the factory
                if let Ok(json_str) = serde_json::to_string(plugin_config) {
                    match factory.create_action_plugin_from_json(&json_str) {
                        Some(plugin) => {
                            debug!("Successfully created action plugin {} from JSON configuration", idx);
                            controller_ref.add_action_plugin(plugin);
                        },
                        None => {
                            warn!("Failed to create action plugin {} from JSON, skipping", idx);
                        }
                    }
                } else {
                    warn!("Failed to serialize plugin configuration to JSON string, skipping action plugin {}", idx);
                }
            }
        }
        
        Ok(controller)
    }

    /// Check if the given player name and ID match the active player
    fn is_active_player(&self, player_name: &str, player_id: &str) -> bool {
        if let Ok(active_idx) = self.active_index.read() {
            if *active_idx < self.controllers.len() {
                if let Ok(controller) = self.controllers[*active_idx].read() {
                    return controller.get_player_name() == player_name && 
                           controller.get_player_id() == player_id;
                }
            }
        }
        false
    }
    
    /// Add an action plugin to the controller
    /// Returns the index of the added plugin
    pub fn add_action_plugin(&mut self, mut plugin: Box<dyn ActionPlugin + Send + Sync>) -> usize {
        // Initialize the plugin with a reference to this controller
        if let Ok(self_ref) = self.self_ref.read() {
            if let Some(weak_ref) = self_ref.as_ref() {
                plugin.initialize(weak_ref.clone());
                plugin.init(); // Call the plugin's init method

                if let Ok(mut plugins) = self.action_plugins.write() {
                    plugins.push(plugin);
                    debug!("Added action plugin at index {}", plugins.len() - 1);
                    return plugins.len() - 1;
                } else {
                    error!("Failed to acquire write lock for action_plugins");
                }
            } else {
                error!("Cannot add action plugin: AudioController self-reference not initialized");
            }
        } else {
            error!("Failed to acquire read lock for self_ref");
        }
        0
    }

    /// Remove an action plugin by index
    /// Returns true if the plugin was successfully removed
    pub fn remove_action_plugin(&mut self, index: usize) -> bool {
        if let Ok(mut plugins) = self.action_plugins.write() {
            if index < plugins.len() {
                plugins.remove(index);
                debug!("Removed action plugin at index {}", index);
                return true;
            }
            false
        } else {
            error!("Failed to acquire write lock for action_plugins");
            false
        }
    }

    /// Get the number of action plugins
    pub fn action_plugin_count(&self) -> usize {
        if let Ok(plugins) = self.action_plugins.read() {
            plugins.len()
        } else {
            0
        }
    }

    /// Clear all action plugins
    pub fn clear_action_plugins(&mut self) -> usize {
        if let Ok(mut plugins) = self.action_plugins.write() {
            let count = plugins.len();
            plugins.clear();
            debug!("Cleared {} action plugins", count);
            count
        } else {
            error!("Failed to acquire write lock for action_plugins");
            0
        }
    }

    /// Add multiple action plugins from a vector
    pub fn add_action_plugins(&mut self, plugins: Vec<Box<dyn ActionPlugin + Send + Sync>>) -> usize {
        let count = plugins.len();
        
        // Initialize each plugin and add it
        for plugin in plugins {
            self.add_action_plugin(plugin);
        }
        
        debug!("Added {} action plugins", count);
        count
    }    
    
    /// Process an event
    fn process_event(&self, event: PlayerEvent, is_active: bool) {
        // Then handle the event as before
        // TODO: handle state changes to find active player
    }    

    /// Get information about all registered action plugins
    pub fn get_action_plugin_info(&self) -> Vec<(String, String)> {
        if let Ok(plugins) = self.action_plugins.read() {
            plugins.iter()
                .map(|plugin| (plugin.name().to_string(), plugin.version().to_string()))
                .collect()
        } else {
            error!("Failed to acquire read lock for action_plugins");
            Vec::new()
        }
    }
}

#[cfg(test)]
mod tests {
    // Add tests here later
}