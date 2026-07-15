//! List input devices and show which keys map to which actions.
//!
//! The support tool for "my remote does nothing": it shows whether a device was
//! matched, and `--watch` names the exact keycode a button emits so it can be
//! put in the `keymap` config.

use clap::Parser;

#[derive(Parser, Debug)]
#[command(
    name = "audiocontrol_input_devices",
    about = "List input devices usable as remote controls"
)]
struct Args {
    /// Live-dump keycodes as keys are pressed
    #[arg(short, long)]
    watch: bool,

    /// Config file to read the keymap from
    #[arg(short, long, default_value = "/etc/audiocontrol/audiocontrol.json")]
    config: String,
}

/// Whether the `audiocontrol` service user is a member of the `input` group,
/// checked directly via `getent group input` rather than by looking at the
/// invoking process's own groups.
///
/// This matters because `probe_permission_denied()` (and the `denied` list it
/// produces) only reflects what *this process* can open. Support engineers
/// almost always run this tool as root, where every device opens fine and
/// `denied` is empty -- even when the `audiocontrol` user itself is not in
/// `input` and the service would be unable to see any remote at all.
///
/// Returns `None` when the check is inconclusive (no `getent` binary, no
/// `input` group, or the output could not be parsed) rather than guessing --
/// this is the normal case on a development box where `audiocontrol` is not a
/// real user, and callers must not treat that as an error.
#[cfg(target_os = "linux")]
fn audiocontrol_in_input_group() -> Option<bool> {
    let output = std::process::Command::new("getent")
        .args(["group", "input"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    // getent group format: name:password:GID:member1,member2,...
    let text = String::from_utf8_lossy(&output.stdout);
    let members = text.trim_end().split(':').nth(3)?;
    Some(members.split(',').any(|u| u == "audiocontrol"))
}

#[cfg(target_os = "linux")]
fn run(args: Args) -> i32 {
    use audiocontrol::inputs::keyboard::evdev_source::probe_permission_denied;
    use audiocontrol::inputs::keyboard::keymap::key_display_name;
    use audiocontrol::inputs::keyboard::{evaluate_device, DeviceVerdict, KeyboardConfig};
    use evdev::EventType;

    // A missing or malformed config is not fatal: fall back to defaults, since
    // the point of this tool is to diagnose a broken setup.
    let config_value: serde_json::Value = match std::fs::read_to_string(&args.config) {
        Ok(text) => serde_json::from_str(&text).unwrap_or_else(|e| {
            eprintln!("warning: could not parse {}: {} -- using defaults", args.config, e);
            serde_json::json!({})
        }),
        Err(e) => {
            eprintln!("warning: could not read {}: {} -- using defaults", args.config, e);
            serde_json::json!({})
        }
    };

    let kb_config = KeyboardConfig::from_config(
        config_value.get("inputs").and_then(|v| v.get("keyboard")),
    );

    // Probe before enumerating: evdev::enumerate() silently skips devices it
    // cannot open, so this walk is the only way to see a remote hidden by a
    // permission problem -- even when some unrelated device (a power button,
    // an HDMI-CEC node) opens fine and the listing below looks normal.
    let denied = probe_permission_denied();

    // Devices worth listening to in --watch mode: every device that passes the
    // configured `device` name filter, whether or not it currently matches the
    // keymap. Watch mode exists specifically to discover the codes an
    // unmatched remote emits, so restricting it to already-matched devices
    // would defeat its purpose.
    let mut watch_devices = Vec::new();
    let mut matched_count = 0usize;
    let mut any = false;

    for (path, device) in evdev::enumerate() {
        any = true;
        let path_str = path.to_string_lossy().to_string();
        let name = device.name().unwrap_or("unknown").to_string();
        let keys: Option<Vec<u16>> = device
            .supported_keys()
            .map(|ks| ks.iter().map(evdev::KeyCode::code).collect());

        match evaluate_device(&kb_config, &name, keys.as_deref()) {
            DeviceVerdict::FilteredOut => {
                println!("{:<20} {:<28} filtered out by device filter '{}'",
                         path_str, format!("\"{}\"", name), kb_config.device);
            }
            DeviceVerdict::NoMappedKeys => {
                println!("{:<20} {:<28} no mapped keys", path_str, format!("\"{}\"", name));
                watch_devices.push((path_str, name, device));
            }
            DeviceVerdict::Matched(matched) => {
                println!("{:<20} {:<28} MATCHED ({} mapped keys)",
                         path_str, format!("\"{}\"", name), matched.len());
                matched_count += 1;
                watch_devices.push((path_str, name, device));
            }
        }
    }

    for path in &denied {
        println!("{:<20} {:<28} PERMISSION DENIED -- could not open", path, "");
    }

    // Exit-code contract: non-zero means a permission problem was detected --
    // that is always worth a script noticing. No hardware at all is not by
    // itself an error (most systems have no remote plugged in; scan_devices
    // treats it the same way), so it does not affect the exit code.
    let exit_code = if !denied.is_empty() { 1 } else { 0 };

    if !any && denied.is_empty() {
        eprintln!("No input devices found. If this is unexpected, check permissions:");
        eprintln!("  ls -l /dev/input/event*   # should be group 'input'");
        eprintln!("  id audiocontrol           # should include the 'input' group");
        return exit_code;
    }

    if !denied.is_empty() {
        eprintln!();
        eprintln!(
            "{} device(s) above could not be opened -- a remote may be hidden by this:",
            denied.len()
        );
        eprintln!("  ls -l /dev/input/event*   # should be group 'input'");
        eprintln!("  id audiocontrol           # should include the 'input' group");
    }

    // This is deliberately unconditional on `denied`: `denied` only reflects
    // what *this* process can open, and this tool is normally run as root by a
    // support engineer -- root can open everything, so `denied` is empty even
    // when the `audiocontrol` service user is locked out entirely. Check the
    // service user's actual group membership directly instead.
    if audiocontrol_in_input_group() == Some(false) {
        eprintln!();
        eprintln!("warning: the 'audiocontrol' user is not in the 'input' group.");
        eprintln!("The listing above reflects what THIS process can see, not what the");
        eprintln!("audiocontrol service can open -- it may not see any device at all. Fix:");
        eprintln!("  sudo usermod -a -G input audiocontrol && sudo systemctl restart audiocontrol");
    }

    if matched_count == 0 {
        println!();
        println!("No device advertises any mapped key.");
        println!("Run with --watch and press a button to see what your remote emits.");
    }

    if !args.watch {
        return exit_code;
    }

    println!();
    println!("press a key... (Ctrl-C to stop)");

    let mut handles = Vec::new();
    for (_path, name, mut device) in watch_devices {
        let keymap = kb_config.keymap.clone();
        handles.push(std::thread::spawn(move || loop {
            let events = match device.fetch_events() {
                Ok(e) => e,
                Err(e) => {
                    eprintln!("  '{}' read error: {}", name, e);
                    return;
                }
            };
            for event in events {
                if event.event_type() != EventType::KEY || event.value() != 1 {
                    continue;
                }
                let code = event.code();
                let key_name = key_display_name(code);
                match keymap.get(code) {
                    Some(action) => println!("  {} ({})  -> {}", key_name, code, action.as_str()),
                    None => println!("  {} ({})  -> unmapped", key_name, code),
                }
            }
        }));
    }
    for h in handles {
        let _ = h.join();
    }
    exit_code
}

#[cfg(not(target_os = "linux"))]
fn run(_args: Args) -> i32 {
    eprintln!("audiocontrol_input_devices is only supported on Linux");
    1
}

fn main() {
    let args = Args::parse();
    std::process::exit(run(args));
}
