use crate::players::player_controller::{BasePlayerController, PlayerController};
use crate::data::{PlayerCapability, PlayerCapabilitySet, Song, LoopMode, PlaybackState, PlayerCommand};
use delegate::delegate;
use log::{debug, info, warn};
use std::any::Any;

/// A null player controller that does nothing
/// 
/// This implementation is useful for debugging and testing purposes.
/// All methods return default values and no actual operations are performed.
pub struct NullPlayerController {
    /// Base controller for managing state listeners
    base: BasePlayerController,
}

impl Default for NullPlayerController {
    fn default() -> Self {
        Self::new()
    }
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
        let capabilities = vec![
            PlayerCapability::Play,
            PlayerCapability::Pause,
            PlayerCapability::PlayPause,
            PlayerCapability::Stop,
            PlayerCapability::Next,
            PlayerCapability::Previous,
            PlayerCapability::Seek,
            PlayerCapability::Loop,
            PlayerCapability::Shuffle,
            // Killable capability not supported in NullPlayerController
        ];
        
        self.base.set_capabilities(capabilities, false); // Don't notify on initialization
    }
}

impl PlayerController for NullPlayerController {
    delegate! {
        to self.base {
            fn get_capabilities(&self) -> PlayerCapabilitySet;
            fn get_last_seen(&self) -> Option<std::time::SystemTime>;
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
    
    fn get_playback_state(&self) -> PlaybackState {
        debug!("NullPlayerController: get_playback_state called");
        PlaybackState::Stopped // Always return stopped state
    }
    
    fn get_position(&self) -> Option<f64> {
        debug!("NullPlayerController: get_position called");
        None // No position information for the null player
    }
    
    fn get_shuffle(&self) -> bool {
        debug!("NullPlayerController: get_shuffle called");
        false // Default shuffle state
    }
    
    fn get_player_name(&self) -> String {
        "null".to_string()
    }
    
    fn get_player_id(&self) -> String {
        "null".to_string()
    }
    
    fn send_command(&self, command: PlayerCommand) -> bool {
        match command {
            PlayerCommand::Kill => {
                info!("NullPlayerController: Kill command received but not supported");
                warn!("NullPlayerController: Kill operation not supported, Killable capability not advertised");
                false // Return failure since this operation is not supported
            },
            _ => {
                info!("NullPlayerController: Command received (no action taken): {}", command);
                true // Return success for all other commands
            }
        }
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
    
    fn get_queue(&self) -> Vec<crate::data::Track> {
        debug!("NullPlayerController: get_queue called - returning empty vector");
        Vec::new()
    }
}