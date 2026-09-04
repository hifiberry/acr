//! Data types and pure helpers shared by the AudioControl daemons.
//! Nothing here does I/O; nothing here depends on Rocket, SQLite or a player.

pub mod album;
pub mod album_artists;
pub mod album_key;
pub mod artist;
pub mod artist_split;
pub mod config;
pub mod identifier;
pub mod metadata;
pub mod order_result;
pub mod playback_state;
pub mod player_source;
pub mod sanitize;
pub mod serializable;
pub mod song;
pub mod track;
pub mod url_encoding;
pub mod urlprefix;

pub use album::Album;
pub use album_artists::AlbumArtists;
pub use artist::Artist;
pub use identifier::Identifier;
pub use metadata::ArtistMeta;
pub use order_result::OrderResult;
pub use playback_state::PlaybackState;
pub use player_source::PlayerSource;
pub use song::Song;
pub use track::Track;
