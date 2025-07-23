use rocket::{get, post, delete, routes};
use rocket::serde::json::Json;
use rocket::serde::{Serialize, Deserialize};
use log::{info, error};

use crate::data::song::Song;
use crate::helpers::favourites;

/// Request payload for adding/removing favourites
#[derive(Deserialize)]
pub struct FavouriteRequest {
    artist: String,
    title: String,
}

/// Response for favourite status check
#[derive(Serialize)]
pub struct FavouriteStatusResponse {
    is_favourite: bool,
    providers: Vec<String>,
}

/// Response for favourite operations
#[derive(Serialize)]
pub struct FavouriteOperationResponse {
    success: bool,
    message: String,
    providers: Vec<String>,
    updated_providers: Vec<String>,
}

/// Error response
#[derive(Serialize)]
pub struct ErrorResponse {
    error: String,
}

/// Check if a song is favourite
#[get("/is_favourite?<artist>&<title>")]
pub fn is_favourite(artist: String, title: String) -> Json<Result<FavouriteStatusResponse, ErrorResponse>> {
    info!("Checking favourite status for '{}' by '{}'", title, artist);
    
    let song = Song {
        artist: Some(artist),
        title: Some(title),
        ..Default::default()
    };
    
    match favourites::get_favourite_providers_display_names(&song) {
        Ok((is_fav, provider_display_names)) => {
            Json(Ok(FavouriteStatusResponse {
                is_favourite: is_fav,
                providers: provider_display_names,
            }))
        }
        Err(e) => {
            error!("Error checking favourite status: {}", e);
            Json(Err(ErrorResponse {
                error: e.to_string(),
            }))
        }
    }
}

/// Add a song to favourites
#[post("/add", data = "<request>")]
pub fn add_favourite(request: Json<FavouriteRequest>) -> Json<Result<FavouriteOperationResponse, ErrorResponse>> {
    info!("Adding favourite: '{}' by '{}'", request.title, request.artist);
    
    let song = Song {
        artist: Some(request.artist.clone()),
        title: Some(request.title.clone()),
        ..Default::default()
    };
    
    let all_providers = favourites::get_enabled_providers();
    
    match favourites::add_favourite(&song) {
        Ok(updated_providers) => {
            info!("Successfully added favourite: '{}' by '{}' to providers: {:?}", request.title, request.artist, updated_providers);
            Json(Ok(FavouriteOperationResponse {
                success: true,
                message: format!("Added '{}' by '{}' to favourites", request.title, request.artist),
                providers: all_providers,
                updated_providers,
            }))
        }
        Err(e) => {
            error!("Error adding favourite: {}", e);
            Json(Err(ErrorResponse {
                error: e.to_string(),
            }))
        }
    }
}

/// Remove a song from favourites
#[delete("/remove", data = "<request>")]
pub fn remove_favourite(request: Json<FavouriteRequest>) -> Json<Result<FavouriteOperationResponse, ErrorResponse>> {
    info!("Removing favourite: '{}' by '{}'", request.title, request.artist);
    
    let song = Song {
        artist: Some(request.artist.clone()),
        title: Some(request.title.clone()),
        ..Default::default()
    };
    
    let all_providers = favourites::get_enabled_providers();
    
    match favourites::remove_favourite(&song) {
        Ok(updated_providers) => {
            info!("Successfully removed favourite: '{}' by '{}' from providers: {:?}", request.title, request.artist, updated_providers);
            Json(Ok(FavouriteOperationResponse {
                success: true,
                message: format!("Removed '{}' by '{}' from favourites", request.title, request.artist),
                providers: all_providers,
                updated_providers,
            }))
        }
        Err(e) => {
            error!("Error removing favourite: {}", e);
            Json(Err(ErrorResponse {
                error: e.to_string(),
            }))
        }
    }
}

/// Get favourite provider status
#[get("/providers")]
pub fn get_providers() -> Json<serde_json::Value> {
    let (total, enabled) = favourites::get_provider_count();
    let enabled_providers = favourites::get_enabled_providers();
    let provider_details = favourites::get_provider_details();
    
    Json(serde_json::json!({
        "enabled_providers": enabled_providers,
        "total_providers": total,
        "enabled_count": enabled,
        "providers": provider_details
    }))
}

/// Export routes for mounting in the main server
pub fn routes() -> Vec<rocket::Route> {
    routes![is_favourite, add_favourite, remove_favourite, get_providers]
}
