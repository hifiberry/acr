//! Data types and pure helpers shared by the AudioControl daemons.
//! Nothing here does I/O; nothing here depends on Rocket, SQLite or a player.

pub mod album;
pub mod album_artists;
pub mod album_key;
pub mod artist;
pub mod artist_split;
pub mod config;
pub mod enrichment;
pub mod identifier;
pub mod library_version;
pub mod metadata;
pub mod now_playing;
pub mod order_result;
pub mod playback_state;
pub mod player_source;
pub mod resolver;
pub mod sanitize;
pub mod serializable;
pub mod song;
pub mod token;
pub mod track;
pub mod url_encoding;
pub mod urlprefix;

/// The internal mount point every API route is served under.
///
/// One definition for the whole workspace: the root package re-exports it as
/// `crate::constants::API_PREFIX`, and `urlprefix` and `acr_store::imagecache`
/// use it directly rather than keeping copies that could drift.
pub const API_PREFIX: &str = "/api";

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
