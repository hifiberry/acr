/// Metadata handling for AudioControl3
pub mod data;

/// Configuration utilities with backward compatibility support
pub mod config;

/// Player implementation and controllers
pub mod players;

/// Audio controller for managing multiple players
pub mod audiocontrol;

/// Plugin system for event filtering and extensions
pub mod plugins;

/// Helper utilities for I/O and other common tasks
pub mod helpers;

/// API server for REST endpoints
pub mod api;

/// Logging configuration and utilities
pub mod logging;

/// Global constants
pub mod constants;

/// Secrets management
pub mod secrets;

pub use crate::audiocontrol::audiocontrol::AudioController;
pub use crate::data::PlayerCommand;
pub use crate::players::PlayerController;

use tokio::runtime::Runtime;
use once_cell::sync::Lazy;
use log::info;

// Global Tokio runtime for async operations
static TOKIO_RUNTIME: Lazy<Runtime> = Lazy::new(|| {
    let rt = Runtime::new().expect("Failed to create Tokio runtime");
    info!("Global Tokio runtime initialized");
    rt
});

/// Initialize the global Tokio runtime
///
/// This function is called automatically when get_tokio_runtime() is first called,
/// but can be called explicitly to initialize the runtime at a specific point.
pub fn initialize_tokio_runtime() {
    Lazy::force(&TOKIO_RUNTIME);
}

/// Get a reference to the global Tokio runtime
///
/// This function will initialize the runtime if it hasn't been initialized yet.
pub fn get_tokio_runtime() -> &'static Runtime {
    &TOKIO_RUNTIME
}
