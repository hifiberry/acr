use crate::AudioController;
use crate::data::{PlaybackState, PlayerCommand, LoopMode, Song, Track, PlayerUpdate, PlayerCapability}; // Added PlayerCapability
use crate::players::PlayerController; // Fixed: Using the public re-export
use rocket::serde::json::Json;
use rocket::{get, post, State};

/// Pause all players with optional exclusion
#[post("/players/pause-all?<except>")]
pub fn pause_all_players(controller: &State<Arc<AudioController>>, except: Option<String>) -> Json<CommandResponse> {
    let audio_controller = controller.inner();
    let mut success_count = 0;
    let mut skipped_count = 0;
    let controllers = audio_controller.list_controllers();
    
    for ctrl_lock in controllers {
        if let Ok(ctrl) = ctrl_lock.read() {
            let player_name = ctrl.get_player_name();
            let player_id = ctrl.get_player_id();
            
            // Check if this player should be excluded
            if let Some(ref except_name) = except {
                if player_name.eq_ignore_ascii_case(except_name) || player_id.eq_ignore_ascii_case(except_name) {
                    skipped_count += 1;
                    continue;
                }
            }
            
            let caps = ctrl.get_capabilities();
            let did_pause = if caps.has_capability(crate::data::capabilities::PlayerCapability::Pause) {
                ctrl.send_command(PlayerCommand::Pause)
            } else if caps.has_capability(crate::data::capabilities::PlayerCapability::Stop) {
                ctrl.send_command(PlayerCommand::Stop)
            } else {
                false
            };
            if did_pause {
                success_count += 1;
            }
        }
    }
    
    let success = success_count > 0;
    let message = if let Some(ref except_name) = except {
        if success {
            format!("Paused or stopped {} players (skipped {} player '{}')", success_count, skipped_count, except_name)
        } else {
            if skipped_count > 0 {
                format!("No players paused or stopped (skipped {} player '{}')", skipped_count, except_name)
            } else {
                "No players paused or stopped".to_string()
            }
        }
    } else {
        if success {
            format!("Paused or stopped {} players", success_count)
        } else {
            "No players paused or stopped".to_string()
        }
    };
    
    Json(CommandResponse {
        success,
        message,
    })
}

/// Stop all players with optional exclusion
#[post("/players/stop-all?<except>")]
pub fn stop_all_players(controller: &State<Arc<AudioController>>, except: Option<String>) -> Json<CommandResponse> {
    let audio_controller = controller.inner();
    let mut success_count = 0;
    let mut skipped_count = 0;
    let controllers = audio_controller.list_controllers();
    
    for ctrl_lock in controllers {
        if let Ok(ctrl) = ctrl_lock.read() {
            let player_name = ctrl.get_player_name();
            let player_id = ctrl.get_player_id();
            
            // Check if this player should be excluded
            if let Some(ref except_name) = except {
                if player_name.eq_ignore_ascii_case(except_name) || player_id.eq_ignore_ascii_case(except_name) {
                    skipped_count += 1;
                    continue;
                }
            }
            
            let caps = ctrl.get_capabilities();
            let did_stop = if caps.has_capability(crate::data::capabilities::PlayerCapability::Stop) {
                ctrl.send_command(PlayerCommand::Stop)
            } else if caps.has_capability(crate::data::capabilities::PlayerCapability::Pause) {
                ctrl.send_command(PlayerCommand::Pause)
            } else {
                false
            };
            if did_stop {
                success_count += 1;
            }
        }
    }
    
    let success = success_count > 0;
    let message = if let Some(ref except_name) = except {
        if success {
            format!("Stopped or paused {} players (skipped {} player '{}')", success_count, skipped_count, except_name)
        } else {
            if skipped_count > 0 {
                format!("No players stopped or paused (skipped {} player '{}')", skipped_count, except_name)
            } else {
                "No players stopped or paused".to_string()
            }
        }
    } else {
        if success {
            format!("Stopped or paused {} players", success_count)
        } else {
            "No players stopped or paused".to_string()
        }
    };
    
    Json(CommandResponse {
        success,
        message,
    })
}
use std::sync::Arc;
use rocket::response::status::Custom;
use rocket::http::Status;
use std::str::FromStr; // Add this line to import FromStr trait
use log::debug;

/// Response struct for the current active player
#[derive(serde::Serialize)]
pub struct CurrentPlayerResponse {
    name: String,
    id: String,
    state: PlaybackState,
    last_seen: Option<String>, // ISO 8601 formatted timestamp of when the player was last seen
}

