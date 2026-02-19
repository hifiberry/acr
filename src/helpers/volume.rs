use std::error::Error;
use std::fmt;
use std::thread;
use std::time::Duration;
use std::sync::Arc;
use parking_lot::RwLock;

use crate::data::PlayerEvent;
use crate::audiocontrol::eventbus::EventBus;

/// Error types for volume control operations
#[derive(Debug)]
pub enum VolumeError {
    /// Device not found or inaccessible
    DeviceError(String),
    /// Control not found on device
    ControlNotFound(String),
    /// Volume value out of range
    InvalidRange(String),
    /// ALSA library error
    AlsaError(String),
    /// Generic I/O error
    IoError(String),
    /// Feature not supported by this control
    NotSupported(String),
}

impl fmt::Display for VolumeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VolumeError::DeviceError(msg) => write!(f, "Device error: {}", msg),
            VolumeError::ControlNotFound(msg) => write!(f, "Control not found: {}", msg),
            VolumeError::InvalidRange(msg) => write!(f, "Invalid range: {}", msg),
            VolumeError::AlsaError(msg) => write!(f, "ALSA error: {}", msg),
            VolumeError::IoError(msg) => write!(f, "I/O error: {}", msg),
            VolumeError::NotSupported(msg) => write!(f, "Not supported: {}", msg),
        }
    }
}

impl Error for VolumeError {}

/// Publish a volume change event to the global event bus
fn publish_volume_change_event(
    control_name: String,
    display_name: String,
    percentage: f64,
    decibels: Option<f64>,
    raw_value: Option<i64>,
) {
    log::debug!("Publishing volume change event: {} ({}) -> {:.1}% ({} dB) [raw: {}]",
               display_name, control_name, percentage,
               decibels.map(|db| format!("{:.1}", db)).unwrap_or_else(|| "N/A".to_string()),
               raw_value.map(|r| r.to_string()).unwrap_or_else(|| "N/A".to_string()));
    
    let event = PlayerEvent::VolumeChanged {
        control_name,
        display_name,
        percentage,
        decibels,
        raw_value,
    };
    
    let event_bus = EventBus::instance();
    event_bus.publish(event);
}

/// Volume change event
#[derive(Debug, Clone)]
pub struct VolumeChangeEvent {
    /// Control that changed
    pub control_name: String,
    /// New volume percentage
    pub new_percentage: f64,
    /// New volume in dB (if available)
    pub new_db: Option<f64>,
}

/// Trait for receiving volume change notifications
pub trait VolumeChangeListener {
    /// Called when volume changes
    fn on_volume_change(&self, event: VolumeChangeEvent);
}

/// Represents a decibel range for volume controls that support dB scale
#[derive(Debug, Clone)]
pub struct DecibelRange {
    /// Minimum dB value (typically negative)
    pub min_db: f64,
    /// Maximum dB value
    pub max_db: f64,
}

impl DecibelRange {
    pub fn new(min_db: f64, max_db: f64) -> Self {
        Self { min_db, max_db }
    }

    /// Convert percentage (0-100) to decibel value within this range
    pub fn percent_to_db(&self, percent: f64) -> f64 {
        if percent <= 0.0 {
            self.min_db
        } else if percent >= 100.0 {
            self.max_db
        } else {
            self.min_db + (percent / 100.0) * (self.max_db - self.min_db)
        }
    }

    /// Convert decibel value to percentage (0-100) within this range
    pub fn db_to_percent(&self, db: f64) -> f64 {
        if db <= self.min_db {
            0.0
        } else if db >= self.max_db {
            100.0
        } else {
            ((db - self.min_db) / (self.max_db - self.min_db)) * 100.0
        }
    }
}

/// Information about a volume control
#[derive(Debug, Clone)]
pub struct VolumeControlInfo {
    /// Internal name used by the system
    pub internal_name: String,
    /// Display name for UI
    pub display_name: String,
    /// Optional decibel range if supported
    pub decibel_range: Option<DecibelRange>,
}

impl VolumeControlInfo {
    pub fn new(internal_name: String, display_name: String) -> Self {
        Self {
            internal_name,
            display_name,
            decibel_range: None,
        }
    }

    pub fn with_decibel_range(mut self, range: DecibelRange) -> Self {
        self.decibel_range = Some(range);
        self
    }
}

