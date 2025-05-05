use crate::data::artist::Artist;
use crate::data::metadata::ArtistMeta;
use crate::data::library::LibraryInterface;
use crate::helpers::attributecache;
use crate::helpers::imagecache;
use crate::helpers::fanarttv;
use log::{info, warn, error, debug};
use reqwest::blocking::Client;
use serde_json::Value;
use std::time::Duration;
use std::sync::{Arc, RwLock, Mutex};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::collections::HashMap;
use std::thread;
use std::path::Path;
use deunicode::deunicode;

/// Convert an artist name to a safe basename for files
///
/// This function:
/// - Replaces non-ASCII characters with their ASCII equivalents
/// - Removes ALL special characters (keeping only letters, numbers, and spaces)
/// - Replaces sequences of spaces with a single space
/// - Converts the entire string to lowercase
///
/// # Arguments
/// * `artist_name` - The artist name to convert
///
/// # Returns
/// A sanitized string suitable for use as a file name
pub fn artist_basename(artist_name: &str) -> String {
    // Step 1: Replace non-ASCII characters with ASCII equivalents
    let ascii_name = deunicode(artist_name);
    
    // Step 2: Process each character: keep alphanumeric and spaces, replace others with space
    let mut processed = String::with_capacity(ascii_name.len());
    let mut last_was_space = true; // Start true to handle leading spaces
    
    for c in ascii_name.chars() {
        if c.is_alphanumeric() {
            processed.push(c);
            last_was_space = false;
        } else if c.is_whitespace() {
            // Collapse multiple spaces into one
            if !last_was_space {
                processed.push(' ');
                last_was_space = true;
            }
        } else {
            // Replace special character with space
            if !last_was_space {
                processed.push(' ');
                last_was_space = true;
            }
        }
    }
    
    // Remove trailing space if it exists
    if processed.ends_with(' ') {
        processed.pop();
    }
    
    // Step 3: Convert to lowercase
    processed.to_lowercase()
}

/// Normalize an artist name for comparison by removing all special characters
/// and common words like "the", "and", etc.
///
/// This function:
/// - Converts to ASCII (removing accents, etc.)
/// - Removes ALL special characters (keeping only letters, numbers, and spaces)
/// - Converts to lowercase
/// - Removes common words like "the", "and" (only complete words, not substrings)
/// - Removes ALL spaces in the final result
/// - Trims whitespace and collapses multiple spaces to single space
///
/// # Arguments
/// * `artist_name` - The artist name to normalize
///
/// # Returns
/// A normalized string suitable for comparison
fn normalize_artist_name_for_comparison(artist_name: &str) -> String {
    // Step 1: Convert to ASCII
    let ascii_name = deunicode(artist_name);
    
    // Step 2: Remove all special characters and convert to lowercase
    let mut normalized = String::new();
    for c in ascii_name.chars() {
        if c.is_alphanumeric() || c.is_whitespace() {
            normalized.push(c.to_ascii_lowercase());
        }
    }
    
    // Step 3: Collapse multiple spaces to single space and trim
    let mut result = String::new();
    let mut last_was_space = true; // Start with true to trim leading spaces
    
    for c in normalized.chars() {
        if c.is_whitespace() {
            if !last_was_space {
                result.push(' ');
                last_was_space = true;
            }
        } else {
            result.push(c);
            last_was_space = false;
        }
    }
    
    // Remove trailing space if it exists
    if result.ends_with(' ') {
        result.pop();
    }
    
    // Step 4: Remove common words (as complete words only, not substrings)
    let common_words = vec!["the", "and"];
    
    // Split into words, filter out common words, and rejoin
    let filtered_words: Vec<&str> = result
        .split(' ')
        .filter(|word| !common_words.contains(word))
        .collect();
    
    // If all words were filtered out, return the original normalized result
    if filtered_words.is_empty() {
        return result;
    }
    
    // Join the filtered words back together
    let result = filtered_words.join(" ");
    
    // Step 5: Remove ALL spaces in the final result
    result.replace(" ", "")
}

