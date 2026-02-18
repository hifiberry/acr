use std::sync::Arc;
use log::{debug, warn};
use serde_json::Value;
use rocket::serde::json::Json;
use rocket::{post, State};
use rocket::response::status::Custom;
use rocket::http::Status;

use crate::AudioController;

/// Generic response structure for player event API endpoints
#[derive(serde::Serialize)]
pub struct PlayerEventResponse {
    pub success: bool,
    pub message: String,
}

/// Generic API endpoint to receive player events via API
#[post("/player/<player_name>/update", data = "<event_data>")]
pub fn player_event_update(
    player_name: String, 
    event_data: Json<Value>,
    controller: &State<Arc<AudioController>>
) -> Result<Json<PlayerEventResponse>, Custom<Json<PlayerEventResponse>>> {
    debug!("Received event via API for player: {}", player_name);
    
    // Find the player by name
    if let Some(player_controller_arc) = controller.get_player_by_name(&player_name) {
        // Get a read lock on the player controller
        let player_controller = player_controller_arc.read();
        // Check if the player supports API events
        if !player_controller.supports_api_events() {
            warn!("Player '{}' does not support API event processing", player_name);
            return Err(Custom(
                Status::BadRequest,
                Json(PlayerEventResponse {
                    success: false,
                    message: format!("Player '{}' does not support API event processing", player_name),
                })
            ));
        }

        // Process the event
        match player_controller.process_api_event(&event_data) {
            true => {
                debug!("Successfully processed API event for player: {}", player_name);
                Ok(Json(PlayerEventResponse {
                    success: true,
                    message: "Event processed successfully".to_string(),
                }))
            }
            false => {
                warn!("Failed to process API event for player: {}", player_name);
                Err(Custom(
                    Status::BadRequest,
                    Json(PlayerEventResponse {
                        success: false,
                        message: "Failed to process event or processor disabled".to_string(),
                    })
                ))
            }
        }
    } else {
        warn!("Player '{}' not found", player_name);
        Err(Custom(
            Status::NotFound,
            Json(PlayerEventResponse {
                success: false,
                message: format!("Player '{}' not found", player_name),
            })
        ))
    }
}