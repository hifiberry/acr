/// Player management and functionality for AudioControl3
mod player_controller;
mod base_controller;
mod mpd;

// Re-export the PlayerController trait and related components
pub use player_controller::{PlayerController, PlayerStateListener};
pub use base_controller::BasePlayerController;
pub use mpd::MPDPlayer;

