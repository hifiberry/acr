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

/// The domain a user-facing volume percentage lives in.
///
/// A hardware mixer's raw range is normally linear in decibels, so treating it
/// directly as a 0-100% scale puts nearly all of the audible change into the
/// top of the range: on a control spanning -103 dB to 0 dB, the slider's
/// mid-point sits around -52 dB, which is inaudible. `Perceptual` applies the
/// same normalisation `alsamixer` and PulseAudio use, which moves the useful
/// listening range into the middle of the slider.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VolumeScale {
    /// Percentage is a linear position within the raw hardware range.
    ///
    /// This is what ACR did unconditionally before the perceptual scale was
    /// added, and it remains the fallback for controls that expose no usable
    /// decibel information.
    Raw,
    /// Percentage follows a cube-root loudness curve over the control's dB
    /// range.
    Perceptual,
}

impl VolumeScale {
    /// The name used in configuration and in the REST API.
    pub fn as_str(&self) -> &'static str {
        match self {
            VolumeScale::Raw => "raw",
            VolumeScale::Perceptual => "perceptual",
        }
    }

    /// Parse a configured scale name, `None` if it is not recognised.
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "raw" | "linear" => Some(VolumeScale::Raw),
            "perceptual" | "normalized" | "normalised" => Some(VolumeScale::Perceptual),
            _ => None,
        }
    }
}

impl fmt::Display for VolumeScale {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
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
    /// Divisor for the normalised volume curve; see [`DecibelRange::normalized`].
    const NORM_DIVISOR: f64 = 60.0;

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

    /// Whether this range is wide enough to convert through.
    fn is_usable(&self) -> bool {
        self.min_db.is_finite() && self.max_db.is_finite() && self.max_db > self.min_db
    }

    /// Normalised loudness of `db`, relative to the top of the range.
    ///
    /// `10^(dB/20)` is amplitude, so `10^(dB/60)` is its cube root. That is the
    /// curve alsa-utils uses in `get_normalized_playback_volume()` and the one
    /// PulseAudio's software volumes follow. It is a convention rather than a
    /// psychoacoustic derivation, but it is the convention every other mixer on
    /// the system already agrees on.
    fn normalized(&self, db: f64) -> f64 {
        10f64.powf((db - self.max_db) / Self::NORM_DIVISOR)
    }

    /// Convert a perceptual percentage (0-100) to a decibel value.
    ///
    /// Falls back to the linear mapping for degenerate ranges.
    pub fn percent_to_db_perceptual(&self, percent: f64) -> f64 {
        if !self.is_usable() {
            return self.percent_to_db(percent);
        }
        if percent <= 0.0 {
            return self.min_db;
        }
        if percent >= 100.0 {
            return self.max_db;
        }

        let min_norm = self.normalized(self.min_db);
        let span = 1.0 - min_norm;
        if span <= f64::EPSILON {
            return self.percent_to_db(percent);
        }

        let target = min_norm + (percent / 100.0) * span;
        self.max_db + Self::NORM_DIVISOR * target.log10()
    }

    /// Convert a decibel value to a perceptual percentage (0-100).
    ///
    /// Falls back to the linear mapping for degenerate ranges.
    pub fn db_to_percent_perceptual(&self, db: f64) -> f64 {
        if !self.is_usable() {
            return self.db_to_percent(db);
        }
        if db <= self.min_db {
            return 0.0;
        }
        if db >= self.max_db {
            return 100.0;
        }

        let min_norm = self.normalized(self.min_db);
        let span = 1.0 - min_norm;
        if span <= f64::EPSILON {
            return self.db_to_percent(db);
        }

        (((self.normalized(db) - min_norm) / span) * 100.0).clamp(0.0, 100.0)
    }

    /// Convert a percentage to decibels using `scale`.
    pub fn percent_to_db_scaled(&self, percent: f64, scale: VolumeScale) -> f64 {
        match scale {
            VolumeScale::Raw => self.percent_to_db(percent),
            VolumeScale::Perceptual => self.percent_to_db_perceptual(percent),
        }
    }

