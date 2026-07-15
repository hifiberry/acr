//! evdev device discovery and reader threads. Linux-only.
//!
//! This is the only place `evdev` is used. Everything else in `inputs` is
//! portable and unit-tested; this shim is verified on hardware.

use crate::inputs::dispatch::ActionSink;
use crate::inputs::keyboard::{
    evaluate_device, handle_key_event, unbound_reason, DeviceVerdict, KeyboardConfig,
    KeyboardStatus, LastKey, UnboundDevice,
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

/// Probe `/dev/input/event*` for nodes that cannot be opened for reading.
///
/// `evdev::enumerate()` silently skips devices it cannot open, so this walk
/// is the only way to see a permission problem at all. Called unconditionally
/// by [`scan_devices`] -- a device binding fine must not hide an unrelated
/// denied path -- and by `audiocontrol_input_devices`, which reports every
/// denied path the same way.
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

/// Everything the startup scan saw.
///
/// Total by construction: a permission failure is data here, not an error, so
/// the snapshot survives to be reported even when nothing could be bound.
/// `start_readers` turns it into the `Err` the caller expects.
pub struct ScanResult {
    pub bound: Vec<DiscoveredDevice>,
    pub unbound: Vec<UnboundDevice>,
    pub denied_paths: Vec<String>,
}

/// Scan `/dev/input/event*` and report every device, bound or not.
///
/// Binds devices that pass the `device` name filter and advertise at least one
/// mapped key -- audiocontrol2's capability-intersection rule. Startup-only;
/// hotplug is out of scope.
///
/// Never fails: a permission problem is recorded in `denied_paths` so the
/// status API can explain it. `start_readers` decides whether that is fatal.
pub fn scan_devices(config: &KeyboardConfig) -> ScanResult {
    let mut bound = Vec::new();
    let mut unbound = Vec::new();

    for (path, device) in evdev::enumerate() {
        let path_str = path.to_string_lossy().to_string();
        let name = device.name().unwrap_or("unknown").to_string();
        let keys: Option<Vec<u16>> = device
            .supported_keys()
            .map(|ks| ks.iter().map(KeyCode::code).collect());

        match evaluate_device(config, &name, keys.as_deref()) {
            DeviceVerdict::Matched(matched) => {
                info!(
                    "keyboard: bound {} '{}' ({} mapped keys)",
                    path_str,
                    name,
                    matched.len()
                );
                bound.push(DiscoveredDevice { path: path_str, name, matched, device });
            }
            verdict => {
                if let Some(reason) = unbound_reason(&verdict) {
                    debug!("keyboard: {} '{}' not bound: {}", path_str, name, reason);
                    unbound.push(UnboundDevice {
                        path: path_str,
                        name: Some(name),
                        reason: reason.to_string(),
                    });
                }
            }
        }
    }

    // Probe unconditionally: evdev::enumerate() silently omits devices it
    // cannot open, so a denied remote is invisible above -- and an unrelated
    // device binding fine must not hide it.
    let denied_paths = probe_permission_denied();
    for path in &denied_paths {
        unbound.push(UnboundDevice {
            path: path.clone(),
            name: None,
            reason: "permission_denied".to_string(),
        });
    }

    ScanResult { bound, unbound, denied_paths }
}

/// Start a reader thread per discovered device.
///
/// Records the scan's `unbound` snapshot into `status` first, before any error
/// path -- that is the whole reason ordering matters here: a permission
/// failure is exactly when the status API most needs to explain itself.
///
/// Returns `Err` only when nothing bound *and* a path was denied
/// (`InputError::PermissionDenied`). No matching device with no denied path is
/// not an error: most systems have no remote.
pub fn start_readers(
    config: &KeyboardConfig,
    sink: ActionSink,
    status: Arc<Mutex<KeyboardStatus>>,
    running: Arc<AtomicBool>,
) -> Result<(), InputError> {
    let scan = scan_devices(config);

    // Record the snapshot before any error path: a permission failure is
    // exactly when the status API most needs to explain itself.
    status.lock().unbound_devices = scan.unbound;

    if scan.bound.is_empty() {
        if let Some(path) = scan.denied_paths.into_iter().next() {
            return Err(InputError::PermissionDenied { path });
        }
        info!("keyboard: no input devices with mapped keys found");
    }

    for mut discovered in scan.bound {
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

// `evaluate_device` and its unit tests now live in `keyboard/mod.rs`, which is
// portable and compiles on every platform (see that module's doc comment).
// `probe_permission_denied` above is genuinely platform-bound -- it walks
// `/dev/input` -- so it has no unit test here; it is exercised on hardware.
