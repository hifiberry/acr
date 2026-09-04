pub mod attributecache;
pub mod imagecache;
pub mod imageprewarm;
pub mod imagepurge;
pub mod image_meta;
pub mod artistupdater;
pub mod albumupdater;
pub mod artist_store;
pub mod artistsplitter;
pub mod backgroundjobs;
pub mod coverart;
pub mod coverart_providers;
pub mod external_coverart;
pub mod local_coverart;
pub mod fanarttv;
pub mod memory_report;
pub mod stream_helper;
pub mod musicbrainz;
pub mod theaudiodb;
pub mod macaddress;
pub mod lastfm;
pub mod security_store;
pub mod settingsdb;
pub mod spotify;
pub mod systemd;
pub mod playback_progress;
pub mod process_helper;
pub mod favourites;
pub mod genre_cleanup;
pub mod volume;
pub mod global_volume;
pub mod configurator;
pub mod lyrics;
pub mod songtitlesplitter;
pub mod songsplitmanager;
pub mod m3u;
pub mod bluez;
#[cfg(unix)]
pub mod mpris;
#[cfg(unix)]
pub mod shairportsync_messages;

use crate::data::artist::Artist;

pub use acr_http::{http_client, ratelimit, retry};
pub use acr_images::{image_grader, imageresize};
pub use acr_types::{sanitize, url_encoding};
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