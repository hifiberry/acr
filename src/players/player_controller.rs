use crate::data::{PlayerCapability, Song, LoopMode, PlayerState, PlayerCommand};
use std::sync::{Arc, Weak};
use std::sync::atomic::AtomicBool;
use std::any::Any;
use crate::players::{MPDPlayer, NullPlayerController};
use serde_json::Value;
use std::error::Error;
use std::fmt;

/// Error type for player creation
#[derive(Debug)]
pub enum PlayerCreationError {
    InvalidType(String),
    MissingField(String),
    ParseError(String),
}

impl fmt::Display for PlayerCreationError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            PlayerCreationError::InvalidType(s) => write!(f, "Invalid player type: {}", s),
            PlayerCreationError::MissingField(s) => write!(f, "Missing required field: {}", s),
            PlayerCreationError::ParseError(s) => write!(f, "Error parsing config: {}", s),
        }
    }
}

impl Error for PlayerCreationError {}

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

/// Factory functions for creating PlayerController instances
pub fn create_player_from_json(config: &Value) -> Result<Box<dyn PlayerController>, PlayerCreationError> {
    // Expect a single key-value pair where key is the player type
    if let Some((player_type, config_obj)) = config.as_object().and_then(|obj| obj.iter().next()) {
        match player_type.as_str() {
            "mpd" => {
                // Create MPDPlayer with config
                let host = config_obj.get("host")
                    .and_then(|v| v.as_str())
                    .unwrap_or("localhost");
                
                let port = config_obj.get("port")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(6600) as u16;
                
                let player = MPDPlayer::with_connection(host, port);
                Ok(Box::new(player))
            },
            "null" => {
                // Create NullPlayerController
                let player = NullPlayerController::new();
                Ok(Box::new(player))
            },
            unknown => {
                Err(PlayerCreationError::InvalidType(unknown.to_string()))
            }
        }
    } else {
        Err(PlayerCreationError::ParseError(
            "Expected object with player type as key".to_string()
        ))
    }
}

/// Helper function to create a player from a JSON string
pub fn create_player_from_json_str(json_str: &str) -> Result<Box<dyn PlayerController>, Box<dyn Error>> {
    let config: Value = serde_json::from_str(json_str)?;
    Ok(create_player_from_json(&config)?)
}