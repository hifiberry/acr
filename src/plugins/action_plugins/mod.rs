pub mod active_monitor;
pub mod event_logger;
pub mod lastfm; // Renamed from lastfm_plugin

// Re-export commonly used items
pub use active_monitor::ActiveMonitor;
pub use event_logger::EventLogger;
pub use lastfm::{Lastfm, LastfmConfig}; // Renamed from lastfm_plugin and updated structs