/// Get metadata for an artist, first checking the attribute cache and then searching MusicBrainz if needed
pub fn get_artist_meta(artist_name: &str) -> Option<ArtistMeta> {
    // Try to get metadata from the attribute cache first
    let cache_key_mbid = format!("artist::{}::mbid", artist_name);
    let _cache_key_thumb = format!("artist::{}::thumbnail", artist_name);
    let _cache_key_banner = format!("artist::{}::banner", artist_name);
    
    // Create an ArtistMeta to store the retrieved metadata
    let mut meta = ArtistMeta::new();
    let mut found_in_cache = false;
    
    // Try to get MusicBrainz ID from cache
    match attributecache::get::<String>(&cache_key_mbid) {
        Ok(Some(mbid)) => {
            debug!("Found MusicBrainz ID for '{}' in cache: {}", artist_name, mbid);
            meta.set_mbid(mbid);
            found_in_cache = true;
        },
        Ok(None) => {
            debug!("No MusicBrainz ID found in cache for '{}'", artist_name);
        },
        Err(e) => {
            warn!("Error retrieving MusicBrainz ID from cache for '{}': {}", artist_name, e);
        }
    }
    
    // If not found in cache, search MusicBrainz for the artist
    if !found_in_cache {
        match search_musicbrainz_for_artist(artist_name) {
            Some(mbid) => {
                debug!("Found MusicBrainz ID for '{}': {}", artist_name, mbid);
                meta.set_mbid(mbid.clone());
                
                // Store in cache for future use
                if let Err(e) = attributecache::set(&cache_key_mbid, &mbid) {
                    warn!("Failed to cache MusicBrainz ID for '{}': {}", artist_name, e);
                }
            },
            None => {
                warn!("Could not find MusicBrainz ID for '{}'", artist_name);
                // Return None if we couldn't find any metadata
                return None;
            }
        }
    }
    
    // Note: Thumbnail and banner retrieval is not implemented yet as per requirements
    
    // Return the metadata if we have at least a MusicBrainz ID
    if meta.mbid.is_some() {
        Some(meta)
    } else {
        None
    }
}

/// Search MusicBrainz API for an artist and return their MBID if found
pub fn search_musicbrainz_for_artist(artist_name: &str) -> Option<String> {
    debug!("Searching MusicBrainz for artist: '{}'", artist_name);
    
    // Create a reqwest client with appropriate timeouts
    let client = match Client::builder()
        .timeout(Duration::from_secs(10))
        .user_agent("AudioControl3/1.0 (https://github.com/hifiberry/audiocontrol3)")
        .build() {
        Ok(client) => client,
        Err(e) => {
            error!("Failed to create HTTP client for MusicBrainz search: {}", e);
            return None;
        }
    };
    
    // URL encode the artist name for the query
    let encoded_name = urlencoding::encode(artist_name).to_string();
    
    // Construct the MusicBrainz API query URL
    let url = format!("https://musicbrainz.org/ws/2/artist?query={}&fmt=json", encoded_name);
    
    // Add a 1-second delay between artists to limit API requests
    thread::sleep(Duration::from_secs(1));

    debug!("Sending request to MusicBrainz API: {}", url);
    let response = match client.get(&url).send() {
        Ok(response) => {
            if !response.status().is_success() {
                warn!("MusicBrainz API returned error status: {}", response.status());
                return None;
            }
            response
        },
        Err(e) => {
            error!("Failed to execute MusicBrainz API request: {}", e);
            return None;
        }
    };
    
    // Parse the response JSON
    let json: Value = match response.json() {
        Ok(json) => json,
        Err(e) => {
            error!("Failed to parse MusicBrainz API response: {}", e);
            return None;
        }
    };
    
    // Extract the first artist's MBID and name if available
    if let Some(artists) = json.get("artists").and_then(|a| a.as_array()) {
        if !artists.is_empty() {
            let artist_obj = &artists[0];
            
            // Extract MBID and artist name from response
            let mbid = artist_obj.get("id").and_then(|id| id.as_str());
            let response_name = artist_obj.get("name").and_then(|name| name.as_str());
            
            // Process the response only if we have both MBID and name
            if let (Some(mbid), Some(response_name)) = (mbid, response_name) {
                let mbid_string = mbid.to_string();
                
                // Use our new normalized comparison that removes all special characters
                let normalized_query = normalize_artist_name_for_comparison(artist_name);
                let normalized_response = normalize_artist_name_for_comparison(response_name);
                
                debug!("Comparing normalized names: '{}' vs '{}'", normalized_query, normalized_response);
                
                // Check if the normalized names match
                if normalized_query == normalized_response {
                    debug!("Found exactly matching artist: '{}' with MBID: {}", response_name, mbid_string);
                    
                    // Store the MBID in the attribute cache
                    let cache_key = format!("artist::{}::mbid", artist_name);
                    debug!("Attempting to store MBID in cache with key: {}", cache_key);
                    
                    match attributecache::set(&cache_key, &mbid_string) {
                        Ok(_) => {
                            debug!("Successfully stored MusicBrainz ID for '{}' in cache", artist_name);
                            
                            // Verify the cache write by reading it back
                            match attributecache::get::<String>(&cache_key) {
                                Ok(Some(cached_mbid)) => {
                                    if cached_mbid == mbid_string {
                                        debug!("Verified MBID in cache matches: {}", cached_mbid);
                                    } else {
                                        warn!("Cache verification failed! Expected: {}, Got: {}", mbid_string, cached_mbid);
                                    }
                                },
                                Ok(None) => warn!("Failed to verify MBID in cache - not found after writing!"),
                                Err(e) => warn!("Failed to verify MBID in cache: {}", e)
                            }
                        },
                        Err(e) => {
                            error!("Failed to cache MusicBrainz ID for '{}': {}", artist_name, e);
                        }
                    }
                    
                    // Return the MBID
                    return Some(mbid_string);
                } else {
                    // For cases where the names don't exactly match, implement a fuzzy comparison
                    // Check if one name is fully contained within the other
                    if normalized_query.contains(&normalized_response) || normalized_response.contains(&normalized_query) {
                        // ignore if the artist name contains "," or "feat."
                        if artist_name.contains(",") || artist_name.contains("feat.") {
                            debug!("Ignoring similar artist match due to multiple artists in name: '{}'", artist_name);
                            return None;
                        }

                        info!("Found similar artist: '{}' (searched for: '{}') with MBID: {}", 
                            response_name, artist_name, mbid_string);
                        
                        // Store the MBID in the cache but mark it as a partial match
                        let cache_key = format!("artist::{}::mbid", artist_name);
                        debug!("Storing MBID for similar artist match in cache with key: {}", cache_key);
                        
                        match attributecache::set(&cache_key, &mbid_string) {
                            Ok(_) => debug!("Stored MBID for similar artist: '{}' -> '{}'", artist_name, response_name),
                            Err(e) => error!("Failed to cache MBID for similar artist: {}", e)
                        }
                        
                        return Some(mbid_string);
                    } else {
                        // Names don't match and aren't similar enough
                        warn!("Artist name mismatch! Searched for: '{}', but found: '{}'", 
                            artist_name, response_name);
                        warn!("Normalized names: '{}' vs '{}'", normalized_query, normalized_response);
                        warn!("Rejecting MBID due to name mismatch");
                        
                        // Fall through to continue searching or return None
                    }
                }
            }
        }
    }
    
    info!("No matching MusicBrainz ID found for artist '{}'", artist_name);
    None
}

