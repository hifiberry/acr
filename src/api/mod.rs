// Import constants for use in API modules
pub use crate::constants::API_PREFIX;

// The forwarded-prefix guard and the image/validation responders moved to
// acr-web, shared with the future metadata daemon. The cover art, Last.fm,
// Spotify, TheAudioDB and favourites routes moved to audiocontrol-metadata;
// the daemon receives them through `start_rocket_server`'s `extra_routes`.
pub use acr_web::{imagecache, imageresponse, urlprefix, validated};

// Export the players module
pub mod players;

// Export the plugins module
pub mod plugins;

// Export the library module
pub mod library;

// Export the event module
pub mod events;

// Export the volume module
pub mod volume;

// Export the inputs module
pub mod inputs;

// Export the lyrics module
pub mod lyrics;

// Export the m3u module
pub mod m3u;

// Export the settings module
pub mod settings;

// Export the cache module
pub mod cache;

// Export the backgroundjobs module
pub mod backgroundjobs;

// Export the genres module
pub mod genres;

// Export the splitters module
pub mod splitters;

// Export the server module
pub mod server;

// Export the capabilities module
pub mod capabilities;