/// Response struct for listing all available players
#[derive(serde::Serialize)]
pub struct PlayersListResponse {
    players: Vec<PlayerInfo>,
}

/// Information about a player for the API response
#[derive(serde::Serialize)]
pub struct PlayerInfo {
    name: String,
    id: String,
    state: PlaybackState,
    is_active: bool,
    has_library: bool,
    supports_api_events: bool, // Whether the player supports receiving API events/updates
    last_seen: Option<String>, // ISO 8601 formatted timestamp of when the player was last seen
    shuffle: bool, // Whether shuffle is enabled
    loop_mode: LoopMode, // Loop mode (None, Track, Playlist)
    position: Option<f64>, // Current playback position in seconds
    capabilities: Vec<PlayerCapability>, // List of capabilities this player supports
}

/// Response for command execution
#[derive(serde::Serialize)]
pub struct CommandResponse {
    success: bool,
    message: String,
}

/// Response struct for the now-playing information
#[derive(serde::Serialize)]
pub struct NowPlayingResponse {
    player: PlayerInfo,
    song: Option<Song>, 
    state: PlaybackState,
    shuffle: bool,
    loop_mode: LoopMode,
    position: Option<f64>, // Current playback position in seconds
}

/// Response struct for the player queue
#[derive(serde::Serialize)]
pub struct QueueResponse {
    player: String,
    queue: Vec<Track>,
}

/// Response struct for player metadata
#[derive(serde::Serialize)]
pub struct MetadataResponse {
    player_name: String,
    metadata: std::collections::HashMap<String, serde_json::Value>,
}

/// Response struct for a specific metadata key
#[derive(serde::Serialize)]
pub struct MetadataKeyResponse {
    player_name: String,
    key: String,
    value: Option<serde_json::Value>,
}

/// Response for player update operation
#[derive(serde::Serialize)]
pub struct PlayerUpdateResponse {
    success: bool,
    message: String,
}

/// Get the current active player
#[get("/player")]
pub fn get_current_player(controller: &State<Arc<AudioController>>) -> Json<CurrentPlayerResponse> {
    let active_controller = controller.inner().get_active_controller();
    
    if let Some(active_ctrl) = active_controller {
        if let Ok(player) = active_ctrl.read() {
            let name = player.get_player_name();
            let id = player.get_player_id();
            let state = player.get_playback_state();
            
            // Format last_seen timestamp if available
            let last_seen = player.get_last_seen()
                .map(|time| {
                    // Convert SystemTime to ISO 8601 format string
                    chrono::DateTime::<chrono::Utc>::from(time).to_rfc3339()
                });
            
            return Json(CurrentPlayerResponse {
                name,
                id,
                state,
                last_seen,
            });
        }
    }
    
    // Return a default response if no active player
    Json(CurrentPlayerResponse {
        name: "none".to_string(),
        id: "none".to_string(),
        state: PlaybackState::Unknown,
        last_seen: None,
    })
}

/// List all available players
#[get("/players")]
pub fn list_players(controller: &State<Arc<AudioController>>) -> Json<PlayersListResponse> {
    let audio_controller = controller.inner();
    let controllers = audio_controller.list_controllers();
    
    // Get current player info through the AudioController
    // We can use these methods because we imported the PlayerController trait
    let current_player_name = audio_controller.get_player_name();
    let current_player_id = audio_controller.get_player_id();
    
    let players_info: Vec<PlayerInfo> = controllers.iter()
        .map(|ctrl_lock| {
            if let Ok(ctrl) = ctrl_lock.read() {
                let name = ctrl.get_player_name();
                let id = ctrl.get_player_id();
                
                // Format last_seen timestamp if available
                let last_seen = ctrl.get_last_seen()
                    .map(|time| {
                        // Convert SystemTime to ISO 8601 format string
                        chrono::DateTime::<chrono::Utc>::from(time).to_rfc3339()
                    });
                
                PlayerInfo {
                    name: name.clone(),
                    id: id.clone(),
                    state: ctrl.get_playback_state(),
                    is_active: name == current_player_name && id == current_player_id,
                    has_library: ctrl.has_library(),
                    supports_api_events: ctrl.supports_api_events(),
                    last_seen,
                    shuffle: ctrl.get_shuffle(),
                    loop_mode: ctrl.get_loop_mode(),
                    position: ctrl.get_position(),
                    capabilities: ctrl.get_capabilities().to_vec(),
                }
            } else {
                // Fallback for locked controllers
                PlayerInfo {
                    name: "unknown".to_string(),
                    id: "unknown".to_string(),
                    state: PlaybackState::Unknown,
                    is_active: false,
                    has_library: false,
                    supports_api_events: false,
                    last_seen: None,
                    shuffle: false,
                    loop_mode: LoopMode::None,
                    position: None,
                    capabilities: vec![],
                }
            }
        })
        .collect();
    
    Json(PlayersListResponse {
        players: players_info,
    })
}