/// Update an artist's metadata by retrieving information from MusicBrainz
pub fn update_artist_metadata(artist: &mut Artist) -> bool {
    if let Some(meta) = get_artist_meta(&artist.name) {
        // Update the artist's metadata
        if artist.metadata.is_none() {
            artist.metadata = Some(meta);
        } else if let Some(ref mut current_meta) = artist.metadata {
            // Update existing metadata
            if let Some(mbid) = meta.mbid {
                current_meta.set_mbid(mbid);
            }
            // Thumbnail and banner handling will be added later
        }
        true
    } else {
        false
    }
}

/// Update an artist with metadata from cache or MusicBrainz API
/// 
/// This function updates the Artist object with metadata from the cache or by searching 
/// MusicBrainz if needed. It returns true if the artist was updated, false otherwise.
/// Multi-artists (with comma in name) are skipped.
pub fn update_artist(artist: &mut Artist) -> bool {    
    debug!("Updating metadata for artist '{}'", artist.name);
    
    // Get metadata using the existing get_artist_meta function
    if let Some(meta) = get_artist_meta(&artist.name) {
        // Update the artist's metadata
        if artist.metadata.is_none() {
            // Artist has no metadata yet, assign directly
            debug!("Adding new metadata for artist '{}'", artist.name);
            artist.metadata = Some(meta);
        } else {
            // Artist already has metadata, update fields if needed
            debug!("Updating existing metadata for artist '{}'", artist.name);
            if let Some(ref mut current_meta) = artist.metadata {
                // Update MusicBrainz ID if we have one
                if let Some(mbid) = meta.mbid {
                    if current_meta.mbid.is_none() || current_meta.mbid.as_ref().unwrap() != &mbid {
                        debug!("Updated MusicBrainz ID for artist '{}': {}", artist.name, mbid);
                        current_meta.set_mbid(mbid);
                    }
                }
                
                // Transfer any thumbnail URL if available
                if current_meta.thumb_url.is_none() && meta.thumb_url.is_some() {
                    current_meta.set_thumb_url(meta.thumb_url.unwrap());
                    debug!("Added thumbnail URL for artist '{}'", artist.name);
                }
                
                // Transfer any banner URL if available
                if current_meta.banner_url.is_none() && meta.banner_url.is_some() {
                    current_meta.set_banner_url(meta.banner_url.unwrap());
                    debug!("Added banner URL for artist '{}'", artist.name);
                }
            }
        }
        true
    } else {
        warn!("No metadata found for artist '{}'", artist.name);
        false
    }
}

