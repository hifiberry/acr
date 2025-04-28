use crate::players::base_controller::BasePlayerController;
use crate::players::player_controller::PlayerController;
use crate::data::{PlayerCapability, Song, LoopMode, PlayerState, PlayerCommand};
use std::sync::{Arc, Weak};

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
        Self {
            base: BasePlayerController::new(),
            hostname: "localhost".to_string(),
            port: 8000,
        }
    }
    
    /// Create a new MPD player controller with custom settings
    pub fn with_connection(hostname: &str, port: u16) -> Self {
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
        self.hostname = hostname.to_string();
        self.port = port;
    }
    
    /// Helper method for simulating state changes (for demo purposes)
    pub fn notify_state_changed(&self, state: PlayerState) {
        self.base.notify_state_changed(state);
    }
    
    /// Helper method for simulating song changes (for demo purposes)
    pub fn notify_song_changed(&self, song: Option<&Song>) {
        self.base.notify_song_changed(song);
    }
    
    /// Helper method for simulating loop mode changes (for demo purposes)
    pub fn notify_loop_mode_changed(&self, mode: LoopMode) {
        self.base.notify_loop_mode_changed(mode);
    }
    
    /// Helper method for simulating capability changes (for demo purposes)
    pub fn notify_capabilities_changed(&self, capabilities: &[PlayerCapability]) {
        self.base.notify_capabilities_changed(capabilities);
    }
}

impl PlayerController for MPDPlayer {
    fn get_capabilities(&self) -> Vec<PlayerCapability> {
        // Return basic capabilities for now
        vec![
            PlayerCapability::Play,
            PlayerCapability::Pause,
            PlayerCapability::PlayPause,
            PlayerCapability::Stop,
        ]
    }
    
    fn get_song(&self) -> Option<Song> {
        // Not implemented yet
        None
    }
    
    fn get_loop_mode(&self) -> LoopMode {
        // Not implemented yet
        LoopMode::None
    }
    
    fn get_player_state(&self) -> PlayerState {
        // Not implemented yet
        PlayerState::Stopped
    }
    
    fn send_command(&self, _command: PlayerCommand) -> bool {
        // Not implemented yet
        false
    }
    
    fn register_state_listener(&mut self, listener: Weak<dyn crate::players::player_controller::PlayerStateListener>) -> bool {
        self.base.register_listener(listener)
    }
    
    fn unregister_state_listener(&mut self, listener: &Arc<dyn crate::players::player_controller::PlayerStateListener>) -> bool {
        self.base.unregister_listener(listener)
    }
}