/// Request body for add_track command
#[derive(serde::Deserialize)]
pub struct AddTrackRequest {
    uri: String,
    #[serde(default)]
    metadata: Option<std::collections::HashMap<String, serde_json::Value>>,
}

/// Send a command to a specific player by name
/// 
/// If the player name is "active", the currently active player will be used.
/// Otherwise, it will find a player with the specified name.
/// 
/// Supported commands:
/// - Simple commands: play, pause, playpause, stop, next, previous, kill, clear_queue
/// - Complex commands with parameters:
///   - set_loop:none|track|playlist - Sets loop mode
///   - seek:<seconds> - Seek to position in seconds
///   - set_random:true|false - Toggle shuffle mode
///   - remove_track:<uri> - Remove a track from the queue
/// - add_track - Add a track to the queue (requires JSON body with uri field)
#[post("/player/<n>/command/<command>", data = "<request_data>")]
pub fn send_command_to_player_by_name(
    n: &str,
    command: &str,
    request_data: Option<Json<serde_json::Value>>,
    controller: &State<Arc<AudioController>>
) -> Result<Json<CommandResponse>, Custom<Json<CommandResponse>>> {
    let audio_controller = controller.inner();
    let player_name = if n.to_lowercase() == "active" {
        // Get the active player's name
        let active_controller = audio_controller.get_active_controller();
        
        if let Some(active_ctrl) = active_controller {
            if let Ok(ctrl) = active_ctrl.read() {
                ctrl.get_player_name()
            } else {
                return Err(Custom(
                    Status::InternalServerError,
                    Json(CommandResponse {
                        success: false,
                        message: "Failed to access active player".to_string(),
                    })
                ));
            }
        } else {
            return Err(Custom(
                Status::NotFound,
                Json(CommandResponse {
                    success: false,
                    message: "No active player found".to_string(),
                })
            ));
        }
    } else {
        n.to_string()
    };
    
    // Parse the command string into a PlayerCommand
    let parsed_command = match parse_player_command(command, request_data.as_ref()) {
        Ok(cmd) => cmd,
        Err(e) => {
            return Err(Custom(
                Status::BadRequest,
                Json(CommandResponse {
                    success: false,
                    message: format!("Invalid command: {} - {}", command, e),
                })
            ));
        }
    };
    
    // Find the controller with the matching name
    let controllers = audio_controller.list_controllers();
    let mut found_controller = None;
    for ctrl_lock in controllers {
        if let Ok(ctrl) = ctrl_lock.read() {
            if ctrl.get_player_name() == player_name {
                found_controller = Some(ctrl_lock.clone());
                break;
            }
        }
    }
    
    // If no controller with the given name was found, return a 404
    let target_controller = match found_controller {
        Some(ctrl) => ctrl,
        None => {
            return Err(Custom(
                Status::NotFound,
                Json(CommandResponse {
                    success: false,
                    message: format!("No player found with name: {}", player_name),
                })
            ));
        }
    };
    
    // Send the command to the found player
    let success = if let Ok(ctrl) = target_controller.read() {
        ctrl.send_command(parsed_command.clone())
    } else {
        false
    };
    
    if success {
        Ok(Json(CommandResponse {
            success: true,
            message: format!("Command '{}' sent successfully to player with name: {}", command, player_name),
        }))
    } else {
        Err(Custom(
            Status::InternalServerError,
            Json(CommandResponse {
                success: false,
                message: format!("Failed to send command '{}' to player with name: {}", command, player_name),
            })
        ))
    }
}

