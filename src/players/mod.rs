/// Player management and functionality for AudioControl3
mod player_controller;
mod mpd;
mod null_controller;
pub mod player_factory;
mod raat;

// Re-export the PlayerController trait and related components
pub use player_controller::{PlayerController, PlayerStateListener, BasePlayerController};
pub use mpd::MPDPlayerController;
pub use null_controller::NullPlayerController;
pub use player_factory::{create_player_from_json, create_player_from_json_str, PlayerCreationError, sample_json_config};
pub use raat::MetadataPipeReader;

