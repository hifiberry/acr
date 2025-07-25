use log::{debug, info, warn};
use crate::data::artist::Artist;
use crate::helpers::musicbrainz::{search_mbids_for_artist, MusicBrainzSearchResult};
use crate::helpers::fanarttv;
use crate::helpers::theaudiodb;
use crate::helpers::ArtistUpdater;
use std::sync::{Arc, RwLock};
use std::collections::HashMap;

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
    // This is now a fully synchronous call since we replaced musicbrainz_rs with direct HTTP
    let search_result = search_mbids_for_artist(artist_name, true, false, true);
    
    match search_result {
        MusicBrainzSearchResult::Found(mbids, _) => {
            debug!("Found {} MusicBrainz ID(s) for artist {}: {:?}", 
                  mbids.len(), artist_name, mbids);
            (mbids, false) // Complete match
        },
        MusicBrainzSearchResult::FoundPartial(mbids, _) => {
            info!("Found {} partial MusicBrainz ID(s) for multi-artist {}: {:?}", 
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
        
        // Use the synchronous function to look up MusicBrainz IDs directly
        // No more need for Tokio runtime since our function is now synchronous
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
        } else if mbid_count > 0 {
            info!("Updated artist '{}' with MusicBrainz data: {} ID(s)", artist.name, mbid_count);
            debug!("Added MusicBrainz ID(s) to artist {}", artist.name);
        }
        
        // Record if this is a partial match in the artist metadata
        if partial_match {
            debug!("Partial match found for multi-artist name: {}", artist.name);
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
            // Create a TheAudioDbUpdater and use it to update the artist
            let theaudiodb_updater = theaudiodb::TheAudioDbUpdater::new();
            artist = theaudiodb_updater.update_artist(artist);
            
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
    }
    
    // Always try to update with Last.fm (doesn't require MusicBrainz ID)
    debug!("Updating artist {} with Last.fm", artist.name);
    let lastfm_updater = crate::helpers::lastfm::LastfmUpdater::new();
    artist = lastfm_updater.update_artist(artist);
    
    // Handle artists without MusicBrainz IDs but with existing thumbnails
    if artist.metadata.as_ref().map_or(false, |meta| meta.mbid.is_empty()) {
        // Check if the artist has thumbnail images
        let has_thumbnails = match &artist.metadata {
            Some(meta) => !meta.thumb_url.is_empty(),
            None => false,
        };
        
        if has_thumbnails {
            debug!("Artist {} has thumbnail image(s) but no MusicBrainz ID, skipping updates", artist.name);
        }
    }

    // Store the updated metadata in cache
    if let Some(metadata) = &artist.metadata {
        // Create a cache key using the artist's name
        let cache_key = format!("artist::metadata::{}", artist.name);
        
        // Store the metadata in the attribute cache
        match crate::helpers::attributecache::set(&cache_key, metadata) {
            Ok(_) => debug!("Stored metadata for artist {} in attribute cache", artist.name),
            Err(e) => warn!("Failed to store metadata for artist {} in attribute cache: {}", artist.name, e),
        }
        
        // If the artist has MusicBrainz IDs, store them separately for faster lookup
        if !metadata.mbid.is_empty() {
            let mbid_key = format!("artist::mbid::{}", artist.name);
            if let Err(e) = crate::helpers::attributecache::set(&mbid_key, &metadata.mbid) {
                warn!("Failed to store MusicBrainz IDs for artist {} in attribute cache: {}", artist.name, e);
            }
        }
    }
    
    // Return the potentially updated artist
    artist
}

/// Start a background thread to update metadata for all artists in the library sequentially
///
/// This function updates artist metadata using the update_data_for_artist method in a background process.
/// It takes an Arc to the artists collection for direct updating and reading.
///
/// # Arguments
/// * `artists_collection` - Arc to the artists collection for updating
pub fn update_library_artists_metadata_in_background(
    artists_collection: Arc<RwLock<HashMap<String, Artist>>>
) {
    debug!("Starting background thread to update artist metadata");    // Spawn a new thread to handle the metadata updates
    use std::thread;
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

        for (index, artist) in artists.into_iter().enumerate() {
            let artist_name = artist.name.clone();
            debug!("Updating metadata for artist: {}", artist_name);

            // Use the synchronous version of update_data_for_artist
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
                artists_map.insert(artist_name.clone(), updated_artist);

                if has_new_metadata {
                    debug!("Successfully updated artist {} in library collection", artist_name);
                }
            } else {
                warn!("Failed to acquire write lock on artists collection for {}", artist_name);
            }

            // Log progress periodically
            let count = index + 1;
            if count % 10 == 0 || count == total {
                info!("Processed {}/{} artists for metadata", count, total);
            }            // Sleep between updates to avoid overwhelming external services
        }

        info!("Artist metadata update process completed");
    });

    info!("Background artist metadata update initiated");
}