/// Get the currently playing song information
#[get("/now-playing")]
pub fn get_now_playing(controller: &State<Arc<AudioController>>) -> Json<NowPlayingResponse> {
    // Create a default response in case of errors
    let default_response = NowPlayingResponse {
        player: PlayerInfo {
            name: "none".to_string(),
            id: "none".to_string(),
            state: PlaybackState::Unknown,
            is_active: false,
            has_library: false,
            supports_api_events: false,
            last_seen: None,
            shuffle: false,
            loop_mode: LoopMode::None,
            position: None,
            capabilities: vec![],
        },
        song: None,
        state: PlaybackState::Unknown,
        shuffle: false,
        loop_mode: LoopMode::None,
        position: None,
    };

    // Get the audio controller safely
    let audio_controller = controller.inner();
    
    // Get active controller with a match pattern to avoid extra method calls
    let active_controller = match audio_controller.get_active_controller() {
        Some(ctrl) => ctrl,
        None => return Json(default_response),
    };
    
    // Try to get a read lock without blocking
    let player = match active_controller.try_read() {
        Ok(guard) => guard,
        Err(_) => {
            // If we can't get a lock, return default response
            return Json(default_response);
        }
    };
    
    // Collect all data using safe operations that don't make HTTP requests
    let name = player.get_player_name();
    let id = player.get_player_id();
    
    // Get the state safely (the implementation now uses cached data)
    let state = player.get_playback_state();
    
    // Get song data (should be cached data)
    let song = player.get_song();
    
    // Get remaining data
    let shuffle = player.get_shuffle();
    let loop_mode = player.get_loop_mode();
    let position = player.get_position();
    
    // Format last_seen timestamp if available
    let last_seen = player.get_last_seen()
        .map(|time| {
            chrono::DateTime::<chrono::Utc>::from(time).to_rfc3339()
        });
    
    // Return the response
    Json(NowPlayingResponse {
        player: PlayerInfo {
            name,
            id,
            state,
            is_active: true,
            has_library: player.has_library(),
            supports_api_events: player.supports_api_events(),
            last_seen,
            shuffle,
            loop_mode,
            position,
            capabilities: player.get_capabilities().to_vec(),
        },
        song,
        state,
        shuffle,
        loop_mode,
        position,
    })
}

/// Get the queue from a specific player
/// 
/// If the player name is "active", the currently active player will be used.
/// Otherwise, it will find a player with the specified name.
#[get("/player/<n>/queue")]
pub fn get_player_queue(
    n: &str,
    controller: &State<Arc<AudioController>>
) -> Result<Json<QueueResponse>, Custom<Json<CommandResponse>>> {
    let audio_controller = controller.inner();
    let player_name = if n.to_lowercase() == "active" {
        // Get the active player's name
        let active_controller = audio_controller.get_active_controller();
        
        if let Some(active_ctrl) = active_controller {
            if let Ok(ctrl) = active_ctrl.read() {
                ctrl.get_player_name()
            } else {
                return Err(Custom(
                    Status::InternalServerError,
                    Json(CommandResponse {
                        success: false,
                        message: "Failed to access active player".to_string(),
                    })
                ));
            }
        } else {
            return Err(Custom(
                Status::NotFound,
                Json(CommandResponse {
                    success: false,
                    message: "No active player found".to_string(),
                })
            ));
        }
    } else {
        n.to_string()
    };
    
    // Find the controller with the matching name
    let controllers = audio_controller.list_controllers();
    let mut found_controller = None;
    for ctrl_lock in controllers {
        if let Ok(ctrl) = ctrl_lock.read() {
            if ctrl.get_player_name() == player_name {
                found_controller = Some(ctrl_lock.clone());
                break;
            }
        }
    }
    
    // If no controller with the given name was found, return a 404
    let target_controller = match found_controller {
        Some(ctrl) => ctrl,
        None => {
            return Err(Custom(
                Status::NotFound,
                Json(CommandResponse {
                    success: false,
                    message: format!("No player found with name: {}", player_name),
                })
            ));
        }
    };
    
    // Get the queue from the found player
    let queue = if let Ok(ctrl) = target_controller.read() {
        ctrl.get_queue()
    } else {
        Vec::new()
    };
    
    Ok(Json(QueueResponse {
        player: player_name,
        queue,
    }))
}

