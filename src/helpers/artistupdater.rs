use log::{debug, info, warn};
use crate::data::artist::Artist;
use std::sync::{Arc, RwLock};
use std::collections::HashMap;

/// Looks up MusicBrainz IDs for an artist and returns them if found
/// 
/// This function delegates to the artist store for MusicBrainz ID lookup.
/// 
/// # Arguments
/// * `artist_name` - The name of the artist to look up
/// 
/// # Returns
/// A tuple containing:
/// * `Vec<String>` - Vector of MusicBrainz IDs if found, empty vector otherwise
/// * `bool` - true if this is a partial match (only some artists in a multi-artist name found)
pub fn lookup_artist_mbids(artist_name: &str) -> (Vec<String>, bool) {
    crate::helpers::artist_store::lookup_artist_mbids(artist_name)
}

/// Download and cache artist images using the cover art system
/// 
/// This function delegates to the artist store for cover art handling.
/// 
/// # Arguments
/// * `artist` - The artist to update with cover art
/// 
/// # Returns
/// The updated artist with image URLs in metadata
fn update_artist_with_coverart(artist: Artist) -> Artist {
    debug!("Updating artist {} with cover art system", artist.name);
    
    // Use the artist store to handle cover art
    crate::helpers::artist_store::update_artist_with_coverart(artist)
}

/// Updates artist data by fetching additional information like MusicBrainz IDs
/// 
/// This function delegates to the artist store for all metadata processing.
/// 
/// # Arguments
/// * `artist` - The artist to update
/// 
/// # Returns
/// The updated artist
pub fn update_data_for_artist(artist: Artist) -> Artist {
    debug!("Delegating artist data update to artist store for: {}", artist.name);
    
    // Delegate all metadata processing to the artist store
    crate::helpers::artist_store::update_data_for_artist(artist)
}

/// Start a background thread to update metadata for all artists in the library sequentially
///
/// This function delegates to the artist store for background metadata processing.
///
/// # Arguments
/// * `artists_collection` - Arc to the artists collection for updating
pub fn update_library_artists_metadata_in_background(
    artists_collection: Arc<RwLock<HashMap<String, Artist>>>
) {
    debug!("Delegating background artist metadata update to artist store");
    
    // Delegate to the artist store for background processing
    crate::helpers::artist_store::update_library_artists_metadata_in_background(artists_collection);
}