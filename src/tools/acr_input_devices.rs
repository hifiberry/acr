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

#[cfg(target_os = "linux")]
fn run(args: Args) -> i32 {
    use audiocontrol::inputs::keyboard::keymap::key_name_from_code;
    use audiocontrol::inputs::keyboard::{device_name_matches, KeyboardConfig};
    use evdev::{EventType, KeyCode};

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
    let wanted = kb_config.keymap.codes();

    let mut matched_devices = Vec::new();
    let mut any = false;

    for (path, device) in evdev::enumerate() {
        any = true;
        let path_str = path.to_string_lossy().to_string();
        let name = device.name().unwrap_or("unknown").to_string();

        if !device_name_matches(&kb_config.device, &name) {
            println!("{:<20} {:<28} filtered out by device filter '{}'",
                     path_str, format!("\"{}\"", name), kb_config.device);
            continue;
        }

        let matched: Vec<u16> = match device.supported_keys() {
            Some(keys) => wanted.iter().copied().filter(|c| keys.contains(KeyCode::new(*c))).collect(),
            None => Vec::new(),
        };

        if matched.is_empty() {
            println!("{:<20} {:<28} no mapped keys", path_str, format!("\"{}\"", name));
        } else {
            println!("{:<20} {:<28} MATCHED ({} mapped keys)",
                     path_str, format!("\"{}\"", name), matched.len());
            matched_devices.push((path_str, name, device));
        }
    }

    if !any {
        eprintln!("No input devices found. If this is unexpected, check permissions:");
        eprintln!("  ls -l /dev/input/event*   # should be group 'input'");
        eprintln!("  id audiocontrol           # should include the 'input' group");
        return 1;
    }

    if matched_devices.is_empty() {
        println!();
        println!("No device advertises any mapped key.");
        println!("Run with --watch and press a button to see what your remote emits.");
    }

    if !args.watch {
        return 0;
    }

    if matched_devices.is_empty() {
        // In watch mode, listen to everything: the whole point is to find codes
        // that are not mapped yet.
        matched_devices = evdev::enumerate()
            .map(|(p, d)| {
                let n = d.name().unwrap_or("unknown").to_string();
                (p.to_string_lossy().to_string(), n, d)
            })
            .collect();
    }

    println!();
    println!("press a key... (Ctrl-C to stop)");

    let mut handles = Vec::new();
    for (_path, name, mut device) in matched_devices {
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
                let key_name = key_name_from_code(code).unwrap_or("(unnamed)");
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
    0
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
