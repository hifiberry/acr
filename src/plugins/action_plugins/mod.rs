pub mod active_monitor;
pub mod event_logger; // Added

// Re-export commonly used items
pub use active_monitor::ActiveMonitor;
pub use event_logger::EventLogger; // Added