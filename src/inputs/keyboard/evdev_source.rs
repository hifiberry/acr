//! evdev device discovery and reader threads. Linux-only.
//!
//! This is the only place `evdev` is used. Everything else in `inputs` is
//! portable and unit-tested; this shim is verified on hardware.

use crate::inputs::dispatch::ActionSink;
use crate::inputs::keyboard::{
    device_name_matches, handle_key_event, KeyboardConfig, KeyboardStatus, LastKey,
};
use crate::inputs::keyboard::keymap::{key_display_name, key_name_from_code};
use crate::inputs::InputError;
use evdev::{Device, EventType, KeyCode};
use log::{debug, info, warn};
use parking_lot::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// A device found by [`scan_devices`].
pub struct DiscoveredDevice {
    pub path: String,
    pub name: String,
    /// Mapped keycodes this device advertises.
    pub matched: Vec<u16>,
    pub device: Device,
}

/// What a device is, for keyboard-input purposes.
///
/// This is the one place the "would audiocontrol bind this device" rule
/// lives. Both [`scan_devices`] (which only needs the devices it binds) and
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
/// Takes plain data rather than a live [`Device`] so this -- the only rule
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

/// Probe `/dev/input/event*` for nodes that cannot be opened for reading.
///
/// `evdev::enumerate()` silently skips devices it cannot open, so this walk
/// is the only way to see a permission problem at all. Shared by
/// [`scan_devices`] (fatal only when nothing else matched -- most systems
/// have no remote at all, and that is not an error) and
/// `audiocontrol_input_devices` (which reports every denied path, since an
/// unrelated device opening fine -- a power button, an HDMI-CEC node -- must
/// not hide a denied remote).
pub fn probe_permission_denied() -> Vec<String> {
    let mut denied = Vec::new();
    let Ok(entries) = std::fs::read_dir("/dev/input") else {
        return denied;
    };
    for entry in entries.flatten() {
        let p = entry.path();
        if !p.to_string_lossy().contains("event") {
            continue;
        }
        if let Err(e) = std::fs::File::open(&p) {
            if e.kind() == std::io::ErrorKind::PermissionDenied {
                denied.push(p.to_string_lossy().to_string());
            }
        }
    }
    denied
}

/// Scan `/dev/input/event*` and return devices that pass the name filter and
/// advertise at least one mapped key.
///
/// This is audiocontrol2's rule: match by capability intersection. It is a
/// startup-only scan; hotplug is out of scope (the unit orders after
/// systemd-udev-settle, so a dongle present at boot is always seen).
///
/// Returns `Err` if no device was bound and the permission probe found an
/// unreadable `/dev/input/event*` node. No matching device with no
/// permission problem is not an error: most systems have no remote.
pub fn scan_devices(config: &KeyboardConfig) -> Result<Vec<DiscoveredDevice>, InputError> {
    let mut found = Vec::new();

    for (path, device) in evdev::enumerate() {
        let path_str = path.to_string_lossy().to_string();
        let name = device.name().unwrap_or("unknown").to_string();
        let keys: Option<Vec<u16>> = device
            .supported_keys()
            .map(|ks| ks.iter().map(KeyCode::code).collect());

        match evaluate_device(config, &name, keys.as_deref()) {
            DeviceVerdict::FilteredOut => {
                debug!("keyboard: {} '{}' filtered out by device filter", path_str, name);
            }
            DeviceVerdict::NoMappedKeys => {
                debug!("keyboard: {} '{}' has no mapped keys", path_str, name);
            }
            DeviceVerdict::Matched(matched) => {
                info!(
                    "keyboard: bound {} '{}' ({} mapped keys)",
                    path_str,
                    name,
                    matched.len()
                );
                found.push(DiscoveredDevice { path: path_str, name, matched, device });
            }
        }
    }

    // evdev::enumerate() silently skips devices it cannot open, so probe for
    // the permission problem explicitly -- it is the most likely failure.
    if found.is_empty() {
        if let Some(path) = probe_permission_denied().into_iter().next() {
            return Err(InputError::PermissionDenied { path });
        }
        info!("keyboard: no input devices with mapped keys found");
    }

    Ok(found)
}

