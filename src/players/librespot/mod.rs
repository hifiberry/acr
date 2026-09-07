// Module declaration for librespot player implementation
mod librespot;
// The Spotify account this daemon owns: the OAuth tokens, their refresh and
// the routes that manage them. It moved here from the metadata crate so that
// playback control needs nothing from the metadata half.
pub mod spotify_account;
// The Spotify Web API requests this daemon makes, using a token from
// spotify_account rather than one fetched across the seam.
pub mod spotify_transport;

// Re-export for easier access from parent module
pub use librespot::LibrespotPlayerController;