/// Trait for volume control operations
pub trait VolumeControl {
    /// Get the current volume as a percentage (0-100)
    fn get_volume_percent(&self) -> Result<f64, VolumeError>;

    /// Set the volume as a percentage (0-100)
    fn set_volume_percent(&self, percent: f64) -> Result<(), VolumeError>;

    /// Get the current volume in decibels (if supported)
    fn get_volume_db(&self) -> Result<f64, VolumeError> {
        if let Some(db_range) = self.get_info().decibel_range {
            let percent = self.get_volume_percent()?;
            Ok(db_range.percent_to_db(percent))
        } else {
            Err(VolumeError::NotSupported("Decibel control not supported".to_string()))
        }
    }

    /// Set the volume in decibels (if supported)
    fn set_volume_db(&self, db: f64) -> Result<(), VolumeError> {
        if let Some(db_range) = self.get_info().decibel_range {
            let percent = db_range.db_to_percent(db);
            self.set_volume_percent(percent)
        } else {
            Err(VolumeError::NotSupported("Decibel control not supported".to_string()))
        }
    }

    /// Get information about this volume control
    fn get_info(&self) -> VolumeControlInfo;

    /// Check if the control is currently available/accessible
    fn is_available(&self) -> bool;

    /// Get the minimum and maximum raw values (implementation specific)
    fn get_raw_range(&self) -> Result<(i64, i64), VolumeError>;

    /// Get the current raw value (implementation specific)
    fn get_raw_value(&self) -> Result<i64, VolumeError>;

    /// Set the raw value (implementation specific)
    fn set_raw_value(&self, value: i64) -> Result<(), VolumeError>;

    /// Start monitoring for volume changes (if supported)
    fn start_change_monitoring(&self) -> Result<(), VolumeError> {
        Err(VolumeError::NotSupported("Volume change monitoring not supported".to_string()))
    }

    /// Check if change monitoring is supported
    fn supports_change_monitoring(&self) -> bool {
        false
    }
}

/// ALSA implementation of VolumeControl
#[cfg(all(feature = "alsa", not(windows)))]
pub struct AlsaVolumeControl {
    device: String,
    control_name: String,
    info: VolumeControlInfo,
}

#[cfg(all(feature = "alsa", not(windows)))]
impl AlsaVolumeControl {
    /// Create a new ALSA volume control
    /// 
    /// # Arguments
    /// * `device` - ALSA device name (e.g., "hw:0", "default")
    /// * `control_name` - ALSA control name (e.g., "Master", "PCM")
    /// * `display_name` - Human-readable name for UI
    pub fn new(device: String, control_name: String, display_name: String) -> Result<Self, VolumeError> {
        let internal_name = format!("alsa:{}:{}", device, control_name);
        let mut info = VolumeControlInfo::new(internal_name, display_name);

        // Try to determine if this control supports dB scale
        let control = Self {
            device: device.clone(),
            control_name: control_name.clone(),
            info: info.clone(),
        };

        // Attempt to get dB range
        if let Ok(db_range) = control.get_alsa_db_range() {
            info = info.with_decibel_range(db_range);
        }

        Ok(Self {
            device,
            control_name,
            info,
        })
    }

