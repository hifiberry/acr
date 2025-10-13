/// Player management and functionality for AudioControl3
mod player_controller;
pub mod mpd;
mod null_controller;
pub mod player_factory;
mod raat;
pub mod librespot;
pub mod lms;
pub mod event_api;
pub mod generic;
pub mod shairport;
pub mod bluetooth;

// MPRIS support is only available on Unix-like systems (Linux, macOS)
#[cfg(not(windows))]
pub mod mpris;

// Re-export the PlayerController trait and related components
pub use player_controller::{PlayerController, BasePlayerController};
pub use mpd::MPDPlayerController;
pub use null_controller::NullPlayerController;
pub use shairport::ShairportController;
pub use bluetooth::BluetoothPlayerController;
pub use player_factory::{create_player_from_json, create_player_from_json_str, PlayerCreationError};
pub use raat::MetadataPipeReader;
// Export the LibrespotPlayerController for use in player_factory
pub use librespot::LibrespotPlayerController;
// Export the GenericPlayerController for use in player_factory
pub use generic::GenericPlayerController;
// Export the MprisPlayerController for use in player_factory (Unix only)
#[cfg(not(windows))]
pub use mpris::MprisPlayerController;
// Export the event API components
pub use event_api::{PlayerEventResponse, player_event_update};