    /// Convert decibels to a percentage using `scale`.
    pub fn db_to_percent_scaled(&self, db: f64, scale: VolumeScale) -> f64 {
        match scale {
            VolumeScale::Raw => self.db_to_percent(db),
            VolumeScale::Perceptual => self.db_to_percent_perceptual(db),
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
    /// Domain the user-facing percentage lives in
    pub scale: VolumeScale,
}

impl VolumeControlInfo {
    pub fn new(internal_name: String, display_name: String) -> Self {
        Self {
            internal_name,
            display_name,
            decibel_range: None,
            scale: VolumeScale::Raw,
        }
    }

    pub fn with_decibel_range(mut self, range: DecibelRange) -> Self {
        self.decibel_range = Some(range);
        self
    }

    pub fn with_scale(mut self, scale: VolumeScale) -> Self {
        self.scale = scale;
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
        let info = self.get_info();
        if let Some(db_range) = info.decibel_range {
            let percent = self.get_volume_percent()?;
            Ok(db_range.percent_to_db_scaled(percent, info.scale))
        } else {
            Err(VolumeError::NotSupported("Decibel control not supported".to_string()))
        }
    }

    /// Set the volume in decibels (if supported)
    fn set_volume_db(&self, db: f64) -> Result<(), VolumeError> {
        let info = self.get_info();
        if let Some(db_range) = info.decibel_range {
            let percent = db_range.db_to_percent_scaled(db, info.scale);
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

/// Decibel values below this are ALSA's "no gain at all" sentinel
/// (`SND_CTL_TLV_DB_GAIN_MUTE`) rather than a level the hardware can produce.
const DB_SENTINEL_FLOOR: f64 = -200.0;

/// How far above the bottom of the raw range to look for the quietest step that
/// carries a real decibel value.
const MUTE_PROBE_STEPS: i64 = 16;

/// Decide a control's usable decibel range from what it reports about itself.
///
/// The hardware lookups are injected rather than called, so this can be
/// exercised against a simulated control. That matters: inventing a range here
/// instead of probing for one is the defect behind issue #42, and it is not
/// reachable from a test that needs a real mixer.
fn resolve_db_range<F>(
    raw_range: Option<(i64, i64)>,
    reported_min_db: f64,
    reported_max_db: f64,
    db_for_raw: F,
) -> Result<DecibelRange, VolumeError>
where
    F: Fn(i64) -> Option<f64>,
{
    let mut min_db = reported_min_db;
    let mut max_db = reported_max_db;

    // Many DAC controls flag their bottom step as a hard mute, for which ALSA
    // reports SND_CTL_TLV_DB_GAIN_MUTE instead of a level. Probe upwards for
    // the quietest step that has a real dB value.
    if !min_db.is_finite() || min_db < DB_SENTINEL_FLOOR {
        let probed = raw_range.and_then(|(raw_min, raw_max)| {
            let limit = (raw_max - raw_min).min(MUTE_PROBE_STEPS);
            (1..=limit)
                .filter_map(|step| db_for_raw(raw_min + step))
                .find(|db| db.is_finite() && *db > DB_SENTINEL_FLOOR)
        });

        match probed {
            Some(db) => min_db = db,
            None => {
                return Err(VolumeError::NotSupported(
                    "Control reports no usable minimum decibel value".to_string(),
                ))
            }
        }
    }

    if !max_db.is_finite() {
        return Err(VolumeError::NotSupported(
            "Control reports no usable maximum decibel value".to_string(),
        ));
    }

    // Guard against implausible values from unusual drivers.
    min_db = min_db.max(DB_SENTINEL_FLOOR);
    max_db = max_db.min(50.0);

    if max_db <= min_db {
        return Err(VolumeError::NotSupported(format!(
            "Control reports a degenerate decibel range ({} .. {})",
            min_db, max_db
        )));
    }

    Ok(DecibelRange::new(min_db, max_db))
}

/// The raw step a requested percentage maps to under `scale`.
///
/// `raw_for_db` is the control's own dB-to-step lookup, injected so the mapping
/// can be tested without a mixer.
fn raw_for_percent<F>(
    percent: f64,
    raw_min: i64,
    raw_max: i64,
    range: Option<&DecibelRange>,
    scale: VolumeScale,
    raw_for_db: F,
) -> i64
where
    F: Fn(f64) -> Option<i64>,
{
    if scale == VolumeScale::Perceptual {
        // 0% means the bottom of the raw range, which on most DAC controls is a
        // real mute step below the quietest dB the TLV describes.
        if percent <= 0.0 {
            return raw_min;
        }
        if let Some(r) = range {
            let db = r.percent_to_db_perceptual(percent);
            if let Some(raw) = raw_for_db(db) {
                return raw.clamp(raw_min, raw_max);
            }
            log::debug!("Control does not support dB lookup, falling back to the raw scale");
        }
    }

    raw_min + ((percent / 100.0) * (raw_max - raw_min) as f64) as i64
}

/// The percentage and reportable decibel value for a control's current position.
fn state_from_reading(
    raw_min: i64,
    raw_max: i64,
    current_raw: i64,
    reported_db: Option<f64>,
    range: Option<&DecibelRange>,
    scale: VolumeScale,
) -> (f64, Option<f64>) {
    // A muted bottom step reports ALSA's sentinel rather than a real level.
    // Substitute the quietest audible level when one is known, and report
    // nothing when it is not -- a nonsensical -99999 dB must never reach a
    // client, whether or not this control has a usable range.
    let db = reported_db.and_then(|db| {
        if db < DB_SENTINEL_FLOOR {
            range.map(|r| r.min_db)
        } else {
            Some(range.map_or(db, |r| db.clamp(r.min_db, r.max_db)))
        }
    });

    let percent = match (scale, range, db) {
        (VolumeScale::Perceptual, Some(r), Some(db)) => r.db_to_percent_perceptual(db),
        _ if raw_max > raw_min => {
            (((current_raw - raw_min) as f64 / (raw_max - raw_min) as f64) * 100.0).clamp(0.0, 100.0)
        }
        _ => 0.0,
    };

    (percent, db)
}

/// The raw range of a mixer element, playback first then capture.
#[cfg(all(feature = "alsa", not(windows)))]
fn selem_raw_range(selem: &alsa::mixer::Selem) -> Option<(i64, i64)> {
    if selem.has_playback_volume() {
        Some(selem.get_playback_volume_range())
    } else if selem.has_capture_volume() {
        Some(selem.get_capture_volume_range())
    } else {
        None
    }
}

/// The current raw value of a mixer element.
#[cfg(all(feature = "alsa", not(windows)))]
fn selem_raw_value(selem: &alsa::mixer::Selem) -> Option<i64> {
    use alsa::mixer::SelemChannelId;

    if selem.has_playback_volume() {
        selem.get_playback_volume(SelemChannelId::mono()).ok()
    } else if selem.has_capture_volume() {
        selem.get_capture_volume(SelemChannelId::mono()).ok()
    } else {
        None
    }
}

/// The dB value the hardware reports for its current setting.
///
/// This asks ALSA rather than deriving a figure from the raw position, so it
/// reflects the control's actual TLV mapping.
#[cfg(all(feature = "alsa", not(windows)))]
fn selem_current_db(selem: &alsa::mixer::Selem) -> Option<f64> {
    use alsa::mixer::{MilliBel, SelemChannelId};

    if selem.has_playback_volume() {
        selem.get_playback_vol_db(SelemChannelId::mono()).ok().map(|mb| MilliBel::to_db(mb) as f64)
    } else if selem.has_capture_volume() {
        selem.get_capture_vol_db(SelemChannelId::mono()).ok().map(|mb| MilliBel::to_db(mb) as f64)
    } else {
        None
    }
}

/// The dB value a given raw step corresponds to, without touching the hardware.
#[cfg(all(feature = "alsa", not(windows)))]
fn selem_db_for_raw(selem: &alsa::mixer::Selem, raw: i64) -> Option<f64> {
    use alsa::mixer::MilliBel;

    if selem.has_playback_volume() {
        selem.ask_playback_vol_db(raw).ok().map(|mb| MilliBel::to_db(mb) as f64)
    } else if selem.has_capture_volume() {
        selem.ask_capture_vol_db(raw).ok().map(|mb| MilliBel::to_db(mb) as f64)
    } else {
        None
    }
}

/// The raw step closest to `db` without exceeding it, without touching the
/// hardware.
#[cfg(all(feature = "alsa", not(windows)))]
fn selem_raw_for_db(selem: &alsa::mixer::Selem, db: f64) -> Option<i64> {
    use alsa::mixer::MilliBel;
    use alsa::Round;

    let mb = MilliBel::from_db(db as f32);
    if selem.has_playback_volume() {
        selem.ask_playback_db_vol(mb, Round::Floor).ok()
    } else if selem.has_capture_volume() {
        selem.ask_capture_db_vol(mb, Round::Floor).ok()
    } else {
        None
    }
}

/// Write a raw value to every channel of a mixer element.
#[cfg(all(feature = "alsa", not(windows)))]
fn selem_write_raw(selem: &alsa::mixer::Selem, value: i64) -> Result<(), VolumeError> {
    if selem.has_playback_volume() {
        selem.set_playback_volume_all(value)
            .map_err(|e| VolumeError::AlsaError(format!("Failed to set playback volume: {}", e)))
    } else if selem.has_capture_volume() {
        selem.set_capture_volume_all(value)
            .map_err(|e| VolumeError::AlsaError(format!("Failed to set capture volume: {}", e)))
    } else {
        Err(VolumeError::NotSupported("Volume control not available".to_string()))
    }
}

/// Read percentage, decibels and raw value from a mixer element in one go.
///
/// Both the polled getters and the change-monitoring thread go through this, so
/// the REST API and the WebSocket events cannot drift apart.
#[cfg(all(feature = "alsa", not(windows)))]
fn selem_read_state(
    selem: &alsa::mixer::Selem,
    range: Option<&DecibelRange>,
    scale: VolumeScale,
) -> Result<(f64, Option<f64>, i64), VolumeError> {
    let (min, max) = selem_raw_range(selem)
        .ok_or_else(|| VolumeError::NotSupported("Volume control not available".to_string()))?;
    let current = selem_raw_value(selem)
        .ok_or_else(|| VolumeError::AlsaError("Failed to get current volume".to_string()))?;

    let (percent, db) =
        state_from_reading(min, max, current, selem_current_db(selem), range, scale);

    Ok((percent, db, current))
}

/// The raw step a requested percentage maps to under `scale`.
#[cfg(all(feature = "alsa", not(windows)))]
fn selem_raw_for_percent(
    selem: &alsa::mixer::Selem,
    percent: f64,
    range: Option<&DecibelRange>,
    scale: VolumeScale,
) -> Result<i64, VolumeError> {
    let (min, max) = selem_raw_range(selem)
        .ok_or_else(|| VolumeError::NotSupported("Volume control not available".to_string()))?;

    Ok(raw_for_percent(percent, min, max, range, scale, |db| {
        selem_raw_for_db(selem, db)
    }))
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
        Self::with_scale(device, control_name, display_name, VolumeScale::Perceptual)
    }

    /// Create a new ALSA volume control using a specific percentage scale
    ///
    /// A control that exposes no usable decibel range falls back to
    /// [`VolumeScale::Raw`] whatever is requested, since the perceptual mapping
    /// has nothing to work from.
    pub fn with_scale(
        device: String,
        control_name: String,
        display_name: String,
        scale: VolumeScale,
    ) -> Result<Self, VolumeError> {
        let internal_name = format!("alsa:{}:{}", device, control_name);
        let mut info = VolumeControlInfo::new(internal_name, display_name);

        // Try to determine if this control supports dB scale
        let control = Self {
            device: device.clone(),
            control_name: control_name.clone(),
            info: info.clone(),
        };

        // Attempt to get dB range
        let effective_scale = match control.get_alsa_db_range() {
            Ok(db_range) => {
                log::debug!("ALSA control {}:{} reports {:.1} dB .. {:.1} dB",
                            device, control_name, db_range.min_db, db_range.max_db);
                info = info.with_decibel_range(db_range);
                scale
            }
            Err(e) => {
                if scale == VolumeScale::Perceptual {
                    log::info!("ALSA control {}:{} exposes no usable decibel range ({}), \
                                using the raw volume scale", device, control_name, e);
                }
                VolumeScale::Raw
            }
        };
        info = info.with_scale(effective_scale);

        Ok(Self {
            device,
            control_name,
            info,
        })
    }

    /// Get the ALSA decibel range for this control
    ///
    /// Returns an error when the control exposes no usable decibel information,
    /// rather than inventing a range: a fabricated range would be reported
    /// verbatim by the API and would silently distort every conversion built on
    /// top of it. The decision itself lives in [`resolve_db_range`].
    fn get_alsa_db_range(&self) -> Result<DecibelRange, VolumeError> {
        self.with_mixer_element(|selem| {
            let (min_mb, max_mb) = if selem.has_playback_volume() {
                selem.get_playback_db_range()
            } else if selem.has_capture_volume() {
                selem.get_capture_db_range()
            } else {
                return Err(VolumeError::NotSupported(
                    "Volume control not available".to_string(),
                ));
            };

            resolve_db_range(
                selem_raw_range(selem),
                alsa::mixer::MilliBel::to_db(min_mb) as f64,
                alsa::mixer::MilliBel::to_db(max_mb) as f64,
                |raw| selem_db_for_raw(selem, raw),
            )
        })
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

    /// Read the control once and announce the level it is actually at.
    ///
    /// Every write path goes through this, so an event never reports a
    /// requested level that the quantised hardware range did not deliver.
    fn publish_current_state(&self, context: &str) {
        let range = self.info.decibel_range.clone();
        let scale = self.info.scale;
        let state = self.with_mixer_element(|selem| selem_read_state(selem, range.as_ref(), scale));

        let Ok((percent, db, raw)) = state else {
            log::warn!("{}: could not read back {}:{} after a successful write",
                       context, self.device, self.control_name);
            return;
        };

        log::debug!("{}: {}:{} -> {:.1}% ({} dB) [raw: {}]",
                   context, self.device, self.control_name, percent,
                   db.map(|db| format!("{:.1}", db)).unwrap_or_else(|| "N/A".to_string()),
                   raw);

        publish_volume_change_event(
            self.info.internal_name.clone(),
            self.info.display_name.clone(),
            percent,
            db,
            Some(raw),
        );
    }
}

#[cfg(all(feature = "alsa", not(windows)))]
impl VolumeControl for AlsaVolumeControl {
    fn get_volume_percent(&self) -> Result<f64, VolumeError> {
        let range = self.info.decibel_range.clone();
        let scale = self.info.scale;
        self.with_mixer_element(|selem| {
            selem_read_state(selem, range.as_ref(), scale).map(|(percent, _, _)| percent)
        })
    }

    /// Read the decibel value the hardware actually reports.
    ///
    /// This deliberately does not use the trait's default implementation, which
    /// interpolates a figure from the percentage and would therefore describe
    /// ACR's own mapping rather than the control's.
    /// Having no usable decibel *range* does not make the current level
    /// unknown, so this reports whatever the control answers rather than
    /// refusing outright. Going through `selem_read_state` is what keeps it
    /// identical to the value the change-monitoring thread publishes.
    fn get_volume_db(&self) -> Result<f64, VolumeError> {
        let range = self.info.decibel_range.clone();
        let scale = self.info.scale;
        self.with_mixer_element(|selem| {
            let (_, db, _) = selem_read_state(selem, range.as_ref(), scale)?;
            db.ok_or_else(|| VolumeError::NotSupported(
                "Control did not report a decibel value".to_string(),
            ))
        })
    }

    /// Set the volume to an absolute decibel level.
    ///
    /// Goes to the hardware's own dB mapping instead of round-tripping through a
    /// percentage, so the level requested is the level applied.
    fn set_volume_db(&self, db: f64) -> Result<(), VolumeError> {
        let Some(range) = self.info.decibel_range.clone() else {
            return Err(VolumeError::NotSupported("Decibel control not supported".to_string()));
        };

        let target_db = db.clamp(range.min_db, range.max_db);
        let raw = self.with_mixer_element(|selem| {
            let (min, max) = selem_raw_range(selem)
                .ok_or_else(|| VolumeError::NotSupported("Volume control not available".to_string()))?;
            selem_raw_for_db(selem, target_db)
                .map(|raw| raw.clamp(min, max))
                .ok_or_else(|| VolumeError::NotSupported(
                    "Control does not support decibel lookup".to_string(),
                ))
        })?;

        self.set_raw_value(raw)
    }

    fn set_volume_percent(&self, percent: f64) -> Result<(), VolumeError> {
        if !(0.0..=100.0).contains(&percent) {
            return Err(VolumeError::InvalidRange(format!("Volume percentage {} is out of range (0-100)", percent)));
        }

        let range = self.info.decibel_range.clone();
        let scale = self.info.scale;

        let result = self.with_mixer_element(|selem| {
            let target_value = selem_raw_for_percent(selem, percent, range.as_ref(), scale)?;
            selem_write_raw(selem, target_value)?;

            Ok(())
        });

        // Report the level the hardware settled on rather than the one that was
        // asked for: the raw range is quantised, so a request lands on the
        // nearest step at or below it.
        if result.is_ok() {
            self.publish_current_state("ALSA volume set programmatically");
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
        let result = self.with_mixer_element(|selem| selem_write_raw(selem, value));

        // If the volume was set successfully, publish an event
        if result.is_ok() {
            self.publish_current_state("ALSA volume set via raw value");
        }

        result
    }

    fn start_change_monitoring(&self) -> Result<(), VolumeError> {
        let device = self.device.clone();
        let control_name = self.control_name.clone();
        let internal_name = self.info.internal_name.clone();
        let display_name = self.info.display_name.clone();
        let range = self.info.decibel_range.clone();
        let scale = self.info.scale;

        thread::spawn(move || {
            log::debug!("Starting ALSA volume change monitoring for {}:{}", device, control_name);

            // Simple polling-based implementation
            // In a real implementation, you'd use ALSA's event system
            let mut last_raw: Option<i64> = None;

            loop {
                // Check volume every 100ms
                thread::sleep(Duration::from_millis(100));

                let Ok(mixer) = alsa::mixer::Mixer::new(&device, false) else {
                    log::debug!("ALSA mixer device {} unavailable, retrying...", device);
                    continue;
                };

                let selem_id = alsa::mixer::SelemId::new(&control_name, 0);
                let Some(selem) = mixer.find_selem(&selem_id) else {
                    log::debug!("ALSA volume control {}:{} not found or unavailable, retrying...",
                                device, control_name);
                    continue;
                };

                // Share the conversion with the polled getters, so a monitored
                // change and a subsequent API read cannot disagree.
                let Ok((percent, db, raw)) = selem_read_state(&selem, range.as_ref(), scale) else {
                    continue;
                };

                // Compare raw values: the percentage is derived, and comparing
                // derived floats would miss a step near the quiet end of a
                // perceptual scale, where many raw steps map into one percent.
                if last_raw == Some(raw) {
                    continue;
                }
                last_raw = Some(raw);

                log::debug!("ALSA volume change detected: {}:{} -> {:.1}% ({} dB) [raw: {}]",
                            device, control_name, percent,
                            db.map(|db| format!("{:.1}", db)).unwrap_or_else(|| "N/A".to_string()),
                            raw);

                // Publish to global event bus
                publish_volume_change_event(
                    internal_name.clone(),
                    display_name.clone(),
                    percent,
                    db,
                    Some(raw),
                );
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

    /// The control from issue #42: a TAS5756 "Digital" mixer, raw 0-207, whose
    /// TLV describes 0.5 dB steps from -103.5 dB up to 0 dB.
    fn issue_42_range() -> DecibelRange {
        DecibelRange::new(-103.5, 0.0)
    }

    fn issue_42_db_for_raw(raw: i64) -> f64 {
        -103.5 + 0.5 * raw as f64
    }

    #[test]
    fn test_perceptual_scale_agrees_with_other_mixers() {
        let range = issue_42_range();

        // Measurements from issue #42: the raw value the hardware was at, and
        // the percentage MPD and AirPlay both reported for that same state.
        // ACR used to report 66.18 / 71.98 / 83.09 / 93.24 here, because it
        // read the raw position as a percentage directly.
        let measurements = [(137, 26.0), (149, 33.0), (172, 51.0), (193, 76.0)];

        for (raw, expected) in measurements {
            let db = issue_42_db_for_raw(raw);
            let percent = range.db_to_percent_perceptual(db);
            assert!(
                (percent - expected).abs() < 2.0,
                "raw {} ({:.1} dB) mapped to {:.2}%, other mixers report {:.0}%",
                raw, db, percent, expected
            );

            // The old linear-on-raw reading is what the issue complained about.
            let linear = ((raw as f64 / 207.0) * 100.0).clamp(0.0, 100.0);
            assert!(
                linear > percent + 10.0,
                "raw {} should read far lower perceptually than linearly", raw
            );
        }
    }

    #[test]
    fn test_perceptual_scale_moves_listening_range_up_the_slider() {
        let range = issue_42_range();

        // The mid-point of the slider used to sit at -51.75 dB, which is
        // inaudible; it should land at a real listening level instead.
        let mid = range.percent_to_db_perceptual(50.0);
        assert!((mid - (-17.58)).abs() < 0.05, "50% mapped to {:.2} dB", mid);
        assert!(mid > range.percent_to_db(50.0) + 30.0);

        // A -40 dB to -10 dB listening window should occupy a usable stretch of
        // travel rather than being crushed against the top.
        let low = range.db_to_percent_perceptual(-40.0);
        let high = range.db_to_percent_perceptual(-10.0);
        assert!(low > 15.0 && low < 25.0, "-40 dB at {:.1}%", low);
        assert!(high > 62.0 && high < 72.0, "-10 dB at {:.1}%", high);
        assert!(high - low > 40.0, "listening range spans only {:.1}%", high - low);
    }

    #[test]
    fn test_perceptual_round_trip() {
        let range = issue_42_range();

        for percent in [0.0, 1.0, 5.0, 17.0, 33.3, 50.0, 66.6, 88.0, 99.0, 100.0] {
            let db = range.percent_to_db_perceptual(percent);
            let back = range.db_to_percent_perceptual(db);
            assert!(
                (back - percent).abs() < 1e-6,
                "{}% round-tripped through {:.4} dB to {}%", percent, db, back
            );
        }
    }

    #[test]
    fn test_perceptual_scale_is_monotonic() {
        let range = issue_42_range();
        let mut previous = f64::NEG_INFINITY;

        for step in 0..=200 {
            let percent = step as f64 / 2.0;
            let db = range.percent_to_db_perceptual(percent);
            assert!(db > previous || percent == 0.0, "not monotonic at {}%", percent);
            assert!(db.is_finite(), "non-finite dB at {}%", percent);
            assert!(db >= range.min_db && db <= range.max_db, "{} dB out of range", db);
            previous = db;
        }
    }

    #[test]
    fn test_perceptual_scale_endpoints_and_clamping() {
        let range = issue_42_range();

        assert_eq!(range.percent_to_db_perceptual(0.0), -103.5);
        assert_eq!(range.percent_to_db_perceptual(100.0), 0.0);
        assert_eq!(range.percent_to_db_perceptual(-5.0), -103.5);
        assert_eq!(range.percent_to_db_perceptual(150.0), 0.0);

        assert_eq!(range.db_to_percent_perceptual(-103.5), 0.0);
        assert_eq!(range.db_to_percent_perceptual(0.0), 100.0);
        assert_eq!(range.db_to_percent_perceptual(-200.0), 0.0);
        assert_eq!(range.db_to_percent_perceptual(12.0), 100.0);
    }

    #[test]
    fn test_perceptual_scale_falls_back_on_degenerate_range() {
        // A control with no usable dB information keeps whatever the linear
        // mapping did, rather than dividing by a zero-width span. Compare with
        // NaN-tolerant equality: an infinite endpoint makes the linear mapping
        // itself produce NaN, and the point here is that the perceptual path
        // does not diverge from it.
        fn same(a: f64, b: f64) -> bool {
            a == b || (a.is_nan() && b.is_nan())
        }

        for range in [
            DecibelRange::new(0.0, 0.0),
            DecibelRange::new(6.0, -6.0),
            DecibelRange::new(f64::NEG_INFINITY, 0.0),
        ] {
            for percent in [0.0, 25.0, 50.0, 100.0] {
                let db = range.percent_to_db_perceptual(percent);
                assert!(
                    same(db, range.percent_to_db(percent)),
                    "{:?} at {}%: {} vs linear {}",
                    range, percent, db, range.percent_to_db(percent)
                );
            }
            assert!(same(
                range.db_to_percent_perceptual(-10.0),
                range.db_to_percent(-10.0)
            ));
        }
    }

    #[test]
    fn test_usable_ranges_never_produce_non_finite_values() {
        // get_alsa_db_range only ever hands out a finite, non-degenerate range,
        // and for those the perceptual conversions must stay finite: a NaN here
        // would reach the REST API, where it is not even valid JSON.
        for range in [
            issue_42_range(),
            DecibelRange::new(-120.0, 0.0),
            DecibelRange::new(-60.0, 6.0),
            DecibelRange::new(-3.0, 0.0),
        ] {
            for step in 0..=100 {
                let percent = step as f64;
                let db = range.percent_to_db_perceptual(percent);
                assert!(db.is_finite(), "{:?} at {}% gave {}", range, percent, db);
                assert!(range.db_to_percent_perceptual(db).is_finite());
            }
        }
    }

    #[test]
    fn test_scaled_conversions_dispatch_on_scale() {
        let range = issue_42_range();

        assert_eq!(
            range.percent_to_db_scaled(40.0, VolumeScale::Raw),
            range.percent_to_db(40.0)
        );
        assert_eq!(
            range.percent_to_db_scaled(40.0, VolumeScale::Perceptual),
            range.percent_to_db_perceptual(40.0)
        );
        assert_eq!(
            range.db_to_percent_scaled(-30.0, VolumeScale::Raw),
            range.db_to_percent(-30.0)
        );
        assert_eq!(
            range.db_to_percent_scaled(-30.0, VolumeScale::Perceptual),
            range.db_to_percent_perceptual(-30.0)
        );
    }

    #[test]
    fn test_volume_scale_parsing_and_naming() {
        assert_eq!(VolumeScale::parse("raw"), Some(VolumeScale::Raw));
        assert_eq!(VolumeScale::parse("linear"), Some(VolumeScale::Raw));
        assert_eq!(VolumeScale::parse(" Perceptual "), Some(VolumeScale::Perceptual));
        assert_eq!(VolumeScale::parse("NORMALIZED"), Some(VolumeScale::Perceptual));
        assert_eq!(VolumeScale::parse("loud"), None);

        assert_eq!(VolumeScale::Raw.as_str(), "raw");
        assert_eq!(VolumeScale::Perceptual.as_str(), "perceptual");
        assert_eq!(VolumeScale::Perceptual.to_string(), "perceptual");
    }

    #[test]
    fn test_volume_control_info_scale_defaults_to_raw() {
        let info = VolumeControlInfo::new("test".to_string(), "Test".to_string());
        assert_eq!(info.scale, VolumeScale::Raw);
        assert_eq!(info.with_scale(VolumeScale::Perceptual).scale, VolumeScale::Perceptual);
    }

    // ---- A simulated version of the control from issue #42 -----------------
    //
    // A TAS5756 "Digital" mixer: raw steps 0-207, TLV declaring 0.5 dB steps
    // from -103.5 dB, with the bottom step flagged as a hard mute. ALSA reports
    // SND_CTL_TLV_DB_GAIN_MUTE for that step, which is what the old code
    // mistook for a real -120 dB floor.

    const SIM_RAW_MIN: i64 = 0;
    const SIM_RAW_MAX: i64 = 207;
    const ALSA_MUTE_SENTINEL: f64 = -99999.99;

    /// The dB the simulated hardware reports for a raw step.
    fn sim_db_for_raw(raw: i64) -> Option<f64> {
        if raw <= SIM_RAW_MIN {
            Some(ALSA_MUTE_SENTINEL)
        } else if raw <= SIM_RAW_MAX {
            Some(-103.5 + 0.5 * raw as f64)
        } else {
            None
        }
    }

    /// The quietest step at or below `db`, the way ALSA rounds a dB request.
    fn sim_raw_for_db(db: f64) -> Option<i64> {
        let exact = (db + 103.5) / 0.5;
        Some((exact.floor() as i64).clamp(SIM_RAW_MIN, SIM_RAW_MAX))
    }

    fn sim_resolved_range() -> DecibelRange {
        resolve_db_range(
            Some((SIM_RAW_MIN, SIM_RAW_MAX)),
            ALSA_MUTE_SENTINEL,
            0.0,
            sim_db_for_raw,
        )
        .expect("the simulated control has a usable range")
    }

    #[test]
    fn test_db_range_probes_past_a_muted_bottom_step() {
        let range = sim_resolved_range();

        // The quietest audible step is raw 1, at -103.0 dB. The old code
        // reported -120.0 dB here, a figure the hardware never mentions.
        assert!(
            (range.min_db - (-103.0)).abs() < 1e-9,
            "expected the quietest audible step, got {}",
            range.min_db
        );
        assert_ne!(range.min_db, -120.0, "the invented floor is back");
        assert_eq!(range.max_db, 0.0);
    }

    #[test]
    fn test_db_range_is_refused_rather_than_invented() {
        // A control with no TLV at all: every lookup fails and both reported
        // bounds are zero. Advertising a made-up range would be worse than
        // admitting there is none.
        assert!(resolve_db_range(Some((0, 100)), 0.0, 0.0, |_| None).is_err());

        // Sentinel floor with nothing usable above it.
        assert!(resolve_db_range(
            Some((0, 100)),
            ALSA_MUTE_SENTINEL,
            0.0,
            |_| Some(ALSA_MUTE_SENTINEL)
        )
        .is_err());

        // An inverted range is degenerate, not a range.
        assert!(resolve_db_range(Some((0, 100)), 6.0, -6.0, sim_db_for_raw).is_err());

        // No raw range to probe through.
        assert!(resolve_db_range(None, ALSA_MUTE_SENTINEL, 0.0, sim_db_for_raw).is_err());
    }

    #[test]
    fn test_db_range_keeps_a_control_that_reports_honestly() {
        // Nothing to probe for: the reported floor is already a real level.
        let range = resolve_db_range(Some((0, 100)), -60.0, 0.0, |_| None)
            .expect("a control reporting a real range needs no probe");
        assert_eq!(range.min_db, -60.0);
        assert_eq!(range.max_db, 0.0);
    }

    #[test]
    fn test_setting_a_percentage_uses_the_perceptual_mapping() {
        let range = sim_resolved_range();

        // This is the reverse direction from issue #42: the reporter set ~68%
        // in the Web UI and MPD showed ~28%. Under the perceptual scale the
        // slider position and the resulting level agree with other mixers.
        let raw = raw_for_percent(
            50.0,
            SIM_RAW_MIN,
            SIM_RAW_MAX,
            Some(&range),
            VolumeScale::Perceptual,
            sim_raw_for_db,
        );

        let db = sim_db_for_raw(raw).unwrap();
        assert!(
            (db - (-17.58)).abs() < 0.6,
            "50% landed on raw {} ({:.2} dB), expected about -17.6 dB",
            raw, db
        );

        // The old linear-on-raw mapping would have put 50% at raw 103, roughly
        // -52 dB, which is inaudible.
        let linear = raw_for_percent(
            50.0,
            SIM_RAW_MIN,
            SIM_RAW_MAX,
            Some(&range),
            VolumeScale::Raw,
            sim_raw_for_db,
        );
        assert_eq!(linear, 103);
        assert!(raw > linear + 50, "perceptual 50% must sit far above raw 50%");
    }

    #[test]
    fn test_setting_zero_percent_reaches_the_mute_step() {
        let range = sim_resolved_range();

        // 0% has to reach the hard mute at the bottom of the raw range, not the
        // quietest audible step: min_db is -103.0 dB, one step above silence.
        let raw = raw_for_percent(
            0.0,
            SIM_RAW_MIN,
            SIM_RAW_MAX,
            Some(&range),
            VolumeScale::Perceptual,
            sim_raw_for_db,
        );
        assert_eq!(raw, SIM_RAW_MIN, "0% must mute, not merely go quiet");
        assert_eq!(sim_db_for_raw(raw), Some(ALSA_MUTE_SENTINEL));

        // And 100% has to reach the top.
        assert_eq!(
            raw_for_percent(100.0, SIM_RAW_MIN, SIM_RAW_MAX, Some(&range),
                            VolumeScale::Perceptual, sim_raw_for_db),
            SIM_RAW_MAX
        );
    }

    #[test]
    fn test_setting_a_percentage_falls_back_when_db_lookup_fails() {
        let range = sim_resolved_range();

        // A driver that refuses the dB lookup still has to get a sane step,
        // and it must be the linear one rather than nothing.
        let raw = raw_for_percent(
            25.0,
            SIM_RAW_MIN,
            SIM_RAW_MAX,
            Some(&range),
            VolumeScale::Perceptual,
            |_| None,
        );
        assert_eq!(raw, 51);

        // Same when the control has no dB range to work from at all.
        let raw = raw_for_percent(
            25.0, SIM_RAW_MIN, SIM_RAW_MAX, None, VolumeScale::Perceptual, sim_raw_for_db,
        );
        assert_eq!(raw, 51);
    }

    #[test]
    fn test_reading_a_position_reports_both_domains() {
        let range = sim_resolved_range();

        // The measurements from issue #42, read back through the same path the
        // REST API and the WebSocket events both use.
        for (raw, perceptual, linear) in [
            (137, 24.7, 66.2),
            (163, 41.9, 78.7),
            (193, 76.0, 93.2),
        ] {
            let (pct, db) = state_from_reading(
                SIM_RAW_MIN, SIM_RAW_MAX, raw, sim_db_for_raw(raw),
                Some(&range), VolumeScale::Perceptual,
            );
            assert!((pct - perceptual).abs() < 0.5, "raw {} gave {:.2}%", raw, pct);
            assert_eq!(db, sim_db_for_raw(raw), "dB must come from the hardware");

            let (pct, _) = state_from_reading(
                SIM_RAW_MIN, SIM_RAW_MAX, raw, sim_db_for_raw(raw),
                Some(&range), VolumeScale::Raw,
            );
            assert!((pct - linear).abs() < 0.5, "raw {} gave {:.2}% on the raw scale", raw, pct);
        }
    }

    #[test]
    fn test_reading_the_mute_step_reports_zero_not_a_sentinel() {
        let range = sim_resolved_range();

        let (pct, db) = state_from_reading(
            SIM_RAW_MIN, SIM_RAW_MAX, SIM_RAW_MIN, Some(ALSA_MUTE_SENTINEL),
            Some(&range), VolumeScale::Perceptual,
        );

        assert_eq!(pct, 0.0);
        // -99999.99 must not reach the API; it is not a level the hardware has.
        assert_eq!(db, Some(range.min_db));
    }

    #[test]
    fn test_reading_without_a_range_never_reports_the_sentinel() {
        // A control whose decibel range could not be resolved still answers
        // "what dB are you at now?", and at a muted step that answer is ALSA's
        // sentinel. There is no floor to substitute, so the only honest report
        // is no value at all -- publishing -99999.99 as a level is the same
        // class of defect as the invented -120 dB floor.
        let (pct, db) = state_from_reading(
            SIM_RAW_MIN, SIM_RAW_MAX, SIM_RAW_MIN, Some(ALSA_MUTE_SENTINEL),
            None, VolumeScale::Raw,
        );

        assert_eq!(db, None, "the ALSA sentinel must not reach a client");
        assert_eq!(pct, 0.0);
    }

    #[test]
    fn test_reading_without_a_range_still_reports_a_real_level() {
        // Having no usable *range* does not make the current level unknown.
        let (_, db) = state_from_reading(
            SIM_RAW_MIN, SIM_RAW_MAX, 137, Some(-35.0), None, VolumeScale::Raw,
        );
        assert_eq!(db, Some(-35.0));
    }

    #[test]
    fn test_reading_falls_back_to_raw_without_a_db_reading() {
        let range = sim_resolved_range();

        // Perceptual is requested but the control gave no dB value: the
        // percentage still has to be sane rather than zero.
        let (pct, db) = state_from_reading(
            SIM_RAW_MIN, SIM_RAW_MAX, 103, None, Some(&range), VolumeScale::Perceptual,
        );
        assert!((pct - 49.8).abs() < 0.5, "got {:.2}%", pct);
        assert_eq!(db, None);

        // A control with no raw span at all must not divide by zero.
        let (pct, _) = state_from_reading(5, 5, 5, None, None, VolumeScale::Perceptual);
        assert_eq!(pct, 0.0);
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