/// Update artist metadata in background thread using a LibraryInterface
/// 
/// This function updates metadata for all artists in a library using a background
/// worker thread. It returns immediately while the updates continue in the background.
/// A 1-second delay is added between each artist to avoid overwhelming external APIs.
/// 
/// # Arguments
/// * `library` - Any object implementing LibraryInterface
pub fn update_library_artists_metadata_in_background<L: LibraryInterface + Send + Sync + 'static>(
    library: Arc<L>
) {
    // Get all artist names first
    let artists = library.get_artists();
    let artist_names: Vec<String> = artists.into_iter().map(|artist| artist.name).collect();
    
    let total_artists = artist_names.len();
    info!("Starting metadata update for {} artists using a single worker", total_artists);
    
    // Create a channel to distribute work
    let (sender, receiver) = std::sync::mpsc::channel();
    let receiver = Arc::new(Mutex::new(receiver));
    
    // Create a wait group to track completion
    let wait_group = Arc::new(AtomicUsize::new(0));
    
    // Send all artist names to the channel
    for artist_name in artist_names {
        if let Err(e) = sender.send(artist_name) {
            error!("Failed to send artist name to channel: {}", e);
        } else {
            wait_group.fetch_add(1, Ordering::SeqCst);
        }
    }
    
    // Create a single worker thread
    let thread_receiver = Arc::clone(&receiver);
    let thread_library = Arc::clone(&library);
    let thread_wait_group = Arc::clone(&wait_group);
    
    let _handle = thread::spawn(move || {
        debug!("Started metadata worker thread");
        
        // Process until channel is empty
        loop {
            // Try to get an artist name from the channel
            let artist_name = match thread_receiver.lock() {
                Ok(guard) => match guard.try_recv() {
                    Ok(name) => name,
                    Err(_) => break // Channel empty or disconnected
                },
                Err(_) => {
                    error!("Worker: Failed to acquire lock on receiver");
                    break;
                }
            };
            
            debug!("Worker: Updating metadata for artist '{}'", artist_name);
            
            // Get the artist, update metadata, then store it back
            if let Some(mut artist) = thread_library.get_artist(&artist_name) {
                let update_result = update_artist(&mut artist);
                
                if update_result {
                    debug!("Worker: Successfully updated metadata for '{}'", artist_name);
                    // Note: We can't store the artist back because get_artist returns a copy.
                    // LibraryInterface implementations will need to handle updates themselves.
                } else {
                    debug!("Worker: No metadata found for '{}'", artist_name);
                }
            } else {
                debug!("Worker: Artist '{}' not found in library", artist_name);
            }
            
            // Mark this task as completed
            thread_wait_group.fetch_sub(1, Ordering::SeqCst);
            
        }
        
        debug!("Worker finished processing");
    });
    
    // Create a monitoring thread to log progress
    let monitor_wait_group = Arc::clone(&wait_group);
    let _monitor = thread::spawn(move || {
        let start_time = std::time::Instant::now();
        let mut last_logged = 0;
        
        loop {
            thread::sleep(Duration::from_secs(5));
            let remaining = monitor_wait_group.load(Ordering::SeqCst);
            let completed = total_artists - remaining;
            
            // Only log if there's been progress or it's been a while
            if completed > last_logged || completed == total_artists {
                let elapsed = start_time.elapsed();
                let progress = if total_artists > 0 {
                    (completed as f32 / total_artists as f32) * 100.0
                } else {
                    100.0
                };
                
                info!("Artist metadata update progress: {:.1}% ({}/{}) - {:?} elapsed", 
                    progress, completed, total_artists, elapsed);
                
                last_logged = completed;
            }
            
            // Exit if all work is done
            if remaining == 0 {
                info!("Artist metadata update complete in {:?}", start_time.elapsed());
                break;
            }
        }
    });
    
    // No need to wait for the thread to complete - let it run in the background
    info!("Started background worker thread for updating artist metadata with 1-second delay between artists");
}

