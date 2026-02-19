use crate::players::{MPDPlayerController, NullPlayerController, PlayerController, raat::RAATPlayerController, librespot::LibrespotPlayerController, lms::lmsaudio::LMSAudioController, generic::GenericPlayerController, ShairportController, BluetoothPlayerController};

// MPRIS support is only available on Unix-like systems
#[cfg(not(windows))]
use crate::players::mpris::MprisPlayerController;
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
        // Filter out players that start with underscore (commented/disabled convention)
        if player_type.starts_with('_') {
            return Err(PlayerCreationError::ParseError(
                format!("Player {} is ignored (starts with underscore)", player_type)
            ));
        }
        
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
                
                // Check if load_mpd_library parameter is specified in the JSON
                let load_library = config_obj.get("load_mpd_library")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(true); // Default to true if not specified

                // Check if enhance_metadata parameter is specified in the JSON
                let enhance_metadata = config_obj.get("enhance_metadata")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(true); // Default to true if not specified
                
                // Check if extract_coverart parameter is specified in the JSON
                let extract_coverart = config_obj.get("extract_coverart")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(true); // Default to true if not specified
                
                // Check if artist_separator array is specified in the JSON
                let artist_separators = config_obj.get("artist_separator")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|val| val.as_str().map(|s| s.to_string()))
                            .collect::<Vec<String>>()
                    });
                
                // Check if max_reconnect_attempts is specified in the JSON
                let max_reconnect_attempts = config_obj.get("max_reconnect_attempts")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(5) as u32; // Default to 5 attempts if not specified
                
                // Check if music_directory is specified in the JSON
                let music_directory = config_obj.get("music_directory")
                    .and_then(|v| v.as_str())
                    .unwrap_or("") // Default to empty string if not specified
                    .to_string();
                
                let mut player = MPDPlayerController::with_connection(host, port);
                player.set_load_mpd_library(load_library);
                player.set_enhance_metadata(enhance_metadata);
                player.set_extract_coverart(extract_coverart);
                player.set_max_reconnect_attempts(max_reconnect_attempts);
                player.set_music_directory(music_directory);
                
                // Set custom artist separators if provided
                if let Some(separators) = artist_separators {
                    player.set_artist_separators(separators);
                }
                
                Ok(Box::new(player))
            },            "raat" => {
                // Create RAATPlayerController with config
                let metadata_source = config_obj.get("metadata_pipe")
                    .and_then(|v| v.as_str())
                    .unwrap_or("/var/run/raat/metadata_pipe");

                let control_pipe = config_obj.get("control_pipe")
                    .and_then(|v| v.as_str())
                    .unwrap_or("/var/run/raat/control_pipe");
                
                // Check if reopen_metadata_pipe parameter is specified in the JSON
                let reopen = config_obj.get("reopen_metadata_pipe")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(true); // Default to true if not specified
                
                // Check if systemd_unit parameter is specified in the JSON
                let systemd_unit = config_obj.get("systemd_unit")
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty()); // Filter out empty strings
                
                let player = RAATPlayerController::with_pipes_and_reopen_and_systemd(
                    metadata_source, 
                    control_pipe, 
                    reopen, 
                    systemd_unit
                );
                Ok(Box::new(player))
            },
            "librespot" => {
                // Create LibrespotPlayerController with config
                let process_name = config_obj.get("process_name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("/usr/bin/librespot");
                
                // Check if systemd_unit parameter is specified in the JSON
                let systemd_unit = config_obj.get("systemd_unit")
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty()); // Filter out empty strings
                
                // Check if on_pause_event parameter is specified in the JSON
                let on_pause_event = config_obj.get("on_pause_event")
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty()) // Filter out empty strings
                    .map(|s| s.to_string());
                
                let mut player = LibrespotPlayerController::with_full_config(
                    process_name,
                    systemd_unit
                );
                
                // Set the on_pause_event configuration
                player.set_on_pause_event(on_pause_event);
                
                Ok(Box::new(player))
            },
            "lms" => {
                // Create LMSAudioController with config
                let player = LMSAudioController::new(config_obj.clone());
                Ok(Box::new(player))
            },
            "generic" => {
                // Create GenericPlayerController from config
                let player = GenericPlayerController::from_config(config_obj)
                    .map_err(PlayerCreationError::ParseError)?;
                Ok(Box::new(player))
            },
            "shairport" => {
                // Create ShairportController with config
                let player = ShairportController::from_config(config_obj);
                Ok(Box::new(player))
            },
            "bluetooth" => {
                // Create BluetoothPlayerController with config
                let device_address = config_obj.get("device_address")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                
                let player = BluetoothPlayerController::new_with_address(device_address);
                Ok(Box::new(player))
            },
            #[cfg(not(windows))]
            "mpris" => {
                // Create MprisPlayerController with config (Unix/Linux only)
                let bus_name = config_obj.get("bus_name")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| PlayerCreationError::MissingField("bus_name".to_string()))?;
                
                let poll_interval = config_obj.get("poll_interval")
                    .and_then(|v| v.as_f64())
                    .map(std::time::Duration::from_secs_f64)
                    .unwrap_or_else(|| std::time::Duration::from_secs_f64(1.0));
                
                let player = MprisPlayerController::new_with_poll_interval(bus_name, poll_interval);
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

// sample_json_config method removed as it's no longer used