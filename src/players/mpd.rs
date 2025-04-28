use crate::players::base_controller::BasePlayerController;
use crate::players::player_controller::PlayerController;
use crate::data::{PlayerCapability, Song, LoopMode, PlayerState, PlayerCommand};
use std::sync::{Arc, Weak};
use log::{debug, info, warn, error};

/// MPD player controller implementation
pub struct MPDPlayer {
    /// Base controller for managing state listeners
    base: BasePlayerController,
    
    /// MPD server hostname
    hostname: String,
    
    /// MPD server port
    port: u16,
}

impl MPDPlayer {
    /// Create a new MPD player controller with default settings
    pub fn new() -> Self {
        debug!("Creating new MPDPlayer with default settings");
        Self {
            base: BasePlayerController::new(),
            hostname: "localhost".to_string(),
            port: 8000,
        }
    }
    
    /// Create a new MPD player controller with custom settings
    pub fn with_connection(hostname: &str, port: u16) -> Self {
        debug!("Creating new MPDPlayer with connection {}:{}", hostname, port);
        Self {
            base: BasePlayerController::new(),
            hostname: hostname.to_string(),
            port,
        }
    }
    
    /// Get the current MPD server hostname
    pub fn hostname(&self) -> &str {
        &self.hostname
    }
    
    /// Get the current MPD server port
    pub fn port(&self) -> u16 {
        self.port
    }
    
    /// Update the connection settings
    pub fn set_connection(&mut self, hostname: &str, port: u16) {
        debug!("Updating MPD connection to {}:{}", hostname, port);
        self.hostname = hostname.to_string();
        self.port = port;
    }
    
    /// Helper method for simulating state changes (for demo purposes)
    pub fn notify_state_changed(&self, state: PlayerState) {
        debug!("MPDPlayer forwarding state change notification: {}", state);
        self.base.notify_state_changed(state);
    }
    
    /// Helper method for simulating song changes (for demo purposes)
    pub fn notify_song_changed(&self, song: Option<&Song>) {
        let song_title = song.map_or("None".to_string(), |s| s.title.as_deref().unwrap_or("Unknown").to_string());
        debug!("MPDPlayer forwarding song change notification: {}", song_title);
        self.base.notify_song_changed(song);
    }
    
    /// Helper method for simulating loop mode changes (for demo purposes)
    pub fn notify_loop_mode_changed(&self, mode: LoopMode) {
        debug!("MPDPlayer forwarding loop mode change notification: {}", mode);
        self.base.notify_loop_mode_changed(mode);
    }
    
    /// Helper method for simulating capability changes (for demo purposes)
    pub fn notify_capabilities_changed(&self, capabilities: &[PlayerCapability]) {
        debug!("MPDPlayer forwarding capabilities change notification with {} capabilities", capabilities.len());
        self.base.notify_capabilities_changed(capabilities);
    }
}

impl PlayerController for MPDPlayer {
    fn get_capabilities(&self) -> Vec<PlayerCapability> {
        debug!("Getting MPDPlayer capabilities");
        // Return basic capabilities for now
        vec![
            PlayerCapability::Play,
            PlayerCapability::Pause,
            PlayerCapability::PlayPause,
            PlayerCapability::Stop,
        ]
    }
    
    fn get_song(&self) -> Option<Song> {
        debug!("Getting current song (not implemented yet)");
        // Not implemented yet
        None
    }
    
    fn get_loop_mode(&self) -> LoopMode {
        debug!("Getting current loop mode (not implemented yet)");
        // Not implemented yet
        LoopMode::None
    }
    
    fn get_player_state(&self) -> PlayerState {
        debug!("Getting current player state (not implemented yet)");
        // Not implemented yet
        PlayerState::Stopped
    }
    
    fn send_command(&self, command: PlayerCommand) -> bool {
        info!("Sending command to MPD: {}", command);
        // Not implemented yet
        warn!("MPD command implementation not yet available");
        false
    }
    
    fn register_state_listener(&mut self, listener: Weak<dyn crate::players::player_controller::PlayerStateListener>) -> bool {
        debug!("Registering new state listener with MPDPlayer");
        self.base.register_listener(listener)
    }
    
    fn unregister_state_listener(&mut self, listener: &Arc<dyn crate::players::player_controller::PlayerStateListener>) -> bool {
        debug!("Unregistering state listener from MPDPlayer");
        self.base.unregister_listener(listener)
    }
}