/// Get all metadata for a player
/// 
/// If the player name is "active", the currently active player will be used.
/// Otherwise, it will find a player with the specified name.
#[get("/player/<player_name>/meta")]
pub fn get_player_metadata(
    player_name: &str,
    controller: &State<Arc<AudioController>>
) -> Result<Json<MetadataResponse>, Custom<String>> {
    let audio_controller = controller.inner();
    let effective_player_name = if player_name.to_lowercase() == "active" {
        // Get the active player's name
        let active_controller = audio_controller.get_active_controller();
        
        if let Some(active_ctrl) = active_controller {
            if let Ok(ctrl) = active_ctrl.read() {
                ctrl.get_player_name()
            } else {
                return Err(Custom(
                    Status::InternalServerError,
                    "Failed to access active player".to_string(),
                ));
            }
        } else {
            return Err(Custom(
                Status::NotFound,
                "No active player found".to_string(),
            ));
        }
    } else {
        player_name.to_string()
    };
    
    // Find the controller with the matching name
    let controllers = audio_controller.list_controllers();
    
    for ctrl_lock in controllers {
        if let Ok(ctrl) = ctrl_lock.read() {
            if ctrl.get_player_name() == effective_player_name {
                // Get all metadata as a HashMap
                let metadata = ctrl.get_metadata()
                    .unwrap_or_default();
                
                return Ok(Json(MetadataResponse {
                    player_name: effective_player_name,
                    metadata,
                }));
            }
        }
    }
    
    // Player not found
    Err(Custom(
        Status::NotFound,
        format!("Player '{}' not found", effective_player_name),
    ))
}

/// Get a specific metadata key for a player
/// 
/// If the player name is "active", the currently active player will be used.
/// Otherwise, it will find a player with the specified name.
#[get("/player/<player_name>/meta/<key>")]
pub fn get_player_metadata_key(
    player_name: &str,
    key: &str,
    controller: &State<Arc<AudioController>>
) -> Result<Json<MetadataKeyResponse>, Custom<String>> {
    let audio_controller = controller.inner();
    let effective_player_name = if player_name.to_lowercase() == "active" {
        // Get the active player's name
        let active_controller = audio_controller.get_active_controller();
        
        if let Some(active_ctrl) = active_controller {
            if let Ok(ctrl) = active_ctrl.read() {
                ctrl.get_player_name()
            } else {
                return Err(Custom(
                    Status::InternalServerError,
                    "Failed to access active player".to_string(),
                ));
            }
        } else {
            return Err(Custom(
                Status::NotFound,
                "No active player found".to_string(),
            ));
        }
    } else {
        player_name.to_string()
    };
    
    // Find the controller with the matching name
    let controllers = audio_controller.list_controllers();
    
    for ctrl_lock in controllers {
        if let Ok(ctrl) = ctrl_lock.read() {
            if ctrl.get_player_name() == effective_player_name {
                // Get all metadata
                let metadata = ctrl.get_metadata()
                    .unwrap_or_default();
                
                // Get the specific key
                let value = metadata.get(key).cloned();
                
                return Ok(Json(MetadataKeyResponse {
                    player_name: effective_player_name,
                    key: key.to_string(),
                    value,
                }));
            }
        }
    }
    
    // Player not found
    Err(Custom(
        Status::NotFound,
        format!("Player '{}' not found", effective_player_name),
    ))
}

/// API endpoint to push an update to a player
#[post("/player/<player_name>/update", data = "<update>")]
pub fn update_player_state(
    player_name: &str,
    update: Json<PlayerUpdate>,
    controller: &State<Arc<AudioController>>
) -> Result<Json<PlayerUpdateResponse>, Custom<Json<PlayerUpdateResponse>>> {
    let audio_controller = controller.inner();
    let effective_player_name = if player_name.to_lowercase() == "active" {
        let active_controller = audio_controller.get_active_controller();
        if let Some(active_ctrl) = active_controller {
            if let Ok(ctrl) = active_ctrl.read() {
                ctrl.get_player_name()
            } else {
                return Err(Custom(
                    Status::InternalServerError,
                    Json(PlayerUpdateResponse {
                        success: false,
                        message: "Failed to access active player".to_string(),
                    }),
                ));
            }
        } else {
            return Err(Custom(
                Status::NotFound,
                Json(PlayerUpdateResponse {
                    success: false,
                    message: "No active player found".to_string(),
                }),
            ));
        }
    } else {
        player_name.to_string()
    };

    let controllers = audio_controller.list_controllers();
    let mut found_controller = None;
    for ctrl_lock in controllers {
        if let Ok(ctrl) = ctrl_lock.read() {
            // Match by player name or player id (case-insensitive)
            if ctrl.get_player_name().eq_ignore_ascii_case(&effective_player_name)
                || ctrl.get_player_id().eq_ignore_ascii_case(&effective_player_name)
            {
                found_controller = Some(ctrl_lock.clone());
                break;
            }
        }
    }

    let target_controller = match found_controller {
        Some(ctrl) => ctrl,
        None => {
            return Err(Custom(
                Status::NotFound,
                Json(PlayerUpdateResponse {
                    success: false,
                    message: format!("Player '{}' not found", effective_player_name),
                }),
            ));
        }
    };

    let success = if let Ok(ctrl) = target_controller.read() {
        ctrl.receive_update(update.into_inner())
    } else {
        false
    };

    if success {
        Ok(Json(PlayerUpdateResponse {
            success: true,
            message: format!("Update sent successfully to player: {}", effective_player_name),
        }))
    } else {
        Err(Custom(
            Status::InternalServerError,
            Json(PlayerUpdateResponse {
                success: false,
                message: format!("Failed to send update to player: {}", effective_player_name),
            }),
        ))
    }
}