    /// Get the ALSA decibel range for this control
    fn get_alsa_db_range(&self) -> Result<DecibelRange, VolumeError> {
        use alsa::mixer::{Mixer, SelemId, MilliBel};
        
        let mixer = Mixer::new(&self.device, false)
            .map_err(|e| VolumeError::DeviceError(format!("Failed to open mixer {}: {}", self.device, e)))?;

        let selem_id = SelemId::new(&self.control_name, 0);
        let selem = mixer.find_selem(&selem_id)
            .ok_or_else(|| VolumeError::ControlNotFound(format!("Control '{}' not found on device '{}'", self.control_name, self.device)))?;

        // Check if playback volume dB range is available
        if selem.has_playback_volume() {
            let (min_db, max_db) = selem.get_playback_db_range();
            // Convert from ALSA's millibel to dB (millibel = 1/100 dB)
            let min_db_f = MilliBel::to_db(min_db) as f64;
            let max_db_f = MilliBel::to_db(max_db) as f64;
            
            // Validate and clamp dB values to reasonable ranges
            // ALSA sometimes returns extreme values that don't make sense
            let min_db_clamped = if min_db_f < -200.0 || min_db_f.is_infinite() || min_db_f.is_nan() {
                -120.0 // Default minimum for digital volume controls
            } else {
                min_db_f.max(-200.0) // Don't go below -200dB
            };
            
            let max_db_clamped = if max_db_f > 50.0 || max_db_f.is_infinite() || max_db_f.is_nan() {
                0.0 // Default maximum
            } else {
                max_db_f.min(50.0) // Don't go above +50dB
            };
            
            return Ok(DecibelRange::new(min_db_clamped, max_db_clamped));
        }

        // Check if capture volume dB range is available
        if selem.has_capture_volume() {
            let (min_db, max_db) = selem.get_capture_db_range();
            // Convert from ALSA's millibel to dB (millibel = 1/100 dB)
            let min_db_f = MilliBel::to_db(min_db) as f64;
            let max_db_f = MilliBel::to_db(max_db) as f64;
            
            // Validate and clamp dB values to reasonable ranges
            let min_db_clamped = if min_db_f < -200.0 || min_db_f.is_infinite() || min_db_f.is_nan() {
                -120.0
            } else {
                min_db_f.max(-200.0)
            };
            
            let max_db_clamped = if max_db_f > 50.0 || max_db_f.is_infinite() || max_db_f.is_nan() {
                0.0
            } else {
                max_db_f.min(50.0)
            };
            
            return Ok(DecibelRange::new(min_db_clamped, max_db_clamped));
        }

        Err(VolumeError::NotSupported("Decibel range not available for this control".to_string()))
    }

    /// Get the ALSA mixer and element for this control
    /// Returns only the selem since the mixer needs to be dropped before returning
    fn with_mixer_element<F, R>(&self, f: F) -> Result<R, VolumeError>
    where
        F: FnOnce(&alsa::mixer::Selem) -> Result<R, VolumeError>,
    {
        use alsa::mixer::{Mixer, SelemId};
        
        let mixer = Mixer::new(&self.device, false)
            .map_err(|e| VolumeError::DeviceError(format!("Failed to open mixer {}: {}", self.device, e)))?;

        let selem_id = SelemId::new(&self.control_name, 0);
        let selem = mixer.find_selem(&selem_id)
            .ok_or_else(|| VolumeError::ControlNotFound(format!("Control '{}' not found on device '{}'", self.control_name, self.device)))?;

        f(&selem)
    }
}

#[cfg(all(feature = "alsa", not(windows)))]
impl VolumeControl for AlsaVolumeControl {
    fn get_volume_percent(&self) -> Result<f64, VolumeError> {
        self.with_mixer_element(|selem| {
            // Try playback volume first, then capture volume
            if selem.has_playback_volume() {
                let (min, max) = selem.get_playback_volume_range();
                let current = selem.get_playback_volume(alsa::mixer::SelemChannelId::mono())
                    .map_err(|e| VolumeError::AlsaError(format!("Failed to get playback volume: {}", e)))?;
                
                if max > min {
                    let percent = ((current - min) as f64 / (max - min) as f64) * 100.0;
                    Ok(percent.clamp(0.0, 100.0))
                } else {
                    Ok(0.0)
                }
            } else if selem.has_capture_volume() {
                let (min, max) = selem.get_capture_volume_range();
                let current = selem.get_capture_volume(alsa::mixer::SelemChannelId::mono())
                    .map_err(|e| VolumeError::AlsaError(format!("Failed to get capture volume: {}", e)))?;
                
                if max > min {
                    let percent = ((current - min) as f64 / (max - min) as f64) * 100.0;
                    Ok(percent.clamp(0.0, 100.0))
                } else {
                    Ok(0.0)
                }
            } else {
                Err(VolumeError::NotSupported("Volume control not available".to_string()))
            }
        })
    }

