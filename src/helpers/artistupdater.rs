use log::{debug, info, warn};
use crate::data::artist::Artist;
use crate::helpers::musicbrainz::{search_mbids_for_artist, MusicBrainzSearchResult};
use crate::helpers::coverart::get_coverart_manager;
use crate::helpers::ArtistUpdater;
use std::sync::{Arc, RwLock};
use std::collections::HashMap;
use std::io::Read;

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

/// Download and cache artist images using the cover art system
/// 
/// This function retrieves artist images from all available cover art providers,
/// selects the highest-rated image, downloads it, and stores it in the image cache.
/// It also checks the settings database for custom artist images first.
/// 
/// # Arguments
/// * `artist` - The artist to update with cover art
/// 
/// # Returns
/// The updated artist with image URLs in metadata
fn update_artist_with_coverart(mut artist: Artist) -> Artist {
    debug!("Updating artist {} with cover art system", artist.name);
    
    // First check if there's a custom image URL stored in settings
    let custom_url_key = format!("artist.image.{}", artist.name);
    if let Ok(Some(custom_url)) = crate::helpers::settingsdb::get_string(&custom_url_key) {
        if !custom_url.is_empty() {
            debug!("Found custom image URL for artist {}: {}", artist.name, custom_url);
            
            // Check if the image already exists in cache
            let cache_path = format!("artists/{}/custom.jpg", crate::helpers::sanitize::filename_from_string(&artist.name));
            if let Ok(_) = std::fs::metadata(&cache_path) {
                debug!("Custom image already cached for artist {}", artist.name);
                
                // Add the custom image to the artist metadata
                if artist.metadata.is_none() {
                    artist.metadata = Some(crate::data::ArtistMeta::new());
                }
                if let Some(ref mut metadata) = artist.metadata {
                    metadata.thumb_url = vec![format!("cache://{}", cache_path)];
                }
                return artist;
            }
            
            // Download and cache the custom image
            if let Ok(image_data) = download_image(&custom_url) {
                if let Err(e) = crate::helpers::imagecache::store_image(&cache_path, &image_data) {
                    warn!("Failed to store custom image for artist {}: {}", artist.name, e);
                } else {
                    info!("Downloaded and cached custom image for artist {}", artist.name);
                    
                    // Add the cached image to the artist metadata
                    if artist.metadata.is_none() {
                        artist.metadata = Some(crate::data::ArtistMeta::new());
                    }
                    if let Some(ref mut metadata) = artist.metadata {
                        metadata.thumb_url = vec![format!("cache://{}", cache_path)];
                    }
                    return artist;
                }
            } else {
                warn!("Failed to download custom image for artist {} from URL: {}", artist.name, custom_url);
            }
        }
    }
    
    // Get cover art from all providers using the cover art system
    let manager = get_coverart_manager();
    let results = if let Ok(manager_guard) = manager.lock() {
        manager_guard.get_artist_coverart(&artist.name)
    } else {
        warn!("Failed to acquire lock on cover art manager");
        Vec::new()
    };
    
    if results.is_empty() {
        debug!("No cover art found for artist {}", artist.name);
        return artist;
    }
    
    // Find the highest-rated image across all providers
    let mut best_image: Option<&crate::helpers::coverart::ImageInfo> = None;
    let mut best_grade = -1;
    
    for result in &results {
        for image in &result.images {
            let grade = image.grade.unwrap_or(0);
            if grade > best_grade {
                best_grade = grade;
                best_image = Some(image);
            }
        }
    }
    
    if let Some(best_image) = best_image {
        debug!("Found best image for artist {} with grade {}: {}", artist.name, best_grade, best_image.url);
        
        // Download and cache the best image
        if let Ok(image_data) = download_image(&best_image.url) {
            let cache_path = format!("artists/{}/cover.jpg", crate::helpers::sanitize::filename_from_string(&artist.name));
            
            if let Err(e) = crate::helpers::imagecache::store_image(&cache_path, &image_data) {
                warn!("Failed to store image for artist {}: {}", artist.name, e);
            } else {
                info!("Downloaded and cached cover art for artist {} (grade: {})", artist.name, best_grade);
                
                // Update artist metadata with the cached image
                if artist.metadata.is_none() {
                    artist.metadata = Some(crate::data::ArtistMeta::new());
                }
                if let Some(ref mut metadata) = artist.metadata {
                    metadata.thumb_url = vec![format!("cache://{}", cache_path)];
                }
            }
        } else {
            warn!("Failed to download image for artist {} from URL: {}", artist.name, best_image.url);
        }
    } else {
        debug!("No images with valid grades found for artist {}", artist.name);
    }
    
    artist
}

/// Download an image from a URL
/// 
/// # Arguments
/// * `url` - The URL to download the image from
/// 
/// # Returns
/// * `Result<Vec<u8>, String>` - The image data or an error message
fn download_image(url: &str) -> Result<Vec<u8>, String> {
    debug!("Downloading image from URL: {}", url);
    
    // Use ureq to download the image
    match ureq::get(url).call() {
        Ok(response) => {
            let mut bytes = Vec::new();
            if let Err(e) = response.into_reader().read_to_end(&mut bytes) {
                return Err(format!("Failed to read image data: {}", e));
            }
            
            if bytes.is_empty() {
                return Err("Downloaded image is empty".to_string());
            }
            
            debug!("Successfully downloaded image: {} bytes", bytes.len());
            Ok(bytes)
        },
        Err(e) => {
            Err(format!("HTTP request failed: {}", e))
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
    
    // If the artist has MusicBrainz IDs, update from the coverart system
    if artist.metadata.as_ref().map_or(false, |meta| !meta.mbid.is_empty()) {
        debug!("Artist {} has MusicBrainz ID(s), updating with cover art system", artist.name);
        artist = update_artist_with_coverart(artist);
    } else {
        // For artists without MusicBrainz IDs, still try coverart system with artist name only
        debug!("Artist {} has no MusicBrainz ID, trying cover art by name only", artist.name);
        artist = update_artist_with_coverart(artist);
    }
    
    // Note: LastFM metadata is now handled by the unified coverart system
    // No need for separate LastFM calls as the coverart system includes LastFM provider
    
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