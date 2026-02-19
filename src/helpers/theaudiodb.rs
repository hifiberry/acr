// filepath: c:\Users\matuschd\devel\hifiberry-os\packages\acr\src\helpers\theaudiodb.rs
use std::sync::atomic::{AtomicBool, Ordering};
use log::{info, debug, warn};
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use serde_json::{Value};
use crate::config::get_service_config;
use crate::helpers::http_client;
use crate::helpers::attributecache;
use crate::helpers::ratelimit;
use crate::data::artist::Artist;
use crate::helpers::ArtistUpdater;

/// Global flag to indicate if TheAudioDB lookups are enabled
static THEAUDIODB_ENABLED: AtomicBool = AtomicBool::new(false);

/// Create a new HTTP client with a timeout of 10 seconds
fn new_client() -> Box<dyn http_client::HttpClient> {
    http_client::new_http_client(10)
}

/// API key storage for TheAudioDB
#[derive(Default)]
struct TheAudioDBConfig {
    api_key: String,
}

// Default API key from secrets.txt compiled at build time
#[cfg(not(test))]
pub fn default_theaudiodb_api_key() -> String {
    crate::secrets::artistdb_api_key()
}

#[cfg(test)]
pub fn default_theaudiodb_api_key() -> String {
    "test_api_key".to_string()
}

// Global singleton for TheAudioDB configuration
static THEAUDIODB_CONFIG: Lazy<Mutex<TheAudioDBConfig>> = Lazy::new(|| {
    Mutex::new(TheAudioDBConfig::default())
});

/// Initialize TheAudioDB module from configuration
pub fn initialize_from_config(config: &serde_json::Value) {    
    if let Some(audiodb_config) = get_service_config(config, "theaudiodb") {
        // Check if enabled flag exists and is set to true
        let enabled = audiodb_config.get("enable")
            .and_then(|v| v.as_bool())
            .unwrap_or(true); // Default to enabled if not specified
        
        THEAUDIODB_ENABLED.store(enabled, Ordering::SeqCst);
        
        // Get API key if provided
        if let Some(api_key) = audiodb_config.get("api_key").and_then(|v| v.as_str()) {
            {
                let mut config = THEAUDIODB_CONFIG.lock();
                debug!("Found TheAudioDB API key in config: {}",
                       if !api_key.is_empty() && api_key.len() > 4 {
                           format!("{}...", &api_key[0..4])
                       } else {
                           "Empty".to_string()
                       });

                config.api_key = api_key.to_string();
                if !api_key.is_empty() {
                    info!("TheAudioDB API key configured");
                } else {
                    // Try to load from the default key (secrets.txt)
                    let default_key = default_theaudiodb_api_key();
                    debug!("Trying default TheAudioDB API key: {}",
                            if default_key != "YOUR_API_KEY_HERE" && default_key.len() > 4 {
                                format!("{}...", &default_key[0..4])
                            } else {
                                "Not available".to_string()
                            });

                    if default_key != "YOUR_API_KEY_HERE" {
                        info!("Using default TheAudioDB API key");
                    } else {
                        warn!("Empty TheAudioDB API key provided");
                    }
                }
            }
        } else {
            warn!("No API key found for TheAudioDB in configuration");
        }
          // Register rate limit - default to 2 requests per second (500ms)
        let rate_limit_ms = audiodb_config.get("rate_limit_ms")
            .and_then(|v| v.as_u64())
            .unwrap_or(500);
            
        ratelimit::register_service("theaudiodb", rate_limit_ms);
        info!("TheAudioDB rate limit set to {} ms", rate_limit_ms);
        
        let status = if enabled { "enabled" } else { "disabled" };
        info!("TheAudioDB lookup {}", status);
    } else {
        // Default to disabled if not in config
        THEAUDIODB_ENABLED.store(false, Ordering::SeqCst);
        debug!("TheAudioDB configuration not found, lookups disabled");
        
        // Register default rate limit even if disabled
        ratelimit::register_service("theaudiodb", 500);
    }
}

/// Check if TheAudioDB lookups are enabled
pub fn is_enabled() -> bool {
    THEAUDIODB_ENABLED.load(Ordering::SeqCst)
}

/// Get the configured API key
pub fn get_api_key() -> Option<String> {
    let config = THEAUDIODB_CONFIG.lock();
    if config.api_key.is_empty() {
        // If no API key is configured in audiocontrol.json, use the default from secrets.txt
        let default_key = default_theaudiodb_api_key();

        if default_key != "YOUR_API_KEY_HERE" {
            info!("Using default secret for TheAudioDB");
            return Some(default_key.to_string());
        }
        None
    } else {
        Some(config.api_key.clone())
    }
}

