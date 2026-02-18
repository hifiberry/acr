use crate::helpers::global_volume;
use crate::helpers::volume::{VolumeControlInfo, DecibelRange};
use rocket::serde::json::Json;
use rocket::{get, post};
use rocket::response::status::Custom;
use rocket::http::Status;
use serde::{Deserialize, Serialize};
use log::debug;

/// Response struct for volume control information
#[derive(Serialize)]
pub struct VolumeInfoResponse {
    /// Whether volume control is available
    pub available: bool,
    /// Control information (if available)
    pub control_info: Option<VolumeControlInfoResponse>,
    /// Current volume state (if available)
    pub current_state: Option<VolumeStateResponse>,
    /// Whether change monitoring is supported
    pub supports_change_monitoring: bool,
}

/// Volume control information for API response
#[derive(Serialize)]
pub struct VolumeControlInfoResponse {
    /// Internal name used by the system
    pub internal_name: String,
    /// Display name for UI
    pub display_name: String,
    /// Decibel range information (if supported)
    pub decibel_range: Option<DecibelRangeResponse>,
}

/// Decibel range information for API response
#[derive(Serialize)]
pub struct DecibelRangeResponse {
    /// Minimum dB value
    pub min_db: f64,
    /// Maximum dB value
    pub max_db: f64,
}

/// Current volume state for API response
#[derive(Serialize)]
pub struct VolumeStateResponse {
    /// Current volume as percentage (0-100)
    pub percentage: f64,
    /// Current volume in dB (if supported)
    pub decibels: Option<f64>,
    /// Raw control value (implementation specific)
    pub raw_value: Option<i64>,
}

/// Request struct for setting volume
#[derive(Deserialize, Debug)]
pub struct SetVolumeRequest {
    /// Volume percentage (0-100)
    pub percentage: Option<f64>,
    /// Volume in decibels
    pub decibels: Option<f64>,
    /// Raw control value
    pub raw_value: Option<i64>,
}

/// Response for volume operations
#[derive(Serialize)]
pub struct VolumeOperationResponse {
    /// Whether the operation succeeded
    pub success: bool,
    /// Success or error message
    pub message: String,
    /// Updated volume state (if successful)
    pub new_state: Option<VolumeStateResponse>,
}

impl From<VolumeControlInfo> for VolumeControlInfoResponse {
    fn from(info: VolumeControlInfo) -> Self {
        Self {
            internal_name: info.internal_name,
            display_name: info.display_name,
            decibel_range: info.decibel_range.map(Into::into),
        }
    }
}

impl From<DecibelRange> for DecibelRangeResponse {
    fn from(range: DecibelRange) -> Self {
        Self {
            min_db: range.min_db,
            max_db: range.max_db,
        }
    }
}

/// Get volume control information and current state
#[get("/info")]
pub fn get_volume_info() -> Json<VolumeInfoResponse> {
    debug!("API: Getting volume information");
    
    let available = global_volume::is_volume_control_available();
    let supports_monitoring = global_volume::supports_volume_change_monitoring();
    
    let control_info = if available {
        global_volume::get_volume_control_info().map(Into::into)
    } else {
        None
    };
    
    let current_state = if available {
        let percentage = global_volume::get_volume_percentage();
        let decibels = global_volume::get_volume_db();
        
        // Try to get raw value (this might fail for some implementations)
        let raw_value = if let Ok(control) = global_volume::get_global_volume_control() {
            control.lock().get_raw_value().ok()
        } else {
            None
        };

        percentage.map(|p| VolumeStateResponse {
            percentage: p,
            decibels,
            raw_value,
        })
    } else {
        None
    };
    
    Json(VolumeInfoResponse {
        available,
        control_info,
        current_state,
        supports_change_monitoring: supports_monitoring,
    })
}

