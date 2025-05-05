use crate::data::artist::Artist;
use crate::data::metadata::ArtistMeta;
use crate::data::library::LibraryInterface;
use crate::helpers::attributecache;
use log::{info, warn, error, debug};
use reqwest::blocking::Client;
use serde_json::Value;
use std::time::Duration;
use std::sync::{Arc, RwLock, Mutex};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::collections::HashMap;
use std::thread;
use deunicode::deunicode;

/// Convert an artist name to a safe basename for files
///
/// This function:
/// - Replaces non-ASCII characters with their ASCII equivalents
/// - Removes '/' and '\' characters
/// - Replaces '[' and '{' with '('
/// - Replaces ']' and '}' with ')'
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
    
    // Step 2: Process each character according to the rules
    let processed: String = ascii_name
        .chars()
        .map(|c| match c {
            '/' | '\\' => ' ',    // Remove slashes
            '[' | '{' => '(',     // Replace brackets with parentheses
            ']' | '}' => ')',     // Replace brackets with parentheses
            _ => c                // Keep other characters
        })
        .collect();
    
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
            info!("Found MusicBrainz ID for '{}' in cache: {}", artist_name, mbid);
            meta.set_mbid(mbid);
            found_in_cache = true;
        },
        Ok(None) => {
            info!("No MusicBrainz ID found in cache for '{}'", artist_name);
        },
        Err(e) => {
            warn!("Error retrieving MusicBrainz ID from cache for '{}': {}", artist_name, e);
        }
    }
    
    // If not found in cache, search MusicBrainz for the artist
    if !found_in_cache {
        match search_musicbrainz_for_artist(artist_name) {
            Some(mbid) => {
                info!("Found MusicBrainz ID for '{}': {}", artist_name, mbid);
                meta.set_mbid(mbid.clone());
                
                // Store in cache for future use
                if let Err(e) = attributecache::set(&cache_key_mbid, &mbid) {
                    warn!("Failed to cache MusicBrainz ID for '{}': {}", artist_name, e);
                }
            },
            None => {
                info!("Could not find MusicBrainz ID for '{}'", artist_name);
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
    
    // Extract the first artist's MBID if available
    if let Some(artists) = json.get("artists").and_then(|a| a.as_array()) {
        if !artists.is_empty() {
            if let Some(mbid) = artists[0].get("id").and_then(|id| id.as_str()) {
                let mbid_string = mbid.to_string();
                
                // Store the MBID in the attribute cache right here
                let cache_key = format!("artist::{}::mbid", artist_name);
                if let Err(e) = attributecache::set(&cache_key, &mbid_string) {
                    warn!("Failed to cache MusicBrainz ID for '{}': {}", artist_name, e);
                } else {
                    debug!("Stored MusicBrainz ID for '{}' in cache: {}", artist_name, mbid_string);
                }
                
                // Return the MBID
                return Some(mbid_string);
            }
        }
    }
    
    // If we get here, we didn't find an MBID
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
    // Skip multi-artists (names containing commas)
    if artist.is_multi() {
        debug!("Skipping metadata update for multi-artist '{}'", artist.name);
        return false;
    }
    
    info!("Updating metadata for artist '{}'", artist.name);
    
    // Get metadata using the existing get_artist_meta function
    if let Some(meta) = get_artist_meta(&artist.name) {
        // Update the artist's metadata
        if artist.metadata.is_none() {
            // Artist has no metadata yet, assign directly
            info!("Adding new metadata for artist '{}'", artist.name);
            artist.metadata = Some(meta);
        } else {
            // Artist already has metadata, update fields if needed
            info!("Updating existing metadata for artist '{}'", artist.name);
            if let Some(ref mut current_meta) = artist.metadata {
                // Update MusicBrainz ID if we have one
                if let Some(mbid) = meta.mbid {
                    if current_meta.mbid.is_none() || current_meta.mbid.as_ref().unwrap() != &mbid {
                        info!("Updated MusicBrainz ID for artist '{}': {}", artist.name, mbid);
                        current_meta.set_mbid(mbid);
                    }
                }
                
                // Transfer any thumbnail URL if available
                if current_meta.thumb_url.is_none() && meta.thumb_url.is_some() {
                    current_meta.set_thumb_url(meta.thumb_url.unwrap());
                    info!("Added thumbnail URL for artist '{}'", artist.name);
                }
                
                // Transfer any banner URL if available
                if current_meta.banner_url.is_none() && meta.banner_url.is_some() {
                    current_meta.set_banner_url(meta.banner_url.unwrap());
                    info!("Added banner URL for artist '{}'", artist.name);
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