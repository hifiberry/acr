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

use crate::data::artist::Artist;

/// Trait for services that can update artist metadata
pub trait ArtistUpdater {
    /// Update an artist with additional metadata from a service
    /// 
    /// # Arguments
    /// * `artist` - The artist to update
    /// * `mbid` - The MusicBrainz ID to use for looking up artist information
    /// 
    /// # Returns
    /// The updated artist with additional metadata
    fn update_artist(&self, artist: Artist, mbid: &str) -> Artist;
}