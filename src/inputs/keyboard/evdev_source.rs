//! evdev device discovery and reader threads. Linux-only.
//!
//! This is the only place `evdev` is used. Everything else in `inputs` is
//! portable and unit-tested; this shim is verified on hardware.

use crate::inputs::dispatch::ActionSink;
use crate::inputs::keyboard::{
    device_name_matches, handle_key_event, KeyboardConfig, KeyboardStatus, LastKey,
};
use crate::inputs::keyboard::keymap::key_name_from_code;
use crate::inputs::InputError;
use evdev::{Device, EventType, KeyCode};
use log::{debug, error, info, warn};
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

/// Scan `/dev/input/event*` and return devices that pass the name filter and
/// advertise at least one mapped key.
///
/// This is audiocontrol2's rule: match by capability intersection. It is a
/// startup-only scan; hotplug is out of scope (the unit orders after
/// systemd-udev-settle, so a dongle present at boot is always seen).
pub fn scan_devices(config: &KeyboardConfig) -> Vec<DiscoveredDevice> {
    let wanted = config.keymap.codes();
    let mut found = Vec::new();
    let mut permission_denied = false;

    for (path, device) in evdev::enumerate() {
        let path_str = path.to_string_lossy().to_string();
        let name = device.name().unwrap_or("unknown").to_string();

        if !device_name_matches(&config.device, &name) {
            debug!("keyboard: {} '{}' filtered out by device filter", path_str, name);
            continue;
        }

        let Some(keys) = device.supported_keys() else {
            continue;
        };
        let matched: Vec<u16> = wanted
            .iter()
            .copied()
            .filter(|c| keys.contains(KeyCode::new(*c)))
            .collect();

        if matched.is_empty() {
            debug!("keyboard: {} '{}' has no mapped keys", path_str, name);
            continue;
        }

        info!(
            "keyboard: bound {} '{}' ({} mapped keys)",
            path_str,
            name,
            matched.len()
        );
        found.push(DiscoveredDevice { path: path_str, name, matched, device });
    }

    // evdev::enumerate() silently skips devices it cannot open, so probe for
    // the permission problem explicitly -- it is the most likely failure.
    if found.is_empty() {
        if let Ok(entries) = std::fs::read_dir("/dev/input") {
            for entry in entries.flatten() {
                let p = entry.path();
                if !p.to_string_lossy().contains("event") {
                    continue;
                }
                if let Err(e) = std::fs::File::open(&p) {
                    if e.kind() == std::io::ErrorKind::PermissionDenied {
                        permission_denied = true;
                        break;
                    }
                }
            }
        }
        if permission_denied {
            error!(
                "keyboard: permission denied reading /dev/input/event*. \
                 Add the 'audiocontrol' user to the 'input' group \
                 (usermod -a -G input audiocontrol) and restart audiocontrol."
            );
        } else {
            info!("keyboard: no input devices with mapped keys found");
        }
    }

    found
}

/// Start a reader thread per discovered device.
///
/// Returns `Err` only if the devices could not be scanned at all. No matching
/// device is not an error: most systems have no remote.
pub fn start_readers(
    config: &KeyboardConfig,
    sink: ActionSink,
    status: Arc<Mutex<KeyboardStatus>>,
    running: Arc<AtomicBool>,
) -> Result<(), InputError> {
    let devices = scan_devices(config);

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
                .map(|c| key_name_from_code(*c).unwrap_or("").to_string())
                .filter(|s| !s.is_empty())
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