/// Download and cache artist images from FanartTV
/// 
/// This function:
/// 1. Looks up the MusicBrainz ID for the artist
/// 2. Fetches thumbnail and banner images from FanartTV
/// 3. Stores the images in the image cache
/// 4. Updates the artist metadata with image URLs
///
/// # Arguments
/// * `artist` - The artist to update with images
///
/// # Returns
/// * `bool` - True if any images were downloaded and cached
pub fn download_artist_images(artist: &mut Artist) -> bool {    
    // Get the artist's MusicBrainz ID
    let mbid = match get_artist_mbid(&artist.name) {
        Some(id) => id,
        None => {
            debug!("No MusicBrainz ID found for artist '{}'", artist.name);
            return false;
        }
    };
    
    // Create a safe basename for the artist
    let safe_name = artist_basename(&artist.name);
    let mut images_updated = false;
    
    // Create the artist directory path
    let artist_dir = format!("artist/{}", safe_name);
    
    // Try to get and store the thumbnail
    if let Some(thumb_url) = fanarttv::get_artist_thumbnail(&mbid) {
        match download_and_cache_image(&thumb_url, &artist_dir, "artist", artist) {
            Ok(_) => {
                debug!("Successfully downloaded and cached thumbnail for '{}'", artist.name);
                images_updated = true;
            },
            Err(e) => {
                warn!("Failed to download thumbnail for '{}': {}", artist.name, e);
            }
        }
    }
    
    // Try to get and store the banner
    if let Some(banner_url) = fanarttv::get_artist_banner(&mbid) {
        match download_and_cache_image(&banner_url, &artist_dir, "artist-banner", artist) {
            Ok(_) => {
                debug!("Successfully downloaded and cached banner for '{}'", artist.name);
                images_updated = true;
            },
            Err(e) => {
                warn!("Failed to download banner for '{}': {}", artist.name, e);
            }
        }
    }
    
    images_updated
}

/// Helper function to download an image and store it in the cache
/// 
/// # Arguments
/// * `url` - URL of the image to download
/// * `dir` - Directory in the image cache to store the image (e.g., "artist/madonna")
/// * `basename` - Base filename without extension (e.g., "artist" or "artist-banner")
/// * `artist` - The artist to update with the image URL
///
/// # Returns
/// * `Result<(), String>` - Success or error message
fn download_and_cache_image(url: &str, dir: &str, basename: &str, artist: &mut Artist) -> Result<(), String> {
    // Extract the file extension from the URL
    let ext = fanarttv::extract_extension_from_url(url);
    
    // Create the full filename
    let filename = format!("{}/{}.{}", dir, basename, ext);
    
    // Download the image data
    let image_data = fanarttv::download_image(url)?;
    
    // Store the image in the cache
    imagecache::store_image(&filename, &image_data)?;
    
    // Update the artist's metadata with the cached image path
    if let Some(ref mut meta) = artist.metadata {
        if basename == "artist" {
            meta.set_thumb_url(format!("cache://{}", filename));
            
            // Store the thumbnail URL in the attribute cache
            let cache_key = format!("artist::{}::thumbnail", artist.name);
            if let Err(e) = attributecache::set(&cache_key, &format!("cache://{}", filename)) {
                warn!("Failed to store thumbnail URL in attribute cache: {}", e);
            }
        } else if basename == "artist-banner" {
            meta.set_banner_url(format!("cache://{}", filename));
            
            // Store the banner URL in the attribute cache
            let cache_key = format!("artist::{}::banner", artist.name);
            if let Err(e) = attributecache::set(&cache_key, &format!("cache://{}", filename)) {
                warn!("Failed to store banner URL in attribute cache: {}", e);
            }
        }
    }
    
    Ok(())
}

/// Get MusicBrainz ID for an artist, first checking the cache
pub fn get_artist_mbid(artist_name: &str) -> Option<String> {
    // Try to get MBID from cache first
    let cache_key = format!("artist::{}::mbid", artist_name);
    
    match attributecache::get::<String>(&cache_key) {
        Ok(Some(mbid)) => {
            debug!("Found MusicBrainz ID for '{}' in cache: {}", artist_name, mbid);
            Some(mbid)
        },
        _ => {
            // Not in cache, search MusicBrainz
            search_musicbrainz_for_artist(artist_name)
        }
    }
}

/// Update an artist with metadata and images
/// 
/// This function updates the Artist object with metadata from MusicBrainz and images
/// from FanartTV, storing everything in the appropriate caches.
pub fn update_artist_with_images(artist: &mut Artist) -> bool {    
    info!("Updating metadata and images for artist '{}'", artist.name);
    
    // First update metadata (MusicBrainz ID)
    let metadata_updated = update_artist(artist);
    
    // Then download and cache images using the MusicBrainz ID
    let images_updated = download_artist_images(artist);
    
    metadata_updated || images_updated
}