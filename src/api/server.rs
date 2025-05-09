use crate::AudioController;
use crate::api::{players, plugins, library, imagecache};
use crate::constants::API_PREFIX;
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
    
    let api_routes = routes![
        get_version,
        
        // Player routes
        players::get_current_player,
        players::list_players,
        players::send_command_to_active,
        players::send_command_to_player_by_name,
        players::get_now_playing,
        players::get_player_queue,
        players::get_player_metadata,      
        players::get_player_metadata_key,   
        
        // Plugin routes
        plugins::list_action_plugins,
        plugins::list_event_filters,
        
        // Library routes
        library::list_libraries,
        library::get_library_info,
        library::get_player_albums,
        library::get_player_artists,
        library::get_album_by_id,
        library::get_albums_by_artist,
        library::get_albums_by_artist_id,
        library::refresh_player_library,
        library::get_artist_by_name,
        library::get_artist_by_id,
        library::get_artist_by_mbid,
        library::get_image,
        library::get_library_metadata,
        library::get_library_metadata_key
    ];
    
    // ImageCache routes
    let imagecache_routes = routes![
        imagecache::get_image_from_cache
    ];
    
    let _rocket = rocket::custom(config)
        .mount(API_PREFIX, api_routes) // Use API_PREFIX here when mounting routes
        .mount(format!("{}/imagecache", API_PREFIX), imagecache_routes) // Mount imagecache routes
        .manage(controller)
        .launch()
        .await?;
    
    Ok(())
}