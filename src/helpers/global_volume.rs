use crate::helpers::volume::{VolumeControl, VolumeChangeEvent};
#[cfg(all(feature = "alsa", not(windows)))]
use crate::helpers::volume::AlsaVolumeControl;
use crate::helpers::volume::DummyVolumeControl;
use std::sync::{Arc, Mutex, mpsc};
use once_cell::sync::OnceCell;
use log::{info, warn, error};
use serde_json::Value;
use crate::config::get_service_config;

/// Global volume control instance
static GLOBAL_VOLUME_CONTROL: OnceCell<Arc<Mutex<Box<dyn VolumeControl + Send + Sync>>>> = OnceCell::new();

/// Initialize the global volume control from configuration
pub fn initialize_volume_control(config: &Value) {
    info!("Initializing volume control from configuration");
    
    if let Some(volume_config) = get_service_config(config, "volume") {
        // Check if volume control is enabled
        let enabled = volume_config
            .get("enable")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        
        if !enabled {
            info!("Volume control is disabled in configuration");
            // Initialize with a dummy control that's not available
            let mut dummy_control = DummyVolumeControl::new(
                "disabled".to_string(),
                "Disabled Volume Control".to_string(),
                0.0
            );
            dummy_control.set_available(false);
            let dummy_control: Box<dyn VolumeControl + Send + Sync> = Box::new(dummy_control);
            let _ = GLOBAL_VOLUME_CONTROL.set(Arc::new(Mutex::new(dummy_control)));
            return;
        }
        
        // Get the volume control type
        let control_type = volume_config
            .get("type")
            .and_then(|v| v.as_str())
            .unwrap_or("dummy");
        
        let control: Box<dyn VolumeControl + Send + Sync> = match control_type {
            #[cfg(all(feature = "alsa", not(windows)))]
            "alsa" => {
                let device = volume_config
                    .get("device")
                    .and_then(|v| v.as_str())
                    .unwrap_or("default");
                
                let control_name = volume_config
                    .get("control_name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Master");
                
                let display_name = volume_config
                    .get("display_name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Master Volume");
                
                match AlsaVolumeControl::new(device.to_string(), control_name.to_string(), display_name.to_string()) {
                    Ok(alsa_control) => {
                        info!("Successfully initialized ALSA volume control on device '{}', control '{}'", device, control_name);
                        Box::new(alsa_control)
                    }
                    Err(e) => {
                        error!("Failed to initialize ALSA volume control: {}. Falling back to dummy control.", e);
                        let mut dummy_control = DummyVolumeControl::new(
                            "alsa_fallback".to_string(),
                            "ALSA Fallback".to_string(),
                            50.0
                        );
                        dummy_control.set_available(false);
                        Box::new(dummy_control)
                    }
                }
            }
            #[cfg(not(all(feature = "alsa", not(windows))))]
            "alsa" => {
                warn!("ALSA volume control requested but ALSA support not compiled in. Falling back to dummy control.");
                let mut dummy_control = DummyVolumeControl::new(
                    "alsa_not_available".to_string(),
                    "ALSA Not Available".to_string(),
                    50.0
                );
                dummy_control.set_available(false);
                Box::new(dummy_control)
            }
            "dummy" => {
                info!("Initialized dummy volume control for testing");
                Box::new(DummyVolumeControl::new(
                    "dummy".to_string(),
                    "Dummy Volume Control".to_string(),
                    50.0
                ))
            }
            _ => {
                warn!("Unknown volume control type '{}', falling back to dummy control", control_type);
                let mut dummy_control = DummyVolumeControl::new(
                    "unknown_fallback".to_string(),
                    "Unknown Type Fallback".to_string(),
                    50.0
                );
                dummy_control.set_available(false);
                Box::new(dummy_control)
            }
        };
        
        // Store the global volume control
        if let Err(_) = GLOBAL_VOLUME_CONTROL.set(Arc::new(Mutex::new(control))) {
            error!("Failed to set global volume control - already initialized");
        } else {
            info!("Global volume control initialized successfully");
        }
    } else {
        info!("No volume configuration found, disabling volume control");
        // Initialize with a dummy control that's not available
        let mut dummy_control = DummyVolumeControl::new(
            "no_config".to_string(),
            "No Config Volume Control".to_string(),
            0.0
        );
        dummy_control.set_available(false);
        let dummy_control: Box<dyn VolumeControl + Send + Sync> = Box::new(dummy_control);
        let _ = GLOBAL_VOLUME_CONTROL.set(Arc::new(Mutex::new(dummy_control)));
    }
}

/// Get the global volume control instance
/// 
/// # Returns
/// 
/// An Arc<Mutex<Box<dyn VolumeControl + Send + Sync>>> if initialized, error otherwise
pub fn get_global_volume_control() -> Result<Arc<Mutex<Box<dyn VolumeControl + Send + Sync>>>, Box<dyn std::error::Error>> {
    GLOBAL_VOLUME_CONTROL.get()
        .cloned()
        .ok_or_else(|| "Volume control not initialized".into())
}

/// Get the current volume as a percentage (0-100%)
/// 
/// # Returns
/// 
/// The current volume percentage, or None if volume control is not available
pub fn get_volume_percentage() -> Option<f64> {
    get_global_volume_control().ok()?.lock().ok()?.get_volume_percent().ok()
}

