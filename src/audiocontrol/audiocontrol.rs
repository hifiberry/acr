use crate::players::PlayerController;
use crate::data::{PlayerCommand, PlayerCapabilitySet, Song, LoopMode, PlaybackState, Track};
use crate::players::{create_player_from_json, PlayerCreationError};
use crate::plugins::ActionPlugin;
use serde_json::Value;
use std::sync::{Arc, Weak, OnceLock};
use parking_lot::RwLock;
use std::any::Any;
use log::{debug, warn, error};
use crate::audiocontrol::eventbus::EventBus;

// Static singleton instance using OnceLock (safe, no unsafe needed)
static AUDIO_CONTROLLER_INSTANCE: OnceLock<Arc<AudioController>> = OnceLock::new();

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
        let active_idx = self.active_index.read();
        if *active_idx < self.controllers.len() {
            let controller = self.controllers[*active_idx].read();
            return controller.get_capabilities();
        }
        PlayerCapabilitySet::empty()
    }

    fn get_song(&self) -> Option<Song> {
        let active_idx = self.active_index.read();
        if *active_idx < self.controllers.len() {
            let controller = self.controllers[*active_idx].read();
            return controller.get_song();
        }
        None
    }

    fn get_loop_mode(&self) -> LoopMode {
        let active_idx = self.active_index.read();
        if *active_idx < self.controllers.len() {
            let controller = self.controllers[*active_idx].read();
            return controller.get_loop_mode();
        }
        LoopMode::None
    }

    fn get_playback_state(&self) -> PlaybackState {
        let active_idx = self.active_index.read();
        if *active_idx < self.controllers.len() {
            let controller = self.controllers[*active_idx].read();
            return controller.get_playback_state();
        }
        PlaybackState::Stopped
    }

    fn get_position(&self) -> Option<f64> {
        let active_idx = self.active_index.read();
        if *active_idx < self.controllers.len() {
            let controller = self.controllers[*active_idx].read();
            return controller.get_position();
        }
        None
    }

    fn get_shuffle(&self) -> bool {
        let active_idx = self.active_index.read();
        if *active_idx < self.controllers.len() {
            let controller = self.controllers[*active_idx].read();
            return controller.get_shuffle();
        }
        false
    }

    fn get_player_name(&self) -> String {
        let active_idx = self.active_index.read();
        if *active_idx < self.controllers.len() {
            let controller = self.controllers[*active_idx].read();
            return controller.get_player_name();
        }
        "audiocontroller".to_string()
    }

    fn get_player_id(&self) -> String {
        let active_idx = self.active_index.read();
        if *active_idx < self.controllers.len() {
            let controller = self.controllers[*active_idx].read();
            return controller.get_player_id();
        }
        "none".to_string()
    }

    fn get_last_seen(&self) -> Option<std::time::SystemTime> {
        let active_idx = self.active_index.read();
        if *active_idx < self.controllers.len() {
            let controller = self.controllers[*active_idx].read();
            return controller.get_last_seen();
        }
        None
    }

    fn send_command(&self, command: PlayerCommand) -> bool {
        let active_idx = self.active_index.read();
        if *active_idx < self.controllers.len() {
            debug!("Sending command to active controller [{}]: {}", active_idx, command);
            let controller = self.controllers[*active_idx].read();
            return controller.send_command(command);
        }
        false
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn start(&self) -> bool {
        let mut success = false;

        for controller_lock in &self.controllers {
            let controller = controller_lock.read();
            if controller.start() {
                success = true;
                debug!("Successfully started player controller: {}", controller.get_player_name());
            } else {
                warn!("Failed to start player controller: {}", controller.get_player_name());
            }
        }

        success
    }

    fn stop(&self) -> bool {
        let mut success = false;

        for controller_lock in &self.controllers {
            let controller = controller_lock.read();
            if controller.stop() {
                success = true;
                debug!("Successfully stopped player controller: {}", controller.get_player_name());
            } else {
                warn!("Failed to stop player controller: {}", controller.get_player_name());
            }
        }

        success
    }

    fn get_queue(&self) -> Vec<Track> {
        let active_idx = self.active_index.read();
        if *active_idx < self.controllers.len() {
            let controller = self.controllers[*active_idx].read();
            return controller.get_queue();
        }
        Vec::new()
    }
}