/// Helper function to parse player commands
fn parse_player_command(cmd_str: &str, request_data: Option<&Json<serde_json::Value>>) -> Result<PlayerCommand, String> {
    // Handle simple commands
    match cmd_str.to_lowercase().as_str() {
        "play" => return Ok(PlayerCommand::Play),
        "pause" => return Ok(PlayerCommand::Pause),
        "playpause" => return Ok(PlayerCommand::PlayPause),
        "stop" => return Ok(PlayerCommand::Stop),
        "next" => return Ok(PlayerCommand::Next),
        "previous" => return Ok(PlayerCommand::Previous),
        "kill" => return Ok(PlayerCommand::Kill),
        "clear_queue" => return Ok(PlayerCommand::ClearQueue),
        "add_track" => {
            // Parse URI from request body
            if let Some(data) = request_data {
                if let Ok(add_request) = serde_json::from_value::<AddTrackRequest>(data.0.clone()) {
                    debug!("Adding track to queue: uri={}, metadata={:?}", 
                           add_request.uri, add_request.metadata);
                    
                    // Create metadata if provided
                    let metadata = if let Some(meta) = add_request.metadata {
                        vec![Some(crate::data::player_command::QueueTrackMetadata {
                            metadata: meta,
                        })]
                    } else {
                        vec![None]
                    };
                    
                    return Ok(PlayerCommand::QueueTracks {
                        uris: vec![add_request.uri],
                        insert_at_beginning: false,
                        metadata,
                    });
                } else {
                    return Err("add_track command requires JSON body with 'uri' field".to_string());
                }
            } else {
                return Err("add_track command requires JSON body with 'uri' field".to_string());
            }
        },
        _ => {} // continue to complex command parsing
    }
    
    // Commands with parameters using colon as separator
    if let Some((cmd, param)) = cmd_str.split_once(':') {
        match cmd.to_lowercase().as_str() {
            "set_loop" | "loop" => {
                // Parse loop mode
                match param.to_lowercase().as_str() {
                    "none" => return Ok(PlayerCommand::SetLoopMode(LoopMode::None)),
                    "track" => return Ok(PlayerCommand::SetLoopMode(LoopMode::Track)),
                    "playlist" => return Ok(PlayerCommand::SetLoopMode(LoopMode::Playlist)),
                    _ => {
                        // Try parsing with from_str if available
                        if let Ok(loop_mode) = LoopMode::from_str(param) {
                            return Ok(PlayerCommand::SetLoopMode(loop_mode));
                        }
                        return Err(format!("Invalid loop mode: {}", param));
                    }
                }
            },
            "seek" => {
                // Parse seek position
                match param.parse::<f64>() {
                    Ok(position) => return Ok(PlayerCommand::Seek(position)),
                    Err(_) => return Err(format!("Invalid seek position: {}", param))
                }
            },
            "set_random" | "random" => {
                // Parse random/shuffle setting
                match param.to_lowercase().as_str() {
                    "true" | "on" | "1" | "yes" => return Ok(PlayerCommand::SetRandom(true)),
                    "false" | "off" | "0" | "no" => return Ok(PlayerCommand::SetRandom(false)),
                    _ => return Err(format!("Invalid random setting: {}", param))
                }
            },
            "remove_track" => {
                // Parse position as usize for track removal
                match param.parse::<usize>() {
                    Ok(position) => return Ok(PlayerCommand::RemoveTrack(position)),
                    Err(_) => return Err(format!("Invalid track position: {}", param))
                }
            },
            "play_queue_index" => {
                // Parse index as usize for playing track at specified index in queue
                match param.parse::<usize>() {
                    Ok(index) => return Ok(PlayerCommand::PlayQueueIndex(index)),
                    Err(_) => return Err(format!("Invalid queue index: {}", param))
                }
            },
            _ => {}
        }
    }
    
    // If we get here, we couldn't parse the command
    Err(format!("Unknown command format: {}", cmd_str))
}