/// Get current volume state only
#[get("/state")]
pub fn get_volume_state() -> Result<Json<VolumeStateResponse>, Custom<Json<VolumeOperationResponse>>> {
    debug!("API: Getting current volume state");
    
    if !global_volume::is_volume_control_available() {
        return Err(Custom(Status::ServiceUnavailable, Json(VolumeOperationResponse {
            success: false,
            message: "Volume control not available".to_string(),
            new_state: None,
        })));
    }
    
    let percentage = global_volume::get_volume_percentage()
        .ok_or_else(|| Custom(Status::InternalServerError, Json(VolumeOperationResponse {
            success: false,
            message: "Failed to get current volume percentage".to_string(),
            new_state: None,
        })))?;
    
    let decibels = global_volume::get_volume_db();
    
    // Try to get raw value
    let raw_value = if let Ok(control) = global_volume::get_global_volume_control() {
        control.lock().get_raw_value().ok()
    } else {
        None
    };

    Ok(Json(VolumeStateResponse {
        percentage,
        decibels,
        raw_value,
    }))
}

/// Set volume level
#[post("/set", data = "<request>")]
pub fn set_volume(request: Json<SetVolumeRequest>) -> Json<VolumeOperationResponse> {
    debug!("API: Setting volume: {:?}", *request);
    
    if !global_volume::is_volume_control_available() {
        return Json(VolumeOperationResponse {
            success: false,
            message: "Volume control not available".to_string(),
            new_state: None,
        });
    }
    
    // Determine which value to set based on what's provided
    let result = if let Some(percentage) = request.percentage {
        if percentage < 0.0 || percentage > 100.0 {
            return Json(VolumeOperationResponse {
                success: false,
                message: format!("Volume percentage {} is out of range (0-100)", percentage),
                new_state: None,
            });
        }
        global_volume::set_volume_percentage(percentage)
    } else if let Some(db) = request.decibels {
        global_volume::set_volume_db(db)
    } else if let Some(raw) = request.raw_value {
        if let Ok(control) = global_volume::get_global_volume_control() {
            control.lock().set_raw_value(raw).is_ok()
        } else {
            false
        }
    } else {
        return Json(VolumeOperationResponse {
            success: false,
            message: "No volume value provided (percentage, decibels, or raw_value required)".to_string(),
            new_state: None,
        });
    };
    
    if result {
        // Get the updated state
        let new_state = if let Some(percentage) = global_volume::get_volume_percentage() {
            let decibels = global_volume::get_volume_db();
            let raw_value = if let Ok(control) = global_volume::get_global_volume_control() {
                control.lock().get_raw_value().ok()
            } else {
                None
            };

            Some(VolumeStateResponse {
                percentage,
                decibels,
                raw_value,
            })
        } else {
            None
        };

        Json(VolumeOperationResponse {
            success: true,
            message: "Volume set successfully".to_string(),
            new_state,
        })
    } else {
        Json(VolumeOperationResponse {
            success: false,
            message: "Failed to set volume".to_string(),
            new_state: None,
        })
    }
}

/// Increase volume by a percentage amount
#[post("/increase?<amount>")]
pub fn increase_volume(amount: Option<f64>) -> Json<VolumeOperationResponse> {
    let increase_amount = amount.unwrap_or(5.0); // Default 5% increase
    debug!("API: Increasing volume by {}%", increase_amount);
    
    if !global_volume::is_volume_control_available() {
        return Json(VolumeOperationResponse {
            success: false,
            message: "Volume control not available".to_string(),
            new_state: None,
        });
    }
    
    if let Some(current) = global_volume::get_volume_percentage() {
        let new_volume = (current + increase_amount).clamp(0.0, 100.0);
        let result = global_volume::set_volume_percentage(new_volume);
        
        if result {
            let new_state = global_volume::get_volume_percentage().map(|percentage| {
                let decibels = global_volume::get_volume_db();
                let raw_value = if let Ok(control) = global_volume::get_global_volume_control() {
                    control.lock().get_raw_value().ok()
                } else {
                    None
                };

                VolumeStateResponse {
                    percentage,
                    decibels,
                    raw_value,
                }
            });

            Json(VolumeOperationResponse {
                success: true,
                message: format!("Volume increased to {:.1}%", new_volume),
                new_state,
            })
        } else {
            Json(VolumeOperationResponse {
                success: false,
                message: "Failed to increase volume".to_string(),
                new_state: None,
            })
        }
    } else {
        Json(VolumeOperationResponse {
            success: false,
            message: "Failed to get current volume".to_string(),
            new_state: None,
        })
    }
}

