// Copyright (c) 2020 Modul 9/HiFiBerry
//
// Permission is hereby granted, free of charge, to any person obtaining a copy
// of this software and associated documentation files (the "Software"), to deal
// in the Software without restriction, including without limitation the rights
// to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
// copies of the Software, and to permit persons to whom the Software is
// furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in all
// copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
// OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
// SOFTWARE.

//! FanArt.tv API client for retrieving album and artist artwork
//! Based on the original Python implementation from audiocontrol2

use serde_json::Value;
use log::{debug, warn, info};
use reqwest;
use crate::helpers::imagecache;

// API key for fanart.tv
const APIKEY: &str = "749a8fca4f2d3b0462b287820ad6ab06";

/// Get the cover URL from FanArt.tv for an artist or album
/// 
/// # Arguments
/// * `artist_mbid` - MusicBrainz ID of the artist
/// * `album_mbid` - MusicBrainz ID of the album
/// * `allow_artist_picture` - If true, return artist picture if no album cover is found
/// 
/// # Returns
/// * `Option<String>` - URL of the cover if found, otherwise None
pub fn get_fanart_cover(artist_mbid: &str, album_mbid: Option<&str>, allow_artist_picture: bool) -> Option<String> {
    let url = format!(
        "http://webservice.fanart.tv/v3/music/{}?api_key={}", 
        artist_mbid,
        APIKEY
    );

    let client = reqwest::blocking::Client::new();
    match client.get(&url).send() {
        Ok(response) => {
            if !response.status().is_success() {
                debug!("Artist does not exist on fanart.tv (HTTP status: {})", response.status());
                return None;
            }

            match response.json::<Value>() {
                Ok(data) => {
                    // Try to find the album cover first if we have an album MBID
                    if let Some(album_id) = album_mbid {
                        if let Some(albums) = data.get("albums") {
                            if let Some(album) = albums.get(album_id) {
                                if let Some(album_cover) = album.get("albumcover") {
                                    if let Some(url) = album_cover.get("url")
                                        .and_then(|u| u.as_str()) {
                                        debug!("Found album cover on fanart.tv");
                                        return Some(url.to_string());
                                    }
                                }
                            }
                        }
                        debug!("Found no album cover on fanart.tv");
                    }
                    
                    // If no album cover exists and artist pictures are allowed, use artist thumb
                    if allow_artist_picture {
                        if let Some(artist_thumbs) = data.get("artistthumb") {
                            if let Some(thumb) = artist_thumbs.get(1) {
                                if let Some(url) = thumb.get("url")
                                    .and_then(|u| u.as_str()) {
                                    debug!("Found artist picture on fanart.tv");
                                    return Some(url.to_string());
                                }
                            }
                        }
                        debug!("Found no artist picture on fanart.tv");
                    }
                }
                Err(e) => {
                    warn!("Failed to parse JSON from fanart.tv: {}", e);
                }
            }
        }
        Err(e) => {
            warn!("Couldn't retrieve data from fanart.tv: {}", e);
        }
    }

    None
}

/// Get artist thumbnail URLs from FanArt.tv
/// 
/// # Arguments
/// * `artist_mbid` - MusicBrainz ID of the artist
/// 
/// # Returns
/// * `Vec<String>` - URLs of all available thumbnails, empty if none found
pub fn get_artist_thumbnails(artist_mbid: &str) -> Vec<String> {
    let url = format!(
        "http://webservice.fanart.tv/v3/music/{}?api_key={}", 
        artist_mbid,
        APIKEY
    );

    let mut thumbnail_urls = Vec::new();
    
    let client = reqwest::blocking::Client::new();
    match client.get(&url).send() {
        Ok(response) => {
            if !response.status().is_success() {
                debug!("Artist does not exist on fanart.tv (HTTP status: {})", response.status());
                return thumbnail_urls;
            }

            match response.json::<Value>() {
                Ok(data) => {
                    // Look for artist thumbnails
                    if let Some(artist_thumbs) = data.get("artistthumb").and_then(|t| t.as_array()) {
                        for thumb in artist_thumbs {
                            if let Some(url) = thumb.get("url").and_then(|u| u.as_str()) {
                                thumbnail_urls.push(url.to_string());
                            }
                        }
                        
                        if !thumbnail_urls.is_empty() {
                            debug!("Found {} artist thumbnails on fanart.tv", thumbnail_urls.len());
                        } else {
                            debug!("Found no artist thumbnails on fanart.tv");
                        }
                    }
                }
                Err(e) => {
                    warn!("Failed to parse JSON from fanart.tv: {}", e);
                }
            }
        }
        Err(e) => {
            warn!("Couldn't retrieve data from fanart.tv: {}", e);
        }
    }

    thumbnail_urls
}

/// Get artist thumbnail URL from FanArt.tv (legacy method returning only first thumbnail)
/// 
/// # Arguments
/// * `artist_mbid` - MusicBrainz ID of the artist
/// 
/// # Returns
/// * `Option<String>` - URL of the thumbnail if found, otherwise None
pub fn get_artist_thumbnail(artist_mbid: &str) -> Option<String> {
    let thumbnails = get_artist_thumbnails(artist_mbid);
    thumbnails.first().cloned()
}

