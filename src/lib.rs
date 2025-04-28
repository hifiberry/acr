/// Metadata handling for AudioControl3
pub mod data;

/// Player implementation and controllers
pub mod players;

/// Audio controller for managing multiple players
pub mod audiocontrol;

// Re-export items from data module for backward compatibility
pub use data::{PlayerState, Song, Player};

// Re-export AudioController for easier access
pub use audiocontrol::AudioController;
