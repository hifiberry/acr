pub mod attributecache;
pub mod imagecache;
pub mod artistupdater;
pub mod fanarttv;
pub mod memory_report;
pub mod stream_helper;
pub mod musicbrainz;
pub mod theaudiodb;
pub mod sanitize;
pub mod macaddress;
pub mod http_client;
pub mod ratelimit;
pub mod lastfm;
pub mod security_store;
pub mod settingsdb;
pub mod spotify;
pub mod retry;
pub mod systemd;
pub mod playback_progress;
pub mod process_helper;
pub mod favourites;
pub mod genre_cleanup;
#[cfg(unix)]
pub mod mpris;
#[cfg(unix)]
pub mod shairportsync_messages;

use crate::data::artist::Artist;

pub use playback_progress::PlayerProgress;

/// Trait for services that can update artist metadata
pub trait ArtistUpdater {
    /// Update an artist with additional metadata from a service
    /// 
    /// # Arguments
    /// * `artist` - The artist to update
    /// 
    /// # Returns
    /// The updated artist with additional metadata
    fn update_artist(&self, artist: Artist) -> Artist;
}