/// Get artist banner URLs from FanArt.tv
/// 
/// # Arguments
/// * `artist_mbid` - MusicBrainz ID of the artist
/// 
/// # Returns
/// * `Vec<String>` - URLs of all available banners, empty if none found
pub fn get_artist_banners(artist_mbid: &str) -> Vec<String> {
    let url = format!(
        "http://webservice.fanart.tv/v3/music/{}?api_key={}", 
        artist_mbid,
        APIKEY
    );

    let mut banner_urls = Vec::new();
    
    let client = reqwest::blocking::Client::new();
    match client.get(&url).send() {
        Ok(response) => {
            if !response.status().is_success() {
                debug!("Artist does not exist on fanart.tv (HTTP status: {})", response.status());
                return banner_urls;
            }

            match response.json::<Value>() {
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
                            debug!("Found no artist banners on fanart.tv");
                        }
                    }
                }
                Err(e) => {
                    warn!("Failed to parse JSON from fanart.tv: {}", e);
                }
            }
        }
        Err(e) => {
            warn!("Couldn't retrieve data from fanart.tv: {}", e);
        }
    }

    banner_urls
}

/// Get artist banner URL from FanArt.tv (legacy method returning only first banner)
/// 
/// # Arguments
/// * `artist_mbid` - MusicBrainz ID of the artist
/// 
/// # Returns
/// * `Option<String>` - URL of the banner if found, otherwise None
pub fn get_artist_banner(artist_mbid: &str) -> Option<String> {
    let banners = get_artist_banners(artist_mbid);
    banners.first().cloned()
}

/// Download artist thumbnails and banners from FanArt.tv
/// 
/// This function follows the naming convention:
/// - artist.0.xxx, artist.1.xxx, etc. for thumbnails
/// - banner.0.xxx, banner.1.xxx, etc. for banners
/// 
/// If the .0 image already exists, it won't download more images of that type.
/// 
/// # Arguments
/// * `artist_mbid` - MusicBrainz ID of the artist
/// * `artist_name` - Name of the artist for caching
/// 
/// # Returns
/// * `bool` - true if the API call was successful (even if no images were found), false only on API error
pub fn download_artist_images(artist_mbid: &str, artist_name: &str) -> bool {
    let artist_basename = crate::helpers::artistupdater::artist_basename(artist_name);
    let mut _thumb_downloaded = false;
    let mut _banner_downloaded = false;
    let mut api_success = true; // Track overall API success
    
    // Check if the first thumbnail already exists
    let first_thumb_path = format!("artists/{}/artist.0", artist_basename);
    if !path_with_any_extension_exists(&first_thumb_path) {
        // Download all thumbnails
        let thumbnail_urls = get_artist_thumbnails(artist_mbid);
        if thumbnail_urls.is_empty() {
            debug!("No thumbnails found on fanart.tv for '{}'", artist_name);
            // This is still considered a success since the API call succeeded
        }
        
        for (i, url) in thumbnail_urls.iter().enumerate() {
            let path = format!("artists/{}/artist.{}.{}", 
                               artist_basename, 
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
        debug!("Artist thumbnail already exists for '{}', skipping download", artist_name);
    }
    
    // Check if the first banner already exists
    let first_banner_path = format!("artists/{}/banner.0", artist_basename);
    if !path_with_any_extension_exists(&first_banner_path) {
        // Download all banners
        let banner_urls = get_artist_banners(artist_mbid);
        if banner_urls.is_empty() {
            debug!("No banners found on fanart.tv for '{}'", artist_name);
            // This is still considered a success since the API call succeeded
        }
        
        for (i, url) in banner_urls.iter().enumerate() {
            let path = format!("artists/{}/banner.{}.{}", 
                               artist_basename, 
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
        debug!("Artist banner already exists for '{}', skipping download", artist_name);
    }
    
    // Return api_success instead of thumb_downloaded || banner_downloaded
    // This allows the function to return true even if no images were found,
    // as long as the API call itself was successful
    api_success
}

/// Check if a path with any extension exists in the image cache
/// 
/// # Arguments
/// * `base_path` - Base path without extension
/// 
/// # Returns
/// * `bool` - True if any file with the base path and any extension exists
pub fn path_with_any_extension_exists(base_path: &str) -> bool {
    // Common image file extensions
    let extensions = ["jpg", "jpeg", "png", "gif", "webp"];
    
    for ext in &extensions {
        let full_path = format!("{}.{}", base_path, ext);
        if std::path::Path::new(&imagecache::get_full_path(&full_path)).exists() {
            return true;
        }
    }
    
    false
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
    let client = match reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build() {
        Ok(client) => client,
        Err(e) => return Err(format!("Failed to create HTTP client: {}", e))
    };
    
    // Execute the request
    let response = match client.get(url).send() {
        Ok(response) => {
            if !response.status().is_success() {
                return Err(format!("HTTP error: {}", response.status()));
            }
            response
        },
        Err(e) => return Err(format!("Request failed: {}", e))
    };
    
    // Read the response body
    match response.bytes() {
        Ok(bytes) => Ok(bytes.to_vec()),
        Err(e) => Err(format!("Failed to read response body: {}", e))
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

#[cfg(test)]
mod tests {
    use super::*;
    // ...existing code...
}