    fn set_volume_percent(&self, percent: f64) -> Result<(), VolumeError> {
        if !(0.0..=100.0).contains(&percent) {
            return Err(VolumeError::InvalidRange(format!("Volume percentage {} is out of range (0-100)", percent)));
        }

        let result = self.with_mixer_element(|selem| {
            // Try playback volume first, then capture volume
            if selem.has_playback_volume() {
                let (min, max) = selem.get_playback_volume_range();
                let target_value = min + ((percent / 100.0) * (max - min) as f64) as i64;
                
                selem.set_playback_volume_all(target_value)
                    .map_err(|e| VolumeError::AlsaError(format!("Failed to set playback volume: {}", e)))?;
            } else if selem.has_capture_volume() {
                let (min, max) = selem.get_capture_volume_range();
                let target_value = min + ((percent / 100.0) * (max - min) as f64) as i64;
                
                selem.set_capture_volume_all(target_value)
                    .map_err(|e| VolumeError::AlsaError(format!("Failed to set capture volume: {}", e)))?;
            } else {
                return Err(VolumeError::NotSupported("Volume control not available".to_string()));
            }

            Ok(())
        });

        // If the volume was set successfully, publish an event
        if result.is_ok() {
            let current_db = self.get_volume_db().ok();
            let current_raw = self.get_raw_value().ok();
            
            log::debug!("ALSA volume set programmatically: {}:{} -> {:.1}% ({} dB) [raw: {}]",
                       self.device, self.control_name, percent,
                       current_db.map(|db| format!("{:.1}", db)).unwrap_or_else(|| "N/A".to_string()),
                       current_raw.map(|r| r.to_string()).unwrap_or_else(|| "N/A".to_string()));
            
            publish_volume_change_event(
                self.info.internal_name.clone(),
                self.info.display_name.clone(),
                percent,
                current_db,
                current_raw,
            );
        }

        result
    }

    fn get_info(&self) -> VolumeControlInfo {
        self.info.clone()
    }

    fn is_available(&self) -> bool {
        use alsa::mixer::{Mixer, SelemId};
        
        let mixer = match Mixer::new(&self.device, false) {
            Ok(mixer) => mixer,
            Err(_) => return false,
        };

        let selem_id = SelemId::new(&self.control_name, 0);
        mixer.find_selem(&selem_id).is_some()
    }

    fn get_raw_range(&self) -> Result<(i64, i64), VolumeError> {
        self.with_mixer_element(|selem| {
            if selem.has_playback_volume() {
                let (min, max) = selem.get_playback_volume_range();
                Ok((min, max))
            } else if selem.has_capture_volume() {
                let (min, max) = selem.get_capture_volume_range();
                Ok((min, max))
            } else {
                Err(VolumeError::NotSupported("Volume control not available".to_string()))
            }
        })
    }

    fn get_raw_value(&self) -> Result<i64, VolumeError> {
        self.with_mixer_element(|selem| {
            if selem.has_playback_volume() {
                selem.get_playback_volume(alsa::mixer::SelemChannelId::mono())
                    .map_err(|e| VolumeError::AlsaError(format!("Failed to get playback volume: {}", e)))
            } else if selem.has_capture_volume() {
                selem.get_capture_volume(alsa::mixer::SelemChannelId::mono())
                    .map_err(|e| VolumeError::AlsaError(format!("Failed to get capture volume: {}", e)))
            } else {
                Err(VolumeError::NotSupported("Volume control not available".to_string()))
            }
        })
    }

    fn set_raw_value(&self, value: i64) -> Result<(), VolumeError> {
        let result = self.with_mixer_element(|selem| {
            if selem.has_playback_volume() {
                selem.set_playback_volume_all(value)
                    .map_err(|e| VolumeError::AlsaError(format!("Failed to set playback volume: {}", e)))?;
            } else if selem.has_capture_volume() {
                selem.set_capture_volume_all(value)
                    .map_err(|e| VolumeError::AlsaError(format!("Failed to set capture volume: {}", e)))?;
            } else {
                return Err(VolumeError::NotSupported("Volume control not available".to_string()));
            }

            Ok(())
        });

        // If the volume was set successfully, publish an event
        if result.is_ok() {
            let current_percent = self.get_volume_percent().unwrap_or(0.0);
            let current_db = self.get_volume_db().ok();
            
            log::debug!("ALSA volume set via raw value: {}:{} -> {:.1}% ({} dB) [raw: {}]",
                       self.device, self.control_name, current_percent,
                       current_db.map(|db| format!("{:.1}", db)).unwrap_or_else(|| "N/A".to_string()),
                       value);
            
            publish_volume_change_event(
                self.info.internal_name.clone(),
                self.info.display_name.clone(),
                current_percent,
                current_db,
                Some(value),
            );
        }

        result
    }

