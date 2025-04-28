use crate::players::base_controller::BasePlayerController;
use crate::players::player_controller::{PlayerController, PlayerStateListener};
use crate::data::{PlayerCapability, Song, LoopMode, PlayerState, PlayerCommand};
use delegate::delegate;
use std::sync::{Arc, Weak};
use log::{debug, info};
use std::any::Any;

/// A null player controller that does nothing
/// 
/// This implementation is useful for debugging and testing purposes.
/// All methods return default values and no actual operations are performed.
pub struct NullPlayerController {
    /// Base controller for managing state listeners
    base: BasePlayerController,
}

impl NullPlayerController {
    /// Create a new null player controller
    pub fn new() -> Self {
        debug!("Creating new NullPlayerController");
        let player = Self {
            base: BasePlayerController::with_player_info("null", "null"),
        };
        
        // Set default capabilities
        player.set_default_capabilities();
        
        player
    }
    
    /// Set the default capabilities for this player
    fn set_default_capabilities(&self) {
        debug!("Setting default NullPlayerController capabilities");
        self.base.set_capabilities(vec![
            PlayerCapability::Play,
            PlayerCapability::Pause,
            PlayerCapability::PlayPause,
            PlayerCapability::Stop,
            PlayerCapability::Next,
            PlayerCapability::Previous,
            PlayerCapability::Seek,
            PlayerCapability::Loop,
            PlayerCapability::Shuffle,
        ], false); // Don't notify on initialization
    }
}

impl PlayerController for NullPlayerController {
    delegate! {
        to self.base {
            fn register_state_listener(&mut self, listener: Weak<dyn PlayerStateListener>) -> bool;
            fn unregister_state_listener(&mut self, listener: &Arc<dyn PlayerStateListener>) -> bool;
            fn get_capabilities(&self) -> Vec<PlayerCapability>;
        }
    }
    
    fn get_song(&self) -> Option<Song> {
        debug!("NullPlayerController: get_song called");
        None // Always return None as we don't have any real song
    }
    
    fn get_loop_mode(&self) -> LoopMode {
        debug!("NullPlayerController: get_loop_mode called");
        LoopMode::None // Default loop mode
    }
    
    fn get_player_state(&self) -> PlayerState {
        debug!("NullPlayerController: get_player_state called");
        PlayerState::Stopped // Always return stopped state
    }
    
    fn get_player_name(&self) -> String {
        "null".to_string()
    }
    
    fn get_player_id(&self) -> String {
        "null".to_string()
    }
    
    fn send_command(&self, command: PlayerCommand) -> bool {
        info!("NullPlayerController: Command received (no action taken): {}", command);
        true // Always return success
    }
    
    fn as_any(&self) -> &dyn Any {
        self
    }
    
    fn start(&self) -> bool {
        debug!("NullPlayerController: start() called (no-op)");
        // Nothing to do for the null player, just return success
        true
    }
    
    fn stop(&self) -> bool {
        debug!("NullPlayerController: stop() called (no-op)");
        // Nothing to do for the null player, just return success
        true
    }
}