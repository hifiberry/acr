use crate::AudioController;
use crate::data::{PlaybackState, PlayerCommand, LoopMode, Song};
use crate::players::PlayerController;  // Import the trait
use rocket::serde::json::Json;
use rocket::{get, State, post};
use std::sync::Arc;
use rocket::response::status::Custom;
use rocket::http::Status;

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
    last_seen: Option<String>, // ISO 8601 formatted timestamp of when the player was last seen
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
                    last_seen,
                }
            } else {
                // Fallback for locked controllers
                PlayerInfo {
                    name: "unknown".to_string(),
                    id: "unknown".to_string(),
                    state: PlaybackState::Unknown,
                    is_active: false,
                    last_seen: None,
                }
            }
        })
        .collect();
    
    Json(PlayersListResponse {
        players: players_info,
    })
}

/// Send a command to the active player
#[post("/player/active/send/<command>")]
pub fn send_command_to_active(
    command: &str,
    controller: &State<Arc<AudioController>>
) -> Result<Json<CommandResponse>, Custom<Json<CommandResponse>>> {
    // Parse the command string into a PlayerCommand
    let parsed_command = match parse_player_command(command) {
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
    
    // Send the command to the active player
    let success = controller.inner().send_command(parsed_command.clone());
    
    if success {
        Ok(Json(CommandResponse {
            success: true,
            message: format!("Command '{}' sent successfully to active player", command),
        }))
    } else {
        Err(Custom(
            Status::InternalServerError,
            Json(CommandResponse {
                success: false,
                message: format!("Failed to send command '{}' to active player", command),
            })
        ))
    }
}

/// Send a command to a specific player by ID
#[post("/player/id/<id>/send/<command>")]
pub fn send_command_to_player(
    id: &str,
    command: &str,
    controller: &State<Arc<AudioController>>
) -> Result<Json<CommandResponse>, Custom<Json<CommandResponse>>> {
    // Get all controllers
    let controllers = controller.inner().list_controllers();
    
    // Find the controller with the matching ID
    let mut found_controller = None;
    for ctrl_lock in controllers {
        if let Ok(ctrl) = ctrl_lock.read() {
            if ctrl.get_player_id() == id {
                found_controller = Some(ctrl_lock.clone());
                break;
            }
        }
    }
    
    // If no controller with the given ID was found, return a 404
    let target_controller = match found_controller {
        Some(ctrl) => ctrl,
        None => {
            return Err(Custom(
                Status::NotFound,
                Json(CommandResponse {
                    success: false,
                    message: format!("No player found with ID: {}", id),
                })
            ));
        }
    };
    
    // Parse the command string into a PlayerCommand
    let parsed_command = match parse_player_command(command) {
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
    
    // Send the command to the found player
    let success = if let Ok(ctrl) = target_controller.read() {
        ctrl.send_command(parsed_command.clone())
    } else {
        false
    };
    
    if success {
        Ok(Json(CommandResponse {
            success: true,
            message: format!("Command '{}' sent successfully to player with ID: {}", command, id),
        }))
    } else {
        Err(Custom(
            Status::InternalServerError,
            Json(CommandResponse {
                success: false,
                message: format!("Failed to send command '{}' to player with ID: {}", command, id),
            })
        ))
    }
}

/// Send a command to a specific player by name
#[post("/player/name/<n>/send/<command>")]
pub fn send_command_to_player_by_name(
    n: &str,
    command: &str,
    controller: &State<Arc<AudioController>>
) -> Result<Json<CommandResponse>, Custom<Json<CommandResponse>>> {
    // Get all controllers
    let controllers = controller.inner().list_controllers();
    
    // Find the controller with the matching name
    let mut found_controller = None;
    for ctrl_lock in controllers {
        if let Ok(ctrl) = ctrl_lock.read() {
            if ctrl.get_player_name() == n {
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
                    message: format!("No player found with name: {}", n),
                })
            ));
        }
    };
    
    // Parse the command string into a PlayerCommand
    let parsed_command = match parse_player_command(command) {
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
    
    // Send the command to the found player
    let success = if let Ok(ctrl) = target_controller.read() {
        ctrl.send_command(parsed_command.clone())
    } else {
        false
    };
    
    if success {
        Ok(Json(CommandResponse {
            success: true,
            message: format!("Command '{}' sent successfully to player with name: {}", command, n),
        }))
    } else {
        Err(Custom(
            Status::InternalServerError,
            Json(CommandResponse {
                success: false,
                message: format!("Failed to send command '{}' to player with name: {}", command, n),
            })
        ))
    }
}

/// Get the currently playing song information
#[get("/now-playing")]
pub fn get_now_playing(controller: &State<Arc<AudioController>>) -> Json<NowPlayingResponse> {
    let audio_controller = controller.inner();
    let active_controller = audio_controller.get_active_controller();
    
    if let Some(active_ctrl) = active_controller {
        if let Ok(player) = active_ctrl.read() {
            let name = player.get_player_name();
            let id = player.get_player_id();
            let state = player.get_playback_state();
            let song = player.get_song();
            let shuffle = player.get_shuffle();
            let loop_mode = player.get_loop_mode();
            
            // Format last_seen timestamp if available
            let last_seen = player.get_last_seen()
                .map(|time| {
                    chrono::DateTime::<chrono::Utc>::from(time).to_rfc3339()
                });
            
            return Json(NowPlayingResponse {
                player: PlayerInfo {
                    name,
                    id,
                    state,
                    is_active: true,
                    last_seen,
                },
                song,
                state,
                shuffle,
                loop_mode,
            });
        }
    }
    
    // Return a default response if no active player
    Json(NowPlayingResponse {
        player: PlayerInfo {
            name: "none".to_string(),
            id: "none".to_string(),
            state: PlaybackState::Unknown,
            is_active: false,
            last_seen: None,
        },
        song: None,
        state: PlaybackState::Unknown,
        shuffle: false,
        loop_mode: LoopMode::None,
    })
}

/// Helper function to parse player commands
fn parse_player_command(cmd_str: &str) -> Result<PlayerCommand, String> {
    // Handle simple commands
    match cmd_str.to_lowercase().as_str() {
        "play" => return Ok(PlayerCommand::Play),
        "pause" => return Ok(PlayerCommand::Pause),
        "playpause" => return Ok(PlayerCommand::PlayPause),
        "next" => return Ok(PlayerCommand::Next),
        "previous" => return Ok(PlayerCommand::Previous),
        "kill" => return Ok(PlayerCommand::Kill),
        _ => {} // continue to complex command parsing
    }
    
    // Commands with parameters
    if let Some((cmd, param)) = cmd_str.split_once(':') {
        match cmd.to_lowercase().as_str() {
            "set_loop" | "loop" => {
                // Parse loop mode
                match param.to_lowercase().as_str() {
                    "none" => return Ok(PlayerCommand::SetLoopMode(LoopMode::None)),
                    "track" => return Ok(PlayerCommand::SetLoopMode(LoopMode::Track)),
                    "playlist" => return Ok(PlayerCommand::SetLoopMode(LoopMode::Playlist)),
                    _ => return Err(format!("Invalid loop mode: {}", param))
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
                    "true" | "on" | "1" => return Ok(PlayerCommand::SetRandom(true)),
                    "false" | "off" | "0" => return Ok(PlayerCommand::SetRandom(false)),
                    _ => return Err(format!("Invalid random setting: {}", param))
                }
            },
            _ => {}
        }
    }
    
    // If we get here, we couldn't parse the command
    Err(format!("Unknown command format: {}", cmd_str))
}