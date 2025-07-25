use serde_json::Value;
use log::{debug, warn, info};
use crate::helpers::http_client;
use crate::helpers::imagecache;
use crate::data::artist::Artist;
use crate::helpers::ArtistUpdater;
use crate::helpers::sanitize::filename_from_string;
use moka::sync::Cache;
use std::time::Duration;
use lazy_static::lazy_static;

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

// API key for fanart.tv
const APIKEY: &str = "749a8fca4f2d3b0462b287820ad6ab06";
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
    // Check negative cache for failed lookups
    if FAILED_MBID_CACHE.get(artist_mbid).is_some() {
        debug!("MBID '{}' found in negative cache (previous FanArt.tv lookup failed)", artist_mbid);
        return Vec::new();
    }

    let max = max_images.unwrap_or(10);
    let url = format!(
        "http://webservice.fanart.tv/v3/music/{}?api_key={}", 
        artist_mbid,
        APIKEY
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
    // Check negative cache for failed lookups
    if FAILED_MBID_CACHE.get(artist_mbid).is_some() {
        debug!("MBID '{}' found in negative cache (previous FanArt.tv lookup failed)", artist_mbid);
        return Vec::new();
    }

    let url = format!(
        "http://webservice.fanart.tv/v3/music/{}?api_key={}", 
        artist_mbid,
        APIKEY
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

#[cfg(test)]
mod tests {
    // ...existing code...
}
