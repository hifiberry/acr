use rocket::get;
use rocket::serde::json::Json;
use rocket::serde::{Deserialize, Serialize};
use crate::helpers::coverart::{get_coverart_manager, CoverartMethod};
use crate::helpers::url_encoding::decode_url_safe;

#[derive(Serialize, Deserialize)]
pub struct CoverartResponse {
    pub coverart_urls: Vec<String>,
}

#[derive(Serialize, Deserialize)]
pub struct CoverartMethodInfo {
    pub method: String,
    pub provider_count: usize,
}

#[derive(Serialize, Deserialize)]
pub struct CoverartProvidersResponse {
    pub total_providers: usize,
    pub methods: Vec<CoverartMethodInfo>,
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
                coverart_urls: vec![],
            });
        }
    };

    let manager = get_coverart_manager();
    let manager_lock = manager.lock().unwrap();
    let coverart_urls = manager_lock.get_artist_coverart(&artist);

    Json(CoverartResponse { coverart_urls })
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
                coverart_urls: vec![],
            });
        }
    };

    let artist = match decode_url_safe(&artist_b64) {
        Some(decoded) => decoded,
        None => {
            log::warn!("Failed to decode artist parameter: {}", artist_b64);
            return Json(CoverartResponse {
                coverart_urls: vec![],
            });
        }
    };

    let manager = get_coverart_manager();
    let manager_lock = manager.lock().unwrap();
    let coverart_urls = manager_lock.get_song_coverart(&title, &artist);

    Json(CoverartResponse { coverart_urls })
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
                coverart_urls: vec![],
            });
        }
    };

    let artist = match decode_url_safe(&artist_b64) {
        Some(decoded) => decoded,
        None => {
            log::warn!("Failed to decode artist parameter: {}", artist_b64);
            return Json(CoverartResponse {
                coverart_urls: vec![],
            });
        }
    };

    let manager = get_coverart_manager();
    let manager_lock = manager.lock().unwrap();
    let coverart_urls = manager_lock.get_album_coverart(&title, &artist, year);

    Json(CoverartResponse { coverart_urls })
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
                coverart_urls: vec![],
            });
        }
    };

    let manager = get_coverart_manager();
    let manager_lock = manager.lock().unwrap();
    let coverart_urls = manager_lock.get_url_coverart(&url);

    Json(CoverartResponse { coverart_urls })
}

/// Get information about available coverart methods and providers
#[get("/providers")]
pub fn get_coverart_providers() -> Json<CoverartProvidersResponse> {
    let manager = get_coverart_manager();
    let manager_lock = manager.lock().unwrap();
    let providers = manager_lock.get_providers();
    
    let total_providers = providers.len();
    
    // Count providers for each method
    let mut method_counts = std::collections::HashMap::new();
    
    for provider in providers {
        let supported_methods = provider.supported_methods();
        for method in supported_methods {
            *method_counts.entry(method).or_insert(0) += 1;
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
            CoverartMethod::Artist => "artist",
            CoverartMethod::Song => "song", 
            CoverartMethod::Album => "album",
            CoverartMethod::Url => "url",
        };
        
        CoverartMethodInfo {
            method: method_name.to_string(),
            provider_count: method_counts.get(method).copied().unwrap_or(0),
        }
    })
    .collect();
    
    Json(CoverartProvidersResponse {
        total_providers,
        methods,
    })
}
