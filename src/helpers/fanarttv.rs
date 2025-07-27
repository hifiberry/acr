use serde_json::Value;
use log::{debug, warn, info, error};
use crate::helpers::http_client;
use crate::helpers::imagecache;
use crate::data::artist::Artist;
use crate::helpers::ArtistUpdater;
use crate::helpers::sanitize::filename_from_string;
use crate::helpers::coverart::{CoverartProvider, CoverartMethod};
use moka::sync::Cache;
use std::time::Duration;
use std::collections::HashSet;
use lazy_static::lazy_static;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use crate::config::get_service_config;
use crate::helpers::ratelimit;

/// Global flag to indicate if FanArt.tv lookups are enabled
static FANARTTV_ENABLED: AtomicBool = AtomicBool::new(false);

/// API key storage for FanArt.tv
#[derive(Default)]
struct FanarttvConfig {
    api_key: String,
}

// Default API key for FanArt.tv
pub fn default_fanarttv_api_key() -> String {
    "749a8fca4f2d3b0462b287820ad6ab06".to_string()
}

// Global singleton for FanArt.tv configuration
lazy_static! {
    static ref FANARTTV_CONFIG: Mutex<FanarttvConfig> = Mutex::new(FanarttvConfig::default());
}

/// Initialize FanArt.tv module from configuration
pub fn initialize_from_config(config: &serde_json::Value) {    
    if let Some(fanarttv_config) = get_service_config(config, "fanarttv") {
        // Check if enabled flag exists and is set to true
        let enabled = fanarttv_config.get("enable")
            .and_then(|v| v.as_bool())
            .unwrap_or(true); // Default to enabled if not specified
        
        FANARTTV_ENABLED.store(enabled, Ordering::SeqCst);
        
        // Get API key if provided
        if let Some(api_key) = fanarttv_config.get("api_key").and_then(|v| v.as_str()) {
            if let Ok(mut config) = FANARTTV_CONFIG.lock() {
                debug!("Found FanArt.tv API key in config: {}", 
                       if !api_key.is_empty() && api_key.len() > 4 { 
                           format!("{}...", &api_key[0..4]) 
                       } else { 
                           "Empty".to_string() 
                       });
                
                config.api_key = api_key.to_string();
                if !api_key.is_empty() {
                    info!("FanArt.tv API key configured");
                } else {
                    // Use the default key
                    let default_key = default_fanarttv_api_key();
                    config.api_key = default_key;
                    info!("Using default FanArt.tv API key");
                }
            } else {
                error!("Failed to acquire lock on FanArt.tv configuration");
            }
        } else {
            // Use default API key if none provided
            if let Ok(mut config) = FANARTTV_CONFIG.lock() {
                let default_key = default_fanarttv_api_key();
                config.api_key = default_key;
                debug!("No API key found for FanArt.tv in configuration, using default");
            }
        }
        
        // Register rate limit - default to 2 requests per second (500ms)
        let rate_limit_ms = fanarttv_config.get("rate_limit_ms")
            .and_then(|v| v.as_u64())
            .unwrap_or(500);
            
        ratelimit::register_service("fanarttv", rate_limit_ms);
        info!("FanArt.tv rate limit set to {} ms", rate_limit_ms);
        
        let status = if enabled { "enabled" } else { "disabled" };
        info!("FanArt.tv lookup {}", status);
    } else {
        // Default to enabled if not in config, with default API key
        FANARTTV_ENABLED.store(true, Ordering::SeqCst);
        if let Ok(mut config) = FANARTTV_CONFIG.lock() {
            let default_key = default_fanarttv_api_key();
            config.api_key = default_key;
        }
        debug!("FanArt.tv configuration not found, using defaults (enabled with default API key)");
        
        // Register default rate limit
        ratelimit::register_service("fanarttv", 500);
    }
}

/// Check if FanArt.tv lookups are enabled
pub fn is_enabled() -> bool {
    FANARTTV_ENABLED.load(Ordering::SeqCst)
}

/// Get the configured API key
pub fn get_api_key() -> Option<String> {
    if let Ok(config) = FANARTTV_CONFIG.lock() {
        if !config.api_key.is_empty() {
            Some(config.api_key.clone())
        } else {
            None
        }
    } else {
        None
    }
}

