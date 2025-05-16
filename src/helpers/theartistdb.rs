use std::sync::atomic::{AtomicBool, Ordering};
use log::{info, debug, warn, error};
use lazy_static::lazy_static;
use std::sync::Mutex;
use serde_json::{Value};
use crate::helpers::http_client;
use crate::helpers::imagecache;
use crate::helpers::attributecache;
use crate::helpers::ratelimit;
use crate::data::artist::Artist;
use crate::helpers::artistupdater::ArtistUpdater;
use crate::helpers::sanitize::filename_from_string;

/// Global flag to indicate if TheArtistDB lookups are enabled
static THEARTISTDB_ENABLED: AtomicBool = AtomicBool::new(false);

// Provider name for image naming
const PROVIDER: &str = "artistdb";

/// Create a new HTTP client with a timeout of 10 seconds
fn new_client() -> Box<dyn http_client::HttpClient> {
    http_client::new_http_client(10)
}

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
          // Register rate limit - default to 2 requests per second (500ms)
        let rate_limit_ms = artistdb_config.get("rate_limit_ms")
            .and_then(|v| v.as_u64())
            .unwrap_or(500);
            
        ratelimit::register_service("theartistdb", rate_limit_ms);
        info!("TheArtistDB rate limit set to {} ms", rate_limit_ms);
        
        let status = if enabled { "enabled" } else { "disabled" };
        info!("TheArtistDB lookup {}", status);
    } else {
        // Default to disabled if not in config
        THEARTISTDB_ENABLED.store(false, Ordering::SeqCst);
        debug!("TheArtistDB configuration not found, lookups disabled");
        
        // Register default rate limit even if disabled
        ratelimit::register_service("theartistdb", 500);
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
pub fn lookup_artistdb_by_mbid(mbid: &str) -> Result<serde_json::Value, String> {
    if !is_enabled() {
        return Err("TheArtistDB lookups are disabled".to_string());
    }
    
    // Create cache keys for both positive and negative results
    let cache_key = format!("theartistdb::mbid::{}", mbid);
    let not_found_cache_key = format!("theartistdb::not_found::{}", mbid);
    
    // Check if we have a positive result cached
    match attributecache::get::<Value>(&cache_key) {
        Ok(Some(artist_data)) => {
            debug!("Found cached TheArtistDB data for MBID {}", mbid);
            return Ok(artist_data);
        },
        Ok(None) => {
            debug!("No cached TheArtistDB data found for MBID {}", mbid);
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
                return Err("No API key configured for TheArtistDB".to_string());
            }
            key
        },
        None => return Err("No API key configured for TheArtistDB".to_string()),
    };    debug!("Looking up artist with MBID {}", mbid);
    
    // Apply rate limiting before making the request
    ratelimit::rate_limit("theartistdb");
    
    // Construct the API URL
    let url = format!(
        "https://www.theaudiodb.com/api/v1/json/{}/artist-mb.php?i={}", 
        api_key, 
        mbid
    );
    
    // Create a client with our http_client
    let client = new_client();
    
    // Make the request
    debug!("Making request to TheArtistDB API for MBID {}", mbid);
    let response_text = match client.get_text(&url) {
        Ok(text) => text,
        Err(e) => return Err(format!("Failed to send request to TheArtistDB: {}", e)),
    };
      // Parse the response as JSON
    match serde_json::from_str::<Value>(&response_text) {
        Ok(json_data) => {
            // Check if the artists array exists, is not empty, and contains exactly one artist
            if let Some(artists) = json_data.get("artists") {
                if artists.is_null() {
                    debug!("No artist data found for MBID {}", mbid);
                    // Cache negative result
                    let not_found_cache_key = format!("theartistdb::not_found::{}", mbid);
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
                            let not_found_cache_key = format!("theartistdb::not_found::{}", mbid);
                            if let Err(e) = attributecache::set(&not_found_cache_key, &true) {
                                debug!("Failed to cache negative result for MBID {}: {}", mbid, e);
                            } else {
                                debug!("Cached negative result for MBID {}", mbid);
                            }
                            return Err(format!("No artist found with MBID {}", mbid));
                        },
                        1 => {
                            debug!("Successfully retrieved artist data for MBID {}", mbid);
                            let artist_data = artists_array[0].clone();
                            
                            // Cache the positive result
                            let cache_key = format!("theartistdb::mbid::{}", mbid);
                            if let Err(e) = attributecache::set(&cache_key, &artist_data) {
                                debug!("Failed to cache artist data for MBID {}: {}", mbid, e);
                            } else {
                                debug!("Cached positive result for MBID {}", mbid);
                            }
                            
                            // Return just the artist object, not the whole array
                            return Ok(artist_data);
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
/// - artist.artistdb.0.xxx for the main thumbnail
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
    
    // Create a cache key for tracking artists with no thumbnails
    let no_thumbnail_cache_key = format!("theartistdb::no_thumbnail::{}", mbid);
    
    // Check if we previously determined this artist has no thumbnail
    match attributecache::get::<bool>(&no_thumbnail_cache_key) {
        Ok(Some(true)) => {
            debug!("Artist '{}' previously marked as having no thumbnail in cache", artist_name);
            return false;
        },
        _ => {
            // Continue with lookup if not marked as no thumbnail or error reading cache
        }
    }

    let artist_basename = filename_from_string(artist_name);

    // Check if the thumbnail already exists
    let thumb_base_path = format!("artists/{}/artist", artist_basename);
    let existing_thumbs = imagecache::count_provider_files(&thumb_base_path, PROVIDER);
    
    if existing_thumbs > 0 {
        debug!("Artist already has {} thumbnails from {}, skipping download", existing_thumbs, PROVIDER);
        return true;
    }

    debug!("Attempting to download TheArtistDB thumbnail for artist '{}'", artist_name);

    // Lookup the artist by MBID to get the thumbnail URL
    match lookup_artistdb_by_mbid(mbid) {
        Ok(artist_data) => {
            // Extract the thumbnail URL from the response
            if let Some(thumb_url) = artist_data.get("strArtistThumb").and_then(|v| v.as_str()) {
                if !thumb_url.is_empty() {
                    debug!("Found thumbnail URL for artist {}: {}", artist_name, thumb_url);
                    
                    // Download the thumbnail using our helper function
                    match crate::helpers::fanarttv::download_image(thumb_url) {
                        Ok(image_data) => {
                            // Determine the file extension
                            let extension = crate::helpers::fanarttv::extract_extension_from_url(thumb_url);
                            
                            // Create the full path with extension using the new naming convention
                            let full_path = format!("artists/{}/artist.{}.{}.{}", 
                                                  artist_basename, 
                                                  PROVIDER, 
                                                  0,
                                                  extension);
                            
                            // Store the image in the cache
                            if let Err(e) = imagecache::store_image(&full_path, &image_data) {
                                warn!("Failed to store TheArtistDB thumbnail for '{}': {}", artist_name, e);
                                return false;
                            } else {
                                info!("Stored TheArtistDB thumbnail for '{}'", artist_name);
                                return true;
                            }
                        },                        Err(e) => {
                            warn!("Failed to download TheArtistDB thumbnail for '{}': {}", artist_name, e);
                            // Don't cache this as a negative result since it might be a temporary network issue
                            return false;
                        }
                    }
                } else {
                    debug!("Empty thumbnail URL for artist '{}' in TheArtistDB", artist_name);
                    // Cache this as a negative result
                    let no_thumbnail_cache_key = format!("theartistdb::no_thumbnail::{}", mbid);
                    if let Err(e) = attributecache::set(&no_thumbnail_cache_key, &true) {
                        debug!("Failed to cache no thumbnail result for artist '{}': {}", artist_name, e);
                    } else {
                        debug!("Cached no thumbnail result for artist '{}'", artist_name);
                    }
                    return false;
                }
            } else {
                debug!("No thumbnail URL found for artist '{}' in TheArtistDB", artist_name);
                // Cache this as a negative result
                let no_thumbnail_cache_key = format!("theartistdb::no_thumbnail::{}", mbid);
                if let Err(e) = attributecache::set(&no_thumbnail_cache_key, &true) {
                    debug!("Failed to cache no thumbnail result for artist '{}': {}", artist_name, e);
                } else {
                    debug!("Cached no thumbnail result for artist '{}'", artist_name);
                }
                return false;
            }
        },
        Err(e) => {
            debug!("Failed to retrieve artist data from TheArtistDB for '{}': {}", artist_name, e);
            // This error is likely already cached as a negative result in lookup_artistdb_by_mbid
            return false;
        }
    }
}

/// Implement the ArtistUpdater trait for TheArtistDB
pub struct TheArtistDbUpdater;

impl TheArtistDbUpdater {
    pub fn new() -> Self {
        TheArtistDbUpdater
    }
}

impl ArtistUpdater for TheArtistDbUpdater {
    /// Updates artist information using TheArtistDB service
    /// 
    /// This function fetches artist information from TheArtistDB using the MusicBrainz ID
    /// from the artist's metadata and updates the artist with thumbnail URLs and other
    /// available metadata.
    /// 
    /// # Arguments
    /// * `artist` - The artist to update
    /// 
    /// # Returns
    /// The updated artist with information from TheArtistDB
    fn update_artist(&self, mut artist: Artist) -> Artist {
        // Check if TheArtistDB lookups are enabled
        if !is_enabled() {
            debug!("TheArtistDB lookups are disabled, skipping artist {}", artist.name);
            return artist;
        }
        
        // Extract and clone the MusicBrainz ID to avoid borrowing issues
        let mbid_opt = artist.metadata.as_ref()
            .and_then(|meta| meta.mbid.first())
            .cloned();
        
        // Proceed only if a MusicBrainz ID is available
        if let Some(mbid) = mbid_opt {
            debug!("Looking up artist information in TheArtistDB for {} with MBID {}", artist.name, mbid);
            
            // Check if we already know this artist has no thumbnail
            let no_thumbnail_cache_key = format!("theartistdb::no_thumbnail::{}", mbid);
            match attributecache::get::<bool>(&no_thumbnail_cache_key) {
                Ok(Some(true)) => {
                    debug!("Artist '{}' previously marked as having no thumbnail in cache, skipping", artist.name);
                    return artist;
                },
                _ => {
                    // Continue with lookup if not marked as no thumbnail or error reading cache
                }
            }
            
            // Lookup artist by MBID
            match lookup_artistdb_by_mbid(&mbid) {
                Ok(artist_data) => {
                    debug!("Successfully retrieved artist data from TheArtistDB for {}", artist.name);
                    
                    // Extract the artist thumbnail URL
                    if let Some(thumb_url) = artist_data.get("strArtistThumb").and_then(|v| v.as_str()) {
                        if !thumb_url.is_empty() {
                            debug!("Found thumbnail URL for artist {}: {}", artist.name, thumb_url);
                            
                            // Ensure we have a metadata struct
                            if artist.metadata.is_none() {
                                artist.ensure_metadata();
                            }
                            
                            // Add the thumbnail URL to the artist metadata
                            if let Some(meta) = &mut artist.metadata {
                                meta.thumb_url.push(thumb_url.to_string());
                                info!("Added TheArtistDB thumbnail URL for artist {}", artist.name);
                            }
                            
                            // Download and cache the thumbnail
                            if download_artist_thumbnail(&mbid, &artist.name) {
                                debug!("Successfully downloaded and cached thumbnail for artist {}", artist.name);
                            } else {
                                debug!("Failed to download thumbnail for artist {}", artist.name);
                            }
                        } else {
                            debug!("Empty thumbnail URL from TheArtistDB for artist {}", artist.name);
                            // Cache that this artist has no thumbnail
                            if let Err(e) = attributecache::set(&no_thumbnail_cache_key, &true) {
                                debug!("Failed to cache no thumbnail result for artist '{}': {}", artist.name, e);
                            } else {
                                debug!("Cached no thumbnail result for artist '{}'", artist.name);
                            }
                        }
                    } else {
                        debug!("No thumbnail available from TheArtistDB for artist {}", artist.name);
                        // Cache that this artist has no thumbnail
                        if let Err(e) = attributecache::set(&no_thumbnail_cache_key, &true) {
                            debug!("Failed to cache no thumbnail result for artist '{}': {}", artist.name, e);
                        } else {
                            debug!("Cached no thumbnail result for artist '{}'", artist.name);
                        }
                    }
                    
                    // Extract additional artist metadata that could be useful
                    if let Some(biography) = artist_data.get("strBiographyEN").and_then(|v| v.as_str()) {
                        if !biography.is_empty() {
                            if let Some(meta) = &mut artist.metadata {
                                meta.biography = Some(biography.to_string());
                                debug!("Added biography from TheArtistDB for artist {}", artist.name);
                            }
                        }
                    }
                    
                    // Extract genre information
                    if let Some(genre) = artist_data.get("strGenre").and_then(|v| v.as_str()) {
                        if !genre.is_empty() {
                            if let Some(meta) = &mut artist.metadata {
                                meta.genres.push(genre.to_string());
                                debug!("Added genre '{}' from TheArtistDB for artist {}", genre, artist.name);
                            }
                        }
                    }
                },
                Err(e) => {
                    info!("Failed to retrieve artist data from TheArtistDB for {} with MBID {}: {}", artist.name, mbid, e);
                    // This error is likely already cached as a negative result in lookup_artistdb_by_mbid
                }
            }
        } else {
            debug!("No MusicBrainz ID available for artist {}, skipping TheArtistDB lookup", artist.name);
        }
        
        artist
    }
}

