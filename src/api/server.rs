use crate::AudioController;
use crate::api::{players, plugins, library, imagecache, events, lastfm};
use crate::api::events::WebSocketManager;
use crate::constants::API_PREFIX;
use crate::players::{PlayerController, PlayerStateListener}; // Added PlayerStateListener import
use log::{info, warn};
use rocket::{routes, get};
use rocket::serde::json::Json;
use rocket::config::Config;
use rocket::fs::FileServer;
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
    // Check if webserver is enabled (default to true if not specified)
    let webserver_enabled = config_json.get("webserver")
        .and_then(|ws| ws.get("enable"))
        .and_then(|e| e.as_bool())
        .unwrap_or(true);
        
    if !webserver_enabled {
        info!("Webserver is disabled in configuration");
        return Ok(());
    }
    
    // Get webserver config or use defaults
    let host = config_json.get("webserver")
        .and_then(|ws| ws.get("host"))
        .and_then(|h| h.as_str())
        .unwrap_or("0.0.0.0");
        
    let port = config_json.get("webserver")
        .and_then(|ws| ws.get("port"))
        .and_then(|p| p.as_u64())
        .unwrap_or(1080);
    
    info!("Starting webserver on {}:{}", host, port);
    
    let config = Config::figment()
        .merge(("port", port))
        .merge(("address", host));
    
    // Create WebSocket manager and start the background pruning task
    let ws_manager = Arc::new(WebSocketManager::new());
    events::start_prune_task(ws_manager.clone());
    
    // Register the WebSocket manager as a listener for all player events
    info!("Registering WebSocketManager as a player event listener");
    
    // Get a mutable reference to register the WebSocketManager as a listener
    let mut_controller = unsafe { &mut *(Arc::as_ptr(&controller) as *mut AudioController) };
    if mut_controller.register_state_listener(Arc::downgrade(&(ws_manager.clone() as Arc<dyn PlayerStateListener>))) {
        info!("WebSocketManager successfully registered as listener");
    } else {
        warn!("Failed to register WebSocketManager as listener");
    }
    
    let api_routes = routes![
        get_version,
        
        // Player routes
        players::get_current_player,
        players::list_players,
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
        library::update_player_library,
        library::get_artist_by_name,
        library::get_artist_by_id,
        library::get_artist_by_mbid,
        library::get_image,
        library::get_library_metadata,
        library::get_library_metadata_key,
          // WebSocket routes
        events::event_messages,
        events::player_event_messages,
    ];
    
    // Define Last.fm specific routes
    let lastfm_routes = routes![
        lastfm::get_status,
        lastfm::get_auth_url_handler,
        lastfm::prepare_complete_auth,
        lastfm::complete_auth,
        lastfm::disconnect_handler
    ];
    
    // ImageCache routes
    let imagecache_routes = routes![
        imagecache::get_image_from_cache
    ];
    
    let mut rocket_builder = rocket::custom(config)
        .mount(API_PREFIX, api_routes) // Use API_PREFIX here when mounting general api routes
        .mount(format!("{}/lastfm", API_PREFIX), lastfm_routes) // Mount Last.fm routes under /api/lastfm (or similar)
        .mount(format!("{}/imagecache", API_PREFIX), imagecache_routes) // Mount imagecache routes
        .manage(controller)
        .manage(ws_manager); // Add WebSocket manager as managed state
    
    // Check for static file routes in the configuration
    if let Some(static_routes) = config_json.get("webserver")
        .and_then(|ws| ws.get("static_routes"))
        .and_then(|sr| sr.as_array()) {
        for (index, route_config) in static_routes.iter().enumerate() {
            if let (Some(url_path), Some(directory)) = (
                route_config.get("url_path").and_then(|p| p.as_str()),
                route_config.get("directory").and_then(|d| d.as_str())
            ) {
                info!("Mounting static files from '{}' at URL path '{}'", directory, url_path);
                rocket_builder = rocket_builder.mount(url_path, FileServer::from(directory));
            } else {
                warn!("Invalid static file route configuration at index {}: missing url_path or directory", index);
            }
        }
    }
    
    let _rocket = rocket_builder.launch().await?;
    
    Ok(())
}