/// Look up artist information from TheAudioDB by MusicBrainz ID
/// 
/// # Arguments
/// * `mbid` - MusicBrainz ID of the artist to look up
/// 
/// # Returns
/// * `Result<serde_json::Value, String>` - Artist information or error message
pub fn lookup_theaudiodb_by_mbid(mbid: &str) -> Result<serde_json::Value, String> {
    if !is_enabled() {
        return Err("TheAudioDB lookups are disabled".to_string());
    }
    
    // Create cache keys for both positive and negative results
    let cache_key = format!("theaudiodb::mbid::{}", mbid);
    let not_found_cache_key = format!("theaudiodb::not_found::{}", mbid);
    
    // Check if we have a positive result cached
    match attributecache::get::<Value>(&cache_key) {
        Ok(Some(artist_data)) => {
            debug!("Found cached TheAudioDB data for MBID {}", mbid);
            return Ok(artist_data);
        },
        Ok(None) => {
            debug!("No cached TheAudioDB data found for MBID {}", mbid);
        },
        Err(e) => {
            debug!("Error reading from cache for MBID {}: {}", mbid, e);
        }
    }
    
    // Check if we have a negative result cached
    match attributecache::get::<bool>(&not_found_cache_key) {
        Ok(Some(true)) => {
            debug!("MBID {} previously marked as not found in cache", mbid);
            return Err(format!("No artist found with MBID {} (from cache)", mbid));
        },
        _ => {
            // Continue with lookup if not marked as not found or error reading cache
        }
    }
    
    let api_key = match get_api_key() {
        Some(key) => {
            if key.is_empty() {
                return Err("No API key configured for TheAudioDB".to_string());
            }
            key
        },
        None => return Err("No API key configured for TheAudioDB".to_string()),
    };    debug!("Looking up artist with MBID {}", mbid);
    
    // Apply rate limiting before making the request
    ratelimit::rate_limit("theaudiodb");
    
    // Construct the API URL
    let url = format!(
        "https://www.theaudiodb.com/api/v1/json/{}/artist-mb.php?i={}", 
        api_key, 
        mbid
    );
    
    // Create a client with our http_client
    let client = new_client();
    
    // Make the request
    debug!("Making request to TheAudioDB API for MBID {}", mbid);
    let response_text = match client.get_text(&url) {
        Ok(text) => text,
        Err(e) => return Err(format!("Failed to send request to TheAudioDB: {}", e)),
    };
      // Parse the response as JSON
    match serde_json::from_str::<Value>(&response_text) {
        Ok(json_data) => {
            // Check if the artists array exists, is not empty, and contains exactly one artist
            if let Some(artists) = json_data.get("artists") {
                if artists.is_null() {
                    debug!("No artist data found for MBID {}", mbid);
                    // Cache negative result
                    let not_found_cache_key = format!("theaudiodb::not_found::{}", mbid);
                    if let Err(e) = attributecache::set(&not_found_cache_key, &true) {
                        debug!("Failed to cache negative result for MBID {}: {}", mbid, e);
                    } else {
                        debug!("Cached negative result for MBID {}", mbid);
                    }
                    return Err(format!("No artist found with MBID {}", mbid));
                }
                
                if let Some(artists_array) = artists.as_array() {
                    match artists_array.len() {
                        0 => {
                            debug!("Empty artists array for MBID {}", mbid);
                            // Cache negative result
                            let not_found_cache_key = format!("theaudiodb::not_found::{}", mbid);
                            if let Err(e) = attributecache::set(&not_found_cache_key, &true) {
                                debug!("Failed to cache negative result for MBID {}: {}", mbid, e);
                            } else {
                                debug!("Cached negative result for MBID {}", mbid);
                            }
                            Err(format!("No artist found with MBID {}", mbid))
                        },
                        1 => {
                            debug!("Successfully retrieved artist data for MBID {}", mbid);
                            let artist_data = artists_array[0].clone();
                            
                            // Cache the positive result
                            let cache_key = format!("theaudiodb::mbid::{}", mbid);
                            if let Err(e) = attributecache::set(&cache_key, &artist_data) {
                                debug!("Failed to cache artist data for MBID {}: {}", mbid, e);
                            } else {
                                debug!("Cached positive result for MBID {}", mbid);
                            }
                            
                            // Return just the artist object, not the whole array
                            Ok(artist_data)
                        },
                        n => {
                            debug!("Found {} artists for MBID {}, expected exactly 1", n, mbid);
                            Err(format!("Found {} artists for MBID {}, expected exactly 1", n, mbid))
                        }
                    }
                } else {
                    debug!("Invalid artists field format from TheAudioDB");
                    Err("Invalid response format from TheAudioDB (artists is not an array)".to_string())
                }
            } else {
                debug!("Invalid response format from TheAudioDB (no artists field)");
                Err("Invalid response format from TheAudioDB (no artists field)".to_string())
            }
        },
        Err(e) => Err(format!("Failed to parse TheAudioDB response: {}", e))
    }
}

/// Look up artist information from TheAudioDB by artist name
/// 
/// # Arguments
/// * `artist_name` - Name of the artist to look up
/// 
/// # Returns
/// * `Result<serde_json::Value, String>` - Artist information or error message
pub fn lookup_theaudiodb_by_artist_name(artist_name: &str) -> Result<serde_json::Value, String> {
    if !is_enabled() {
        return Err("TheAudioDB lookups are disabled".to_string());
    }
    
    // Create cache keys for both positive and negative results
    let cache_key = format!("theaudiodb::artist_name::{}", artist_name);
    let not_found_cache_key = format!("theaudiodb::artist_not_found::{}", artist_name);
    
    // Check if we have a positive result cached
    match attributecache::get::<Value>(&cache_key) {
        Ok(Some(artist_data)) => {
            debug!("Found cached TheAudioDB data for artist '{}'", artist_name);
            return Ok(artist_data);
        },
        Ok(None) => {
            debug!("No cached TheAudioDB data found for artist '{}'", artist_name);
        },
        Err(e) => {
            debug!("Error reading from cache for artist '{}': {}", artist_name, e);
        }
    }
    
    // Check if we have a negative result cached
    match attributecache::get::<bool>(&not_found_cache_key) {
        Ok(Some(true)) => {
            debug!("Artist '{}' previously marked as not found in cache", artist_name);
            return Err(format!("No artist found with name '{}' (from cache)", artist_name));
        },
        _ => {
            // Continue with lookup if not marked as not found or error reading cache
        }
    }
    
    let api_key = match get_api_key() {
        Some(key) => {
            if key.is_empty() {
                return Err("No API key configured for TheAudioDB".to_string());
            }
            key
        },
        None => return Err("No API key configured for TheAudioDB".to_string()),
    };
    
    debug!("Looking up artist by name '{}'", artist_name);
    
    // Apply rate limiting before making the request
    ratelimit::rate_limit("theaudiodb");
    
    // Construct the API URL
    let url = format!(
        "https://www.theaudiodb.com/api/v1/json/{}/search.php?s={}", 
        api_key, 
        urlencoding::encode(artist_name)
    );
    
    // Create a client with our http_client
    let client = new_client();
    
    // Make the request
    debug!("Making request to TheAudioDB API for artist '{}'", artist_name);
    let response_text = match client.get_text(&url) {
        Ok(text) => text,
        Err(e) => return Err(format!("Failed to send request to TheAudioDB: {}", e)),
    };
    
    // Parse the response as JSON
    match serde_json::from_str::<Value>(&response_text) {
        Ok(json_data) => {
            // Check if the artists array exists and is not empty
            if let Some(artists) = json_data.get("artists") {
                if artists.is_null() {
                    debug!("No artist data found for name '{}'", artist_name);
                    // Cache negative result
                    if let Err(e) = attributecache::set(&not_found_cache_key, &true) {
                        debug!("Failed to cache negative result for artist '{}': {}", artist_name, e);
                    } else {
                        debug!("Cached negative result for artist '{}'", artist_name);
                    }
                    return Err(format!("No artist found with name '{}'", artist_name));
                }
                
                if let Some(artists_array) = artists.as_array() {
                    if artists_array.is_empty() {
                        debug!("Empty artists array for name '{}'", artist_name);
                        // Cache negative result
                        if let Err(e) = attributecache::set(&not_found_cache_key, &true) {
                            debug!("Failed to cache negative result for artist '{}': {}", artist_name, e);
                        } else {
                            debug!("Cached negative result for artist '{}'", artist_name);
                        }
                        Err(format!("No artist found with name '{}'", artist_name))
                    } else {
                        debug!("Successfully retrieved artist data for name '{}'", artist_name);
                        let search_result = json_data.clone();
                        
                        // Cache the positive result
                        if let Err(e) = attributecache::set(&cache_key, &search_result) {
                            debug!("Failed to cache artist data for name '{}': {}", artist_name, e);
                        } else {
                            debug!("Cached positive result for artist '{}'", artist_name);
                        }
                        
                        Ok(search_result)
                    }
                } else {
                    debug!("Invalid artists field format from TheAudioDB");
                    Err("Invalid response format from TheAudioDB (artists is not an array)".to_string())
                }
            } else {
                debug!("Invalid response format from TheAudioDB (no artists field)");
                Err("Invalid response format from TheAudioDB (no artists field)".to_string())
            }
        },
        Err(e) => Err(format!("Failed to parse TheAudioDB response: {}", e))
    }
}

