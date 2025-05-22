/// Metadata handling for AudioControl3
pub mod data;

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

/// Global constants
pub mod constants;

/// Secrets management
pub mod secrets;

pub use crate::audiocontrol::audiocontrol::AudioController;
pub use crate::data::PlayerCommand;
pub use crate::players::PlayerController;

use std::sync::Once;
use tokio::runtime::Runtime;
use std::sync::Mutex;
use log::info;

// Global Tokio runtime for async operations
static TOKIO_RUNTIME: Mutex<Option<Runtime>> = Mutex::new(None);
static INIT: Once = Once::new();

/// Initialize the global Tokio runtime
/// 
/// This function is called automatically when get_tokio_runtime() is first called,
/// but can be called explicitly to initialize the runtime at a specific point.
pub fn initialize_tokio_runtime() {
    INIT.call_once(|| {
        match Runtime::new() {
            Ok(rt) => {
                if let Ok(mut runtime) = TOKIO_RUNTIME.lock() {
                    *runtime = Some(rt);
                    info!("Global Tokio runtime initialized");
                }
            },
            Err(e) => {
                panic!("Failed to create Tokio runtime: {}", e);
            }
        }
    });
}

/// Get a reference to the global Tokio runtime
/// 
/// This function will initialize the runtime if it hasn't been initialized yet.
/// 
/// # Panics
/// 
/// Panics if the runtime cannot be initialized or accessed.
pub fn get_tokio_runtime() -> &'static Runtime {
    initialize_tokio_runtime();
    
    unsafe {
        // This is safe because:
        // 1. We've initialized the runtime with initialize_tokio_runtime()
        // 2. The runtime is never dropped once initialized
        // 3. We only return a reference to the static runtime
        match &*TOKIO_RUNTIME.lock().unwrap() {
            Some(rt) => &*(rt as *const Runtime),
            None => panic!("Tokio runtime not initialized"),
        }
    }
}
