use log::{debug, info, warn, error};
use crate::data::artist::Artist;
use crate::helpers::musicbrainz::{search_mbids_for_artist, MusicBrainzSearchResult};
use crate::helpers::theartistdb;
use crate::helpers::fanarttv;
use std::sync::{Arc, RwLock};
use std::collections::HashMap;
use std::thread;

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
/// A tuple containing:
/// * `Vec<String>` - Vector of MusicBrainz IDs if found, empty vector otherwise
/// * `bool` - true if this is a partial match (only some artists in a multi-artist name found)
pub fn lookup_artist_mbids(artist_name: &str) -> (Vec<String>, bool) {
    debug!("Looking up MusicBrainz IDs for artist: {}", artist_name);
    
    // Try to retrieve MusicBrainz ID using search_mbids_for_artist function
    let search_result = search_mbids_for_artist(artist_name, true, false, true);
    
    match search_result {
        MusicBrainzSearchResult::Found(mbids, _) => {
            debug!("Found {} MusicBrainz ID(s) for artist {}: {:?}", 
                  mbids.len(), artist_name, mbids);
            (mbids, false) // Complete match
        },
        MusicBrainzSearchResult::FoundPartial(mbids, _) => {
            warn!("Found {} partial MusicBrainz ID(s) for multi-artist {}: {:?}", 
                  mbids.len(), artist_name, mbids);
            (mbids, true) // Partial match
        },
        MusicBrainzSearchResult::NotFound => {
            info!("No MusicBrainz ID found for artist: {}", artist_name);
            (Vec::new(), false)
        },
        MusicBrainzSearchResult::Error(error) => {
            warn!("Error retrieving MusicBrainz ID for artist {}: {}", artist_name, error);
            (Vec::new(), false)
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
        let (mbids, partial_match) = lookup_artist_mbids(&artist.name);
        let mbid_count = mbids.len();
        
        // Add each MusicBrainz ID to the artist if any were found
        for mbid in mbids {
            artist.add_mbid(mbid);
        }

        // if there is more than one mbid or it was a partial match, it's a multi-artist entry
        if mbid_count > 1 || partial_match {
            artist.is_multi = true; // Mark as multi-artist entry
            artist.clear_metadata(); // Clear metadata for multi-artist entries
            debug!("Cleared metadata for multi-artist entry: {}", artist.name);
        } else {
            debug!("Added MusicBrainz ID(s) to artist {}", artist.name);
        }
        
        // Record if this is a partial match in the artist metadata
        if partial_match {
            warn!("Partial match found for multi-artist name: {}", artist.name);
            if let Some(meta) = &mut artist.metadata {
                meta.is_partial_match = true;
            }
        }
    } else {
        debug!("Artist {} already has MusicBrainz ID(s)", artist.name);
    }
    
    // If the artist has MusicBrainz IDs, always update from both sources
    if artist.metadata.as_ref().map_or(false, |meta| !meta.mbid.is_empty()) {
        // Get the first MusicBrainz ID for the artist
        let mbid_opt = artist.metadata.as_ref().and_then(|meta| meta.mbid.first().cloned());
        
        if mbid_opt.is_some() {
            // Create a TheArtistDbUpdater and use it to update the artist
            debug!("Updating artist {} with TheArtistDB", artist.name);
            let artist_db_updater = theartistdb::TheArtistDbUpdater::new();
            artist = artist_db_updater.update_artist(artist);
            
            // Check if there's only a single MusicBrainz ID
            let mbid_count = artist.metadata.as_ref().map_or(0, |meta| meta.mbid.len());
            
            if mbid_count > 1 {
                debug!("Artist {} has multiple MusicBrainz IDs ({}), skipping FanArt.tv image download", artist.name, mbid_count);
            } else {
                // Create a FanarttvUpdater and use it to update the artist
                debug!("Updating artist {} with FanArt.tv", artist.name);
                let fanarttv_updater = fanarttv::FanarttvUpdater::new();
                artist = fanarttv_updater.update_artist(artist);
            }
        }
    } else {
        // Check if the artist has thumbnail images
        let has_thumbnails = match &artist.metadata {
            Some(meta) => !meta.thumb_url.is_empty(),
            None => false,
        };
        
        if has_thumbnails {
            debug!("Artist {} has thumbnail image(s) but no MusicBrainz ID, skipping updates", artist.name);
        }
    }
    
    // Return the potentially updated artist
    artist
}

/// Start a background thread to update metadata for all artists in the library
///
/// This function updates artist metadata using the update_data_for_artist method in a background thread.
/// It takes an Arc to the artists collection for direct updating and reading.
///
/// # Arguments
/// * `artists_collection` - Arc to the artists collection for updating
pub fn update_library_artists_metadata_in_background(
    artists_collection: Arc<RwLock<HashMap<String, Artist>>>
) {
    debug!("Starting background thread to update artist metadata");
    
    // Spawn a new thread to handle the metadata updates
    thread::spawn(move || {
        info!("Artist metadata update thread started");
        
        // Get all artists from the collection
        let artists = {
            if let Ok(artists_map) = artists_collection.read() {
                // Clone all artists for processing
                artists_map.values().cloned().collect::<Vec<_>>()
            } else {
                warn!("Failed to acquire read lock on artists collection");
                Vec::new()
            }
        };
        
        let total = artists.len();
        info!("Processing metadata for {} artists", total);
        
        let mut count = 0;
        
        // Process each artist one by one
        for artist in artists {
            let artist_name = artist.name.clone();
            debug!("Updating metadata for artist: {}", artist_name);
            
            // Use the existing update_data_for_artist function
            let updated_artist = update_data_for_artist(artist);
            
            // Check if we found new metadata to log appropriately
            let has_new_metadata = {
                let original_metadata = {
                    if let Ok(artists_map) = artists_collection.read() {
                        artists_map.get(&artist_name).and_then(|a| a.metadata.clone())
                    } else {
                        None
                    }
                };
                
                if let Some(new_metadata) = &updated_artist.metadata {
                    if !new_metadata.mbid.is_empty() {
                        match original_metadata {
                            Some(old_meta) if !old_meta.mbid.is_empty() => false,
                            _ => {
                                // Only print when we're actually adding new metadata
                                info!("Adding MusicBrainz ID(s) to artist {}", artist_name);
                                true
                            }
                        }
                    } else {
                        false
                    }
                } else {
                    false
                }
            };
            
            // Update the artist in the collection
            if let Ok(mut artists_map) = artists_collection.write() {
                // Update the artist in the HashMap
                artists_map.insert(artist_name.clone(), updated_artist);
                
                if has_new_metadata {
                    debug!("Successfully updated artist {} in library collection", artist_name);
                }
            } else {
                warn!("Failed to acquire write lock on artists collection for {}", artist_name);
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