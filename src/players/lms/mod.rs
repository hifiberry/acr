/// LMS (Lyrion Music Server) client module
pub mod jsonrps;
pub mod examples;

// Re-export main components for easier access
pub use jsonrps::{LmsRpcClient, LmsRpcError, Player, PlayerStatus, Track, Album, Artist, Playlist, SearchResults};