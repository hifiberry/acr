use crate::data::{PlayerCapability, Song, LoopMode, PlayerState, PlayerCommand, PlayerEvent};
use std::sync::{Arc, Weak};
use std::any::Any;

/// Trait for objects that listen to PlayerController state changes
pub trait PlayerStateListener: Send + Sync {
    /// Called when any player event occurs
    /// 
    /// # Arguments
    /// 
    /// * `event` - The event that occurred
    fn on_event(&self, event: PlayerEvent);
    
    /// Convert to Any for dynamic casting
    fn as_any(&self) -> &dyn Any;
}

/// PlayerController trait - abstract interface for player implementations
/// 
/// This trait defines the core functionality that any player implementation must provide.
/// It serves as an abstraction layer for different media player backends.
pub trait PlayerController: Send + Sync {
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
    
    /// Get the name of this player controller
    /// 
    /// Returns a string identifier for this type of player (e.g., "mpd", "null")
    fn get_player_name(&self) -> String;
    
    /// Get a unique identifier for this player instance
    /// 
    /// Returns a string that uniquely identifies this player instance
    fn get_player_id(&self) -> String;
    
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
    
    /// Downcasts the player controller to a concrete type via Any
    /// 
    /// This allows accessing implementation-specific functionality when needed.
    fn as_any(&self) -> &dyn Any;
    
    /// Starts the player controller
    /// 
    /// This initializes any background threads and connections needed for the player to operate.
    /// Returns true if the player was successfully started, false otherwise.
    fn start(&self) -> bool;
    
    /// Stops the player controller
    /// 
    /// This cleans up any resources used by the player, including stopping background threads
    /// and closing connections. Returns true if the player was successfully stopped, false otherwise.
    fn stop(&self) -> bool;
}