/// Look up albums by artist name from TheAudioDB
/// 
/// # Arguments
/// * `artist_name` - Name of the artist to look up albums for
/// 
/// # Returns
/// * `Result<serde_json::Value, String>` - Album information or error message
pub fn lookup_theaudiodb_albums_by_artist(artist_name: &str) -> Result<serde_json::Value, String> {
    if !is_enabled() {
        return Err("TheAudioDB lookups are disabled".to_string());
    }
    
    // Create cache keys for both positive and negative results
    let cache_key = format!("theaudiodb::albums_by_artist::{}", artist_name);
    let not_found_cache_key = format!("theaudiodb::albums_not_found::{}", artist_name);
    
    // Check if we have a positive result cached
    match attributecache::get::<Value>(&cache_key) {
        Ok(Some(album_data)) => {
            debug!("Found cached TheAudioDB album data for artist '{}'", artist_name);
            return Ok(album_data);
        },
        Ok(None) => {
            debug!("No cached TheAudioDB album data found for artist '{}'", artist_name);
        },
        Err(e) => {
            debug!("Error reading from cache for artist albums '{}': {}", artist_name, e);
        }
    }
    
    // Check if we have a negative result cached
    match attributecache::get::<bool>(&not_found_cache_key) {
        Ok(Some(true)) => {
            debug!("Artist albums '{}' previously marked as not found in cache", artist_name);
            return Err(format!("No albums found for artist '{}' (from cache)", artist_name));
        },
        _ => {
            // Continue with lookup if not marked as not found or error reading cache
        }
    }
    
    let api_key = match get_api_key() {
        Some(key) => {
            if key.is_empty() {
                return Err("No API key configured for TheAudioDB".to_string());
            }
            key
        },
        None => return Err("No API key configured for TheAudioDB".to_string()),
    };
    
    debug!("Looking up albums for artist '{}'", artist_name);
    
    // Apply rate limiting before making the request
    ratelimit::rate_limit("theaudiodb");
    
    // Construct the API URL
    let url = format!(
        "https://www.theaudiodb.com/api/v1/json/{}/searchalbum.php?s={}", 
        api_key, 
        urlencoding::encode(artist_name)
    );
    
    // Create a client with our http_client
    let client = new_client();
    
    // Make the request
    debug!("Making request to TheAudioDB API for albums by artist '{}'", artist_name);
    let response_text = match client.get_text(&url) {
        Ok(text) => text,
        Err(e) => return Err(format!("Failed to send request to TheAudioDB: {}", e)),
    };
    
    // Parse the response as JSON
    match serde_json::from_str::<Value>(&response_text) {
        Ok(json_data) => {
            // Check if the album array exists and is not empty
            if let Some(albums) = json_data.get("album") {
                if albums.is_null() {
                    debug!("No album data found for artist '{}'", artist_name);
                    // Cache negative result
                    if let Err(e) = attributecache::set(&not_found_cache_key, &true) {
                        debug!("Failed to cache negative result for artist albums '{}': {}", artist_name, e);
                    } else {
                        debug!("Cached negative result for artist albums '{}'", artist_name);
                    }
                    return Err(format!("No albums found for artist '{}'", artist_name));
                }
                
                if let Some(albums_array) = albums.as_array() {
                    if albums_array.is_empty() {
                        debug!("Empty albums array for artist '{}'", artist_name);
                        // Cache negative result
                        if let Err(e) = attributecache::set(&not_found_cache_key, &true) {
                            debug!("Failed to cache negative result for artist albums '{}': {}", artist_name, e);
                        } else {
                            debug!("Cached negative result for artist albums '{}'", artist_name);
                        }
                        Err(format!("No albums found for artist '{}'", artist_name))
                    } else {
                        debug!("Successfully retrieved album data for artist '{}'", artist_name);
                        let search_result = json_data.clone();
                        
                        // Cache the positive result
                        if let Err(e) = attributecache::set(&cache_key, &search_result) {
                            debug!("Failed to cache album data for artist '{}': {}", artist_name, e);
                        } else {
                            debug!("Cached positive result for artist albums '{}'", artist_name);
                        }
                        
                        Ok(search_result)
                    }
                } else {
                    debug!("Invalid album field format from TheAudioDB");
                    Err("Invalid response format from TheAudioDB (album is not an array)".to_string())
                }
            } else {
                debug!("Invalid response format from TheAudioDB (no album field)");
                Err("Invalid response format from TheAudioDB (no album field)".to_string())
            }
        },
        Err(e) => Err(format!("Failed to parse TheAudioDB response: {}", e))
    }
}