// Using lazy_static for failed MBID cache with 24-hour expiry  
lazy_static! {
    static ref FAILED_MBID_CACHE: Cache<String, bool> = {
        Cache::builder()
            // Set a 24-hour time-to-live (TTL)
            .time_to_live(Duration::from_secs(24 * 60 * 60))
            // Set a maximum capacity for the cache
            .max_capacity(1000) 
            .build()
    };
}

// Provider name for image naming
const PROVIDER: &str = "fanarttv";

/// Create a new HTTP client with a timeout of 10 seconds
fn http_client() -> Box<dyn http_client::HttpClient> {
    http_client::new_http_client(10)
}

/// Get artist thumbnail URLs from FanArt.tv
/// 
/// # Arguments
/// * `artist_mbid` - MusicBrainz ID of the artist
/// * `max_images` - Maximum number of images to return (default: 10)
/// 
/// # Returns
/// * `Vec<String>` - URLs of all available thumbnails, empty if none found
pub fn get_artist_thumbnails(artist_mbid: &str, max_images: Option<usize>) -> Vec<String> {
    // Check if FanArt.tv is enabled
    if !is_enabled() {
        debug!("FanArt.tv lookups are disabled");
        return Vec::new();
    }

    // Get the configured API key
    let api_key = match get_api_key() {
        Some(key) => key,
        None => {
            warn!("No FanArt.tv API key configured");
            return Vec::new();
        }
    };

    // Check negative cache for failed lookups
    if FAILED_MBID_CACHE.get(artist_mbid).is_some() {
        debug!("MBID '{}' found in negative cache (previous FanArt.tv lookup failed)", artist_mbid);
        return Vec::new();
    }

    let max = max_images.unwrap_or(10);
    let url = format!(
        "http://webservice.fanart.tv/v3/music/{}?api_key={}", 
        artist_mbid,
        api_key
    );

    let mut thumbnail_urls = Vec::new();
    
    let client = http_client();
    match client.get_text(&url) {
        Ok(response_text) => {
            // Parse the JSON response
            match serde_json::from_str::<Value>(&response_text) {
                Ok(data) => {
                    // Look for artist thumbnails
                    if let Some(artist_thumbs) = data.get("artistthumb").and_then(|t| t.as_array()) {
                        for thumb in artist_thumbs {
                            if let Some(url) = thumb.get("url").and_then(|u| u.as_str()) {
                                thumbnail_urls.push(url.to_string());
                                if thumbnail_urls.len() >= max {
                                    break;
                                }
                            }
                        }
                        
                        if !thumbnail_urls.is_empty() {
                            debug!("Found {} artist thumbnails on fanart.tv (limited to max {})", thumbnail_urls.len(), max);
                        } else {
                            debug!("Found no artist thumbnails on fanart.tv for MBID {}", artist_mbid);
                            // Add to negative cache if no thumbnails found
                            FAILED_MBID_CACHE.insert(artist_mbid.to_string(), true);
                        }
                    } else {
                        debug!("No artistthumb data found on fanart.tv for MBID {}", artist_mbid);
                        // Add to negative cache if no artistthumb section found  
                        FAILED_MBID_CACHE.insert(artist_mbid.to_string(), true);
                    }
                }
                Err(e) => {
                    warn!("Failed to parse JSON from fanart.tv for MBID {}: {}", artist_mbid, e);
                    // Add to negative cache on parse error
                    FAILED_MBID_CACHE.insert(artist_mbid.to_string(), true);
                }
            }
        }
        Err(e) => {
            debug!("GET request failed: {}: status code 404", e);
            // Add to negative cache on request failure (includes 404)
            FAILED_MBID_CACHE.insert(artist_mbid.to_string(), true);
        }
    }

    thumbnail_urls
}

