use crate::AudioController;
use crate::data::PlaybackState;
use crate::players::PlayerController;  // Import the trait
use rocket::serde::json::Json;
use rocket::{get, State};
use std::sync::Arc;

/// Response struct for the current active player
#[derive(serde::Serialize)]
pub struct CurrentPlayerResponse {
    name: String,
    id: String,
    state: PlaybackState,
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
            
            return Json(CurrentPlayerResponse {
                name,
                id,
                state,
            });
        }
    }
    
    // Return a default response if no active player
    Json(CurrentPlayerResponse {
        name: "none".to_string(),
        id: "none".to_string(),
        state: PlaybackState::Unknown,
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
                
                PlayerInfo {
                    name: name.clone(),
                    id: id.clone(),
                    state: ctrl.get_playback_state(),
                    is_active: name == current_player_name && id == current_player_id,
                }
            } else {
                // Fallback for locked controllers
                PlayerInfo {
                    name: "unknown".to_string(),
                    id: "unknown".to_string(),
                    state: PlaybackState::Unknown,
                    is_active: false,
                }
            }
        })
        .collect();
    
    Json(PlayersListResponse {
        players: players_info,
    })
}