/// Look up a specific album by artist and album name from TheAudioDB
/// 
/// # Arguments
/// * `artist_name` - Name of the artist
/// * `album_name` - Name of the album
/// 
/// # Returns
/// * `Result<serde_json::Value, String>` - Album information or error message
pub fn lookup_theaudiodb_album_by_name(artist_name: &str, album_name: &str) -> Result<serde_json::Value, String> {
    if !is_enabled() {
        return Err("TheAudioDB lookups are disabled".to_string());
    }
    
    // Create cache keys for both positive and negative results
    let cache_key = format!("theaudiodb::album::{}::{}", artist_name, album_name);
    let not_found_cache_key = format!("theaudiodb::album_not_found::{}::{}", artist_name, album_name);
    
    // Check if we have a positive result cached
    match attributecache::get::<Value>(&cache_key) {
        Ok(Some(album_data)) => {
            debug!("Found cached TheAudioDB data for album '{}' by '{}'", album_name, artist_name);
            return Ok(album_data);
        },
        Ok(None) => {
            debug!("No cached TheAudioDB data found for album '{}' by '{}'", album_name, artist_name);
        },
        Err(e) => {
            debug!("Error reading from cache for album '{}' by '{}': {}", album_name, artist_name, e);
        }
    }
    
    // Check if we have a negative result cached
    match attributecache::get::<bool>(&not_found_cache_key) {
        Ok(Some(true)) => {
            debug!("Album '{}' by '{}' previously marked as not found in cache", album_name, artist_name);
            return Err(format!("No album '{}' found for artist '{}' (from cache)", album_name, artist_name));
        },
        _ => {
            // Continue with lookup if not marked as not found or error reading cache
        }
    }
    
    let api_key = match get_api_key() {
        Some(key) => {
            if key.is_empty() {
                return Err("No API key configured for TheAudioDB".to_string());
            }
            key
        },
        None => return Err("No API key configured for TheAudioDB".to_string()),
    };
    
    debug!("Looking up album '{}' by artist '{}'", album_name, artist_name);
    
    // Apply rate limiting before making the request
    ratelimit::rate_limit("theaudiodb");
    
    // Construct the API URL
    let url = format!(
        "https://www.theaudiodb.com/api/v1/json/{}/searchalbum.php?s={}&a={}", 
        api_key, 
        urlencoding::encode(artist_name),
        urlencoding::encode(album_name)
    );
    
    // Create a client with our http_client
    let client = new_client();
    
    // Make the request
    debug!("Making request to TheAudioDB API for album '{}' by '{}'", album_name, artist_name);
    let response_text = match client.get_text(&url) {
        Ok(text) => text,
        Err(e) => return Err(format!("Failed to send request to TheAudioDB: {}", e)),
    };
    
    // Parse the response as JSON
    match serde_json::from_str::<Value>(&response_text) {
        Ok(json_data) => {
            // Check if the album array exists and is not empty
            if let Some(albums) = json_data.get("album") {
                if albums.is_null() {
                    debug!("No album data found for '{}' by '{}'", album_name, artist_name);
                    // Cache negative result
                    if let Err(e) = attributecache::set(&not_found_cache_key, &true) {
                        debug!("Failed to cache negative result for album '{}' by '{}': {}", album_name, artist_name, e);
                    } else {
                        debug!("Cached negative result for album '{}' by '{}'", album_name, artist_name);
                    }
                    return Err(format!("No album '{}' found for artist '{}'", album_name, artist_name));
                }
                
                if let Some(albums_array) = albums.as_array() {
                    if albums_array.is_empty() {
                        debug!("Empty albums array for '{}' by '{}'", album_name, artist_name);
                        // Cache negative result
                        if let Err(e) = attributecache::set(&not_found_cache_key, &true) {
                            debug!("Failed to cache negative result for album '{}' by '{}': {}", album_name, artist_name, e);
                        } else {
                            debug!("Cached negative result for album '{}' by '{}'", album_name, artist_name);
                        }
                        Err(format!("No album '{}' found for artist '{}'", album_name, artist_name))
                    } else {
                        debug!("Successfully retrieved album data for '{}' by '{}'", album_name, artist_name);
                        let search_result = json_data.clone();
                        
                        // Cache the positive result
                        if let Err(e) = attributecache::set(&cache_key, &search_result) {
                            debug!("Failed to cache album data for '{}' by '{}': {}", album_name, artist_name, e);
                        } else {
                            debug!("Cached positive result for album '{}' by '{}'", album_name, artist_name);
                        }
                        
                        Ok(search_result)
                    }
                } else {
                    debug!("Invalid album field format from TheAudioDB");
                    Err("Invalid response format from TheAudioDB (album is not an array)".to_string())
                }
            } else {
                debug!("Invalid response format from TheAudioDB (no album field)");
                Err("Invalid response format from TheAudioDB (no album field)".to_string())
            }
        },
        Err(e) => Err(format!("Failed to parse TheAudioDB response: {}", e))
    }
}

