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

pub use crate::audiocontrol::audiocontrol::AudioController;
pub use crate::data::PlayerCommand;
pub use crate::players::PlayerController;
pub use crate::players::PlayerStateListener;
