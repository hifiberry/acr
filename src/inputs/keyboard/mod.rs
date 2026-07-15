//! Keyboard / USB HID remote input source.
//!
//! The evdev dependency lives only in `evdev_source` (Linux-only). Config
//! parsing and the key-event rule live here, and are portable and unit-tested.

pub mod keymap;

#[cfg(target_os = "linux")]
pub mod evdev_source;

use crate::inputs::dispatch::ActionSink;
use crate::inputs::{Action, InputController, InputError};
use keymap::KeyMap;
use log::debug;
use parking_lot::Mutex;
use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Default volume percentage points per volume action. Matches audiocontrol2's
/// `change_volume_percent(5)`. Visible outside this module so `inputs::init_inputs`'s
/// per-source-type fallback can reference it instead of duplicating the literal.
pub(crate) const DEFAULT_VOLUME_STEP: f64 = 5.0;

/// Parsed `inputs.keyboard` configuration.
#[derive(Debug, Clone)]
pub struct KeyboardConfig {
    /// Whether to run the keyboard source at all.
    pub enable: bool,
    /// Volume percentage points per volume action.
    pub volume_step: f64,
    /// Whether to grab devices exclusively (EVIOCGRAB). Default false, matching
    /// audiocontrol2: keys still reach the console.
    pub grab: bool,
    /// Case-insensitive substring filter on device name. Empty matches all.
    pub device: String,
    /// Keycode -> action map.
    pub keymap: KeyMap,
}

