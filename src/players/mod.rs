/// Player management and functionality for AudioControl3
mod player_controller;
mod mpd;
mod null_controller;
pub mod player_factory;
mod raat;
pub mod librespot;
pub mod lms;
pub mod event_api;

// Re-export the PlayerController trait and related components
pub use player_controller::{PlayerController, BasePlayerController};
pub use mpd::MPDPlayerController;
pub use null_controller::NullPlayerController;
pub use player_factory::{create_player_from_json, create_player_from_json_str, PlayerCreationError};
pub use raat::MetadataPipeReader;
// Export the LibrespotPlayerController for use in player_factory
pub use librespot::LibrespotPlayerController;
// Export the event API components
pub use event_api::{PlayerEventResponse, player_event_update};

