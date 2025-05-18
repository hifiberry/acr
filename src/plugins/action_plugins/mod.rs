pub mod active_monitor;
pub mod event_logger; // Added
pub mod lastfm_plugin; // Added

// Re-export commonly used items
pub use active_monitor::ActiveMonitor;
pub use event_logger::EventLogger; // Added
pub use lastfm_plugin::{LastfmPlugin, LastfmPluginConfig}; // Added