/// Set the volume as a percentage (0-100%)
/// 
/// # Arguments
/// 
/// * `percentage` - Volume level as a percentage (0.0 to 100.0)
/// 
/// # Returns
/// 
/// true if the volume was set successfully, false otherwise
pub fn set_volume_percentage(percentage: f64) -> bool {
    if let Ok(control) = get_global_volume_control() {
        if let Ok(control) = control.lock() {
            return control.set_volume_percent(percentage).is_ok();
        }
    }
    false
}

/// Get the current volume in decibels
/// 
/// # Returns
/// 
/// The current volume in dB, or None if volume control is not available or doesn't support dB
pub fn get_volume_db() -> Option<f64> {
    get_global_volume_control().ok()?.lock().ok()?.get_volume_db().ok()
}

/// Set the volume in decibels
/// 
/// # Arguments
/// 
/// * `db` - Volume level in decibels
/// 
/// # Returns
/// 
/// true if the volume was set successfully, false otherwise
pub fn set_volume_db(db: f64) -> bool {
    if let Ok(control) = get_global_volume_control() {
        if let Ok(control) = control.lock() {
            return control.set_volume_db(db).is_ok();
        }
    }
    false
}

/// Check if volume control is available
/// 
/// # Returns
/// 
/// true if volume control is available and functional, false otherwise
pub fn is_volume_control_available() -> bool {
    if let Ok(control) = get_global_volume_control() {
        if let Ok(control) = control.lock() {
            return control.is_available();
        }
    }
    false
}

/// Get volume control information
/// 
/// # Returns
/// 
/// VolumeControlInfo if available, None otherwise
pub fn get_volume_control_info() -> Option<crate::helpers::volume::VolumeControlInfo> {
    Some(get_global_volume_control().ok()?.lock().ok()?.get_info())
}

/// Start monitoring volume changes and call the provided callback for each change
/// 
/// # Arguments
/// 
/// * `callback` - Function to call when volume changes are detected
/// 
/// # Returns
/// 
/// A receiver for volume change events, or an error if monitoring cannot be started
pub fn start_volume_change_monitoring<F>(callback: F) -> Result<mpsc::Receiver<VolumeChangeEvent>, Box<dyn std::error::Error>>
where 
    F: Fn(VolumeChangeEvent) + Send + 'static,
{
    let control = get_global_volume_control()?;
    let control = control.lock().map_err(|e| format!("Failed to lock volume control: {}", e))?;
    control.start_change_monitoring(Box::new(callback))
        .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)
}

/// Check if the current volume control supports change monitoring
/// 
/// # Returns
/// 
/// true if change monitoring is supported, false otherwise
pub fn supports_volume_change_monitoring() -> bool {
    if let Ok(control) = get_global_volume_control() {
        if let Ok(control) = control.lock() {
            return control.supports_change_monitoring();
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // Since GLOBAL_VOLUME_CONTROL is a OnceCell, we can only set it once per test run
    // These tests demonstrate the functionality but may interfere with each other
    // In a real application, you'd want separate instances for testing
    
    #[test]
    fn test_volume_control_api() {
        // Test the volume control API functions regardless of which control is initialized
        
        // These functions should not panic even if no volume control is available
        let _available = is_volume_control_available();
        let _volume = get_volume_percentage();
        let _db_volume = get_volume_db();
        let _info = get_volume_control_info();
        
        // Set operations should return false if no control is available, true if successful
        let set_result = set_volume_percentage(75.0);
        let set_db_result = set_volume_db(-10.0);
        
        // These are successful if they don't panic
        println!("Volume control available: {}", _available);
        println!("Set percentage result: {}", set_result);
        println!("Set dB result: {}", set_db_result);
    }

    #[test]
    fn test_dummy_volume_control_creation() {
        // Test creating dummy volume controls directly
        let dummy_control = DummyVolumeControl::new(
            "test".to_string(),
            "Test Control".to_string(),
            50.0
        );
        
        assert!(dummy_control.is_available());
        assert_eq!(dummy_control.get_volume_percent().unwrap(), 50.0);
        
        let info = dummy_control.get_info();
        assert_eq!(info.internal_name, "test");
        assert_eq!(info.display_name, "Test Control");
        assert!(info.decibel_range.is_some());
    }

    #[test] 
    fn test_config_parsing() {
        // Test configuration parsing without setting global state
        let dummy_config = json!({
            "services": {
                "volume": {
                    "enable": true,
                    "type": "dummy",
                    "display_name": "Test Volume"
                }
            }
        });
        
        let volume_config = get_service_config(&dummy_config, "volume");
        assert!(volume_config.is_some());
        
        let enabled = volume_config.unwrap()
            .get("enable")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        assert!(enabled);
    }

    #[test]
    fn test_disabled_config() {
        let disabled_config = json!({
            "services": {
                "volume": {
                    "enable": false,
                    "type": "dummy"
                }
            }
        });
        
        let volume_config = get_service_config(&disabled_config, "volume");
        assert!(volume_config.is_some());
        
        let enabled = volume_config.unwrap()
            .get("enable")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        assert!(!enabled);
    }
}
