use log::{debug, info, warn, error};
use crate::data::artist::Artist;
use crate::helpers::musicbrainz::{search_mbids_for_artist, MusicBrainzSearchResult};
use crate::helpers::theartistdb;
use crate::helpers::fanarttv;
use std::sync::{Arc, RwLock};
use std::collections::HashMap;
use tokio::runtime::Runtime;
use tokio::task;
use futures::future::join_all;
use std::sync::atomic::{AtomicUsize, Ordering};

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
pub async fn lookup_artist_mbids(artist_name: &str) -> (Vec<String>, bool) {
    debug!("Looking up MusicBrainz IDs for artist: {}", artist_name);
    
    // Try to retrieve MusicBrainz ID using search_mbids_for_artist function
    // Note: If musicbrainz function needs async, convert it separately
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
        // Create a runtime for this blocking call
        let rt = Runtime::new().expect("Failed to create Tokio runtime");
        let (mbids, partial_match) = rt.block_on(lookup_artist_mbids(&artist.name));
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

/// Start a background thread to update metadata for all artists in the library using async tasks
///
/// This function updates artist metadata using the update_data_for_artist method in a background process.
/// It takes an Arc to the artists collection for direct updating and reading.
///
/// # Arguments
/// * `artists_collection` - Arc to the artists collection for updating
pub fn update_library_artists_metadata_in_background(
    artists_collection: Arc<RwLock<HashMap<String, Artist>>>
) {
    debug!("Starting background thread to update artist metadata");
    
    // Spawn a new thread to handle the metadata updates
    std::thread::spawn(move || {
        info!("Artist metadata update thread started");
        
        // Create a tokio runtime for async operations
        let runtime = match Runtime::new() {
            Ok(rt) => rt,
            Err(e) => {
                error!("Failed to create Tokio runtime: {}", e);
                return;
            }
        };
        
        // Run the async update process in the runtime
        runtime.block_on(async {
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
            
            // Counter for processed artists
            let processed_count = Arc::new(AtomicUsize::new(0));
            
            // Process artists concurrently but with controlled parallelism
            // Process in batches to avoid overwhelming external APIs
            let batch_size = 5;
            
            // Clone the artists into a vector of batches to avoid lifetime issues
            let batched_artists: Vec<Vec<Artist>> = artists
                .chunks(batch_size)
                .map(|chunk| chunk.to_vec())
                .collect();
                
            for batch in batched_artists {
                let mut tasks = Vec::new();
                
                for artist in batch {
                    let artist_name = artist.name.clone();
                    let artists_collection_clone = Arc::clone(&artists_collection);
                    let processed_count_clone = Arc::clone(&processed_count);
                    
                    // Spawn a task for each artist in the batch
                    let task = task::spawn(async move {
                        debug!("Updating metadata for artist: {}", artist_name);
                          // Use the synchronous version of update_data_for_artist
                        let updated_artist = update_data_for_artist(artist);
                        
                        // Check if we found new metadata to log appropriately
                        let has_new_metadata = {
                            let original_metadata = {
                                if let Ok(artists_map) = artists_collection_clone.read() {
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
                        if let Ok(mut artists_map) = artists_collection_clone.write() {
                            // Update the artist in the HashMap
                            artists_map.insert(artist_name.clone(), updated_artist);
                            
                            if has_new_metadata {
                                debug!("Successfully updated artist {} in library collection", artist_name);
                            }
                        } else {
                            warn!("Failed to acquire write lock on artists collection for {}", artist_name);
                        }
                        
                        // Increment processed count and log progress periodically
                        let count = processed_count_clone.fetch_add(1, Ordering::SeqCst) + 1;
                        if count % 10 == 0 || count == total {
                            info!("Processed {}/{} artists for metadata", count, total);
                        }
                        
                        // Return the artist name for potential logging
                        artist_name
                    });
                    
                    tasks.push(task);
                }
                
                // Wait for all artists in this batch to be processed before starting the next batch
                let _results = join_all(tasks).await;
                
                // Add a small delay between batches to avoid overwhelming external services
                tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
            }
            
            info!("Artist metadata update process completed");
        });
    });
    
    info!("Background artist metadata update initiated");
}