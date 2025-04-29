// Data structures for AudioControl3

pub mod capabilities;
pub mod loop_mode;
pub mod player;
pub mod player_command;
pub mod player_event;
pub mod player_state;
pub mod serializable;
pub mod song;
pub mod stream_details; // Add the new module

// Re-export types from child modules
pub use capabilities::*;
pub use loop_mode::*;
pub use player::*;
pub use player_command::*;
pub use player_event::*;
pub use player_state::*;
pub use serializable::*;
pub use song::*;
pub use stream_details::*; // Re-export the new module