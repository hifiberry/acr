use crate::players::{MPDPlayerController, NullPlayerController, PlayerController};
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

/// Factory functions for creating PlayerController instances
pub fn create_player_from_json(config: &Value) -> Result<Box<dyn PlayerController>, PlayerCreationError> {
    // Expect a single key-value pair where key is the player type
    if let Some((player_type, config_obj)) = config.as_object().and_then(|obj| obj.iter().next()) {
        // Check if the player is enabled (default to true if not specified)
        let enabled = config_obj.get("enable")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
            
        // Skip creating the player if it's disabled
        if !enabled {
            return Err(PlayerCreationError::ParseError(
                format!("Player {} is disabled in configuration", player_type)
            ));
        }
        
        match player_type.as_str() {
            "mpd" => {
                // Create MPDPlayer with config
                let host = config_obj.get("host")
                    .and_then(|v| v.as_str())
                    .unwrap_or("localhost");
                
                let port = config_obj.get("port")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(6600) as u16;
                
                let player = MPDPlayerController::with_connection(host, port);
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

/// Returns a default JSON configuration string that includes all available player controllers
///
/// This function is useful for initializing a new project with all available player controllers
/// in their default configuration.
///
/// # Returns
///
/// A JSON string containing only the players array with all available controllers
pub fn sample_json_config() -> String {
    // Create a JSON configuration with all available player controllers
    let config = serde_json::json!([
        {
            "mpd": {
                "host": "localhost", 
                "port": 6600,
                "enable": true
            }
        },
        {
            "null": {
                "enable": false
            }
        }
    ]);

    serde_json::to_string_pretty(&config).unwrap_or_else(|_| "[]".to_string())
}