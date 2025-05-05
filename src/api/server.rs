use crate::AudioController;
use crate::api::{players, plugins};
use log::info;
use rocket::{routes, get};
use rocket::serde::json::Json;
use rocket::config::Config;
use std::sync::Arc;

// Define the version response struct
#[derive(serde::Serialize)]
struct VersionResponse {
    version: String,
}

// API endpoint to get the version
#[get("/version")]
fn get_version() -> Json<VersionResponse> {
    Json(VersionResponse {
        version: env!("CARGO_PKG_VERSION").to_string(),
    })
}

// Start the Rocket server
pub async fn start_rocket_server(controller: Arc<AudioController>, config_json: &serde_json::Value) -> Result<(), rocket::Error> {
    // Default port is 1080
    let api_port = config_json.get("api_port")
        .and_then(|p| p.as_u64())
        .unwrap_or(1080);
    
    info!("Starting API server on port {}", api_port);
    
    let config = Config::figment()
        .merge(("port", api_port))
        .merge(("address", "0.0.0.0"));
    
    let _rocket = rocket::custom(config)
        .mount("/", routes![
            get_version,
            players::get_current_player,
            players::list_players,
            plugins::list_action_plugins,
            plugins::list_event_filters
        ])
        .manage(controller)
        .launch()
        .await?;
    
    Ok(())
}