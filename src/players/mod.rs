/// Player management and functionality for AudioControl3
mod player_controller;
mod base_controller;
mod mpd;
mod null_controller;

// Re-export the PlayerController trait and related components
pub use player_controller::{PlayerController, PlayerStateListener};
pub use player_controller::{create_player_from_json, create_player_from_json_str, PlayerCreationError};
pub use base_controller::BasePlayerController;
pub use mpd::MPDPlayer;
pub use null_controller::NullPlayerController;

