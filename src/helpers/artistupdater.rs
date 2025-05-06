use log::{debug, info, warn};
use crate::data::artist::Artist;
use crate::helpers::musicbrainz::{search_mbids_for_artist, MusicBrainzSearchResult};
use std::sync::Arc;
use std::thread;

/// Create a "clean" artist name without unicode characters (converted to ascii), 
/// special characters or double spaces
/// convert to lowercase and trim whitespace
pub fn artist_basename(artist_name: &str) -> String {
    // Convert to ASCII (remove diacritics and other non-ascii characters)
    let ascii_name = deunicode::deunicode(artist_name);
    
    // Keep only alphanumeric characters and spaces, replace others with spaces
    let mut clean_name = String::with_capacity(ascii_name.len());
    for c in ascii_name.chars() {
        if c.is_alphanumeric() || c == ' ' {
            clean_name.push(c);
        } else {
            clean_name.push(' ');
        }
    }
    
    // Convert to lowercase
    let lowercase_name = clean_name.to_lowercase();
    
    // Remove double spaces
    let mut result = String::with_capacity(lowercase_name.len());
    let mut last_was_space = false;
    
    for c in lowercase_name.chars() {
        if c == ' ' {
            if !last_was_space {
                result.push(c);
            }
            last_was_space = true;
        } else {
            result.push(c);
            last_was_space = false;
        }
    }
    
    // Trim whitespace
    result.trim().to_string()
}

/// Looks up MusicBrainz IDs for an artist and returns them if found
/// 
/// This function searches for MusicBrainz IDs associated with the given artist name.
/// 
/// # Arguments
/// * `artist_name` - The name of the artist to look up
/// 
/// # Returns
/// A vector of MusicBrainz IDs if found, empty vector otherwise
pub fn lookup_artist_mbids(artist_name: &str) -> Vec<String> {
    debug!("Looking up MusicBrainz IDs for artist: {}", artist_name);
    
    // Try to retrieve MusicBrainz ID using search_mbids_for_artist function
    let search_result = search_mbids_for_artist(artist_name, true, false);
    
    match search_result {
        MusicBrainzSearchResult::Found(mbids) | MusicBrainzSearchResult::FoundCached(mbids) => {
            info!("Found {} MusicBrainz ID(s) for artist {}: {:?}", 
                  mbids.len(), artist_name, mbids);
            mbids
        },
        MusicBrainzSearchResult::NotFound => {
            warn!("No MusicBrainz ID found for artist: {}", artist_name);
            Vec::new()
        },
        MusicBrainzSearchResult::Error(error) => {
            warn!("Error retrieving MusicBrainz ID for artist {}: {}", artist_name, error);
            Vec::new()
        }
    }
}

/// Updates artist data by fetching additional information like MusicBrainz IDs
/// 
/// This function takes an artist and attempts to retrieve and set any missing data
/// such as MusicBrainz IDs.
/// 
/// # Arguments
/// * `artist` - The artist to update
/// 
/// # Returns
/// The updated artist
pub fn update_data_for_artist(mut artist: Artist) -> Artist {
    debug!("Updating data for artist: {}", artist.name);
    
    // Check if the artist already has MusicBrainz IDs set
    let has_mbid = match &artist.metadata {
        Some(meta) => !meta.mbid.is_empty(),
        None => false,
    };
    
    if !has_mbid {
        debug!("No MusicBrainz ID set for artist {}, attempting to retrieve it", artist.name);
        
        // Use the extracted function to look up MusicBrainz IDs
        let mbids = lookup_artist_mbids(&artist.name);
        
        // Add each MusicBrainz ID to the artist if any were found
        for mbid in mbids {
            artist.add_mbid(mbid);
        }
    } else {
        debug!("Artist {} already has MusicBrainz ID(s)", artist.name);
    }
    
    // Return the potentially updated artist
    artist
}

/// Start a background thread to update metadata for all artists in the library
///
/// This function takes a library that implements the LibraryInterface trait, gets all artists,
/// and updates their metadata using the update_data_for_artist method in a background thread.
///
/// # Arguments
/// * `library` - An Arc-wrapped library instance implementing the LibraryInterface trait
pub fn update_library_artists_metadata_in_background<T>(library: Arc<T>) 
where
    T: crate::data::LibraryInterface + Send + Sync + 'static,
{
    debug!("Starting background thread to update artist metadata");
    
    // Spawn a new thread to handle the metadata updates
    thread::spawn(move || {
        info!("Artist metadata update thread started");
        
        // Get all artists from the library
        let artists = library.get_artists();
        let total = artists.len();
        info!("Processing metadata for {} artists", total);
        
        let mut count = 0;
        
        // Process each artist one by one
        for artist in artists {
            debug!("Updating metadata for artist: {}", artist.name);
            
            // Use the existing update_data_for_artist function
            let updated_artist = update_data_for_artist(artist);
            
            // Save the updated artist back to the library
            if let Some(metadata) = &updated_artist.metadata {
                if !metadata.mbid.is_empty() {
                    // Only update if we actually got new MusicBrainz IDs
                    if let Some(stored_artist) = library.get_artist(&updated_artist.name) {
                        if stored_artist.metadata.is_none() || 
                           stored_artist.metadata.as_ref().map_or(true, |m| m.mbid.is_empty()) {
                            // Only print when we're actually adding new metadata
                            info!("Adding MusicBrainz ID(s) to artist {}", updated_artist.name);
                        }
                    }
                }
            }
            
            count += 1;
            if count % 10 == 0 || count == total {
                info!("Processed {}/{} artists for metadata", count, total);
            }
        }
        
        info!("Artist metadata update thread completed");
    });
    
    info!("Background artist metadata update initiated");
}