    fn start_change_monitoring(&self) -> Result<(), VolumeError> {
        let device = self.device.clone();
        let control_name = self.control_name.clone();

        thread::spawn(move || {
            log::debug!("Starting ALSA volume change monitoring for {}:{}", device, control_name);
            
            // Simple polling-based implementation
            // In a real implementation, you'd use ALSA's event system
            let mut last_volume = None;
            
            loop {
                // Check volume every 100ms
                thread::sleep(Duration::from_millis(100));
                
                if let Ok(mixer) = alsa::mixer::Mixer::new(&device, false) {
                    let selem_id = alsa::mixer::SelemId::new(&control_name, 0);
                    
                    if let Some(selem) = mixer.find_selem(&selem_id) {
                        if selem.has_playback_volume() {
                            if let Ok(volume_raw) = selem.get_playback_volume(alsa::mixer::SelemChannelId::mono()) {
                                let (min_raw, max_raw) = selem.get_playback_volume_range();
                                let volume_percent = if max_raw > min_raw {
                                    ((volume_raw - min_raw) as f64 / (max_raw - min_raw) as f64) * 100.0
                                } else {
                                    0.0
                                };
                                
                                // Only send event if volume actually changed
                                if last_volume.is_none_or(|last: f64| (last - volume_percent).abs() > 0.1) {
                                    last_volume = Some(volume_percent);
                                    
                                    // Try to get dB value
                                    let db_value = if selem.has_playback_volume() {
                                        let (min_db, max_db) = selem.get_playback_db_range();
                                        let min_db_f = alsa::mixer::MilliBel::to_db(min_db) as f64;
                                        let max_db_f = alsa::mixer::MilliBel::to_db(max_db) as f64;
                                        let current_db = min_db_f + (volume_percent / 100.0) * (max_db_f - min_db_f);
                                        Some(current_db)
                                    } else {
                                        None
                                    };
                                    
                                    log::debug!("ALSA volume change detected: {}:{} -> {:.1}% ({} dB) [raw: {}]",
                                               device, control_name, volume_percent, 
                                               db_value.map(|db| format!("{:.1}", db)).unwrap_or_else(|| "N/A".to_string()),
                                               volume_raw);
                                    
                                    // Publish to global event bus
                                    publish_volume_change_event(
                                        format!("alsa:{}:{}", device, control_name),
                                        format!("ALSA {}", control_name),
                                        volume_percent,
                                        db_value,
                                        Some(volume_raw),
                                    );
                                }
                            }
                        }
                    } else {
                        log::debug!("ALSA volume control {}:{} not found or unavailable, retrying...", device, control_name);
                    }
                } else {
                    log::debug!("ALSA mixer device {} unavailable, retrying...", device);
                }
            }
        });

        Ok(())
    }

    fn supports_change_monitoring(&self) -> bool {
        true
    }
}

/// Dummy implementation of VolumeControl for testing
/// 
/// This implementation doesn't control any real hardware and is primarily used for unit tests.
/// It simulates a volume control with a range from -120dB to 0dB.
pub struct DummyVolumeControl {
    info: VolumeControlInfo,
    current_percent: Arc<RwLock<f64>>,
    is_available: bool,
}

impl DummyVolumeControl {
    /// Create a new dummy volume control
    /// 
    /// # Arguments
    /// * `internal_name` - Internal name for the control
    /// * `display_name` - Human-readable name for UI
    /// * `initial_percent` - Initial volume percentage (0-100)
    pub fn new(internal_name: String, display_name: String, initial_percent: f64) -> Self {
        let db_range = DecibelRange::new(-120.0, 0.0);
        let info = VolumeControlInfo::new(internal_name, display_name)
            .with_decibel_range(db_range);
        
        Self {
            info,
            current_percent: Arc::new(RwLock::new(initial_percent.clamp(0.0, 100.0))),
            is_available: true,
        }
    }

    /// Create a new dummy volume control with default settings
    pub fn new_default() -> Self {
        Self::new(
            "dummy:test".to_string(),
            "Test Volume Control".to_string(),
            50.0
        )
    }