/// Get artist banner URLs from FanArt.tv
/// 
/// # Arguments
/// * `artist_mbid` - MusicBrainz ID of the artist
/// 
/// # Returns
/// * `Vec<String>` - URLs of all available banners, empty if none found
pub fn get_artist_banners(artist_mbid: &str) -> Vec<String> {
    // Check if FanArt.tv is enabled
    if !is_enabled() {
        debug!("FanArt.tv lookups are disabled");
        return Vec::new();
    }

    // Get the configured API key
    let api_key = match get_api_key() {
        Some(key) => key,
        None => {
            warn!("No FanArt.tv API key configured");
            return Vec::new();
        }
    };

    // Check negative cache for failed lookups
    if FAILED_MBID_CACHE.get(artist_mbid).is_some() {
        debug!("MBID '{}' found in negative cache (previous FanArt.tv lookup failed)", artist_mbid);
        return Vec::new();
    }

    let url = format!(
        "http://webservice.fanart.tv/v3/music/{}?api_key={}", 
        artist_mbid,
        api_key
    );

    let mut banner_urls = Vec::new();
    
    let client = http_client();
    match client.get_text(&url) {
        Ok(response_text) => {
            // Parse the JSON response
            match serde_json::from_str::<Value>(&response_text) {
                Ok(data) => {
                    // Look for artist banners
                    if let Some(artist_banners) = data.get("musicbanner").and_then(|b| b.as_array()) {
                        for banner in artist_banners {
                            if let Some(url) = banner.get("url").and_then(|u| u.as_str()) {
                                banner_urls.push(url.to_string());
                            }
                        }
                        
                        if !banner_urls.is_empty() {
                            debug!("Found {} artist banners on fanart.tv", banner_urls.len());
                        } else {
                            debug!("Found no artist banners on fanart.tv for MBID {}", artist_mbid);
                            // Add to negative cache if no banners found
                            FAILED_MBID_CACHE.insert(artist_mbid.to_string(), true);
                        }
                    } else {
                        debug!("No musicbanner data found on fanart.tv for MBID {}", artist_mbid);
                        // Add to negative cache if no musicbanner section found
                        FAILED_MBID_CACHE.insert(artist_mbid.to_string(), true);
                    }
                }
                Err(e) => {
                    warn!("Failed to parse JSON from fanart.tv for MBID {}: {}", artist_mbid, e);
                    // Add to negative cache on parse error
                    FAILED_MBID_CACHE.insert(artist_mbid.to_string(), true);
                }
            }
        }
        Err(e) => {
            debug!("GET request failed: {}: status code 404", e);
            // Add to negative cache on request failure (includes 404)
            FAILED_MBID_CACHE.insert(artist_mbid.to_string(), true);
        }
    }

    banner_urls
}

/// Download artist thumbnails and banners from FanArt.tv
/// 
/// This function follows the naming convention:
/// - artist.fanarttv.0.xxx, artist.fanarttv.1.xxx, etc. for thumbnails
/// - banner.fanarttv.0.xxx, banner.fanarttv.1.xxx, etc. for banners
/// 
/// If images of the type already exist, it won't download more images of that type.
/// 
/// # Arguments
/// * `artist_mbid` - MusicBrainz ID of the artist
/// * `artist_name` - Name of the artist for caching
/// 
/// # Returns
/// * `bool` - true if the API call was successful (even if no images were found), false only on API error
pub fn download_artist_images(artist_mbid: &str, artist_name: &str) -> bool {
    let artist_basename = filename_from_string(artist_name);
    let mut _thumb_downloaded = false;
    let mut _banner_downloaded = false;
    let mut api_success = true; // Track overall API success
    
    // Check if thumbnails already exist
    let thumb_base_path = format!("artists/{}/artist", artist_basename);
    let existing_thumbs = imagecache::count_provider_files(&thumb_base_path, PROVIDER);
    
    if existing_thumbs == 0 {
        // Download all thumbnails
        let thumbnail_urls = get_artist_thumbnails(artist_mbid, None);
        if thumbnail_urls.is_empty() {
            debug!("No thumbnails found on fanart.tv for '{}'", artist_name);
            // This is still considered a success since the API call succeeded
        }
        
        for (i, url) in thumbnail_urls.iter().enumerate() {
            let path = format!("artists/{}/artist.{}.{}.{}", 
                               artist_basename,
                               PROVIDER, 
                               i,
                               extract_extension_from_url(url));
            
            match download_image(url) {
                Ok(image_data) => {
                    if let Err(e) = imagecache::store_image(&path, &image_data) {
                        warn!("Failed to store artist thumbnail: {}", e);
                        api_success = false;
                    } else {
                        info!("Stored artist thumbnail {} for '{}'", i, artist_name);
                        _thumb_downloaded = true;
                    }
                },
                Err(e) => {
                    warn!("Failed to download artist thumbnail: {}", e);
                    api_success = false;
                }
            }
        }
    } else {
        debug!("Artist already has {} thumbnails from {}, skipping download", existing_thumbs, PROVIDER);
    }
    
    // Check if banners already exist
    let banner_base_path = format!("artists/{}/banner", artist_basename);
    let existing_banners = imagecache::count_provider_files(&banner_base_path, PROVIDER);
    
    if existing_banners == 0 {
        // Download all banners
        let banner_urls = get_artist_banners(artist_mbid);
        if banner_urls.is_empty() {
            debug!("No banners found on fanart.tv for '{}'", artist_name);
            // This is still considered a success since the API call succeeded
        }
        
        for (i, url) in banner_urls.iter().enumerate() {
            let path = format!("artists/{}/banner.{}.{}.{}", 
                               artist_basename,
                               PROVIDER, 
                               i,
                               extract_extension_from_url(url));
            
            match download_image(url) {
                Ok(image_data) => {
                    if let Err(e) = imagecache::store_image(&path, &image_data) {
                        warn!("Failed to store artist banner: {}", e);
                        api_success = false;
                    } else {
                        info!("Stored artist banner {} for '{}'", i, artist_name);
                        _banner_downloaded = true;
                    }
                },
                Err(e) => {
                    warn!("Failed to download artist banner: {}", e);
                    api_success = false;
                }
            }
        }
    } else {
        debug!("Artist already has {} banners from {}, skipping download", existing_banners, PROVIDER);
    }
    
    // Return api_success instead of thumb_downloaded || banner_downloaded
    // This allows the function to return true even if no images were found,
    // as long as the API call itself was successful
    api_success
}