/// Decrease volume by a percentage amount
#[post("/decrease?<amount>")]
pub fn decrease_volume(amount: Option<f64>) -> Json<VolumeOperationResponse> {
    let decrease_amount = amount.unwrap_or(5.0); // Default 5% decrease
    debug!("API: Decreasing volume by {}%", decrease_amount);
    
    if !global_volume::is_volume_control_available() {
        return Json(VolumeOperationResponse {
            success: false,
            message: "Volume control not available".to_string(),
            new_state: None,
        });
    }
    
    if let Some(current) = global_volume::get_volume_percentage() {
        let new_volume = (current - decrease_amount).clamp(0.0, 100.0);
        let result = global_volume::set_volume_percentage(new_volume);
        
        if result {
            let new_state = global_volume::get_volume_percentage().map(|percentage| {
                let decibels = global_volume::get_volume_db();
                let raw_value = if let Ok(control) = global_volume::get_global_volume_control() {
                    control.lock().get_raw_value().ok()
                } else {
                    None
                };

                VolumeStateResponse {
                    percentage,
                    decibels,
                    raw_value,
                }
            });

            Json(VolumeOperationResponse {
                success: true,
                message: format!("Volume decreased to {:.1}%", new_volume),
                new_state,
            })
        } else {
            Json(VolumeOperationResponse {
                success: false,
                message: "Failed to decrease volume".to_string(),
                new_state: None,
            })
        }
    } else {
        Json(VolumeOperationResponse {
            success: false,
            message: "Failed to get current volume".to_string(),
            new_state: None,
        })
    }
}

/// Mute or unmute volume
#[post("/mute")]
pub fn toggle_mute() -> Json<VolumeOperationResponse> {
    debug!("API: Toggling mute");
    
    // For now, implement mute as setting volume to 0
    // In a more sophisticated implementation, you might store the previous volume
    if !global_volume::is_volume_control_available() {
        return Json(VolumeOperationResponse {
            success: false,
            message: "Volume control not available".to_string(),
            new_state: None,
        });
    }
    
    if let Some(current) = global_volume::get_volume_percentage() {
        let new_volume = if current > 0.0 { 0.0 } else { 50.0 }; // Toggle between 0 and 50%
        let result = global_volume::set_volume_percentage(new_volume);
        
        if result {
            let new_state = global_volume::get_volume_percentage().map(|percentage| {
                let decibels = global_volume::get_volume_db();
                let raw_value = if let Ok(control) = global_volume::get_global_volume_control() {
                    control.lock().get_raw_value().ok()
                } else {
                    None
                };

                VolumeStateResponse {
                    percentage,
                    decibels,
                    raw_value,
                }
            });

            let action = if new_volume == 0.0 { "muted" } else { "unmuted" };
            Json(VolumeOperationResponse {
                success: true,
                message: format!("Volume {} at {:.1}%", action, new_volume),
                new_state,
            })
        } else {
            Json(VolumeOperationResponse {
                success: false,
                message: "Failed to toggle mute".to_string(),
                new_state: None,
            })
        }
    } else {
        Json(VolumeOperationResponse {
            success: false,
            message: "Failed to get current volume".to_string(),
            new_state: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::helpers::volume::{VolumeControlInfo, DecibelRange};

    #[test]
    fn test_volume_control_info_conversion() {
        let db_range = DecibelRange::new(-60.0, 0.0);
        let info = VolumeControlInfo::new("test".to_string(), "Test Control".to_string())
            .with_decibel_range(db_range);
        
        let response: VolumeControlInfoResponse = info.into();
        
        assert_eq!(response.internal_name, "test");
        assert_eq!(response.display_name, "Test Control");
        assert!(response.decibel_range.is_some());
        
        let db_response = response.decibel_range.unwrap();
        assert_eq!(db_response.min_db, -60.0);
        assert_eq!(db_response.max_db, 0.0);
    }

    #[test]
    fn test_decibel_range_conversion() {
        let range = DecibelRange::new(-120.0, 6.0);
        let response: DecibelRangeResponse = range.into();
        
        assert_eq!(response.min_db, -120.0);
        assert_eq!(response.max_db, 6.0);
    }
}
