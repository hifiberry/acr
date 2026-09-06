// Module declaration for librespot player implementation
mod librespot;
// The Spotify Web API calls this backend makes directly, using a token from
// crate::audiocontrol::token rather than owning an OAuth client.
pub mod spotify_transport;

// Re-export for easier access from parent module
pub use librespot::LibrespotPlayerController;