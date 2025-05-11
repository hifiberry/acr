/// LMS (Lyrion Music Server) client module
pub mod jsonrps;
pub mod lmsserver;
pub mod lmsaudio;

// Re-export main components for easier access
pub use jsonrps::{LmsRpcClient, LmsRpcError, Player, PlayerStatus, Track, Album, Artist, Playlist, SearchResults};
pub use lmsserver::{LmsServer, find_local_servers};
pub use lmsaudio::{LMSAudioController, LMSAudioConfig};