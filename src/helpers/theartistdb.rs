use std::sync::atomic::{AtomicBool, Ordering};
use log::{info, debug, warn, error};
use lazy_static::lazy_static;
use reqwest;
use std::sync::Mutex;
use serde_json::{Value, json};
use std::time::Duration;
use crate::helpers::imagecache;

/// Global flag to indicate if TheArtistDB lookups are enabled
static THEARTISTDB_ENABLED: AtomicBool = AtomicBool::new(false);

/// API key storage for TheArtistDB
#[derive(Default)]
struct TheArtistDBConfig {
    api_key: String,
}

// Global singleton for TheArtistDB configuration
lazy_static! {
    static ref THEARTISTDB_CONFIG: Mutex<TheArtistDBConfig> = Mutex::new(TheArtistDBConfig::default());
}

/// Initialize TheArtistDB module from configuration
pub fn initialize_from_config(config: &serde_json::Value) {
    if let Some(artistdb_config) = config.get("theartistdb") {
        // Check if enabled flag exists and is set to true
        let enabled = artistdb_config.get("enable")
            .and_then(|v| v.as_bool())
            .unwrap_or(true); // Default to enabled if not specified
        
        THEARTISTDB_ENABLED.store(enabled, Ordering::SeqCst);
        
        // Get API key if provided
        if let Some(api_key) = artistdb_config.get("api_key").and_then(|v| v.as_str()) {
            if let Ok(mut config) = THEARTISTDB_CONFIG.lock() {
                config.api_key = api_key.to_string();
                if !api_key.is_empty() {
                    info!("TheArtistDB API key configured");
                } else {
                    warn!("Empty TheArtistDB API key provided");
                }
            } else {
                error!("Failed to acquire lock on TheArtistDB configuration");
            }
        } else {
            warn!("No API key found for TheArtistDB in configuration");
        }
        
        let status = if enabled { "enabled" } else { "disabled" };
        info!("TheArtistDB lookup {}", status);
    } else {
        // Default to disabled if not in config
        THEARTISTDB_ENABLED.store(false, Ordering::SeqCst);
        debug!("TheArtistDB configuration not found, lookups disabled");
    }
}

/// Check if TheArtistDB lookups are enabled
pub fn is_enabled() -> bool {
    THEARTISTDB_ENABLED.load(Ordering::SeqCst)
}

/// Get the configured API key
pub fn get_api_key() -> Option<String> {
    if let Ok(config) = THEARTISTDB_CONFIG.lock() {
        if config.api_key.is_empty() {
            None
        } else {
            Some(config.api_key.clone())
        }
    } else {
        None
    }
}

/// Look up artist information from TheArtistDB by MusicBrainz ID
/// 
/// # Arguments
/// * `mbid` - MusicBrainz ID of the artist to look up
/// 
/// # Returns
/// * `Result<serde_json::Value, String>` - Artist information or error message
pub fn lookup_mbid(mbid: &str) -> Result<serde_json::Value, String> {
    if !is_enabled() {
        return Err("TheArtistDB lookups are disabled".to_string());
    }
    
    let api_key = match get_api_key() {
        Some(key) => {
            if key.is_empty() {
                return Err("No API key configured for TheArtistDB".to_string());
            }
            key
        },
        None => return Err("No API key configured for TheArtistDB".to_string()),
    };

    debug!("Looking up artist with MBID {}", mbid);
    
    // Construct the API URL
    let url = format!(
        "https://www.theaudiodb.com/api/v1/json/{}/artist-mb.php?i={}", 
        api_key, 
        mbid
    );
    
    // Create a client with a reasonable timeout
    let client = match reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(10))
        .build() {
            Ok(c) => c,
            Err(e) => return Err(format!("Failed to create HTTP client: {}", e)),
        };
    
    // Make the request
    debug!("Making request to TheArtistDB API for MBID {}", mbid);
    let response = match client.get(&url).send() {
        Ok(resp) => resp,
        Err(e) => return Err(format!("Failed to send request to TheArtistDB: {}", e)),
    };
    
    // Check if the request was successful
    if !response.status().is_success() {
        return Err(format!(
            "TheArtistDB API returned error code: {}", 
            response.status()
        ));
    }
    
    // Parse the response as JSON
    match response.json::<Value>() {
        Ok(json_data) => {
            // Check if the artists array exists, is not empty, and contains exactly one artist
            if let Some(artists) = json_data.get("artists") {
                if artists.is_null() {
                    debug!("No artist data found for MBID {}", mbid);
                    return Err(format!("No artist found with MBID {}", mbid));
                }
                
                if let Some(artists_array) = artists.as_array() {
                    match artists_array.len() {
                        0 => {
                            debug!("Empty artists array for MBID {}", mbid);
                            return Err(format!("No artist found with MBID {}", mbid));
                        },
                        1 => {
                            debug!("Successfully retrieved artist data for MBID {}", mbid);
                            // Return just the artist object, not the whole array
                            return Ok(artists_array[0].clone());
                        },
                        n => {
                            debug!("Found {} artists for MBID {}, expected exactly 1", n, mbid);
                            return Err(format!("Found {} artists for MBID {}, expected exactly 1", n, mbid));
                        }
                    }
                } else {
                    debug!("Invalid artists field format from TheArtistDB");
                    return Err("Invalid response format from TheArtistDB (artists is not an array)".to_string());
                }
            } else {
                debug!("Invalid response format from TheArtistDB (no artists field)");
                return Err("Invalid response format from TheArtistDB (no artists field)".to_string());
            }
        },
        Err(e) => Err(format!("Failed to parse TheArtistDB response: {}", e))
    }
}