/// Download an image from a URL
/// 
/// # Arguments
/// * `url` - URL of the image to download
/// 
/// # Returns
/// * `Result<Vec<u8>, String>` - The image data if successful, otherwise an error message
pub fn download_image(url: &str) -> Result<Vec<u8>, String> {
    debug!("Downloading image from URL: {}", url);
    
    // Create a client with appropriate timeout
    let client = http_client();
    
    // Execute the request
    match client.get_binary(url) {
        Ok((binary_data, _)) => {
            // Return the binary data directly
            Ok(binary_data)
        },
        Err(e) => Err(format!("Request failed: {}", e))
    }
}

/// Extract file extension from a URL
///
/// # Arguments
/// * `url` - URL to extract extension from
///
/// # Returns
/// * `String` - The file extension (e.g., "jpg") or "jpg" as default
pub fn extract_extension_from_url(url: &str) -> String {
    url.split('.')
        .last()
        .and_then(|ext| {
            // Remove any query parameters
            let clean_ext = ext.split('?').next().unwrap_or(ext);
            if clean_ext.len() <= 4 {
                Some(clean_ext.to_lowercase())
            } else {
                None
            }
        })
        .unwrap_or("jpg".to_string())
}

/// Implement the ArtistUpdater trait for FanArt.tv
pub struct FanarttvUpdater;

impl FanarttvUpdater {
    pub fn new() -> Self {
        FanarttvUpdater
    }
}

impl ArtistUpdater for FanarttvUpdater {
    /// Updates artist information using FanArt.tv service
    /// 
    /// This function fetches thumbnail URLs for an artist and downloads them for caching.
    /// First checks if images already exist for this provider, and if so, skips fetching.
    /// 
    /// # Arguments
    /// * `artist` - The artist to update
    /// 
    /// # Returns
    /// The updated artist with thumbnail URLs
    fn update_artist(&self, mut artist: Artist) -> Artist {
        // Extract and clone the MusicBrainz ID to avoid borrowing issues
        let mbid_opt = artist.metadata.as_ref()
            .and_then(|meta| meta.mbid.first())
            .cloned();
            
        // Proceed only if a MusicBrainz ID is available
        if let Some(mbid) = mbid_opt {
            let artist_basename = filename_from_string(&artist.name);
            
            // Check if we already have cached images for this artist from our provider
            let thumb_base_path = format!("artists/{}/artist", artist_basename);
            let existing_thumbs = imagecache::count_provider_files(&thumb_base_path, PROVIDER);
            
            if existing_thumbs > 0 {
                debug!("Artist {} already has {} thumbnail(s) from {}, skipping fetch", 
                      artist.name, existing_thumbs, PROVIDER);
                
                // We already have images, no need to fetch URLs or download again
                return artist;
            }
            
            debug!("Fetching thumbnail URLs for artist {} with MBID {}", artist.name, mbid);
            
            // Get thumbnail URLs from FanArt.tv
            let thumbnail_urls = get_artist_thumbnails(&mbid, Some(5));
            
            // Check if we have any thumbnails before trying to add them
            let has_thumbnails = !thumbnail_urls.is_empty();
            
            // Add each thumbnail URL to the artist
            if let Some(meta) = &mut artist.metadata {
                for url in &thumbnail_urls {
                    meta.thumb_url.push(url.clone());
                    debug!("Added thumbnail URL for artist {}", artist.name);
                }
            }
            
            // If thumbnails were found, also try to download them for caching
            if has_thumbnails {
                debug!("Downloading artist images for {}", artist.name);
                let download_result = download_artist_images(&mbid, &artist.name);
                if download_result {
                    debug!("Successfully downloaded images for artist {}", artist.name);
                } else {
                    debug!("Failed to download some images for artist {}", artist.name);
                }
            }
        } else {
            debug!("No MusicBrainz ID available for artist {}, skipping FanArt.tv lookup", artist.name);
        }
        
        artist
    }
}