impl KeyboardConfig {
    /// Parse from the `inputs.keyboard` config value. An absent value yields
    /// defaults: the source is enabled by default, as it was in audiocontrol2.
    pub fn from_config(value: Option<&serde_json::Value>) -> Self {
        let enable = value
            .and_then(|v| v.get("enable"))
            .and_then(|v| v.as_bool())
            .unwrap_or(true);

        let volume_step = value
            .and_then(|v| v.get("volume_step"))
            .and_then(|v| v.as_f64())
            .unwrap_or(DEFAULT_VOLUME_STEP);

        let grab = value
            .and_then(|v| v.get("grab"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let device = value
            .and_then(|v| v.get("device"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let keymap = KeyMap::from_config(value.and_then(|v| v.get("keymap")));

        KeyboardConfig { enable, volume_step, grab, device, keymap }
    }
}

/// Whether a device name passes the configured filter. An empty filter matches
/// everything.
pub fn device_name_matches(filter: &str, name: &str) -> bool {
    filter.is_empty() || name.to_lowercase().contains(&filter.to_lowercase())
}

/// Handle one key event, dispatching the mapped action if the repeat rule allows.
///
/// `value` follows the evdev convention: 0 = release, 1 = press, 2 = autorepeat.
/// Presses fire any action; autorepeat fires only actions where
/// [`Action::repeats_on_hold`] is true, so holding volume-up ramps but holding
/// next does not skip repeatedly. Releases are ignored.
///
/// Returns the action that fired, or `None`.
pub fn handle_key_event(
    keymap: &KeyMap,
    code: u16,
    value: i32,
    sink: &ActionSink,
) -> Option<Action> {
    let action = keymap.get(code)?;

    let fire = match value {
        1 => true,
        2 => action.repeats_on_hold(),
        _ => false,
    };
    if !fire {
        return None;
    }

    debug!("keyboard: key {} -> {}", code, action.as_str());
    sink.dispatch(action);
    Some(action)
}

/// A device the keyboard source is listening to.
#[derive(Debug, Clone, Serialize, Default)]
pub struct BoundDevice {
    pub path: String,
    pub name: String,
    pub matched_keys: Vec<String>,
}

/// The most recent mapped keypress, for diagnostics.
#[derive(Debug, Clone, Serialize)]
pub struct LastKey {
    pub code: u16,
    pub name: Option<String>,
    pub action: Option<String>,
    pub device: String,
}

/// Status reported by `GET /api/inputs`.
#[derive(Debug, Clone, Serialize, Default)]
pub struct KeyboardStatus {
    pub devices: Vec<BoundDevice>,
    pub last_key: Option<LastKey>,
}

/// The keyboard / USB HID remote input source.
pub struct KeyboardInput {
    config: KeyboardConfig,
    status: Arc<Mutex<KeyboardStatus>>,
    running: Arc<AtomicBool>,
}

impl KeyboardInput {
    pub fn new(config: KeyboardConfig) -> Self {
        KeyboardInput {
            config,
            status: Arc::new(Mutex::new(KeyboardStatus::default())),
            running: Arc::new(AtomicBool::new(false)),
        }
    }
}

impl InputController for KeyboardInput {
    fn name(&self) -> &str {
        "keyboard"
    }

    #[cfg(target_os = "linux")]
    fn start(&mut self, sink: ActionSink) -> Result<(), InputError> {
        self.running.store(true, Ordering::Relaxed);
        evdev_source::start_readers(
            &self.config,
            sink,
            self.status.clone(),
            self.running.clone(),
        )
    }

    #[cfg(not(target_os = "linux"))]
    fn start(&mut self, _sink: ActionSink) -> Result<(), InputError> {
        log::info!("keyboard: input devices are only supported on Linux");
        Ok(())
    }

    fn stop(&mut self) {
        self.running.store(false, Ordering::Relaxed);
    }

    fn status(&self) -> serde_json::Value {
        let status = self.status.lock().clone();
        serde_json::json!({
            "enabled": self.config.enable,
            "volume_step": self.config.volume_step,
            "grab": self.config.grab,
            "device_filter": self.config.device,
            "mapped_keys": self.config.keymap.len(),
            "devices": status.devices,
            "last_key": status.last_key,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inputs::dispatch::{ActionSink, ActionTarget};
    use crate::data::PlayerCommand;
    use parking_lot::Mutex;
    use serde_json::json;
    use std::sync::Arc;

    #[derive(Default)]
    struct RecordingTarget {
        adjusts: Mutex<Vec<f64>>,
        commands: Mutex<Vec<PlayerCommand>>,
    }

    impl ActionTarget for RecordingTarget {
        fn volume_adjust(&self, delta: f64) -> bool {
            self.adjusts.lock().push(delta);
            true
        }
        fn volume_toggle_mute(&self) -> bool { true }
        fn volume_available(&self) -> bool { true }
        fn player_command(&self, cmd: PlayerCommand) -> bool {
            self.commands.lock().push(cmd);
            true
        }
    }

    fn sink() -> (Arc<RecordingTarget>, ActionSink) {
        let t = Arc::new(RecordingTarget::default());
        let s = ActionSink::new(t.clone(), 5.0);
        (t, s)
    }

    // --- config ---

    #[test]
    fn test_config_defaults_when_absent() {
        let c = KeyboardConfig::from_config(None);
        assert!(c.enable);
        assert_eq!(c.volume_step, 5.0);
        assert!(!c.grab);
        assert_eq!(c.device, "");
        assert_eq!(c.keymap, KeyMap::default_map());
    }

    #[test]
    fn test_config_explicit_values() {
        let cfg = json!({
            "enable": false,
            "volume_step": 2.5,
            "grab": true,
            "device": "USBRemote",
            "keymap": { "KEY_ENTER": "playpause" }
        });
        let c = KeyboardConfig::from_config(Some(&cfg));
        assert!(!c.enable);
        assert_eq!(c.volume_step, 2.5);
        assert!(c.grab);
        assert_eq!(c.device, "USBRemote");
        assert_eq!(c.keymap.len(), 1);
    }

    #[test]
    fn test_config_partial_keeps_other_defaults() {
        let cfg = json!({ "volume_step": 10 });
        let c = KeyboardConfig::from_config(Some(&cfg));
        assert!(c.enable);
        assert_eq!(c.volume_step, 10.0);
        assert_eq!(c.keymap, KeyMap::default_map());
    }

    // --- event handling ---

    #[test]
    fn test_key_down_fires_any_action() {
        let (t, s) = sink();
        let m = KeyMap::default_map();
        assert_eq!(handle_key_event(&m, 115, 1, &s), Some(Action::VolumeUp));
        assert_eq!(handle_key_event(&m, 163, 1, &s), Some(Action::Next));
        assert_eq!(*t.adjusts.lock(), vec![5.0]);
        assert_eq!(*t.commands.lock(), vec![PlayerCommand::Next]);
    }

    #[test]
    fn test_key_release_ignored() {
        let (t, s) = sink();
        let m = KeyMap::default_map();
        assert_eq!(handle_key_event(&m, 115, 0, &s), None);
        assert!(t.adjusts.lock().is_empty());
    }

    /// Holding volume-up must ramp.
    #[test]
    fn test_autorepeat_fires_volume() {
        let (t, s) = sink();
        let m = KeyMap::default_map();
        assert_eq!(handle_key_event(&m, 115, 2, &s), Some(Action::VolumeUp));
        assert_eq!(handle_key_event(&m, 114, 2, &s), Some(Action::VolumeDown));
        assert_eq!(*t.adjusts.lock(), vec![5.0, -5.0]);
    }

    /// Holding next must NOT skip thirty tracks.
    #[test]
    fn test_autorepeat_ignored_for_transport() {
        let (t, s) = sink();
        let m = KeyMap::default_map();
        assert_eq!(handle_key_event(&m, 163, 2, &s), None);
        assert_eq!(handle_key_event(&m, 28, 2, &s), None);
        assert_eq!(handle_key_event(&m, 113, 2, &s), None);
        assert!(t.commands.lock().is_empty());
    }

    #[test]
    fn test_unmapped_key_ignored() {
        let (t, s) = sink();
        let m = KeyMap::default_map();
        assert_eq!(handle_key_event(&m, 172, 1, &s), None);
        assert!(t.adjusts.lock().is_empty());
        assert!(t.commands.lock().is_empty());
    }

    // --- device filter ---

    #[test]
    fn test_device_filter() {
        assert!(device_name_matches("", "anything at all"));
        assert!(device_name_matches("usbremote", "HiFiBerry USBRemote"));
        assert!(device_name_matches("USBRemote", "HiFiBerry USBRemote"));
        assert!(!device_name_matches("USBRemote", "Power Button"));
    }
}
