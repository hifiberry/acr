use rocket::get;
use rocket::post;
use rocket::serde::json::Json;
use rocket::serde::{Deserialize, Serialize};
use log::{debug, info, warn, error};
use crate::helpers::coverart::{get_coverart_manager, CoverartMethod, CoverartResult, ProviderInfo};
use crate::helpers::url_encoding::decode_url_safe;
use crate::helpers::settingsdb;

#[derive(Serialize, Deserialize)]
pub struct CoverartResponse {
    pub results: Vec<CoverartResult>,
}

#[derive(Serialize, Deserialize)]
pub struct CoverartMethodInfo {
    pub method: String,
    pub providers: Vec<ProviderInfo>,
}

#[derive(Serialize)]
pub struct CoverartMethodsResponse {
    methods: Vec<CoverartMethodInfo>,
}

#[derive(Deserialize)]
pub struct UpdateImageRequest {
    url: String,
}

#[derive(Serialize)]
pub struct UpdateImageResponse {
    success: bool,
    message: String,
}

/// Get cover art for an artist
/// 
/// # Parameters
/// * `artist_b64` - Base64 encoded artist name
#[get("/artist/<artist_b64>")]
pub fn get_artist_coverart(artist_b64: String) -> Json<CoverartResponse> {
    let artist = match decode_url_safe(&artist_b64) {
        Some(decoded) => decoded,
        None => {
            log::warn!("Failed to decode artist parameter: {}", artist_b64);
            return Json(CoverartResponse {
                results: vec![],
            });
        }
    };

    let manager = get_coverart_manager();
    let manager_lock = manager.lock();
    let results = manager_lock.get_artist_coverart(&artist);

    Json(CoverartResponse { results })
}

/// Get cover art for a song
/// 
/// # Parameters
/// * `title_b64` - Base64 encoded song title
/// * `artist_b64` - Base64 encoded artist name
#[get("/song/<title_b64>/<artist_b64>")]
pub fn get_song_coverart(title_b64: String, artist_b64: String) -> Json<CoverartResponse> {
    let title = match decode_url_safe(&title_b64) {
        Some(decoded) => decoded,
        None => {
            log::warn!("Failed to decode title parameter: {}", title_b64);
            return Json(CoverartResponse {
                results: vec![],
            });
        }
    };

    let artist = match decode_url_safe(&artist_b64) {
        Some(decoded) => decoded,
        None => {
            log::warn!("Failed to decode artist parameter: {}", artist_b64);
            return Json(CoverartResponse {
                results: vec![],
            });
        }
    };

    let manager = get_coverart_manager();
    let manager_lock = manager.lock();
    let results = manager_lock.get_song_coverart(&title, &artist);

    Json(CoverartResponse { results })
}

/// Get cover art for an album
/// 
/// # Parameters
/// * `title_b64` - Base64 encoded album title
/// * `artist_b64` - Base64 encoded artist name
/// * `year` - Optional release year
#[get("/album/<title_b64>/<artist_b64>")]
pub fn get_album_coverart(title_b64: String, artist_b64: String) -> Json<CoverartResponse> {
    get_album_coverart_with_year(title_b64, artist_b64, None)
}

/// Get cover art for an album with year
/// 
/// # Parameters
/// * `title_b64` - Base64 encoded album title
/// * `artist_b64` - Base64 encoded artist name
/// * `year` - Release year
#[get("/album/<title_b64>/<artist_b64>/<year>")]
pub fn get_album_coverart_with_year(title_b64: String, artist_b64: String, year: Option<i32>) -> Json<CoverartResponse> {
    let title = match decode_url_safe(&title_b64) {
        Some(decoded) => decoded,
        None => {
            log::warn!("Failed to decode title parameter: {}", title_b64);
            return Json(CoverartResponse {
                results: vec![],
            });
        }
    };

    let artist = match decode_url_safe(&artist_b64) {
        Some(decoded) => decoded,
        None => {
            log::warn!("Failed to decode artist parameter: {}", artist_b64);
            return Json(CoverartResponse {
                results: vec![],
            });
        }
    };

    let manager = get_coverart_manager();
    let manager_lock = manager.lock();
    let results = manager_lock.get_album_coverart(&title, &artist, year);

    Json(CoverartResponse { results })
}