    /// Set whether this control should appear as available
    pub fn set_available(&mut self, available: bool) {
        self.is_available = available;
    }

    /// Get the current volume percentage (for testing)
    pub fn get_current_percent(&self) -> f64 {
        *self.current_percent.read()
    }
}

impl VolumeControl for DummyVolumeControl {
    fn get_volume_percent(&self) -> Result<f64, VolumeError> {
        if !self.is_available {
            return Err(VolumeError::DeviceError("Dummy device not available".to_string()));
        }
        Ok(*self.current_percent.read())
    }

    fn set_volume_percent(&self, percent: f64) -> Result<(), VolumeError> {
        if !self.is_available {
            return Err(VolumeError::DeviceError("Dummy device not available".to_string()));
        }
        
        if !(0.0..=100.0).contains(&percent) {
            return Err(VolumeError::InvalidRange(format!("Volume percentage {} is out of range (0-100)", percent)));
        }

        // Update the current value
        *self.current_percent.write() = percent;
        
        // Publish volume change event
        let db_value = self.get_volume_db().ok();
        publish_volume_change_event(
            self.info.internal_name.clone(),
            self.info.display_name.clone(),
            percent,
            db_value,
            Some(percent as i64),
        );
        
        Ok(())
    }

    fn get_info(&self) -> VolumeControlInfo {
        self.info.clone()
    }

    fn is_available(&self) -> bool {
        self.is_available
    }

    fn get_raw_range(&self) -> Result<(i64, i64), VolumeError> {
        if !self.is_available {
            return Err(VolumeError::DeviceError("Dummy device not available".to_string()));
        }
        // Simulate a raw range from 0 to 100 (matching percentage)
        Ok((0, 100))
    }

    fn get_raw_value(&self) -> Result<i64, VolumeError> {
        if !self.is_available {
            return Err(VolumeError::DeviceError("Dummy device not available".to_string()));
        }
        Ok(*self.current_percent.read() as i64)
    }

    fn set_raw_value(&self, value: i64) -> Result<(), VolumeError> {
        if !self.is_available {
            return Err(VolumeError::DeviceError("Dummy device not available".to_string()));
        }
        
        if !(0..=100).contains(&value) {
            return Err(VolumeError::InvalidRange(format!("Raw value {} is out of range (0-100)", value)));
        }

        // Update the current value
        let percent = value as f64;
        *self.current_percent.write() = percent;
        
        // Publish volume change event
        let db_value = self.get_volume_db().ok();
        publish_volume_change_event(
            self.info.internal_name.clone(),
            self.info.display_name.clone(),
            percent,
            db_value,
            Some(value),
        );
        
        Ok(())
    }
}

/// Create a new ALSA volume control
/// 
/// # Arguments
/// * `device` - ALSA device name (e.g., "hw:0", "default")
/// * `control_name` - ALSA control name (e.g., "Master", "PCM")
/// * `display_name` - Human-readable name for UI
/// 
/// # Returns
/// A boxed VolumeControl trait object
#[cfg(all(feature = "alsa", not(windows)))]
pub fn create_alsa_volume_control(
    device: String, 
    control_name: String, 
    display_name: String
) -> Result<Box<dyn VolumeControl>, VolumeError> {
    let control = AlsaVolumeControl::new(device, control_name, display_name)?;
    Ok(Box::new(control))
}

