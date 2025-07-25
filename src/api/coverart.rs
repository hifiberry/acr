use rocket::get;
use rocket::serde::json::Json;
use rocket::serde::{Deserialize, Serialize};
use crate::helpers::coverart::{get_coverart_manager, CoverartMethod, CoverartResult, ProviderInfo};
use crate::helpers::url_encoding::decode_url_safe;

#[derive(Serialize, Deserialize)]
pub struct CoverartResponse {
    pub results: Vec<CoverartResult>,
}

#[derive(Serialize, Deserialize)]
pub struct CoverartMethodInfo {
    pub method: String,
    pub providers: Vec<ProviderInfo>,
}

#[derive(Serialize, Deserialize)]
pub struct CoverartMethodsResponse {
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
                results: vec![],
            });
        }
    };

    let manager = get_coverart_manager();
    let manager_lock = manager.lock().unwrap();
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
    let manager_lock = manager.lock().unwrap();
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
    let manager_lock = manager.lock().unwrap();
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
    let manager_lock = manager.lock().unwrap();
    let results = manager_lock.get_url_coverart(&url);

    Json(CoverartResponse { results })
}

/// Get information about available coverart methods and providers
#[get("/methods")]
pub fn get_coverart_methods() -> Json<CoverartMethodsResponse> {
    let manager = get_coverart_manager();
    let manager_lock = manager.lock().unwrap();
    let providers = manager_lock.get_providers();
    
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