/// Get cover art from a URL
/// 
/// # Parameters
/// * `url_b64` - Base64 encoded URL
#[get("/url/<url_b64>")]
pub fn get_url_coverart(url_b64: String) -> Json<CoverartResponse> {
    let url = match decode_url_safe(&url_b64) {
        Some(decoded) => decoded,
        None => {
            log::warn!("Failed to decode url parameter: {}", url_b64);
            return Json(CoverartResponse {
                results: vec![],
            });
        }
    };

    let manager = get_coverart_manager();
    let manager_lock = manager.lock();
    let results = manager_lock.get_url_coverart(&url);

    Json(CoverartResponse { results })
}

/// Get information about available coverart methods and providers
#[get("/methods")]
pub fn get_coverart_methods() -> Json<CoverartMethodsResponse> {
    let manager = get_coverart_manager();
    let manager_lock = manager.lock();
    let providers = manager_lock.get_providers();
    
    log::debug!("API: Total providers found: {}", providers.len());
    for (i, provider) in providers.iter().enumerate() {
        log::debug!("API: Provider {}: {} ({})", i, provider.name(), provider.display_name());
        log::debug!("API: Provider {} supported methods: {:?}", i, provider.supported_methods());
    }
    
    // Group providers by supported methods
    let mut method_providers = std::collections::HashMap::new();
    
    for provider in providers {
        let supported_methods = provider.supported_methods();
        let provider_info = ProviderInfo {
            name: provider.name().to_string(),
            display_name: provider.display_name().to_string(),
        };
        
        for method in supported_methods {
            method_providers
                .entry(method)
                .or_insert_with(Vec::new)
                .push(provider_info.clone());
        }
    }
    
    // Convert to response format
    let methods: Vec<CoverartMethodInfo> = [
        CoverartMethod::Artist,
        CoverartMethod::Song,
        CoverartMethod::Album,
        CoverartMethod::Url,
    ]
    .iter()
    .map(|method| {
        let method_name = match method {
            CoverartMethod::Artist => "Artist",
            CoverartMethod::Song => "Song", 
            CoverartMethod::Album => "Album",
            CoverartMethod::Url => "Url",
        };
        
        CoverartMethodInfo {
            method: method_name.to_string(),
            providers: method_providers.get(method).cloned().unwrap_or_default(),
        }
    })
    .collect();
    
    Json(CoverartMethodsResponse { methods })
}