/// Create a new dummy volume control
/// 
/// # Arguments
/// * `internal_name` - Internal name for the control
/// * `display_name` - Human-readable name for UI
/// * `initial_percent` - Initial volume percentage (0-100)
/// 
/// # Returns
/// A boxed VolumeControl trait object
pub fn create_dummy_volume_control(
    internal_name: String,
    display_name: String,
    initial_percent: f64
) -> Box<dyn VolumeControl> {
    let control = DummyVolumeControl::new(internal_name, display_name, initial_percent);
    Box::new(control)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decibel_range() {
        let range = DecibelRange::new(-60.0, 0.0);
        
        // Test percent to dB conversion
        assert_eq!(range.percent_to_db(0.0), -60.0);
        assert_eq!(range.percent_to_db(100.0), 0.0);
        assert_eq!(range.percent_to_db(50.0), -30.0);
        
        // Test dB to percent conversion
        assert_eq!(range.db_to_percent(-60.0), 0.0);
        assert_eq!(range.db_to_percent(0.0), 100.0);
        assert_eq!(range.db_to_percent(-30.0), 50.0);
        
        // Test edge cases
        assert_eq!(range.percent_to_db(-10.0), -60.0); // Clamp to min
        assert_eq!(range.percent_to_db(110.0), 0.0);   // Clamp to max
        assert_eq!(range.db_to_percent(-70.0), 0.0);   // Clamp to min
        assert_eq!(range.db_to_percent(10.0), 100.0);  // Clamp to max
    }

    #[test]
    fn test_decibel_range_wide() {
        let range = DecibelRange::new(-120.0, 0.0);
        
        // Test wide range conversions
        assert_eq!(range.percent_to_db(0.0), -120.0);
        assert_eq!(range.percent_to_db(100.0), 0.0);
        assert_eq!(range.percent_to_db(25.0), -90.0);
        assert_eq!(range.percent_to_db(75.0), -30.0);
        
        assert_eq!(range.db_to_percent(-120.0), 0.0);
        assert_eq!(range.db_to_percent(0.0), 100.0);
        assert_eq!(range.db_to_percent(-90.0), 25.0);
        assert_eq!(range.db_to_percent(-30.0), 75.0);
    }

    #[test]
    fn test_volume_control_info() {
        let info = VolumeControlInfo::new("test".to_string(), "Test Control".to_string());
        assert_eq!(info.internal_name, "test");
        assert_eq!(info.display_name, "Test Control");
        assert!(info.decibel_range.is_none());
        
        let range = DecibelRange::new(-60.0, 0.0);
        let info_with_db = info.with_decibel_range(range);
        assert!(info_with_db.decibel_range.is_some());
        
        let db_range = info_with_db.decibel_range.unwrap();
        assert_eq!(db_range.min_db, -60.0);
        assert_eq!(db_range.max_db, 0.0);
    }

    #[test]
    fn test_dummy_volume_control_basic() {
        let control = DummyVolumeControl::new_default();
        
        // Test basic properties
        assert!(control.is_available());
        assert_eq!(control.get_current_percent(), 50.0);
        
        let info = control.get_info();
        assert_eq!(info.internal_name, "dummy:test");
        assert_eq!(info.display_name, "Test Volume Control");
        assert!(info.decibel_range.is_some());
        
        let db_range = info.decibel_range.unwrap();
        assert_eq!(db_range.min_db, -120.0);
        assert_eq!(db_range.max_db, 0.0);
    }

    #[test]
    fn test_dummy_volume_control_operations() {
        let control = DummyVolumeControl::new(
            "test_control".to_string(),
            "Test Control".to_string(),
            75.0
        );
        
        // Test volume operations
        assert_eq!(control.get_volume_percent().unwrap(), 75.0);
        assert!(control.set_volume_percent(50.0).is_ok());
        assert!(control.set_volume_percent(0.0).is_ok());
        assert!(control.set_volume_percent(100.0).is_ok());
        
        // Test invalid ranges
        assert!(control.set_volume_percent(-10.0).is_err());
        assert!(control.set_volume_percent(110.0).is_err());
    }

    #[test]
    fn test_dummy_volume_control_raw_operations() {
        let control = DummyVolumeControl::new_default();
        
        // Test raw operations
        let (min, max) = control.get_raw_range().unwrap();
        assert_eq!(min, 0);
        assert_eq!(max, 100);
        
        assert_eq!(control.get_raw_value().unwrap(), 50);
        
        assert!(control.set_raw_value(25).is_ok());
        assert!(control.set_raw_value(0).is_ok());
        assert!(control.set_raw_value(100).is_ok());
        
        // Test invalid raw values
        assert!(control.set_raw_value(-10).is_err());
        assert!(control.set_raw_value(110).is_err());
    }

    #[test]
    fn test_dummy_volume_control_availability() {
        let mut control = DummyVolumeControl::new_default();
        
        // Initially available
        assert!(control.is_available());
        assert!(control.get_volume_percent().is_ok());
        
        // Make unavailable
        control.set_available(false);
        assert!(!control.is_available());
        assert!(control.get_volume_percent().is_err());
        assert!(control.set_volume_percent(50.0).is_err());
        assert!(control.get_raw_range().is_err());
        assert!(control.get_raw_value().is_err());
        assert!(control.set_raw_value(50).is_err());
        
        // Make available again
        control.set_available(true);
        assert!(control.is_available());
        assert!(control.get_volume_percent().is_ok());
    }

    #[test]
    fn test_volume_control_db_operations() {
        let control = DummyVolumeControl::new_default();
        
        // Test dB operations (using default trait implementations)
        let current_db = control.get_volume_db().unwrap();
        // 50% of -120dB to 0dB range should be -60dB
        assert_eq!(current_db, -60.0);
        
        // Test setting dB values
        assert!(control.set_volume_db(-90.0).is_ok()); // Should be 25%
        assert!(control.set_volume_db(-30.0).is_ok()); // Should be 75%
        assert!(control.set_volume_db(0.0).is_ok());   // Should be 100%
        assert!(control.set_volume_db(-120.0).is_ok()); // Should be 0%
    }

    #[test]
    fn test_volume_control_without_db_support() {
        // Create a control without dB range
        let mut control = DummyVolumeControl::new_default();
        control.info.decibel_range = None;
        
        // dB operations should fail
        assert!(control.get_volume_db().is_err());
        assert!(control.set_volume_db(-60.0).is_err());
        
        // Percentage operations should still work
        assert!(control.get_volume_percent().is_ok());
        assert!(control.set_volume_percent(75.0).is_ok());
    }

    #[test]
    fn test_create_dummy_volume_control() {
        let control = create_dummy_volume_control(
            "factory_test".to_string(),
            "Factory Test Control".to_string(),
            25.0
        );
        
        assert_eq!(control.get_volume_percent().unwrap(), 25.0);
        
        let info = control.get_info();
        assert_eq!(info.internal_name, "factory_test");
        assert_eq!(info.display_name, "Factory Test Control");
        assert!(info.decibel_range.is_some());
    }

    #[test]
    fn test_volume_error_display() {
        let errors = vec![
            VolumeError::DeviceError("test device error".to_string()),
            VolumeError::ControlNotFound("test control".to_string()),
            VolumeError::InvalidRange("test range".to_string()),
            VolumeError::AlsaError("test alsa error".to_string()),
            VolumeError::IoError("test io error".to_string()),
            VolumeError::NotSupported("test not supported".to_string()),
        ];
        
        let expected_prefixes = vec![
            "Device error:",
            "Control not found:",
            "Invalid range:",
            "ALSA error:",
            "I/O error:",
            "Not supported:",
        ];
        
        for (error, expected_prefix) in errors.iter().zip(expected_prefixes.iter()) {
            let error_string = format!("{}", error);
            assert!(error_string.starts_with(expected_prefix));
        }
    }

    #[test]
    fn test_volume_control_trait_object() {
        // Test that we can use the trait as a trait object
        let controls: Vec<Box<dyn VolumeControl>> = vec![
            create_dummy_volume_control("test1".to_string(), "Test 1".to_string(), 30.0),
            create_dummy_volume_control("test2".to_string(), "Test 2".to_string(), 70.0),
        ];
        
        for control in controls {
            assert!(control.is_available());
            assert!(control.get_volume_percent().is_ok());
            assert!(control.get_info().internal_name.starts_with("test"));
        }
    }

    #[test]
    fn test_clamping_edge_cases() {
        let range = DecibelRange::new(-120.0, 0.0);
        
        // Test very small positive and negative numbers
        // Use approximate comparison for floating point precision
        let result = range.percent_to_db(0.001);
        assert!((result - (-119.9988)).abs() < 0.001); // Should be very close to min_db + small delta
        
        let result = range.percent_to_db(99.999);
        assert!((result - (-0.0012)).abs() < 0.001); // Should be very close to max_db - small delta
        
        // Test exact boundary values
        assert_eq!(range.db_to_percent(-120.0), 0.0);
        assert_eq!(range.db_to_percent(0.0), 100.0);
        
        // Test values just outside boundaries
        assert_eq!(range.db_to_percent(-120.1), 0.0);
        assert_eq!(range.db_to_percent(0.1), 100.0);
    }
}
