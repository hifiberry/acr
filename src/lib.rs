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

// Re-export items from data module for backward compatibility
pub use data::{PlaybackState, Song, PlayerState};

// Re-export AudioController for easier access
pub use audiocontrol::AudioController;