/// Update artist image with custom URL
/// 
/// # Parameters
/// * `artist_b64` - Base64 encoded artist name
/// * `request` - JSON request body containing the image URL
#[post("/artist/<artist_b64>/update", data = "<request>")]
pub fn update_artist_image(artist_b64: String, request: Json<UpdateImageRequest>) -> Json<UpdateImageResponse> {
    debug!("Received artist image update request: artist_b64={}, url={}", artist_b64, request.url);
    
    let artist_name = match decode_url_safe(&artist_b64) {
        Some(name) => name,
        None => {
            warn!("Invalid artist name encoding: {}", artist_b64);
            return Json(UpdateImageResponse {
                success: false,
                message: "Invalid artist name encoding".to_string(),
            });
        }
    };

    debug!("Decoded artist name: {}", artist_name);

    // Store the custom URL in settings database
    let settings_key = format!("artist.image.{}", artist_name);
    debug!("Storing custom image URL in settings: key={}, url={}", settings_key, request.url);
    
    match settingsdb::set_string(&settings_key, &request.url) {
        Ok(_) => {
            info!("Successfully stored custom image URL for artist '{}': {}", artist_name, request.url);
            
            // Clear any cached image to force refresh
            let cache_path = format!("artists/{}/cover.jpg", crate::helpers::url_encoding::encode_url_safe(&artist_name));
            debug!("Attempting to clear cached image at: {}", cache_path);
            
            match std::fs::remove_file(&cache_path) {
                Ok(_) => {
                    debug!("Successfully cleared cached image for artist: {}", artist_name);
                }
                Err(e) => {
                    debug!("No cached image to clear for artist '{}' ({}): {}", artist_name, cache_path, e);
                }
            }
            
            // If URL is not empty, try to trigger immediate download to user directory
            if !request.url.is_empty() {
                debug!("Attempting to trigger immediate download of custom image to user directory for artist: {}", artist_name);
                
                // Use the global artist store to download the image to user directory
                let artist_store = crate::helpers::artist_store::get_artist_store();
                let mut store_lock = artist_store.lock();
                
                match store_lock.download_and_store_user_image(&artist_name, &request.url, "custom") {
                    crate::helpers::artist_store::ArtistImageResult::Found { cache_path } => {
                        info!("Successfully downloaded and stored custom image in user directory for artist '{}': {}", artist_name, cache_path);
                    }
                    crate::helpers::artist_store::ArtistImageResult::NotFound => {
                        warn!("Failed to download custom image for artist '{}' from URL: {}", artist_name, request.url);
                    }
                    crate::helpers::artist_store::ArtistImageResult::Error(error) => {
                        warn!("Error downloading custom image for artist '{}' from URL {}: {}", artist_name, request.url, error);
                    }
                }
            } else {
                info!("Empty URL provided - custom image cleared for artist: {}", artist_name);
            }
            
            Json(UpdateImageResponse {
                success: true,
                message: format!("Artist image URL updated successfully for '{}'", artist_name),
            })
        }
        Err(e) => {
            error!("Failed to store custom image URL for artist '{}': {}", artist_name, e);
            Json(UpdateImageResponse {
                success: false,
                message: format!("Failed to update artist image: {}", e),
            })
        }
    }
}

/// Get artist image directly
/// 
/// This endpoint serves the actual artist image file if available in cache.
/// Returns a 404 if no image is found.
/// 
/// # Parameters
/// * `artist_b64` - Base64 encoded artist name
#[get("/artist/<artist_b64>/image")]
pub fn get_artist_image(artist_b64: String) -> Result<(rocket::http::ContentType, Vec<u8>), rocket::response::status::Custom<String>> {
    use rocket::http::Status;
    use rocket::response::status::Custom;
    
    let artist_name = match decode_url_safe(&artist_b64) {
        Some(decoded) => decoded,
        None => {
            log::warn!("Failed to decode artist parameter: {}", artist_b64);
            return Err(Custom(
                Status::BadRequest,
                "Invalid artist name encoding".to_string(),
            ));
        }
    };

    // Try to get the cached image from the artist store
    match crate::helpers::artist_store::get_or_download_artist_image(&artist_name) {
        Some(cache_path) => {
            // Read the image file
            match std::fs::read(&cache_path) {
                Ok(image_data) => {
                    // Determine content type based on file extension
                    let content_type = if cache_path.ends_with(".png") {
                        rocket::http::ContentType::PNG
                    } else if cache_path.ends_with(".gif") {
                        rocket::http::ContentType::GIF
                    } else if cache_path.ends_with(".webp") {
                        rocket::http::ContentType::new("image", "webp")
                    } else {
                        rocket::http::ContentType::JPEG // Default to JPEG
                    };
                    
                    debug!("Serving artist image for '{}' from cache: {}", artist_name, cache_path);
                    Ok((content_type, image_data))
                },
                Err(e) => {
                    log::warn!("Failed to read cached image for artist '{}' at '{}': {}", artist_name, cache_path, e);
                    Err(Custom(
                        Status::InternalServerError,
                        format!("Failed to read cached image: {}", e),
                    ))
                }
            }
        },
        None => {
            debug!("No cached image found for artist: {}", artist_name);
            Err(Custom(
                Status::NotFound,
                format!("No image found for artist '{}'", artist_name),
            ))
        }
    }
}
