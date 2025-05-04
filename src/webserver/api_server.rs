use std::sync::Arc;
use actix_web::{web, App, HttpResponse, HttpServer, Responder, get};
use log::{info, error};
use serde::{Deserialize, Serialize};

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
}

/// API Server for providing HTTP endpoints
pub struct ApiServer {
    /// Server configuration
    config: ApiConfig,
    /// Server version
    version: String,
}

impl ApiServer {
    /// Create a new API server with default configuration
    pub fn new() -> Self {
        Self {
            config: ApiConfig::default(),
            version: env!("CARGO_PKG_VERSION").to_string(),
        }
    }
    
    /// Create a new API server with custom configuration
    pub fn with_config(config: ApiConfig) -> Self {
        Self {
            config,
            version: env!("CARGO_PKG_VERSION").to_string(),
        }
    }

    /// Start the API server
    pub async fn start(&self) -> std::io::Result<()> {
        if !self.config.enable {
            info!("API server is disabled in configuration");
            return Ok(());
        }

        info!("Starting API server on {}:{}", self.config.host, self.config.port);
        
        // Clone the values we need from self
        let host = self.config.host.clone();
        let port = self.config.port;
        let version = self.version.clone();
        
        // Create shared application state
        let app_state = web::Data::new(AppState {
            version,
        });
        
        // Create HTTP server
        HttpServer::new(move || {
            App::new()
                .app_data(app_state.clone())
                // Register version endpoint
                .service(get_version)
        })
        .bind((host, port))?
        .run()
        .await
    }
}

/// GET /version endpoint handler
#[get("/version")]
async fn get_version(app_state: web::Data<AppState>) -> impl Responder {
    let response = VersionResponse {
        version: app_state.version.clone(),
        build_date: env!("CARGO_PKG_VERSION").to_string(),
    };
    
    HttpResponse::Ok().json(response)
}