/// Get artist cover art URLs from TheAudioDB
/// 
/// # Arguments
/// * `artist_name` - Name of the artist
/// 
/// # Returns
/// * `Vec<String>` - URLs to artist cover art images
pub fn get_artist_coverart(artist_name: &str) -> Vec<String> {
    debug!("TheAudioDB: Searching for artist cover art: {}", artist_name);
    
    match lookup_theaudiodb_by_artist_name(artist_name) {
        Ok(search_result) => {
            // Extract artist images from search results
            if let Some(artists) = search_result.get("artists")
                .and_then(|a| a.as_array()) 
            {
                let mut urls = Vec::new();
                
                for artist_data in artists {
                    // Get the main artist thumbnail
                    if let Some(thumb_url) = artist_data.get("strArtistThumb")
                        .and_then(|u| u.as_str()) 
                    {
                        if !thumb_url.is_empty() {
                            urls.push(thumb_url.to_string());
                        }
                    }
                    
                    // Get additional artist images if available
                    if let Some(banner_url) = artist_data.get("strArtistBanner")
                        .and_then(|u| u.as_str()) 
                    {
                        if !banner_url.is_empty() {
                            urls.push(banner_url.to_string());
                        }
                    }
                    
                    if let Some(fanart_url) = artist_data.get("strArtistFanart")
                        .and_then(|u| u.as_str()) 
                    {
                        if !fanart_url.is_empty() {
                            urls.push(fanart_url.to_string());
                        }
                    }
                    
                    if let Some(logo_url) = artist_data.get("strArtistLogo")
                        .and_then(|u| u.as_str()) 
                    {
                        if !logo_url.is_empty() {
                            urls.push(logo_url.to_string());
                        }
                    }
                }
                
                debug!("TheAudioDB: Found {} artist images for '{}'", urls.len(), artist_name);
                urls
            } else {
                debug!("TheAudioDB: No artist images found for '{}'", artist_name);
                Vec::new()
            }
        }
        Err(e) => {
            warn!("TheAudioDB: Failed to search for artist '{}': {}", artist_name, e);
            Vec::new()
        }
    }
}

/// Get album cover art URLs from TheAudioDB
/// 
/// # Arguments
/// * `album_name` - Name of the album
/// * `artist_name` - Name of the artist
/// * `_year` - Optional release year (not used by TheAudioDB)
/// 
/// # Returns
/// * `Vec<String>` - URLs to album cover art images
pub fn get_album_coverart(album_name: &str, artist_name: &str, _year: Option<i32>) -> Vec<String> {
    debug!("TheAudioDB: Searching for album cover art: '{}' by '{}'", album_name, artist_name);
    
    // First try specific album search
    match lookup_theaudiodb_album_by_name(artist_name, album_name) {
        Ok(search_result) => {
            if let Some(albums) = search_result.get("album")
                .and_then(|a| a.as_array()) 
            {
                let mut urls = Vec::new();
                
                for album_data in albums {
                    // Get the main album thumbnail
                    if let Some(thumb_url) = album_data.get("strAlbumThumb")
                        .and_then(|u| u.as_str()) 
                    {
                        if !thumb_url.is_empty() {
                            urls.push(thumb_url.to_string());
                        }
                    }
                    
                    // Get additional album images if available
                    if let Some(thumb_3d_url) = album_data.get("strAlbumThumb3D")
                        .and_then(|u| u.as_str()) 
                    {
                        if !thumb_3d_url.is_empty() {
                            urls.push(thumb_3d_url.to_string());
                        }
                    }
                    
                    if let Some(spine_url) = album_data.get("strAlbumSpine")
                        .and_then(|u| u.as_str()) 
                    {
                        if !spine_url.is_empty() {
                            urls.push(spine_url.to_string());
                        }
                    }
                    
                    if let Some(cd_art_url) = album_data.get("strAlbumCDart")
                        .and_then(|u| u.as_str()) 
                    {
                        if !cd_art_url.is_empty() {
                            urls.push(cd_art_url.to_string());
                        }
                    }
                }
                
                if !urls.is_empty() {
                    debug!("TheAudioDB: Found {} album images for '{}' by '{}' (specific search)", urls.len(), album_name, artist_name);
                    return urls;
                }
            }
        }
        Err(e) => {
            debug!("TheAudioDB: Specific album search failed for '{}' by '{}': {}", album_name, artist_name, e);
        }
    }
    
    // Fallback: search all albums by artist and find matching album name
    match lookup_theaudiodb_albums_by_artist(artist_name) {
        Ok(search_result) => {
            if let Some(albums) = search_result.get("album")
                .and_then(|a| a.as_array()) 
            {
                let mut urls = Vec::new();
                let album_name_lower = album_name.to_lowercase();
                
                for album_data in albums {
                    // Check if album name matches (case-insensitive)
                    if let Some(album_title) = album_data.get("strAlbum")
                        .and_then(|n| n.as_str()) 
                    {
                        if album_title.to_lowercase().contains(&album_name_lower) || 
                           album_name_lower.contains(&album_title.to_lowercase()) {
                            
                            // Get the main album thumbnail
                            if let Some(thumb_url) = album_data.get("strAlbumThumb")
                                .and_then(|u| u.as_str()) 
                            {
                                if !thumb_url.is_empty() {
                                    urls.push(thumb_url.to_string());
                                }
                            }
                            
                            // Get additional album images if available
                            if let Some(thumb_3d_url) = album_data.get("strAlbumThumb3D")
                                .and_then(|u| u.as_str()) 
                            {
                                if !thumb_3d_url.is_empty() {
                                    urls.push(thumb_3d_url.to_string());
                                }
                            }
                            
                            break; // Found matching album, use first match
                        }
                    }
                }
                
                debug!("TheAudioDB: Found {} album images for '{}' by '{}' (fallback search)", urls.len(), album_name, artist_name);
                urls
            } else {
                debug!("TheAudioDB: No album images found for '{}' by '{}'", album_name, artist_name);
                Vec::new()
            }
        }
        Err(e) => {
            warn!("TheAudioDB: Failed to search for albums by '{}': {}", artist_name, e);
            Vec::new()
        }
    }
}

/// Implement the ArtistUpdater trait for TheAudioDB
pub struct TheAudioDbUpdater;

impl Default for TheAudioDbUpdater {
    fn default() -> Self {
        Self::new()
    }
}

impl TheAudioDbUpdater {
    pub fn new() -> Self {
        TheAudioDbUpdater
    }
}

