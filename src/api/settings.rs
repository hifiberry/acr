use rocket::serde::json::Json;
use rocket::{post, State};
use serde::{Deserialize, Serialize};
use log::{debug, warn, error};
use crate::helpers::settingsdb;

/// Request structure for getting a setting value
#[derive(Deserialize)]
pub struct GetSettingRequest {
    pub key: String,
}

/// Request structure for setting a setting value
#[derive(Deserialize)]
pub struct SetSettingRequest {
    pub key: String,
    pub value: serde_json::Value,
}

/// Response structure for successful get operations
#[derive(Serialize)]
pub struct GetSettingResponse {
    pub success: bool,
    pub key: String,
    pub value: Option<serde_json::Value>,
    pub exists: bool,
}

/// Response structure for successful set operations
#[derive(Serialize)]
pub struct SetSettingResponse {
    pub success: bool,
    pub key: String,
    pub value: serde_json::Value,
    pub previous_value: Option<serde_json::Value>,
}

/// Response structure for error operations
#[derive(Serialize)]
pub struct ErrorResponse {
    pub success: bool,
    pub message: String,
}

/// Get a setting value from the settings database
/// 
/// This endpoint retrieves the value of a specific setting key from the database.
/// Uses POST method to handle non-ASCII characters in keys properly.
#[post("/get", data = "<request>")]
pub fn get_setting(request: Json<GetSettingRequest>) -> Json<serde_json::Value> {
    debug!("Getting setting for key: {}", request.key);
    
    // Try to get the value from the settings database
    match settingsdb::get::<serde_json::Value>(&request.key) {
        Ok(value_opt) => {
            let exists = value_opt.is_some();
            let response = GetSettingResponse {
                success: true,
                key: request.key.clone(),
                value: value_opt,
                exists,
            };
            
            debug!("Successfully retrieved setting '{}', exists: {}", request.key, exists);
            Json(serde_json::to_value(response).unwrap())
        }
        Err(e) => {
            error!("Failed to get setting '{}': {}", request.key, e);
            let response = ErrorResponse {
                success: false,
                message: format!("Failed to get setting: {}", e),
            };
            Json(serde_json::to_value(response).unwrap())
        }
    }
}

/// Set a setting value in the settings database
/// 
/// This endpoint sets the value of a specific setting key in the database.
/// Returns the previous value if it existed.
#[post("/set", data = "<request>")]
pub fn set_setting(request: Json<SetSettingRequest>) -> Json<serde_json::Value> {
    debug!("Setting value for key: {} = {:?}", request.key, request.value);
    
    // First, try to get the current value to return as previous_value
    let previous_value = match settingsdb::get::<serde_json::Value>(&request.key) {
        Ok(value_opt) => value_opt,
        Err(e) => {
            warn!("Could not retrieve previous value for key '{}': {}", request.key, e);
            None
        }
    };
    
    // Try to set the new value
    match settingsdb::set(&request.key, &request.value) {
        Ok(()) => {
            debug!("Successfully set setting '{}' to {:?}", request.key, request.value);
            let response = SetSettingResponse {
                success: true,
                key: request.key.clone(),
                value: request.value.clone(),
                previous_value,
            };
            Json(serde_json::to_value(response).unwrap())
        }
        Err(e) => {
            error!("Failed to set setting '{}': {}", request.key, e);
            let response = ErrorResponse {
                success: false,
                message: format!("Failed to set setting: {}", e),
            };
            Json(serde_json::to_value(response).unwrap())
        }
    }
}
