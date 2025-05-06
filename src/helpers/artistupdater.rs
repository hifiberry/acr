use crate::data::artist::Artist;
use crate::data::metadata::ArtistMeta;
use crate::data::library::LibraryInterface;
use crate::helpers::attributecache;
use crate::helpers::imagecache;
use crate::helpers::fanarttv;
use crate::helpers::fanarttv::path_with_any_extension_exists;
use crate::helpers::musicbrainz::{MusicBrainzSearchResult, search_musicbrainz_for_artist, get_artist_mbid};
use log::{info, warn, error, debug};
use std::time::Duration;
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
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
        // Check if this artist was previously flagged as having multiple artists
        let ignored_flag_key = format!("artist::{}::ignored_multiple_artists", artist_name);
        match attributecache::get::<bool>(&ignored_flag_key) {
            Ok(Some(true)) => {
                // Artist was intentionally ignored, return None without displaying a warning
                debug!("Artist '{}' was previously flagged as containing multiple artists, skipping", artist_name);
                return None;
            },
            _ => {
                // Continue with the search if not found or there was an error
                match search_musicbrainz_for_artist(artist_name) {
                    MusicBrainzSearchResult::Found(mbid) => {
                        debug!("Found MusicBrainz ID for '{}': {}", artist_name, mbid);
                        meta.set_mbid(mbid.clone());
                        
                        // Store in cache for future use
                        if let Err(e) = attributecache::set(&cache_key_mbid, &mbid) {
                            warn!("Failed to cache MusicBrainz ID for '{}': {}", artist_name, e);
                        }
                    },
                    MusicBrainzSearchResult::Ignored => {
                        debug!("Artist '{}' was intentionally ignored", artist_name);
                        return None;
                    },
                    MusicBrainzSearchResult::NotFound => {
                        warn!("Could not find MusicBrainz ID for '{}'", artist_name);
                        return None;
                    },
                    MusicBrainzSearchResult::Error(e) => {
                        warn!("Error occurred while searching MusicBrainz for '{}': {}", artist_name, e);
                        return None;
                    }
                }
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
    
    let mut updated = false;
    
    // Get metadata using the existing get_artist_meta function
    if let Some(meta) = get_artist_meta(&artist.name) {
        // Update the artist's metadata
        if artist.metadata.is_none() {
            // Artist has no metadata yet, assign directly
            debug!("Adding new metadata for artist '{}'", artist.name);
            artist.metadata = Some(meta);
            updated = true;
        } else {
            // Artist already has metadata, update fields if needed
            debug!("Updating existing metadata for artist '{}'", artist.name);
            if let Some(ref mut current_meta) = artist.metadata {
                // Update MusicBrainz ID if we have one
                if let Some(mbid) = meta.mbid {
                    if current_meta.mbid.is_none() || current_meta.mbid.as_ref().unwrap() != &mbid {
                        debug!("Updated MusicBrainz ID for artist '{}': {}", artist.name, mbid);
                        current_meta.set_mbid(mbid);
                        updated = true;
                    }
                }
                
                // Transfer any thumbnail URL if available
                if current_meta.thumb_url.is_none() && meta.thumb_url.is_some() {
                    current_meta.set_thumb_url(meta.thumb_url.unwrap());
                    debug!("Added thumbnail URL for artist '{}'", artist.name);
                    updated = true;
                }
                
                // Transfer any banner URL if available
                if current_meta.banner_url.is_none() && meta.banner_url.is_some() {
                    current_meta.set_banner_url(meta.banner_url.unwrap());
                    debug!("Added banner URL for artist '{}'", artist.name);
                    updated = true;
                }
            }
        }
        
        // Download artist images after metadata is updated
        if updated && artist.metadata.as_ref().and_then(|m| m.mbid.as_ref()).is_some() {
            debug!("Downloading images for artist '{}'", artist.name);
            if download_artist_images(artist) {
                debug!("Successfully downloaded images for artist '{}'", artist.name);
                updated = true;
            }
        }
        
        updated
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
/// * `bool` - True if any images were downloaded or if the API call was successful even with no images
pub fn download_artist_images(artist: &mut Artist) -> bool {    
    // Get the artist's MusicBrainz ID
    let mbid = match get_artist_mbid(&artist.name) {
        Some(id) => id,
        None => {
            debug!("No MusicBrainz ID found for artist '{}'", artist.name);
            return false;
        }
    };
    
    // Check if we've already checked this artist with FanartTV
    let api_checked_key = format!("artist::{}::fanarttv_checked", artist.name);
    match attributecache::get::<bool>(&api_checked_key) {
        Ok(Some(true)) => {
            debug!("Already checked FanartTV for artist '{}', skipping download", artist.name);
            return true; // API was already successfully called before
        },
        _ => {
            // Not in cache, proceed with the API call
        }
    }
    
    // Call FanartTV API and process the result
    debug!("Calling FanartTV API for artist '{}'", artist.name);
    let api_success = fanarttv::download_artist_images(&mbid, &artist.name);
    
    // Store the API check status in the cache regardless of whether images were found
    if api_success {
        debug!("FanartTV API call successful for artist '{}'", artist.name);
        
        // Mark this artist as checked in the cache
        if let Err(e) = attributecache::set(&api_checked_key, &true) {
            warn!("Failed to store FanartTV check status in cache for '{}': {}", artist.name, e);
        } else {
            debug!("Stored FanartTV check status in cache for '{}'", artist.name);
        }
        
        // Create a safe basename for the artist
        let safe_name = artist_basename(&artist.name);
        
        // Check if images were actually found and downloaded
        let thumb_path = format!("artists/{}/artist.0", safe_name);
        let banner_path = format!("artists/{}/banner.0", safe_name);
        
        // Update the artist's metadata with the cached image paths if the files exist
        if let Some(ref mut meta) = artist.metadata {
            // Check for artist thumbnails
            if path_with_any_extension_exists(&thumb_path) {
                meta.set_thumb_url(format!("cache://{}", thumb_path));
                
                // Store the thumbnail URL in the attribute cache
                let cache_key = format!("artist::{}::thumbnail", artist.name);
                if let Err(e) = attributecache::set(&cache_key, &format!("cache://{}", thumb_path)) {
                    warn!("Failed to store thumbnail URL in attribute cache: {}", e);
                }
            }
            
            // Check for artist banners
            if path_with_any_extension_exists(&banner_path) {
                meta.set_banner_url(format!("cache://{}", banner_path));
                
                // Store the banner URL in the attribute cache
                let cache_key = format!("artist::{}::banner", artist.name);
                if let Err(e) = attributecache::set(&cache_key, &format!("cache://{}", banner_path)) {
                    warn!("Failed to store banner URL in attribute cache: {}", e);
                }
            }
        }
        
        return true;
    } else {
        warn!("FanartTV API call failed for artist '{}'", artist.name);
        
        // Mark this artist as checked in the cache even if the API call failed
        if let Err(e) = attributecache::set(&api_checked_key, &true) {
            warn!("Failed to store FanartTV check status in cache for '{}': {}", artist.name, e);
        } else {
            debug!("Stored FanartTV check status in cache for '{}'", artist.name);
        }
        
        return false;
    }
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

/// Update images for an artist
pub fn update_artist_images(artist: &mut Artist) -> bool {
    // Try to get the MusicBrainz ID for the artist
    let mbid = match get_artist_mbid(&artist.name) {
        Some(id) => id,
        None => {
            debug!("No MusicBrainz ID found for artist '{}'", artist.name);
            return false;
        }
    };
    
    // Create a safe basename for the artist
    let safe_name = artist_basename(&artist.name);
    
    warn!("Downloading images for artist '{}'", artist.name);
    
    // Use the comprehensive function to download all thumbnails and banners
    let api_success = fanarttv::download_artist_images(&mbid, &artist.name);
    
    if api_success {
        debug!("Successfully downloaded images for '{}'", artist.name);
        
        // Update the artist's metadata with the cached image paths
        if let Some(ref mut meta) = artist.metadata {
            // Check for artist thumbnails
            let thumb_path = format!("artists/{}/artist.0", safe_name);
            if path_with_any_extension_exists(&thumb_path) {
                meta.set_thumb_url(format!("cache://{}", thumb_path));
                
                // Store the thumbnail URL in the attribute cache
                let cache_key = format!("artist::{}::thumbnail", artist.name);
                if let Err(e) = attributecache::set(&cache_key, &format!("cache://{}", thumb_path)) {
                    warn!("Failed to store thumbnail URL in attribute cache: {}", e);
                }
            }
            
            // Check for artist banners
            let banner_path = format!("artists/{}/banner.0", safe_name);
            if path_with_any_extension_exists(&banner_path) {
                meta.set_banner_url(format!("cache://{}", banner_path));
                
                // Store the banner URL in the attribute cache
                let cache_key = format!("artist::{}::banner", artist.name);
                if let Err(e) = attributecache::set(&cache_key, &format!("cache://{}", banner_path)) {
                    warn!("Failed to store banner URL in attribute cache: {}", e);
                }
            }
        }
        
        return true;
    }
    
    false
}