/// Start a reader thread per discovered device.
///
/// Returns `Err` if [`scan_devices`] found no bindable device because
/// `/dev/input/event*` was unreadable. No matching device is not an error:
/// most systems have no remote.
pub fn start_readers(
    config: &KeyboardConfig,
    sink: ActionSink,
    status: Arc<Mutex<KeyboardStatus>>,
    running: Arc<AtomicBool>,
) -> Result<(), InputError> {
    let devices = scan_devices(config)?;

    for mut discovered in devices {
        let path = discovered.path.clone();
        let name = discovered.name.clone();

        if config.grab {
            if let Err(e) = discovered.device.grab() {
                warn!("keyboard: could not grab {} exclusively: {}", path, e);
            }
        }

        status.lock().devices.push(crate::inputs::keyboard::BoundDevice {
            path: path.clone(),
            name: name.clone(),
            matched_keys: discovered
                .matched
                .iter()
                .map(|c| key_display_name(*c))
                .collect(),
        });

        let keymap = config.keymap.clone();
        let sink = sink.clone();
        let status = status.clone();
        let running = running.clone();
        let mut device = discovered.device;

        // One blocking reader thread per device. A failure here must never take
        // down audio: log, exit this thread, leave the others alone.
        let builder = std::thread::Builder::new().name(format!("input-kbd-{}", name));
        let spawned = builder.spawn(move || {
            info!("keyboard: listener started for '{}'", name);
            // fetch_events() blocks, so a stopped listener lingers until its
            // device emits one more event. Only reached at shutdown, where the
            // process exits regardless.
            while running.load(Ordering::Relaxed) {
                let events = match device.fetch_events() {
                    Ok(events) => events,
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {
                        // EINTR: a signal interrupted the blocking read. The
                        // device is still there -- retry instead of killing
                        // the listener.
                        debug!("keyboard: '{}' interrupted read, retrying", name);
                        continue;
                    }
                    Err(e) => {
                        warn!("keyboard: '{}' read error ({}), listener stopping", name, e);
                        return;
                    }
                };
                for event in events {
                    if event.event_type() != EventType::KEY {
                        continue;
                    }
                    let code = event.code();
                    let value = event.value();
                    if let Some(action) = handle_key_event(&keymap, code, value, &sink) {
                        let mut s = status.lock();
                        s.last_key = Some(LastKey {
                            code,
                            name: key_name_from_code(code).map(|n| n.to_string()),
                            action: Some(action.as_str().to_string()),
                            device: name.clone(),
                        });
                    }
                }
            }
            info!("keyboard: listener for '{}' stopped", name);
        });

        if let Err(e) = spawned {
            warn!("keyboard: could not start listener thread for {}: {}", path, e);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(device_filter: &str) -> KeyboardConfig {
        let mut c = KeyboardConfig::from_config(None);
        c.device = device_filter.to_string();
        c
    }

    #[test]
    fn test_evaluate_device_filtered_out_before_capability_check() {
        // A device that would match on keys but fails the name filter must be
        // FilteredOut, not NoMappedKeys -- the filter runs first.
        let c = config("USBRemote");
        let keys = [115u16]; // KEY_VOLUMEUP, present in the default map
        assert_eq!(
            evaluate_device(&c, "Power Button", Some(&keys)),
            DeviceVerdict::FilteredOut
        );
    }

    #[test]
    fn test_evaluate_device_no_key_capability() {
        // supported_keys() returned None: the device has no key capability at all.
        let c = config("");
        assert_eq!(evaluate_device(&c, "Some Mouse", None), DeviceVerdict::NoMappedKeys);
    }

    #[test]
    fn test_evaluate_device_opens_but_no_mapped_keys() {
        let c = config("");
        let keys = [999u16]; // not in the default map
        assert_eq!(
            evaluate_device(&c, "Random Keyboard", Some(&keys)),
            DeviceVerdict::NoMappedKeys
        );
    }

    #[test]
    fn test_evaluate_device_matched_carries_mapped_codes() {
        let c = config("USBRemote");
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
        let c = config("");
        let keys = [115u16];
        assert!(matches!(
            evaluate_device(&c, "anything at all", Some(&keys)),
            DeviceVerdict::Matched(_)
        ));
    }

    #[test]
    fn test_probe_permission_denied_does_not_panic() {
        // No assertion on contents: real /dev/input contents are
        // environment-dependent (root vs. non-root test runs). This just
        // guards that the walk is total and never panics, e.g. if
        // /dev/input does not exist on some test host.
        let _ = probe_permission_denied();
    }
}
