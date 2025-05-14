/// LMS (Lyrion Music Server) client module
pub mod jsonrps;
pub mod lmsserver;
pub mod lmsaudio;
pub mod lmspplayer;
pub mod player_finder;
pub mod cli_listener;
pub mod mapping;
pub mod library;
pub mod libraryloader;

// Re-export main components for easier access
pub use jsonrps::{LmsRpcClient, LmsRpcError, Player, PlayerStatus, Track, Album, Artist, Playlist, SearchResults};
pub use lmsserver::{LmsServer, find_local_servers};
pub use lmsaudio::{LMSAudioController, LMSAudioConfig};
pub use lmspplayer::LMSPlayer;
pub use library::LMSLibrary;