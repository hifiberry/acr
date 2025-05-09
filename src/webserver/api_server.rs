use std::sync::Arc;
use tokio::sync::Mutex;
use log::{info, error};
use serde::{Deserialize, Serialize};
use crate::AudioController;
use rocket::routes;

/// API Server configuration
#[derive(Clone, Debug)]
pub struct ApiConfig {
    /// Whether the API server is enabled
    pub enable: bool,
    /// Host to listen on
    pub host: String,
    /// Port to listen on
    pub port: u16,
}

impl Default for ApiConfig {
    fn default() -> Self {
        Self {
            enable: true,
            host: "0.0.0.0".to_string(),
            port: 1080,
        }
    }
}

/// Version response returned by the /version endpoint
#[derive(Serialize, Deserialize)]
struct VersionResponse {
    version: String,
    build_date: String,
}

/// Application shared state
#[derive(Clone)]
pub struct AppState {
    /// Server version
    pub version: String,
    /// AudioController reference
    pub audio_controller: Option<Arc<AudioController>>,
}

/// API Server for providing HTTP endpoints
pub struct ApiServer {
    /// Server configuration
    config: ApiConfig,
    /// Server version
    version: String,
    /// Audio controller reference
    audio_controller: Option<Arc<Mutex<AudioController>>>,
}

impl ApiServer {
    /// Create a new API server with default configuration
    pub fn new() -> Self {
        Self {
            config: ApiConfig::default(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            audio_controller: None,
        }
    }
    
    /// Create a new API server with custom configuration
    pub fn with_config(config: ApiConfig) -> Self {
        Self {
            config,
            version: env!("CARGO_PKG_VERSION").to_string(),
            audio_controller: None,
        }
    }
    
    /// Set the AudioController reference
    pub fn set_audio_controller(&mut self, controller: Arc<Mutex<AudioController>>) {
        self.audio_controller = Some(controller);
    }

    /// Start the API server
    pub async fn start(&self) -> std::io::Result<()> {
        if !self.config.enable {
            info!("API server is disabled in configuration");
            return Ok(());
        }

        info!("API server is configured to run on {}:{}, but Actix Web implementation has been removed", 
            self.config.host, self.config.port);
        
        // This is a placeholder - actual server implementation using Rocket is in src/api/server.rs
        Ok(())
    }
}

/// Build a complete Rocket instance with all API routes
fn build_rocket(
    address: String,
    port: u16,
    static_path: Option<String>,
    controller: Arc<AudioController>
) -> rocket::Rocket<rocket::Build> {
    
    let figment = rocket::Config::figment()
        .merge(("address", address))
        .merge(("port", port));
        
    let mut rocket_builder = rocket::custom(figment)
        .mount(
            "/api",
            routes![
                crate::api::players::get_current_player,
                crate::api::players::list_players,
                crate::api::players::send_command_to_active,
                crate::api::players::send_command_to_player_by_name,
                crate::api::players::get_now_playing,
                crate::api::players::get_player_queue,
                crate::api::library::list_artists,
                // ...existing routes...
            ]
        );
}