impl ArtistUpdater for TheAudioDbUpdater {
    /// Updates artist information using TheAudioDB service
    /// 
    /// This function fetches artist information from TheAudioDB using the MusicBrainz ID
    /// from the artist's metadata and updates the artist with thumbnail URLs and other
    /// available metadata.
    /// 
    /// # Arguments
    /// * `artist` - The artist to update
    /// 
    /// # Returns
    /// The updated artist with information from TheAudioDB
    fn update_artist(&self, mut artist: Artist) -> Artist {
        // Check if TheAudioDB lookups are enabled
        if !is_enabled() {
            debug!("TheAudioDB lookups are disabled, skipping artist {}", artist.name);
            return artist;
        }
        
        // Extract and clone the MusicBrainz ID to avoid borrowing issues
        let mbid_opt = artist.metadata.as_ref()
            .and_then(|meta| meta.mbid.first())
            .cloned();
        
        // Proceed only if a MusicBrainz ID is available
        if let Some(mbid) = mbid_opt {
            debug!("Looking up artist information in TheAudioDB for {} with MBID {}", artist.name, mbid);
            
            // Lookup artist by MBID
            match lookup_theaudiodb_by_mbid(&mbid) {
                Ok(artist_data) => {
                    debug!("Successfully retrieved artist data from TheAudioDB for {}", artist.name);
                    
                    let mut updated_data = Vec::new();
                    

                    
                    // Extract additional artist metadata that could be useful
                    if let Some(biography) = artist_data.get("strBiographyEN").and_then(|v| v.as_str()) {
                        if !biography.is_empty() {
                            if let Some(meta) = &mut artist.metadata {
                                meta.biography = Some(biography.to_string());
                                meta.biography_source = Some("TheAudioDB".to_string());
                                updated_data.push("biography".to_string());
                                debug!("Added biography from TheAudioDB for artist {}", artist.name);
                            }
                        }
                    }
                    
                    // Extract genre information
                    if let Some(genre) = artist_data.get("strGenre").and_then(|v| v.as_str()) {
                        if !genre.is_empty() {
                            if let Some(meta) = &mut artist.metadata {
                                // Apply genre cleanup
                                let genres_to_add = crate::helpers::genre_cleanup::clean_genres_global(vec![genre.to_string()]);
                                for cleaned_genre in genres_to_add {
                                    if !meta.genres.contains(&cleaned_genre) {
                                        meta.genres.push(cleaned_genre.clone());
                                        debug!("Added cleaned genre '{}' from TheAudioDB for artist {}", cleaned_genre, artist.name);
                                    }
                                }
                                if !meta.genres.is_empty() {
                                    updated_data.push("genre".to_string());
                                }
                            }
                        }
                    }
                    
                    // Log successful update with summary of what was added
                    if !updated_data.is_empty() {
                        info!("Updated artist '{}' with TheAudioDB data: {}", artist.name, updated_data.join(", "));
                    }
                },
                Err(e) => {
                    info!("Failed to retrieve artist data from TheAudioDB for {} with MBID {}: {}", artist.name, mbid, e);
                    // This error is likely already cached as a negative result in lookup_theaudiodb_by_mbid
                }
            }
        } else {
            debug!("No MusicBrainz ID available for artist {}, skipping TheAudioDB lookup", artist.name);
        }
        
        artist
    }
}

/// Cover Art Provider implementation for TheAudioDB
pub struct TheAudioDbCoverartProvider;

impl Default for TheAudioDbCoverartProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl TheAudioDbCoverartProvider {
    pub fn new() -> Self {
        TheAudioDbCoverartProvider
    }
}

impl crate::helpers::coverart::CoverartProvider for TheAudioDbCoverartProvider {
    fn name(&self) -> &str {
        "theaudiodb"
    }

    fn display_name(&self) -> &str {
        "TheAudioDB"
    }

    fn supported_methods(&self) -> std::collections::HashSet<crate::helpers::coverart::CoverartMethod> {
        use crate::helpers::coverart::CoverartMethod;
        let mut methods = std::collections::HashSet::new();
        methods.insert(CoverartMethod::Artist);
        methods.insert(CoverartMethod::Album);
        methods
    }

    fn get_artist_coverart_impl(&self, artist: &str) -> Vec<String> {
        get_artist_coverart(artist)
    }

    fn get_album_coverart_impl(&self, album: &str, artist: &str, year: Option<i32>) -> Vec<String> {
        get_album_coverart(album, artist, year)
    }
}

#[cfg(test)]
mod tests {
    //! Unit tests for TheAudioDB functionality
    //! 
    //! These tests will skip if no real API key is available.
    //! When compiled with secrets.txt containing a real TheAudioDB API key,
    //! all tests will run. Otherwise, tests requiring API access will be skipped
    //! and only local functionality (like configuration and caching) will be tested.
    
    use super::*;
    use std::sync::Once;
    use serial_test::serial;

    static INIT: Once = Once::new();

    fn init() {
        INIT.call_once(|| {
            env_logger::builder()
                .filter_level(log::LevelFilter::Debug)
                .try_init()
                .ok();
        });
    }

    /// Check if we have a real API key available for testing
    fn has_real_api_key() -> bool {
        let api_key = default_theaudiodb_api_key();
        !api_key.is_empty() && api_key != "test_api_key" && api_key != "YOUR_API_KEY_HERE"
    }

    /// Skip test if no real API key is available
    fn skip_if_no_api_key() {
        if !has_real_api_key() {
            println!("Skipping test: No real TheAudioDB API key available");
            return;
        }
    }

    fn setup_test_config() {
        init();
        let config = serde_json::json!({
            "theaudiodb": {
                "enable": true,
                "api_key": default_theaudiodb_api_key(),
                "rate_limit_ms": 100  // Faster for testing
            }
        });
        initialize_from_config(&config);
    }

    #[test]
    #[serial]
    fn test_is_enabled_default() {
        // Test that it's disabled by default
        THEAUDIODB_ENABLED.store(false, Ordering::SeqCst);
        assert!(!is_enabled());
    }

    #[test]
    #[serial]
    fn test_initialize_from_config_enabled() {
        let config = serde_json::json!({
            "theaudiodb": {
                "enable": true,
                "api_key": "test_key_123",
                "rate_limit_ms": 250
            }
        });
        
        initialize_from_config(&config);
        assert!(is_enabled());
    }

    #[test]
    #[serial]
    fn test_initialize_from_config_disabled() {
        let config = serde_json::json!({
            "theaudiodb": {
                "enable": false,
                "api_key": "test_key_123"
            }
        });
        
        initialize_from_config(&config);
        assert!(!is_enabled());
    }

