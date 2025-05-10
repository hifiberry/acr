use crate::players::PlayerController;
use crate::players::PlayerStateListener;
use crate::data::{PlayerCommand, PlayerCapabilitySet, Song, LoopMode, PlaybackState, PlayerEvent, PlayerSource, Track};
use crate::players::{create_player_from_json, PlayerCreationError};
use crate::plugins::EventFilter;
use crate::plugins::ActionPlugin;
use serde_json::Value;
use std::sync::{Arc, RwLock, Weak, Mutex, Once};
use std::any::Any;
use log::{debug, warn, error};

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
    
    /// List of state listeners registered with this controller
    listeners: Arc<RwLock<Vec<Weak<dyn PlayerStateListener>>>>,
    
    /// List of event filters for incoming events
    event_filters: Arc<RwLock<Vec<Box<dyn EventFilter + Send + Sync>>>>,
    
    /// List of action plugins that respond to events
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

// Implement PlayerStateListener for AudioController
impl PlayerStateListener for AudioController {
    fn on_event(&self, event: PlayerEvent) {
        // Determine if this event is from the active player
        let is_active = match &event {
            PlayerEvent::StateChanged { source, .. } => 
                self.is_active_player(&source.player_name, &source.player_id),
            PlayerEvent::SongChanged { source, .. } => 
                self.is_active_player(&source.player_name, &source.player_id),
            PlayerEvent::LoopModeChanged { source, .. } => 
                self.is_active_player(&source.player_name, &source.player_id),
            PlayerEvent::CapabilitiesChanged { source, .. } => 
                self.is_active_player(&source.player_name, &source.player_id),
            PlayerEvent::PositionChanged { source, .. } => 
                self.is_active_player(&source.player_name, &source.player_id),
            PlayerEvent::DatabaseUpdating { source, .. } => 
                self.is_active_player(&source.player_name, &source.player_id),
            PlayerEvent::QueueChanged { source } => 
                self.is_active_player(&source.player_name, &source.player_id),
        };

        // Pass the event through all filters
        let mut filtered_event = Some(event);
        if let Ok(filters) = self.event_filters.read() {
            for filter in filters.iter() {
                if let Some(current_event) = filtered_event {
                    filtered_event = filter.filter_event(current_event, is_active);
                    if filtered_event.is_none() {
                        debug!("Event was filtered out by {}", filter.name());
                        break;  // Event was filtered out, stop processing
                    }
                }
            }
        }

        // Process the filtered event
        if let Some(filtered_event) = filtered_event {
            self.process_filtered_event(filtered_event, is_active);
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
            active_index: Arc::new(RwLock::new(0)),
            listeners: Arc::new(RwLock::new(Vec::new())),
            event_filters: Arc::new(RwLock::new(Vec::new())),
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
    }

    /// Get the singleton instance of AudioController
    pub fn instance() -> Arc<AudioController> {
        // Initialize once with mutex protection for thread safety
        let _guard = AUDIO_CONTROLLER_MUTEX.lock().unwrap();
        
        unsafe {
            AUDIO_CONTROLLER_INIT.call_once(|| {
                // Create a default instance if none exists
                let default_config = serde_json::from_str(&Self::sample_json_config())
                    .expect("Failed to parse default AudioController config");
                
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
                weak_ref.clone() as Weak<dyn PlayerStateListener>
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
    /// - "event_filters": Array of event filter configurations
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
        
        // Get a mutable reference to add players and filters
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
        
        // Process event filter configurations if present
        if let Some(filters_config) = config.get("event_filters").and_then(|v| v.as_array()) {
            debug!("Creating event filters from JSON array with {} elements", filters_config.len());
            
            let factory = crate::plugins::plugin_factory::PluginFactory::new();
            
            for (idx, filter_config) in filters_config.iter().enumerate() {
                // Check if this filter is enabled
                if let Some(enabled) = filter_config.get("enabled").and_then(Value::as_bool) {
                    if !enabled {
                        debug!("Skipping disabled event filter at index {}", idx);
                        continue;
                    }
                }
                
                // Convert the filter config to a string for the factory
                if let Ok(json_str) = serde_json::to_string(filter_config) {
                    match factory.create_event_filter_from_json(&json_str) {
                        Some(filter) => {
                            debug!("Successfully created event filter {} from JSON configuration", idx);
                            controller_ref.add_event_filter(filter);
                        },
                        None => {
                            warn!("Failed to create event filter {} from JSON, skipping", idx);
                        }
                    }
                } else {
                    warn!("Failed to serialize filter configuration to JSON string, skipping filter {}", idx);
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
    
    /// Forward state changed event to all registered listeners
    fn forward_state_changed(&self, player_name: String, player_id: String, state: PlaybackState) {
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
    fn forward_capabilities_changed(&self, player_name: String, player_id: String, capabilities: PlayerCapabilitySet) {
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
    
    /// Forward position changed event to all registered listeners
    fn forward_position_changed(&self, player_name: String, player_id: String, position: f64) {
        // Prune dead listeners
        self.prune_dead_listeners();
        
        let source = PlayerSource::new(player_name, player_id);
        
        let event = PlayerEvent::PositionChanged {
            source,
            position,
        };
        
        // Forward the event to all active listeners
        if let Ok(listeners) = self.listeners.read() {
            for listener_weak in listeners.iter() {
                if let Some(listener) = listener_weak.upgrade() {
                    listener.on_event(event.clone());
                }
            }
        } else {
            warn!("Failed to acquire read lock for listeners when forwarding position change");
        }
    }
    
    /// Forward database update event to all registered listeners
    fn forward_database_update(&self, player_name: String, player_id: String, 
                             artist: Option<String>, album: Option<String>, 
                             song: Option<String>, percentage: Option<f32>) {
        // Prune dead listeners
        self.prune_dead_listeners();
        
        let source = PlayerSource::new(player_name, player_id);
        
        let event = PlayerEvent::DatabaseUpdating {
            source,
            artist,
            album,
            song,
            percentage,
        };
        
        // Forward the event to all active listeners
        if let Ok(listeners) = self.listeners.read() {
            for listener_weak in listeners.iter() {
                if let Some(listener) = listener_weak.upgrade() {
                    listener.on_event(event.clone());
                }
            }
        } else {
            warn!("Failed to acquire read lock for listeners when forwarding database update");
        }
    }

    /// Forward queue changed event to all registered listeners
    fn forward_queue_changed(&self, player_name: String, player_id: String) {
        // Prune dead listeners
        self.prune_dead_listeners();

        let source = PlayerSource::new(player_name, player_id);

        let event = PlayerEvent::QueueChanged { source };

        // Forward the event to all active listeners
        if let Ok(listeners) = self.listeners.read() {
            for listener_weak in listeners.iter() {
                if let Some(listener) = listener_weak.upgrade() {
                    listener.on_event(event.clone());
                }
            }
        } else {
            warn!("Failed to acquire read lock for listeners when forwarding queue change");
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

    /// Add an event filter to the controller
    /// Returns the index of the added filter
    pub fn add_event_filter(&mut self, filter: Box<dyn EventFilter + Send + Sync>) -> usize {
        if let Ok(mut filters) = self.event_filters.write() {
            filters.push(filter);
            debug!("Added event filter at index {}", filters.len() - 1);
            return filters.len() - 1;
        } else {
            error!("Failed to acquire write lock for event_filters");
            0
        }
    }

    /// Remove an event filter by index
    /// Returns true if the filter was successfully removed
    pub fn remove_event_filter(&mut self, index: usize) -> bool {
        if let Ok(mut filters) = self.event_filters.write() {
            if index < filters.len() {
                filters.remove(index);
                debug!("Removed event filter at index {}", index);
                return true;
            }
            false
        } else {
            error!("Failed to acquire write lock for event_filters");
            false
        }
    }

    /// Get the number of event filters
    pub fn event_filter_count(&self) -> usize {
        if let Ok(filters) = self.event_filters.read() {
            filters.len()
        } else {
            0
        }
    }

    /// Clear all event filters
    pub fn clear_event_filters(&mut self) -> usize {
        if let Ok(mut filters) = self.event_filters.write() {
            let count = filters.len();
            filters.clear();
            debug!("Cleared {} event filters", count);
            count
        } else {
            error!("Failed to acquire write lock for event_filters");
            0
        }
    }

    /// Add multiple event filters from a vector
    pub fn add_event_filters(&mut self, mut filters: Vec<Box<dyn EventFilter + Send + Sync>>) -> usize {
        if let Ok(mut existing_filters) = self.event_filters.write() {
            let count = filters.len();
            existing_filters.append(&mut filters);
            debug!("Added {} event filters", count);
            count
        } else {
            error!("Failed to acquire write lock for event_filters");
            0
        }
    }

    /// Add an action plugin to the controller
    /// Returns the index of the added plugin
    pub fn add_action_plugin(&mut self, mut plugin: Box<dyn ActionPlugin + Send + Sync>) -> usize {
        // Initialize the plugin with a reference to this controller
        if let Ok(self_ref) = self.self_ref.read() {
            if let Some(weak_ref) = self_ref.as_ref() {
                plugin.initialize(weak_ref.clone());
                
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

    /// Process an event with all registered action plugins
    fn process_event_with_action_plugins(&self, event: &PlayerEvent, is_active_player: bool) {
        if let Ok(mut plugins) = self.action_plugins.write() {
            for plugin in plugins.iter_mut() {
                plugin.on_event(event, is_active_player);
            }
        }
    }

    /// Process a filtered event
    fn process_filtered_event(&self, event: PlayerEvent, is_active: bool) {
        // First pass the event to all action plugins
        self.process_event_with_action_plugins(&event, is_active);
        
        // Then handle the event as before
        match event {
            PlayerEvent::StateChanged { source, state } => {
                // Check if the event is from the active player
                if is_active {
                    debug!("AudioController forwarding state change from active player {}: {}", source.player_id, state);
                    self.forward_state_changed(source.player_name, source.player_id, state);
                } else {
                    debug!("AudioController ignoring state change from inactive player {}", source.player_id);
                }
            },
            PlayerEvent::SongChanged { source, song } => {
                // Check if the event is from the active player
                if is_active {
                    let song_title = song.as_ref().map_or("None".to_string(), |s| s.title.as_deref().unwrap_or("Unknown").to_string());
                    debug!("AudioController forwarding song change from active player {}: {}", source.player_id, song_title);
                    self.forward_song_changed(source.player_name, source.player_id, song);
                } else {
                    debug!("AudioController ignoring song change from inactive player {}", source.player_id);
                }
            },
            PlayerEvent::LoopModeChanged { source, mode } => {
                // Check if the event is from the active player
                if is_active {
                    debug!("AudioController forwarding loop mode change from active player {}: {}", source.player_id, mode);
                    self.forward_loop_mode_changed(source.player_name, source.player_id, mode);
                } else {
                    debug!("AudioController ignoring loop mode change from inactive player {}", source.player_id);
                }
            },
            PlayerEvent::CapabilitiesChanged { source, capabilities } => {
                // Check if the event is from the active player
                if is_active {
                    debug!("AudioController forwarding capabilities change from active player {}", source.player_id);
                    self.forward_capabilities_changed(source.player_name, source.player_id, capabilities);
                } else {
                    debug!("AudioController ignoring capabilities change from inactive player {}", source.player_id);
                }
            },
            PlayerEvent::PositionChanged { source, position } => {
                // Check if the event is from the active player
                if is_active {
                    debug!("AudioController forwarding position change from active player {}: {:.1}s", source.player_id, position);
                    self.forward_position_changed(source.player_name, source.player_id, position);
                } else {
                    debug!("AudioController ignoring position change from inactive player {}", source.player_id);
                }
            },
            PlayerEvent::DatabaseUpdating { source, artist, album, song, percentage } => {
                // Forward database update events to listeners if they're from the active player
                if is_active {
                    let progress_str = percentage.map_or(String::new(), |p| format!(" ({:.1}%)", p));
                    let details = match (artist.as_deref(), album.as_deref(), song.as_deref()) {
                        (Some(a), Some(b), Some(s)) => format!("artist: {}, album: {}, song: {}", a, b, s),
                        (Some(a), Some(b), None) => format!("artist: {}, album: {}", a, b),
                        (Some(a), None, None) => format!("artist: {}", a),
                        (None, Some(b), None) => format!("album: {}", b),
                        (None, None, Some(s)) => format!("song: {}", s),
                        _ => "database".to_string(),
                    };
                    debug!("AudioController forwarding database update from active player {}: {}{}", 
                           source.player_id, details, progress_str);
                    self.forward_database_update(source.player_name, source.player_id, artist, album, song, percentage);
                } else {
                    debug!("AudioController ignoring database update from inactive player {}", source.player_id);
                }
            },
            PlayerEvent::QueueChanged { source } => {
                // Check if the event is from the active player
                if is_active {
                    debug!("AudioController forwarding queue change from active player {}", source.player_id);
                    // Forward the queue changed event
                    self.forward_queue_changed(source.player_name, source.player_id);
                } else {
                    debug!("AudioController ignoring queue change from inactive player {}", source.player_id);
                }
            }
        }
    }

    /// Returns a default JSON configuration for AudioController with all available players and plugins
    ///
    /// This function uses the default player configuration and adds event filters and action plugins,
    /// providing a complete configuration for initializing a new project.
    ///
    /// # Returns
    ///
    /// A JSON string containing the complete AudioController configuration
    pub fn sample_json_config() -> String {
        use crate::players::sample_json_config;
        use crate::plugins::plugin_factory::PluginFactory;
        
        // Get the default players configuration as a JSON Value
        let players_str = sample_json_config();
        let players_value: serde_json::Value = serde_json::from_str(&players_str)
            .unwrap_or_else(|_| serde_json::json!([]));
            
        // Get the default event filters configuration as a JSON Value
        let filters_str = PluginFactory::sample_json_config();
        let filters_value: serde_json::Value = serde_json::from_str(&filters_str)
            .unwrap_or_else(|_| serde_json::json!([]));
            
        // Get the default action plugins configuration as a JSON Value
        let plugins_str = PluginFactory::sample_action_plugins_config();
        let plugins_value: serde_json::Value = serde_json::from_str(&plugins_str)
            .unwrap_or_else(|_| serde_json::json!([]));
            
        // Create the complete AudioController configuration
        let config = serde_json::json!({
            "players": players_value,
            "event_filters": filters_value,
            "action_plugins": plugins_value
        });
        
        serde_json::to_string_pretty(&config).unwrap_or_else(|_| "{}".to_string())
    }

    /// Display the state of all players to the console
    pub fn display_all_player_states(&self) {
        println!("\n=== Player States ===");
        
        if self.controllers.is_empty() {
            println!("No players registered.");
            return;
        }
        
        let active_idx_value = if let Ok(active_idx) = self.active_index.read() {
            *active_idx
        } else {
            0
        };
        
        for (idx, controller) in self.controllers.iter().enumerate() {
            if let Ok(player) = controller.read() {
                let is_active = idx == active_idx_value;
                let active_marker = if is_active { "* " } else { "  " };
                
                println!("{}[{}] {}: {}", 
                    active_marker,
                    idx,
                    player.get_player_name(),
                    player.get_player_id());
                
                println!("    State: {}", player.get_playback_state());
                
                if let Some(song) = player.get_song() {
                    let title = song.title.unwrap_or_else(|| "Unknown".to_string());
                    let artist = song.artist.unwrap_or_else(|| "Unknown".to_string());
                    let album = song.album.unwrap_or_else(|| "Unknown".to_string());
                    
                    println!("    Now playing: {} by {} ({})", title, artist, album);
                } else {
                    println!("    No song currently playing");
                }
                
                println!("    Loop mode: {}", player.get_loop_mode());
                println!("    Shuffle: {}", if player.get_shuffle() { "On" } else { "Off" });
                
                let capabilities = player.get_capabilities();
                println!("    Capabilities: {}", capabilities);
                
                println!();
            } else {
                println!("  [{}] <Unable to access player>", idx);
            }
        }
        
        println!("===================");
    }

    /// Get information about all registered event filters
    pub fn get_event_filter_info(&self) -> Vec<(String, String)> {
        if let Ok(filters) = self.event_filters.read() {
            filters.iter()
                .map(|filter| (filter.name().to_string(), filter.version().to_string()))
                .collect()
        } else {
            error!("Failed to acquire read lock for event_filters");
            Vec::new()
        }
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