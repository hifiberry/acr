use crate::helpers::volume::VolumeControl;
#[cfg(all(feature = "alsa", not(windows)))]
use crate::helpers::volume::AlsaVolumeControl;
use crate::helpers::volume::DummyVolumeControl;
use crate::helpers::configurator;
use std::sync::Arc;
use parking_lot::Mutex;
use once_cell::sync::OnceCell;
use log::{info, warn, error};
use serde_json::Value;
use crate::config::get_service_config;

/// Global volume control instance
static GLOBAL_VOLUME_CONTROL: OnceCell<Arc<Mutex<Box<dyn VolumeControl + Send + Sync>>>> = OnceCell::new();

/// Volume level saved when muting, restored on unmute. `None` means not muted.
///
/// Lock order is always GLOBAL_VOLUME_CONTROL first, then this. parking_lot
/// mutexes are not reentrant, so functions holding the volume guard must never
/// call the public helpers below.
static MUTE_STATE: Mutex<Option<f64>> = Mutex::new(None);

/// Initialize the global volume control from configuration
pub fn initialize_volume_control(config: &Value) {
    info!("Initializing volume control from configuration");
    
    if let Some(volume_config) = get_service_config(config, "volume") {
        // Check if volume control is enabled
        let enabled = volume_config
            .get("enable")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);  // Default to enabled
        
        if !enabled {
            info!("Volume control is explicitly disabled in configuration");
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
                    .unwrap_or("");
                
                let control_name = volume_config
                    .get("control_name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                
                let display_name = volume_config
                    .get("display_name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Master Volume");

                // Auto-detect device and control name from configurator API if not provided
                let (final_device, final_control_name) = if device.is_empty() || control_name.is_empty() {
                    info!("Auto-detecting ALSA volume settings from configurator API (device='{}', control_name='{}')", device, control_name);
                    
                    // Get retry configuration from volume config or use defaults
                    let retry_count = volume_config
                        .get("auto_detect_retry_count")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(2) as usize;
                    
                    let retry_delay_seconds = volume_config
                        .get("auto_detect_retry_delay_seconds")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(10);
                    
                    // Attempt to get system info with retries
                    let mut last_error = String::new();
                    let mut result: Option<(String, String)> = None;
                    
                    for attempt in 0..=retry_count {
                        if attempt > 0 {
                            info!("Retrying auto-detection after {} seconds (attempt {}/{})", retry_delay_seconds, attempt + 1, retry_count + 1);
                            std::thread::sleep(std::time::Duration::from_secs(retry_delay_seconds));
                        }
                        
                        match configurator::get_system_info() {
                            Ok(system_info) => {
                                let auto_device = if device.is_empty() {
                                    if let Some(soundcard) = &system_info.soundcard {
                                        if let Some(hw_index) = soundcard.hardware_index {
                                            format!("hw:{}", hw_index)
                                        } else {
                                            "default".to_string()
                                        }
                                    } else {
                                        "default".to_string()
                                    }
                                } else {
                                    device.to_string()
                                };

                                let auto_control_name = if control_name.is_empty() {
                                    if let Some(soundcard) = &system_info.soundcard {
                                        if let Some(vol_control) = &soundcard.volume_control {
                                            vol_control.clone()
                                        } else {
                                            "Master".to_string()
                                        }
                                    } else {
                                        "Master".to_string()
                                    }
                                } else {
                                    control_name.to_string()
                                };

                                info!("Auto-detected ALSA volume settings from configurator API: device='{}', control='{}'", auto_device, auto_control_name);
                                result = Some((auto_device, auto_control_name));
                                break;
                            }
                            Err(e) => {
                                last_error = e.to_string();
                                if attempt < retry_count {
                                    warn!("Failed to get system info from configurator API (attempt {}/{}): {}. Retrying...", attempt + 1, retry_count + 1, e);
                                } else {
                                    warn!("Failed to get system info from configurator API after {} attempts: {}", retry_count + 1, e);
                                }
                            }
                        }
                    }
                    
                    // Check if we got a result from the retry loop
                    if let Some((detected_device, detected_control)) = result {
                        (detected_device, detected_control)
                    } else {
                        // If all retries failed
                        // If both device and control_name were empty (auto-detection requested)
                        // and API fails after all retries, disable volume control
                        if device.is_empty() && control_name.is_empty() {
                            error!("Auto-detection failed after {} retries and no manual configuration provided. Disabling volume control.", retry_count + 1);
                            let mut dummy_control = DummyVolumeControl::new(
                                "auto_detection_failed".to_string(),
                                format!("Auto-detection Failed ({})", last_error),
                                0.0
                            );
                            dummy_control.set_available(false);
                            let dummy_control: Box<dyn VolumeControl + Send + Sync> = Box::new(dummy_control);
                            let _ = GLOBAL_VOLUME_CONTROL.set(Arc::new(Mutex::new(dummy_control)));
                            return;
                        }
                        
                        // Only use fallback if at least one value was explicitly configured
                        let fallback_device = if device.is_empty() { "default".to_string() } else { device.to_string() };
                        let fallback_control = if control_name.is_empty() { "Master".to_string() } else { control_name.to_string() };
                        warn!("Using fallback ALSA volume settings after auto-detection failure: device='{}', control='{}'", fallback_device, fallback_control);
                        (fallback_device, fallback_control)
                    }
                } else {
                    info!("Using configured ALSA volume settings: device='{}', control='{}'", device, control_name);
                    (device.to_string(), control_name.to_string())
                };
                
                match AlsaVolumeControl::new(final_device.clone(), final_control_name.clone(), display_name.to_string()) {
                    Ok(alsa_control) => {
                        info!("Successfully initialized ALSA volume control on device '{}', control '{}'", final_device, final_control_name);
                        log::debug!("ALSA volume control supports change monitoring: {}", alsa_control.supports_change_monitoring());
                        log::debug!("To start volume change monitoring, call start_volume_change_monitoring()");
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
                let internal_name = volume_config
                    .get("internal_name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("dummy");
                
                let display_name = volume_config
                    .get("display_name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Dummy Volume Control");
                
                let initial_percent = volume_config
                    .get("initial_percent")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(50.0);
                
                info!("Initialized dummy volume control '{}' with initial volume {}%", display_name, initial_percent);
                Box::new(DummyVolumeControl::new(
                    internal_name.to_string(),
                    display_name.to_string(),
                    initial_percent
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
        if GLOBAL_VOLUME_CONTROL.set(Arc::new(Mutex::new(control))).is_err() {
            error!("Failed to set global volume control - already initialized");
        } else {
            info!("Global volume control initialized successfully");
        }
    } else {
        info!("No volume configuration found, using dummy volume control");
        // Create a working dummy volume control
        let dummy_control: Box<dyn VolumeControl + Send + Sync> = Box::new(DummyVolumeControl::new(
            "no_config".to_string(),
            "Default Volume Control".to_string(),
            50.0
        ));
        
        if GLOBAL_VOLUME_CONTROL.set(Arc::new(Mutex::new(dummy_control))).is_err() {
            error!("Failed to set global volume control - already initialized");
        } else {
            info!("Dummy volume control initialized successfully");
        }
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
    get_global_volume_control().ok()?.lock().get_volume_percent().ok()
}

/// Set the volume as a percentage (0-100%)
///
/// An explicit volume set clears any saved mute level, so changing volume
/// while muted (e.g. via the WebUI) does not leave stale mute state behind.
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
        let ok = control.lock().set_volume_percent(percentage).is_ok();
        if ok {
            *MUTE_STATE.lock() = None;
        }
        return ok;
    }
    false
}

/// Adjust the volume by a relative amount, clamped to 0-100%.
///
/// The read-modify-write happens inside a single lock acquisition, unlike the
/// previous per-handler implementations in `api::volume`, which raced. This
/// matters under keyboard autorepeat, which drives ~20 adjustments/second.
///
/// Clears any saved mute level: adjusting volume is an explicit volume change.
///
/// # Arguments
///
/// * `delta` - Percentage points to add (negative to reduce)
///
/// # Returns
///
/// true if the volume was adjusted successfully, false otherwise
pub fn adjust_volume_percentage(delta: f64) -> bool {
    let Ok(control) = get_global_volume_control() else {
        return false;
    };
    let guard = control.lock();
    let Ok(current) = guard.get_volume_percent() else {
        return false;
    };
    let target = (current + delta).clamp(0.0, 100.0);
    let ok = guard.set_volume_percent(target).is_ok();
    drop(guard);
    if ok {
        *MUTE_STATE.lock() = None;
    }
    ok
}

/// Toggle mute.
///
/// Muting saves the current level and sets 0%; unmuting restores the saved
/// level. This replaces the previous behaviour of unmuting to a hardcoded 50%,
/// which blasted anyone listening below that.
///
/// Muting while already at 0% is a no-op: there is nothing meaningful to
/// restore later.
///
/// # Returns
///
/// true if the operation succeeded, false otherwise
pub fn toggle_mute() -> bool {
    let Ok(control) = get_global_volume_control() else {
        return false;
    };
    let guard = control.lock();
    let mut mute_state = MUTE_STATE.lock();

    match *mute_state {
        Some(saved) => {
            // Unmute: restore the pre-mute level.
            if guard.set_volume_percent(saved).is_ok() {
                *mute_state = None;
                true
            } else {
                false
            }
        }
        None => {
            let Ok(current) = guard.get_volume_percent() else {
                return false;
            };
            if current <= 0.0 {
                // Already silent; nothing worth saving.
                return true;
            }
            if guard.set_volume_percent(0.0).is_ok() {
                *mute_state = Some(current);
                true
            } else {
                false
            }
        }
    }
}

/// Whether the volume is currently muted via `toggle_mute`.
pub fn is_muted() -> bool {
    MUTE_STATE.lock().is_some()
}

/// Get the current volume in decibels
/// 
/// # Returns
/// 
/// The current volume in dB, or None if volume control is not available or doesn't support dB
pub fn get_volume_db() -> Option<f64> {
    get_global_volume_control().ok()?.lock().get_volume_db().ok()
}

/// Set the volume in decibels
///
/// An explicit volume set clears any saved mute level, so changing volume
/// while muted (e.g. via the WebUI) does not leave stale mute state behind.
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
        let ok = control.lock().set_volume_db(db).is_ok();
        if ok {
            *MUTE_STATE.lock() = None;
        }
        return ok;
    }
    false
}

/// Set the raw (implementation-specific) volume value.
///
/// An explicit volume set clears any saved mute level, so changing volume
/// while muted (e.g. via the WebUI) does not leave stale mute state behind.
///
/// # Arguments
///
/// * `raw` - Raw control value, implementation specific (see `get_raw_range`)
///
/// # Returns
///
/// true if the volume was set successfully, false otherwise
pub fn set_volume_raw(raw: i64) -> bool {
    if let Ok(control) = get_global_volume_control() {
        let ok = control.lock().set_raw_value(raw).is_ok();
        if ok {
            *MUTE_STATE.lock() = None;
        }
        return ok;
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
        return control.lock().is_available();
    }
    false
}

/// Get volume control information
/// 
/// # Returns
/// 
/// VolumeControlInfo if available, None otherwise
pub fn get_volume_control_info() -> Option<crate::helpers::volume::VolumeControlInfo> {
    Some(get_global_volume_control().ok()?.lock().get_info())
}

/// Start monitoring volume changes on the global volume control
/// 
/// # Returns
/// 
/// Ok(()) if monitoring was started successfully, or an error if monitoring cannot be started
pub fn start_volume_change_monitoring() -> Result<(), Box<dyn std::error::Error>> {
    log::debug!("Starting global volume change monitoring");
    let control = get_global_volume_control()?;
    let control = control.lock();
    
    let supports_monitoring = control.supports_change_monitoring();
    log::debug!("Global volume control supports change monitoring: {}", supports_monitoring);
    
    control.start_change_monitoring()
        .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)
}

/// Check if the current volume control supports change monitoring
/// 
/// # Returns
/// 
/// true if change monitoring is supported, false otherwise
pub fn supports_volume_change_monitoring() -> bool {
    if let Ok(control) = get_global_volume_control() {
        return control.lock().supports_change_monitoring();
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use serial_test::serial;

    // Since GLOBAL_VOLUME_CONTROL is a OnceCell, we can only set it once per test run
    // These tests demonstrate the functionality but may interfere with each other
    // In a real application, you'd want separate instances for testing
    
    #[test]
    #[serial]
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

    /// The global control is a OnceCell, so it can only be set once per test
    /// process. Initialise it as a dummy and reset to a known level per test.
    /// All tests here must be #[serial]: they share this one global.
    fn init_dummy_at(percent: f64) {
        initialize_volume_control(&json!({
            "services": { "volume": { "enable": true, "type": "dummy", "initial_percent": 50.0 } }
        }));
        assert!(set_volume_percentage(percent));
        assert!(!is_muted(), "mute state must be clear at test start");
    }

    #[test]
    #[serial]
    fn test_adjust_up_and_down() {
        init_dummy_at(50.0);
        assert!(adjust_volume_percentage(5.0));
        assert_eq!(get_volume_percentage(), Some(55.0));
        assert!(adjust_volume_percentage(-15.0));
        assert_eq!(get_volume_percentage(), Some(40.0));
    }

    #[test]
    #[serial]
    fn test_adjust_clamps_at_bounds() {
        init_dummy_at(98.0);
        assert!(adjust_volume_percentage(5.0));
        assert_eq!(get_volume_percentage(), Some(100.0));

        init_dummy_at(2.0);
        assert!(adjust_volume_percentage(-5.0));
        assert_eq!(get_volume_percentage(), Some(0.0));
    }

    /// The audiocontrol2 / old-API bug: unmuting must restore the previous
    /// level, not jump to a hardcoded 50%.
    #[test]
    #[serial]
    fn test_mute_restores_previous_level() {
        init_dummy_at(20.0);
        assert!(toggle_mute());
        assert!(is_muted());
        assert_eq!(get_volume_percentage(), Some(0.0));

        assert!(toggle_mute());
        assert!(!is_muted());
        assert_eq!(get_volume_percentage(), Some(20.0));
    }

    #[test]
    #[serial]
    fn test_mute_at_zero_is_noop() {
        init_dummy_at(0.0);
        assert!(toggle_mute());
        assert!(!is_muted(), "muting at 0% stores nothing to restore");
        assert_eq!(get_volume_percentage(), Some(0.0));
    }

    #[test]
    #[serial]
    fn test_explicit_set_clears_mute_state() {
        init_dummy_at(20.0);
        assert!(toggle_mute());
        assert!(is_muted());

        // e.g. the user moves the WebUI volume slider while muted.
        assert!(set_volume_percentage(35.0));
        assert!(!is_muted());

        // Toggling now mutes from 35, and restores to 35 -- not the stale 20.
        assert!(toggle_mute());
        assert_eq!(get_volume_percentage(), Some(0.0));
        assert!(toggle_mute());
        assert_eq!(get_volume_percentage(), Some(35.0));
    }

    #[test]
    #[serial]
    fn test_adjust_clears_mute_state() {
        init_dummy_at(20.0);
        assert!(toggle_mute());
        assert!(is_muted());

        // Pressing volume-up on the remote while muted is an explicit volume
        // change: it must not leave a stale restore level behind.
        assert!(adjust_volume_percentage(5.0));
        assert!(!is_muted());
        assert_eq!(get_volume_percentage(), Some(5.0));
    }

    /// `POST /volume/set` with `{"decibels": ...}` must clear stale mute state
    /// just like the percentage path, or a later mute press will "unmute" by
    /// raising volume to the old pre-mute level instead of muting.
    #[test]
    #[serial]
    fn test_set_db_clears_mute_state() {
        init_dummy_at(20.0);
        assert!(toggle_mute());
        assert!(is_muted());

        assert!(set_volume_db(-10.0));
        assert!(!is_muted());
    }

    /// Same as above for `POST /volume/set` with `{"raw_value": ...}`.
    #[test]
    #[serial]
    fn test_set_raw_clears_mute_state() {
        init_dummy_at(20.0);
        assert!(toggle_mute());
        assert!(is_muted());

        // DummyVolumeControl's raw range is 0..100, 1:1 with percent.
        assert!(set_volume_raw(50));
        assert!(!is_muted());
        assert_eq!(get_volume_percentage(), Some(50.0));
    }
}
