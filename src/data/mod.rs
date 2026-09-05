// Data structures for AudioControl3

pub mod capabilities;
pub mod loop_mode;
pub mod player;
pub mod player_command;
pub mod player_event;
pub mod player_update;
pub mod song_update;
pub mod stream_details;
pub mod library;
pub mod system_event;

pub use acr_types::{album, album_artists, artist, metadata, serializable, song, track};
pub use acr_types::{Album, AlbumArtists, Artist, ArtistMeta, Identifier, PlaybackState, PlayerSource, Song, Track};

// Re-export types from child modules
pub use capabilities::*;
pub use loop_mode::*;
pub use player::*;
pub use player_command::*;
pub use player_event::*;
pub use player_update::*;
pub use song_update::*;
pub use stream_details::*;
pub use library::*;
pub use system_event::*;