impl Default for AudioController {
    fn default() -> Self {
        Self::new()
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
        {
            let mut self_ref = controller.self_ref.write();
            *self_ref = Some(weak_ref);
            debug!("AudioController self-reference initialized");
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
        AUDIO_CONTROLLER_INSTANCE.get_or_init(|| {
            let default_config = serde_json::json!({
                "players": [],
                "action_plugins": []
            });

            Self::from_json(&default_config)
                .expect("Failed to create default AudioController")
        }).clone()
    }

    /// Initialize the singleton instance with a specific controller
    pub fn initialize_instance(controller: Arc<AudioController>) -> Result<(), String> {
        AUDIO_CONTROLLER_INSTANCE.set(controller)
            .map_err(|_| "AudioController singleton already initialized".to_string())
    }

    /// Reset the singleton instance (mainly for testing)
    #[cfg(test)]
    pub fn reset_instance() {
        // OnceLock doesn't support reset; tests should use local instances instead
    }

    /// Add a player controller to the list
    ///
    /// If this is the first controller added, it becomes the active controller.
    pub fn add_controller(&mut self, controller: Box<dyn PlayerController + Send + Sync>) -> usize {
        // Check if we have a self reference for listener registration
        let _self_weak = {
            let self_ref = self.self_ref.read();
            self_ref.as_ref().map(|weak_ref| weak_ref.clone() as Weak<dyn PlayerController + Send + Sync>)
        };

        // Wrap in Arc+RwLock and store
        let controller = Arc::new(RwLock::new(controller));
        self.controllers.push(controller);

        // If this is the first controller, make it active
        if self.controllers.len() == 1 {
            let mut active_idx = self.active_index.write();
            *active_idx = 0;
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
        let mut active_idx = self.active_index.write();
        if *active_idx == index {
            *active_idx = 0;
        } else if *active_idx > index {
            *active_idx -= 1;
        }

        true
    }

    /// Get the list of controllers
    pub fn list_controllers(&self) -> Vec<Arc<RwLock<Box<dyn PlayerController + Send + Sync>>>> {
        self.controllers.clone()
    }

    /// Get a controller by player name
    pub fn get_player_by_name(&self, player_name: &str) -> Option<Arc<RwLock<Box<dyn PlayerController + Send + Sync>>>> {
        for ctrl_lock in &self.controllers {
            let ctrl = ctrl_lock.read();
            if ctrl.get_player_name().eq_ignore_ascii_case(player_name)
                || ctrl.get_player_id().eq_ignore_ascii_case(player_name)
            {
                return Some(ctrl_lock.clone());
            }
        }
        None
    }

    /// Set the active controller by index
    ///
    /// Returns true if the active controller was changed, false if the index was invalid.
    pub fn set_active_controller(&self, index: usize) -> bool {
        if index >= self.controllers.len() {
            return false;
        }

        // Check if this is actually a change
        {
            let active_idx = self.active_index.read();
            if index == *active_idx {
                debug!("Active controller already set to index {}", index);
                return true;
            }
        }

        // Set the new active index
        let mut active_idx = self.active_index.write();
        *active_idx = index;
        debug!("Changing active controller to index {}", index);
        true
    }

    /// Get the currently active controller, if any
    pub fn get_active_controller(&self) -> Option<Arc<RwLock<Box<dyn PlayerController + Send + Sync>>>> {
        let active_idx = self.active_index.read();
        if *active_idx < self.controllers.len() {
            return Some(self.controllers[*active_idx].clone());
        }
        None
    }

    /// Send a command to the active player controller
    ///
    /// Returns true if the command was sent successfully, false if there is no active controller.
    pub fn send_command(&self, command: PlayerCommand) -> bool {
        let active_idx = self.active_index.read();
        if *active_idx < self.controllers.len() {
            let controller = self.controllers[*active_idx].read();
            return controller.send_command(command);
        }
        false
    }

    /// Send a command to all inactive player controllers
    ///
    /// Returns the number of controllers that successfully processed the command.
    pub fn send_command_to_inactives(&self, command: PlayerCommand) -> usize {
        let mut success_count = 0;

        let active_idx_value = *self.active_index.read();

        for (idx, controller) in self.controllers.iter().enumerate() {
            if idx == active_idx_value {
                continue;
            }

            let controller = controller.read();
            if controller.send_command(command.clone()) {
                success_count += 1;
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
        // Build the AudioController as an owned value so we can use &mut self
        let mut controller = AudioController::new();

        // Process player configurations if present
        if let Some(players_config) = config.get("players").and_then(|v| v.as_array()) {
            debug!("Creating AudioController players from JSON array with {} elements", players_config.len());

            for (idx, player_config) in players_config.iter().enumerate() {
                match create_player_from_json(player_config) {
                    Ok(player) => {
                        debug!("Successfully created player {} from JSON configuration", idx);
                        controller.add_controller(player);
                    },
                    Err(e) => {
                        if let PlayerCreationError::ParseError(msg) = &e {
                            if msg.contains("disabled in configuration") || msg.contains("ignored (starts with underscore)") {
                                debug!("Skipping disabled/filtered player {}: {}", idx, msg);
                                continue;
                            }
                        }

                        error!("Failed to create player {}: {}", idx, e);
                        return Err(e);
                    }
                }
            }

            if controller.controllers.is_empty() {
                warn!("No valid player controllers found in configuration");
            }
        } else if let Some(players_config) = config.as_array() {
            debug!("Using legacy format - Creating AudioController from JSON array with {} elements", players_config.len());

            for (idx, player_config) in players_config.iter().enumerate() {
                match create_player_from_json(player_config) {
                    Ok(player) => {
                        debug!("Successfully created player {} from JSON configuration", idx);
                        controller.add_controller(player);
                    },
                    Err(e) => {
                        if let PlayerCreationError::ParseError(msg) = &e {
                            if msg.contains("disabled in configuration") || msg.contains("ignored (starts with underscore)") {
                                debug!("Skipping disabled/filtered player {}: {}", idx, msg);
                                continue;
                            }
                        }

                        error!("Failed to create player {}: {}", idx, e);
                        return Err(e);
                    }
                }
            }
        }

        // Wrap in Arc now that mutation is done
        let controller = Arc::new(controller);

        // Initialize the self-reference (needs Arc)
        AudioController::initialize(&controller);

        // Process action plugin configurations if present
        if let Some(plugins_config) = config.get("action_plugins").and_then(|v| v.as_array()) {
            debug!("Creating action plugins from JSON array with {} elements", plugins_config.len());

            let factory = crate::plugins::plugin_factory::PluginFactory::new();

            for (idx, plugin_config) in plugins_config.iter().enumerate() {
                if let Some(enabled) = plugin_config.get("enabled").and_then(Value::as_bool) {
                    if !enabled {
                        debug!("Skipping disabled action plugin at index {}", idx);
                        continue;
                    }
                }

                if let Ok(json_str) = serde_json::to_string(plugin_config) {
                    match factory.create_action_plugin_from_json(&json_str) {
                        Some(plugin) => {
                            debug!("Successfully created action plugin {} from JSON configuration", idx);
                            controller.add_action_plugin(plugin);
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

    /// Add an action plugin to the controller
    /// Returns the index of the added plugin
    pub fn add_action_plugin(&self, mut plugin: Box<dyn ActionPlugin + Send + Sync>) -> usize {
        let self_ref = self.self_ref.read();
        if let Some(weak_ref) = self_ref.as_ref() {
            plugin.initialize(weak_ref.clone());
            plugin.init();

            let mut plugins = self.action_plugins.write();
            plugins.push(plugin);
            debug!("Added action plugin at index {}", plugins.len() - 1);
            return plugins.len() - 1;
        } else {
            error!("Cannot add action plugin: AudioController self-reference not initialized");
        }
        0
    }

    /// Remove an action plugin by index
    /// Returns true if the plugin was successfully removed
    pub fn remove_action_plugin(&self, index: usize) -> bool {
        let mut plugins = self.action_plugins.write();
        if index < plugins.len() {
            plugins.remove(index);
            debug!("Removed action plugin at index {}", index);
            return true;
        }
        false
    }

    /// Get the number of action plugins
    pub fn action_plugin_count(&self) -> usize {
        let plugins = self.action_plugins.read();
        plugins.len()
    }

    /// Clear all action plugins
    pub fn clear_action_plugins(&self) -> usize {
        let mut plugins = self.action_plugins.write();
        let count = plugins.len();
        plugins.clear();
        debug!("Cleared {} action plugins", count);
        count
    }

    /// Add multiple action plugins from a vector
    pub fn add_action_plugins(&self, plugins: Vec<Box<dyn ActionPlugin + Send + Sync>>) -> usize {
        let count = plugins.len();

        for plugin in plugins {
            self.add_action_plugin(plugin);
        }

        debug!("Added {} action plugins", count);
        count
    }

    /// Get information about all registered action plugins
    pub fn get_action_plugin_info(&self) -> Vec<(String, String)> {
        let plugins = self.action_plugins.read();
        plugins.iter()
            .map(|plugin| (plugin.name().to_string(), plugin.version().to_string()))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    // Add tests here later
}
