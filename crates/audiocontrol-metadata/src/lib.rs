//! External metadata providers, cover art, accounts and the caches that
//! serve them. Knows nothing about players.

pub mod albumupdater;
pub mod artist_store;
pub mod artistsplitter;
pub mod artistupdater;
pub mod coverart;
pub mod coverart_providers;
pub mod external_coverart;
pub mod fanarttv;
pub mod favourites;
pub mod image_meta;
pub mod lastfm;
pub mod lastfm_worker;
pub mod library_enricher;
pub mod musicbrainz;
pub mod now_playing;
pub mod security_store;
pub mod spotify;
pub mod theaudiodb;
pub mod api;
pub mod secrets;

use acr_types::Artist;

/// Trait for services that can update artist metadata.
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
