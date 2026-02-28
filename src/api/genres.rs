use crate::helpers::genre_cleanup::{
    self, GenreConfig,
    get_effective_config, get_user_config, save_user_config,
    set_genre_mapping, delete_genre_mapping, add_genre_ignore, remove_genre_ignore,
};
use rocket::serde::json::Json;
use rocket::{get, post, put, delete};
use rocket::response::status::Custom;
use rocket::http::Status;
use serde::{Deserialize, Serialize};

/// Response wrapper for the effective (merged) genre config
#[derive(Serialize)]
pub struct GenreConfigResponse {
    /// Merged config (system + user) — what is currently active
    pub config: GenreConfig,
    /// Path where user changes are saved
    pub user_config_path: String,
}

/// Response wrapper for user-only genre config
#[derive(Serialize)]
pub struct UserGenreConfigResponse {
    pub config: GenreConfig,
    pub path: String,
}

/// Request body for adding/updating a mapping
#[derive(Deserialize)]
pub struct MappingRequest {
    pub from: String,
    pub to: String,
}

/// Request body for adding a genre to the ignore list
#[derive(Deserialize)]
pub struct IgnoreRequest {
    pub genre: String,
}

/// Simple status response
#[derive(Serialize)]
pub struct StatusResponse {
    pub success: bool,
    pub message: String,
}

fn ok(msg: impl Into<String>) -> Json<StatusResponse> {
    Json(StatusResponse { success: true, message: msg.into() })
}

fn err_response(status: Status, msg: impl Into<String>) -> Custom<Json<StatusResponse>> {
    Custom(status, Json(StatusResponse { success: false, message: msg.into() }))
}

/// GET /genres/config — returns the current effective (merged) genre config
#[get("/config")]
pub fn get_config() -> Result<Json<GenreConfigResponse>, Custom<Json<StatusResponse>>> {
    match get_effective_config() {
        Some(config) => Ok(Json(GenreConfigResponse {
            config,
            user_config_path: genre_cleanup::user_config_path().to_string_lossy().to_string(),
        })),
        None => Err(err_response(Status::ServiceUnavailable, "Genre cleanup not initialized")),
    }
}

/// GET /genres/user-config — returns the user-only config (what the user has explicitly set)
#[get("/user-config")]
pub fn get_user_config_endpoint() -> Json<UserGenreConfigResponse> {
    Json(UserGenreConfigResponse {
        config: get_user_config(),
        path: genre_cleanup::user_config_path().to_string_lossy().to_string(),
    })
}

/// PUT /genres/user-config — replace the entire user config and reload
#[put("/user-config", data = "<config>")]
pub fn put_user_config(config: Json<GenreConfig>) -> Result<Json<StatusResponse>, Custom<Json<StatusResponse>>> {
    match save_user_config(config.into_inner()) {
        Ok(_) => Ok(ok("User genre config saved and reloaded")),
        Err(e) => Err(err_response(Status::InternalServerError, format!("Failed to save config: {}", e))),
    }
}

/// POST /genres/mapping — add or update a single mapping entry in the user config
#[post("/mapping", data = "<req>")]
pub fn post_mapping(req: Json<MappingRequest>) -> Result<Json<StatusResponse>, Custom<Json<StatusResponse>>> {
    let r = req.into_inner();
    match set_genre_mapping(r.from.clone(), r.to.clone()) {
        Ok(_) => Ok(ok(format!("Mapping '{}' → '{}' saved", r.from, r.to))),
        Err(e) => Err(err_response(Status::InternalServerError, format!("Failed to save mapping: {}", e))),
    }
}

/// DELETE /genres/mapping/<genre> — remove a mapping from the user config
#[delete("/mapping/<genre>")]
pub fn delete_mapping(genre: &str) -> Result<Json<StatusResponse>, Custom<Json<StatusResponse>>> {
    match delete_genre_mapping(genre) {
        Ok(_) => Ok(ok(format!("Mapping for '{}' removed", genre))),
        Err(e) => Err(err_response(Status::InternalServerError, format!("Failed to remove mapping: {}", e))),
    }
}

/// POST /genres/ignore — add a genre to the user ignore list
#[post("/ignore", data = "<req>")]
pub fn post_ignore(req: Json<IgnoreRequest>) -> Result<Json<StatusResponse>, Custom<Json<StatusResponse>>> {
    let genre = req.into_inner().genre;
    match add_genre_ignore(genre.clone()) {
        Ok(_) => Ok(ok(format!("'{}' added to ignore list", genre))),
        Err(e) => Err(err_response(Status::InternalServerError, format!("Failed to update ignore list: {}", e))),
    }
}

/// DELETE /genres/ignore/<genre> — remove a genre from the user ignore list
#[delete("/ignore/<genre>")]
pub fn delete_ignore(genre: &str) -> Result<Json<StatusResponse>, Custom<Json<StatusResponse>>> {
    match remove_genre_ignore(genre) {
        Ok(_) => Ok(ok(format!("'{}' removed from ignore list", genre))),
        Err(e) => Err(err_response(Status::InternalServerError, format!("Failed to update ignore list: {}", e))),
    }
}
