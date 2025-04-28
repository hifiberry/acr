use crate::data::{PlayerCapability, Song, LoopMode, PlayerState, PlayerCommand};
use std::sync::{Arc, Weak};
use std::any::Any;

/// Trait for objects that listen to PlayerController state changes
pub trait PlayerStateListener: Send + Sync {
    /// Called when the player state changes
    fn on_state_changed(&self, state: PlayerState);
    
    /// Called when the current song changes
    fn on_song_changed(&self, song: Option<Song>);
    
    /// Called when the loop mode changes
    fn on_loop_mode_changed(&self, mode: LoopMode);
    
    /// Called when a player capability is added or removed
    fn on_capabilities_changed(&self, capabilities: Vec<PlayerCapability>);
    
    /// Convert to Any for dynamic casting
    fn as_any(&self) -> &dyn Any;
}

/// PlayerController trait - abstract interface for player implementations
/// 
/// This trait defines the core functionality that any player implementation must provide.
/// It serves as an abstraction layer for different media player backends.
pub trait PlayerController {
    /// Get the capabilities of the player
    /// 
    /// Returns a vector of capabilities supported by this player
    fn get_capabilities(&self) -> Vec<PlayerCapability>;
    
    /// Get the current song being played
    /// 
    /// Returns the current song, or None if no song is playing
    fn get_song(&self) -> Option<Song>;
    
    /// Get the current loop mode setting
    /// 
    /// Returns the current loop mode of the player
    fn get_loop_mode(&self) -> LoopMode;
    
    /// Get the current player state
    /// 
    /// Returns the current state of the player (playing, paused, stopped, etc.)
    fn get_player_state(&self) -> PlayerState;
    
    /// Send a command to the player
    /// 
    /// # Arguments
    /// 
    /// * `command` - The command to send to the player
    /// 
    /// # Returns
    /// 
    /// `true` if the command was successfully processed, `false` otherwise
    fn send_command(&self, command: PlayerCommand) -> bool;
    
    /// Register a state listener to be notified of state changes
    /// 
    /// # Arguments
    /// 
    /// * `listener` - The listener to register
    /// 
    /// # Returns
    /// 
    /// `true` if the listener was successfully registered, `false` otherwise
    fn register_state_listener(&mut self, listener: Weak<dyn PlayerStateListener>) -> bool;
    
    /// Unregister a previously registered state listener
    /// 
    /// # Arguments
    /// 
    /// * `listener` - The listener to unregister
    /// 
    /// # Returns
    /// 
    /// `true` if the listener was successfully unregistered, `false` if it wasn't registered
    fn unregister_state_listener(&mut self, listener: &Arc<dyn PlayerStateListener>) -> bool;
}