/// Download artist thumbnail from TheArtistDB
/// 
/// This function downloads the artist thumbnail from TheArtistDB if available
/// and stores it in the image cache following the naming convention:
/// - artistdb.0.xxx for the main thumbnail
/// 
/// # Arguments
/// * `mbid` - MusicBrainz ID of the artist
/// * `artist_name` - Name of the artist for caching
/// 
/// # Returns
/// * `bool` - true if the download was successful, false otherwise
pub fn download_artist_thumbnail(mbid: &str, artist_name: &str) -> bool {
    if !is_enabled() {
        debug!("TheArtistDB lookups are disabled, skipping thumbnail download");
        return false;
    }

    let artist_basename = crate::helpers::artistupdater::artist_basename(artist_name);

    // Check if the thumbnail already exists
    let thumb_path = format!("artists/{}/artistdb.0", artist_basename);
    if crate::helpers::fanarttv::path_with_any_extension_exists(&thumb_path) {
        debug!("TheArtistDB thumbnail already exists for '{}', skipping download", artist_name);
        return true;
    }

    debug!("Attempting to download TheArtistDB thumbnail for artist '{}'", artist_name);

    // Lookup the artist by MBID to get the thumbnail URL
    match lookup_mbid(mbid) {
        Ok(artist_data) => {
            // Extract the thumbnail URL from the response
            if let Some(thumb_url) = artist_data.get("strArtistThumb").and_then(|v| v.as_str()) {
                if !thumb_url.is_empty() {
                    debug!("Found thumbnail URL for artist {}: {}", artist_name, thumb_url);
                    
                    // Download the thumbnail
                    match crate::helpers::fanarttv::download_image(thumb_url) {
                        Ok(image_data) => {
                            // Determine the file extension
                            let extension = crate::helpers::fanarttv::extract_extension_from_url(thumb_url);
                            
                            // Create the full path with extension
                            let full_path = format!("artists/{}/artistdb.0.{}", artist_basename, extension);
                            
                            // Store the image in the cache
                            if let Err(e) = imagecache::store_image(&full_path, &image_data) {
                                warn!("Failed to store TheArtistDB thumbnail for '{}': {}", artist_name, e);
                                return false;
                            } else {
                                info!("Stored TheArtistDB thumbnail for '{}'", artist_name);
                                return true;
                            }
                        },
                        Err(e) => {
                            warn!("Failed to download TheArtistDB thumbnail for '{}': {}", artist_name, e);
                            return false;
                        }
                    }
                } else {
                    debug!("Empty thumbnail URL for artist '{}' in TheArtistDB", artist_name);
                    return false;
                }
            } else {
                debug!("No thumbnail URL found for artist '{}' in TheArtistDB", artist_name);
                return false;
            }
        },
        Err(e) => {
            debug!("Failed to retrieve artist data from TheArtistDB for '{}': {}", artist_name, e);
            return false;
        }
    }
}