    #[test]
    #[serial]
    fn test_get_api_key_with_config() {
        let config = serde_json::json!({
            "theaudiodb": {
                "enable": true,
                "api_key": "configured_key_123"
            }
        });
        
        initialize_from_config(&config);
        let api_key = get_api_key();
        assert!(api_key.is_some());
        assert_eq!(api_key.unwrap(), "configured_key_123");
    }

    #[test]
    #[serial]
    fn test_get_api_key_default() {
        let config = serde_json::json!({
            "theaudiodb": {
                "enable": true,
                "api_key": ""
            }
        });
        
        initialize_from_config(&config);
        let api_key = get_api_key();
        
        if has_real_api_key() {
            assert!(api_key.is_some());
            assert_ne!(api_key.unwrap(), "YOUR_API_KEY_HERE");
        } else {
            // In test environment, default key is "test_api_key"
            assert!(api_key.is_none() || api_key.unwrap() == "test_api_key");
        }
    }

    #[test]
    #[serial]
    fn test_lookup_theaudiodb_by_mbid_disabled() {
        THEAUDIODB_ENABLED.store(false, Ordering::SeqCst);
        
        let result = lookup_theaudiodb_by_mbid("5b11f4ce-a62d-471e-81fc-a69a8278c7da");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("disabled"));
    }

    #[test]
    #[serial]
    fn test_lookup_theaudiodb_by_mbid_no_api_key() {
        setup_test_config();
        
        // Override the config to have empty API key
        let config = serde_json::json!({
            "theaudiodb": {
                "enable": true,
                "api_key": ""
            }
        });
        initialize_from_config(&config);
        
        if !has_real_api_key() {
            let result = lookup_theaudiodb_by_mbid("5b11f4ce-a62d-471e-81fc-a69a8278c7da");
            // In test environment, this should either fail with no API key or with network error
            // since we have a test key that doesn't work
            assert!(result.is_err());
            let error = result.unwrap_err();
            let is_no_api_key_error = error.contains("No API key");
            let is_network_error = error.contains("Failed to send request") || error.contains("status code");
            assert!(is_no_api_key_error || is_network_error, "Expected no API key or network error, got: {}", error);
        }
    }

    #[test]
    #[serial]
    fn test_lookup_theaudiodb_by_mbid_valid() {
        if !has_real_api_key() {
            skip_if_no_api_key();
            return;
        }
        
        setup_test_config();
        
        // Use Radiohead's MBID as a test case
        let result = lookup_theaudiodb_by_mbid("a74b1b7f-71a5-4011-9441-d0b5e4122711");
        
        match result {
            Ok(artist_data) => {
                // Verify we got valid artist data
                assert!(artist_data.is_object());
                // Should have basic artist fields
                if let Some(artist_name) = artist_data.get("strArtist") {
                    assert!(artist_name.is_string());
                    println!("Found artist: {}", artist_name.as_str().unwrap_or("Unknown"));
                }
            },
            Err(e) => {
                // This might fail if the MBID doesn't exist or network issues
                println!("Lookup failed (expected for some test cases): {}", e);
            }
        }
    }

    #[test]
    #[serial]
    fn test_lookup_theaudiodb_by_mbid_invalid() {
        if !has_real_api_key() {
            skip_if_no_api_key();
            return;
        }
        
        setup_test_config();
        
        // Use an invalid MBID
        let result = lookup_theaudiodb_by_mbid("invalid-mbid-12345");
        assert!(result.is_err());
    }

    #[test]
    #[serial] 
    fn test_lookup_theaudiodb_by_artist_name_disabled() {
        THEAUDIODB_ENABLED.store(false, Ordering::SeqCst);
        
        let result = lookup_theaudiodb_by_artist_name("Radiohead");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("disabled"));
    }

    #[test]
    #[serial]
    fn test_lookup_theaudiodb_by_artist_name_valid() {
        if !has_real_api_key() {
            skip_if_no_api_key();
            return;
        }
        
        setup_test_config();
        
        let result = lookup_theaudiodb_by_artist_name("Radiohead");
        
        match result {
            Ok(search_result) => {
                // Verify we got valid search result
                assert!(search_result.is_object());
                if let Some(artists) = search_result.get("artists") {
                    if let Some(artists_array) = artists.as_array() {
                        assert!(!artists_array.is_empty());
                        println!("Found {} artists", artists_array.len());
                    }
                }
            },
            Err(e) => {
                println!("Artist search failed (might be expected): {}", e);
            }
        }
    }

    #[test]
    #[serial]
    fn test_lookup_theaudiodb_by_artist_name_nonexistent() {
        if !has_real_api_key() {
            skip_if_no_api_key();
            return;
        }
        
        setup_test_config();
        
        let result = lookup_theaudiodb_by_artist_name("ThisArtistDoesNotExist12345XYZ");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("No artist found"));
    }

    #[test]
    #[serial]
    fn test_lookup_theaudiodb_albums_by_artist_disabled() {
        THEAUDIODB_ENABLED.store(false, Ordering::SeqCst);
        
        let result = lookup_theaudiodb_albums_by_artist("Radiohead");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("disabled"));
    }

    #[test]
    #[serial]
    fn test_lookup_theaudiodb_albums_by_artist_valid() {
        if !has_real_api_key() {
            skip_if_no_api_key();
            return;
        }
        
        setup_test_config();
        
        let result = lookup_theaudiodb_albums_by_artist("Radiohead");
        
        match result {
            Ok(search_result) => {
                // Verify we got valid search result
                assert!(search_result.is_object());
                if let Some(albums) = search_result.get("album") {
                    if let Some(albums_array) = albums.as_array() {
                        assert!(!albums_array.is_empty());
                        println!("Found {} albums", albums_array.len());
                    }
                }
            },
            Err(e) => {
                println!("Album search failed (might be expected): {}", e);
            }
        }
    }

    #[test]
    #[serial]
    fn test_lookup_theaudiodb_album_by_name_disabled() {
        THEAUDIODB_ENABLED.store(false, Ordering::SeqCst);
        
        let result = lookup_theaudiodb_album_by_name("Radiohead", "OK Computer");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("disabled"));
    }

    #[test]
    #[serial]
    fn test_lookup_theaudiodb_album_by_name_valid() {
        if !has_real_api_key() {
            skip_if_no_api_key();
            return;
        }
        
        setup_test_config();
        
        let result = lookup_theaudiodb_album_by_name("Radiohead", "OK Computer");
        
        match result {
            Ok(search_result) => {
                // Verify we got valid search result
                assert!(search_result.is_object());
                if let Some(albums) = search_result.get("album") {
                    if let Some(albums_array) = albums.as_array() {
                        assert!(!albums_array.is_empty());
                        println!("Found {} matching albums", albums_array.len());
                    }
                }
            },
            Err(e) => {
                println!("Specific album search failed (might be expected): {}", e);
            }
        }
    }

    #[test]
    #[serial]
    fn test_get_artist_coverart_disabled() {
        THEAUDIODB_ENABLED.store(false, Ordering::SeqCst);
        
        let urls = get_artist_coverart("Radiohead");
        assert!(urls.is_empty());
    }

    #[test]
    #[serial]
    fn test_get_artist_coverart_valid() {
        if !has_real_api_key() {
            skip_if_no_api_key();
            return;
        }
        
        setup_test_config();
        
        let urls = get_artist_coverart("Radiohead");
        
        if !urls.is_empty() {
            println!("Found {} artist images", urls.len());
            for (i, url) in urls.iter().enumerate() {
                assert!(url.starts_with("http"));
                println!("Image {}: {}", i + 1, url);
            }
        } else {
            println!("No artist images found (might be expected)");
        }
    }

    #[test]
    #[serial]
    fn test_get_album_coverart_disabled() {
        THEAUDIODB_ENABLED.store(false, Ordering::SeqCst);
        
        let urls = get_album_coverart("OK Computer", "Radiohead", Some(1997));
        assert!(urls.is_empty());
    }

    #[test]
    #[serial]
    fn test_get_album_coverart_valid() {
        if !has_real_api_key() {
            skip_if_no_api_key();
            return;
        }
        
        setup_test_config();
        
        let urls = get_album_coverart("OK Computer", "Radiohead", Some(1997));
        
        if !urls.is_empty() {
            println!("Found {} album images", urls.len());
            for (i, url) in urls.iter().enumerate() {
                assert!(url.starts_with("http"));
                println!("Image {}: {}", i + 1, url);
            }
        } else {
            println!("No album images found (might be expected)");
        }
    }

    #[test]
    #[serial]
    fn test_get_album_coverart_nonexistent() {
        if !has_real_api_key() {
            skip_if_no_api_key();
            return;
        }
        
        setup_test_config();
        
        let urls = get_album_coverart("ThisAlbumDoesNotExist12345", "NonExistentArtist", None);
        assert!(urls.is_empty());
    }

    #[test]
    #[serial]
    fn test_theaudiodb_updater_disabled() {
        THEAUDIODB_ENABLED.store(false, Ordering::SeqCst);
        
        let updater = TheAudioDbUpdater::new();
        let mut artist = Artist {
            id: crate::data::Identifier::String("test_artist".to_string()),
            name: "Test Artist".to_string(),
            is_multi: false,
            metadata: None,
        };
        artist.ensure_metadata();
        
        if let Some(metadata) = &mut artist.metadata {
            metadata.mbid.push("a74b1b7f-71a5-4011-9441-d0b5e4122711".to_string());
        }
        
        let original_name = artist.name.clone();
        let updated_artist = updater.update_artist(artist);
        
        // Should return unchanged artist when disabled
        assert_eq!(updated_artist.name, original_name);
    }

    #[test]
    #[serial]
    fn test_theaudiodb_updater_no_mbid() {
        if !has_real_api_key() {
            skip_if_no_api_key();
            return;
        }
        
        setup_test_config();
        
        let updater = TheAudioDbUpdater::new();
        let artist = Artist {
            id: crate::data::Identifier::String("test_artist".to_string()),
            name: "Test Artist".to_string(),
            is_multi: false,
            metadata: None,
        };
        
        let original_name = artist.name.clone();
        let updated_artist = updater.update_artist(artist);
        
        // Should return unchanged artist when no MBID
        assert_eq!(updated_artist.name, original_name);
    }

    #[test]
    #[serial]
    fn test_caching_behavior() {
        if !has_real_api_key() {
            skip_if_no_api_key();
            return;
        }
        
        setup_test_config();
        
        // Test caching by making the same request twice
        let artist_name = "Radiohead";
        
        // First request - should hit the API
        let start = std::time::Instant::now();
        let result1 = lookup_theaudiodb_by_artist_name(artist_name);
        let duration1 = start.elapsed();
        
        // Second request - should hit the cache
        let start = std::time::Instant::now();
        let result2 = lookup_theaudiodb_by_artist_name(artist_name);
        let duration2 = start.elapsed();
        
        // Both should return the same result
        match (result1, result2) {
            (Ok(data1), Ok(data2)) => {
                assert_eq!(data1, data2);
                // Cache hit should be faster (though this might not always be reliable)
                println!("First request: {:?}, Second request: {:?}", duration1, duration2);
            },
            (Err(e1), Err(e2)) => {
                // Both failed with same error (cached negative result)
                assert_eq!(e1, e2);
                println!("Both requests failed with cached error: {}", e1);
            },
            _ => {
                panic!("Inconsistent results between cached and non-cached requests");
            }
        }
    }

    #[test]
    #[serial]
    fn test_rate_limiting() {
        if !has_real_api_key() {
            skip_if_no_api_key();
            return;
        }
        
        setup_test_config();
        
        // Make multiple requests and ensure they're rate limited
        let start = std::time::Instant::now();
        
        // Make 3 requests that should be rate limited
        for i in 0..3 {
            let artist_name = format!("TestArtist{}", i);
            let _ = lookup_theaudiodb_by_artist_name(&artist_name);
        }
        
        let duration = start.elapsed();
        
        // With 100ms rate limit, 3 requests should take at least 200ms
        assert!(duration.as_millis() >= 200, "Rate limiting not working properly: took {:?}", duration);
        println!("Rate limiting test: 3 requests took {:?}", duration);
    }
}
