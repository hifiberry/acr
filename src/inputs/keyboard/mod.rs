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

/// What a device is, for keyboard-input purposes.
///
/// This is the one place the "would audiocontrol bind this device" rule
/// lives. Both `scan_devices` (which only needs the devices it binds) and
/// `audiocontrol_input_devices` (which needs to explain every device, matched
/// or not) decide from this.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeviceVerdict {
    /// Advertises at least one mapped key. Carries the mapped keycodes.
    Matched(Vec<u16>),
    /// Excluded by the `device` name filter, before capabilities were checked.
    FilteredOut,
    /// Passed the name filter but advertises none of the keymap's keycodes
    /// (including devices with no key capability at all, i.e.
    /// `supported_keys()` returned `None`).
    NoMappedKeys,
}

/// Decide what one device is, given its name and the keycodes it supports.
///
/// Takes plain data rather than a live evdev `Device` so this -- the only rule
/// that decides what audiocontrol would bind -- is unit-testable without
/// hardware. `keys` is `None` when `Device::supported_keys()` returned `None`.
///
/// The name filter is applied before the capability check, matching
/// audiocontrol2.
pub fn evaluate_device(config: &KeyboardConfig, name: &str, keys: Option<&[u16]>) -> DeviceVerdict {
    if !device_name_matches(&config.device, name) {
        return DeviceVerdict::FilteredOut;
    }

    let Some(keys) = keys else {
        return DeviceVerdict::NoMappedKeys;
    };

    let matched: Vec<u16> = config
        .keymap
        .codes()
        .into_iter()
        .filter(|c| keys.contains(c))
        .collect();

    if matched.is_empty() {
        DeviceVerdict::NoMappedKeys
    } else {
        DeviceVerdict::Matched(matched)
    }
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

/// A device the startup scan saw but did not bind, and why.
///
/// `name` is `None` for `permission_denied`: that verdict comes from probing
/// `/dev/input` paths, and a device that cannot be opened cannot report a name.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct UnboundDevice {
    pub path: String,
    pub name: Option<String>,
    pub reason: String,
}

/// The `reason` string for a verdict that did not bind, or `None` if it did.
///
/// These strings are API surface: `GET /api/inputs` reports them and the WebUI
/// switches on them.
pub fn unbound_reason(verdict: &DeviceVerdict) -> Option<&'static str> {
    match verdict {
        DeviceVerdict::Matched(_) => None,
        DeviceVerdict::FilteredOut => Some("filtered_out"),
        DeviceVerdict::NoMappedKeys => Some("no_mapped_keys"),
    }
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
    /// Devices the startup scan bound. Published in 0.8.0 -- do not change.
    pub devices: Vec<BoundDevice>,
    /// Devices the startup scan saw but did not bind. Added in 0.8.1.
    pub unbound_devices: Vec<UnboundDevice>,
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

    // --- evaluate_device ---

    fn device_verdict_config(device_filter: &str) -> KeyboardConfig {
        let mut c = KeyboardConfig::from_config(None);
        c.device = device_filter.to_string();
        c
    }

    #[test]
    fn test_evaluate_device_filtered_out_before_capability_check() {
        // A device that would match on keys but fails the name filter must be
        // FilteredOut, not NoMappedKeys -- the filter runs first.
        let c = device_verdict_config("USBRemote");
        let keys = [115u16]; // KEY_VOLUMEUP, present in the default map
        assert_eq!(
            evaluate_device(&c, "Power Button", Some(&keys)),
            DeviceVerdict::FilteredOut
        );
    }

    #[test]
    fn test_evaluate_device_no_key_capability() {
        // supported_keys() returned None: the device has no key capability at all.
        let c = device_verdict_config("");
        assert_eq!(evaluate_device(&c, "Some Mouse", None), DeviceVerdict::NoMappedKeys);
    }

    #[test]
    fn test_evaluate_device_opens_but_no_mapped_keys() {
        let c = device_verdict_config("");
        let keys = [999u16]; // not in the default map
        assert_eq!(
            evaluate_device(&c, "Random Keyboard", Some(&keys)),
            DeviceVerdict::NoMappedKeys
        );
    }

    #[test]
    fn test_evaluate_device_matched_carries_mapped_codes() {
        let c = device_verdict_config("USBRemote");
        // Device advertises far more keys than the mapped set; only the
        // intersection should come back.
        let keys = [115u16, 114, 999, 1000];
        match evaluate_device(&c, "HiFiBerry USBRemote", Some(&keys)) {
            DeviceVerdict::Matched(mut matched) => {
                matched.sort();
                assert_eq!(matched, vec![114, 115]);
            }
            other => panic!("expected Matched, got {:?}", other),
        }
    }

    #[test]
    fn test_evaluate_device_empty_filter_matches_any_name() {
        let c = device_verdict_config("");
        let keys = [115u16];
        assert!(matches!(
            evaluate_device(&c, "anything at all", Some(&keys)),
            DeviceVerdict::Matched(_)
        ));
    }

    // --- unbound devices ---

    #[test]
    fn test_unbound_reason_maps_each_verdict() {
        assert_eq!(unbound_reason(&DeviceVerdict::FilteredOut), Some("filtered_out"));
        assert_eq!(unbound_reason(&DeviceVerdict::NoMappedKeys), Some("no_mapped_keys"));
    }

    /// A matched device belongs in `devices`, never in `unbound_devices`.
    #[test]
    fn test_matched_verdict_has_no_unbound_reason() {
        assert_eq!(unbound_reason(&DeviceVerdict::Matched(vec![115])), None);
        assert_eq!(unbound_reason(&DeviceVerdict::Matched(vec![])), None);
    }

    #[test]
    fn test_status_defaults_to_no_unbound_devices() {
        let s = KeyboardStatus::default();
        assert!(s.unbound_devices.is_empty());
        assert!(s.devices.is_empty());
    }

    /// The API contract: field names the WebUI reads.
    #[test]
    fn test_unbound_device_serializes_with_expected_field_names() {
        let d = UnboundDevice {
            path: "/dev/input/event1".to_string(),
            name: Some("ADW USB DOGLE".to_string()),
            reason: "no_mapped_keys".to_string(),
        };
        let v = serde_json::to_value(&d).unwrap();
        assert_eq!(v["path"], "/dev/input/event1");
        assert_eq!(v["name"], "ADW USB DOGLE");
        assert_eq!(v["reason"], "no_mapped_keys");
    }

    /// `name` is null for permission_denied: the probe only knows the path.
    #[test]
    fn test_unbound_device_serializes_null_name() {
        let d = UnboundDevice {
            path: "/dev/input/event5".to_string(),
            name: None,
            reason: "permission_denied".to_string(),
        };
        let v = serde_json::to_value(&d).unwrap();
        assert!(v["name"].is_null());
        assert_eq!(v["reason"], "permission_denied");
    }
}