/// Implement the CoverartProvider trait for FanArt.tv
impl CoverartProvider for FanarttvUpdater {
    /// Returns the internal name identifier for this provider
    fn name(&self) -> &str {
        "fanarttv"
    }
    
    /// Returns the human-readable display name for this provider
    fn display_name(&self) -> &str {
        "FanArt.tv"
    }
    
    /// Returns the set of cover art methods this provider supports
    fn supported_methods(&self) -> HashSet<CoverartMethod> {
        let mut methods = HashSet::new();
        methods.insert(CoverartMethod::Artist);
        methods
    }
    
    /// Implementation for artist cover art retrieval
    /// Returns thumbnail URLs for the given artist
    fn get_artist_coverart_impl(&self, artist: &str) -> Vec<String> {
        debug!("FanArt.tv: Getting cover art for artist '{}'", artist);
        
        // For FanArt.tv, we need the MusicBrainz ID to make API calls
        // Since we only have the artist name, we can't directly query the API
        // This would typically require a MusicBrainz lookup first
        // For now, we'll return an empty vector and log a debug message
        
        debug!("FanArt.tv: Artist cover art retrieval requires MusicBrainz ID, not available from artist name alone");
        Vec::new()
    }
}

/// A dedicated CoverArt provider for FanArt.tv that includes MusicBrainz integration
pub struct FanarttvCoverartProvider;

impl FanarttvCoverartProvider {
    pub fn new() -> Self {
        FanarttvCoverartProvider
    }
    
    /// Helper function to get artist MusicBrainz ID by name
    /// This would typically integrate with a MusicBrainz lookup service
    fn get_artist_mbid(&self, artist_name: &str) -> Option<String> {
        // Placeholder for MusicBrainz integration
        // In a real implementation, this would lookup the artist MBID
        debug!("FanArt.tv: Would lookup MusicBrainz ID for artist '{}'", artist_name);
        None
    }
}

impl CoverartProvider for FanarttvCoverartProvider {
    /// Returns the internal name identifier for this provider
    fn name(&self) -> &str {
        "fanarttv_coverart"
    }
    
    /// Returns the human-readable display name for this provider
    fn display_name(&self) -> &str {
        "FanArt.tv Cover Art"
    }
    
    /// Returns the set of cover art methods this provider supports
    fn supported_methods(&self) -> HashSet<CoverartMethod> {
        let mut methods = HashSet::new();
        methods.insert(CoverartMethod::Artist);
        methods
    }
    
