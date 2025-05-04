// Web API server implementation for AudioControl/Rust
mod api_server;

// Re-export the API server and configuration
pub use api_server::{ApiServer, ApiConfig, AppState};