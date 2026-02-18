use rocket::serde::json::Json;
use rocket::post;
use serde::{Deserialize, Serialize};
use log::{debug, warn, error};
use crate::helpers::settingsdb;

/// Request structure for getting a setting value
#[derive(Deserialize, Serialize)]
pub struct GetSettingRequest {
    pub key: String,
}

/// Request structure for setting a setting value
#[derive(Deserialize, Serialize)]
pub struct SetSettingRequest {
    pub key: String,
    pub value: serde_json::Value,
}

/// Response structure for successful get operations
#[derive(Serialize, Deserialize)]
pub struct GetSettingResponse {
    pub success: bool,
    pub key: String,
    pub value: Option<serde_json::Value>,
    pub exists: bool,
}

/// Response structure for successful set operations
#[derive(Serialize, Deserialize)]
pub struct SetSettingResponse {
    pub success: bool,
    pub key: String,
    pub value: serde_json::Value,
    pub previous_value: Option<serde_json::Value>,
}

/// Response structure for error operations
#[derive(Serialize, Deserialize)]
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
            Json(serde_json::to_value(response).unwrap_or_else(|e| {
                error!("Failed to serialize get response: {}", e);
                serde_json::json!({"success": false, "message": "Internal serialization error"})
            }))
        }
        Err(e) => {
            error!("Failed to get setting '{}': {}", request.key, e);
            let response = ErrorResponse {
                success: false,
                message: format!("Failed to get setting: {}", e),
            };
            Json(serde_json::to_value(response).unwrap_or_else(|e| {
                error!("Failed to serialize error response: {}", e);
                serde_json::json!({"success": false, "message": "Internal serialization error"})
            }))
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
            Json(serde_json::to_value(response).unwrap_or_else(|e| {
                error!("Failed to serialize set response: {}", e);
                serde_json::json!({"success": false, "message": "Internal serialization error"})
            }))
        }
        Err(e) => {
            error!("Failed to set setting '{}': {}", request.key, e);
            let response = ErrorResponse {
                success: false,
                message: format!("Failed to set setting: {}", e),
            };
            Json(serde_json::to_value(response).unwrap_or_else(|e| {
                error!("Failed to serialize error response: {}", e);
                serde_json::json!({"success": false, "message": "Internal serialization error"})
            }))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::TempDir;
    use crate::helpers::settingsdb;

    /// Setup a temporary directory for testing and initialize the database
    /// Returns a temporary directory that will clean up when dropped
    fn setup_test_env() -> TempDir {
        use std::sync::{Once, Mutex};
        static INIT: Once = Once::new();
        static COUNTER: Mutex<u32> = Mutex::new(0);
        
        let temp_dir = TempDir::new().expect("Failed to create temp directory");
        
        // Initialize the global settings database with a unique subdirectory for this test
        INIT.call_once(|| {
            // First test gets to initialize
        });
        
        // Create a unique subdirectory for this test to avoid conflicts
        let mut counter = COUNTER.lock().unwrap();
        *counter += 1;
        let test_subdir = temp_dir.path().join(format!("test_{}", *counter));
        std::fs::create_dir_all(&test_subdir).expect("Failed to create test subdirectory");
        
        // Initialize the global settings database with the test-specific directory
        settingsdb::SettingsDb::initialize_global(&test_subdir)
            .expect("Failed to initialize test database");
            
        temp_dir
    }

    // Serialization tests - these test the data structures without database or HTTP

    #[test]
    fn test_get_setting_request_serialization() {
        let request = GetSettingRequest {
            key: "test_key".to_string(),
        };
        
        let json_str = serde_json::to_string(&request).unwrap();
        assert!(json_str.contains("test_key"));
        
        let deserialized: GetSettingRequest = serde_json::from_str(&json_str).unwrap();
        assert_eq!(deserialized.key, "test_key");
    }

    #[test]
    fn test_set_setting_request_serialization() {
        let request = SetSettingRequest {
            key: "test_key".to_string(),
            value: json!("test_value"),
        };
        
        let json_str = serde_json::to_string(&request).unwrap();
        assert!(json_str.contains("test_key"));
        assert!(json_str.contains("test_value"));
        
        let deserialized: SetSettingRequest = serde_json::from_str(&json_str).unwrap();
        assert_eq!(deserialized.key, "test_key");
        assert_eq!(deserialized.value, json!("test_value"));
    }

    #[test]
    fn test_get_setting_response_serialization() {
        let response = GetSettingResponse {
            success: true,
            key: "test_key".to_string(),
            value: Some(json!("test_value")),
            exists: true,
        };
        
        let json_str = serde_json::to_string(&response).unwrap();
        assert!(json_str.contains("test_key"));
        assert!(json_str.contains("test_value"));
        assert!(json_str.contains("\"success\":true"));
        assert!(json_str.contains("\"exists\":true"));
        
        let deserialized: GetSettingResponse = serde_json::from_str(&json_str).unwrap();
        assert_eq!(deserialized.key, "test_key");
        assert_eq!(deserialized.value, Some(json!("test_value")));
        assert!(deserialized.success);
        assert!(deserialized.exists);
    }

    #[test]
    fn test_set_setting_response_serialization() {
        let response = SetSettingResponse {
            success: true,
            key: "test_key".to_string(),
            value: json!("new_value"),
            previous_value: Some(json!("old_value")),
        };
        
        let json_str = serde_json::to_string(&response).unwrap();
        assert!(json_str.contains("test_key"));
        assert!(json_str.contains("new_value"));
        assert!(json_str.contains("old_value"));
        assert!(json_str.contains("\"success\":true"));
        
        let deserialized: SetSettingResponse = serde_json::from_str(&json_str).unwrap();
        assert_eq!(deserialized.key, "test_key");
        assert_eq!(deserialized.value, json!("new_value"));
        assert_eq!(deserialized.previous_value, Some(json!("old_value")));
        assert!(deserialized.success);
    }

    #[test]
    fn test_error_response_serialization() {
        let response = ErrorResponse {
            success: false,
            message: "Test error message".to_string(),
        };
        
        let json_str = serde_json::to_string(&response).unwrap();
        assert!(json_str.contains("Test error message"));
        assert!(json_str.contains("\"success\":false"));
        
        let deserialized: ErrorResponse = serde_json::from_str(&json_str).unwrap();
        assert_eq!(deserialized.message, "Test error message");
        assert!(!deserialized.success);
    }

    // Basic database functionality tests - test the settings database directly

    #[test]
    fn test_database_set_and_get_string_value() {
        let _temp_dir = setup_test_env();
        
        let test_key = "test_string_key";
        let test_value = json!("Hello, World!");
        
        // Test setting a value directly using the settings database
        let result = settingsdb::get_settings_db().set(test_key, &test_value);
        assert!(result.is_ok());
        
        // Test getting the value directly
        let retrieved: Result<Option<serde_json::Value>, String> = settingsdb::get_settings_db().get(test_key);
        assert!(retrieved.is_ok());
        
        let retrieved_value = retrieved.unwrap().unwrap();
        assert_eq!(retrieved_value, test_value);
    }

    #[test]
    fn test_database_nonexistent_key() {
        let _temp_dir = setup_test_env();
        
        let test_key = "nonexistent_key_12345";
        
        // Test getting a nonexistent key
        let retrieved: Result<Option<serde_json::Value>, String> = settingsdb::get_settings_db().get(test_key);
        assert!(retrieved.is_ok());
        assert!(retrieved.unwrap().is_none());
    }
}