    /// Implementation for artist cover art retrieval
    /// Returns thumbnail URLs for the given artist by looking up their MusicBrainz ID
    fn get_artist_coverart_impl(&self, artist: &str) -> Vec<String> {
        debug!("FanArt.tv CoverArt: Getting cover art for artist '{}'", artist);
        
        // First, attempt to get the MusicBrainz ID for the artist
        if let Some(mbid) = self.get_artist_mbid(artist) {
            debug!("FanArt.tv CoverArt: Found MBID '{}' for artist '{}'", mbid, artist);
            
            // Get artist thumbnails using the MBID
            let thumbnails = get_artist_thumbnails(&mbid, Some(5));
            if !thumbnails.is_empty() {
                debug!("FanArt.tv CoverArt: Found {} thumbnails for artist '{}'", thumbnails.len(), artist);
                return thumbnails;
            } else {
                debug!("FanArt.tv CoverArt: No thumbnails found for artist '{}'", artist);
            }
        } else {
            debug!("FanArt.tv CoverArt: No MusicBrainz ID found for artist '{}'", artist);
        }
        
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::helpers::coverart::CoverartProvider;
    
    #[test]
    fn test_fanarttv_updater_coverart_provider_name() {
        let provider = FanarttvUpdater::new();
        assert_eq!(provider.name(), "fanarttv");
    }
    
    #[test]
    fn test_fanarttv_updater_coverart_provider_display_name() {
        let provider = FanarttvUpdater::new();
        assert_eq!(provider.display_name(), "FanArt.tv");
    }
    
    #[test]
    fn test_fanarttv_updater_supported_methods() {
        let provider = FanarttvUpdater::new();
        let methods = provider.supported_methods();
        assert_eq!(methods.len(), 1);
        assert!(methods.contains(&CoverartMethod::Artist));
        assert!(!methods.contains(&CoverartMethod::Song));
        assert!(!methods.contains(&CoverartMethod::Album));
        assert!(!methods.contains(&CoverartMethod::Url));
    }
    
    #[test]
    fn test_fanarttv_updater_get_artist_coverart_impl() {
        let provider = FanarttvUpdater::new();
        let result = provider.get_artist_coverart_impl("Test Artist");
        // Should return empty since we can't lookup MBID from name alone
        assert!(result.is_empty());
    }
    
    #[test]
    fn test_fanarttv_coverart_provider_name() {
        let provider = FanarttvCoverartProvider::new();
        assert_eq!(provider.name(), "fanarttv_coverart");
    }
    
    #[test]
    fn test_fanarttv_coverart_provider_display_name() {
        let provider = FanarttvCoverartProvider::new();
        assert_eq!(provider.display_name(), "FanArt.tv Cover Art");
    }
    
    #[test]
    fn test_fanarttv_coverart_provider_supported_methods() {
        let provider = FanarttvCoverartProvider::new();
        let methods = provider.supported_methods();
        assert_eq!(methods.len(), 1);
        assert!(methods.contains(&CoverartMethod::Artist));
        assert!(!methods.contains(&CoverartMethod::Song));
        assert!(!methods.contains(&CoverartMethod::Album));
        assert!(!methods.contains(&CoverartMethod::Url));
    }
    
    #[test]
    fn test_fanarttv_coverart_provider_get_artist_coverart_impl() {
        let provider = FanarttvCoverartProvider::new();
        let result = provider.get_artist_coverart_impl("Test Artist");
        // Should return empty since get_artist_mbid returns None (placeholder implementation)
        assert!(result.is_empty());
    }
    
    #[test]
    fn test_fanarttv_coverart_provider_get_artist_mbid() {
        let provider = FanarttvCoverartProvider::new();
        let result = provider.get_artist_mbid("Test Artist");
        // Should return None since it's a placeholder implementation
        assert!(result.is_none());
    }
    
    #[test]
    fn test_extract_extension_from_url() {
        assert_eq!(extract_extension_from_url("http://example.com/image.jpg"), "jpg");
        assert_eq!(extract_extension_from_url("http://example.com/image.png"), "png");
        assert_eq!(extract_extension_from_url("http://example.com/image.jpeg"), "jpeg");
        assert_eq!(extract_extension_from_url("http://example.com/image.gif"), "gif");
        assert_eq!(extract_extension_from_url("http://example.com/image.JPG"), "jpg");
        assert_eq!(extract_extension_from_url("http://example.com/image.png?size=large"), "png");
        assert_eq!(extract_extension_from_url("http://example.com/image.jpeg?quality=high&format=web"), "jpeg");
        assert_eq!(extract_extension_from_url("http://example.com/image"), "jpg"); // default fallback
        assert_eq!(extract_extension_from_url("http://example.com/image.verylongextension"), "jpg"); // too long, fallback
    }
    
    #[test]
    fn test_coverart_manager_integration() {
        use crate::helpers::coverart::CoverartManager;
        use std::sync::Arc;
        
        let mut manager = CoverartManager::new();
        
        // Register both FanArt.tv providers
        let fanarttv_updater = Arc::new(FanarttvUpdater::new());
        let fanarttv_coverart = Arc::new(FanarttvCoverartProvider::new());
        
        manager.register_provider(fanarttv_updater);
        manager.register_provider(fanarttv_coverart);
        
        // Test artist coverart retrieval (should return empty since no MusicBrainz lookup)
        let results = manager.get_artist_coverart("Test Artist");
        
        // Both providers should be called but return no results
        assert_eq!(results.len(), 0);
    }
}
