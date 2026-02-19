#![cfg(unix)]

use std::env;
use dbus::blocking::{Connection, Proxy};
use dbus::arg::RefArg;
use std::time::Duration;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use audiocontrol::helpers::mpris::{find_mpris_players, BusType, get_dbus_property, retrieve_mpris_metadata, extract_song_from_mpris_metadata};

fn main() {
    env_logger::init();
    
    let args: Vec<String> = env::args().collect();
    
    if args.len() < 2 || args[1] == "--help" || args[1] == "-h" {
        print_help();
        return;
    }
    
    let player_identifier = &args[1];
    
    println!("AudioControl MPRIS State Monitor");
    println!("================================");
    println!("Player: {}", player_identifier);
    println!("Press Ctrl+C to stop monitoring...");
    println!();
    
    // Find the specified player
    let player = match find_player(player_identifier) {
        Some(p) => p,
        None => {
            println!("Error: Player '{}' not found.", player_identifier);
            println!();
            println!("Available players:");
            list_available_players();
            return;
        }
    };
    
    println!("Found player: {} ({})", player.bus_name, player.bus_type);
    println!("Monitoring state changes by polling...");
    println!();
    
    // Set up signal handler for Ctrl+C
    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();
    
    if let Err(e) = ctrlc::set_handler(move || {
        println!("\nReceived Ctrl+C, shutting down...");
        r.store(false, Ordering::SeqCst);
    }) {
        eprintln!("Error: Failed to set Ctrl+C handler: {}", e);
        std::process::exit(1);
    }
    
    // Connect to the appropriate bus
    let conn = match player.bus_type {
        BusType::Session => match Connection::new_session() {
            Ok(c) => c,
            Err(e) => {
                println!("Error: Failed to connect to session bus: {}", e);
                return;
            }
        },
        BusType::System => match Connection::new_system() {
            Ok(c) => c,
            Err(e) => {
                println!("Error: Failed to connect to system bus: {}", e);
                return;
            }
        },
    };
    
    // Create proxy for getting current state
    let proxy = Proxy::new(&player.bus_name, "/org/mpris/MediaPlayer2", Duration::from_millis(5000), &conn);
    
    // Print initial state and store it for comparison
    let mut last_state = get_current_state(&proxy);
    print_current_state(&last_state);
    
    // Start monitoring loop
    while running.load(Ordering::SeqCst) {
        std::thread::sleep(Duration::from_millis(500)); // Poll every 500ms
        
        let current_state = get_current_state(&proxy);
        
        // Compare with last state and print changes
        let changes = detect_changes(&last_state, &current_state);
        if !changes.is_empty() {
            let timestamp = chrono::Local::now().format("%H:%M:%S%.3f");
            println!("[{}] State changes detected:", timestamp);
            for change in changes {
                println!("  {}", change);
            }
            print_current_state(&current_state);
            println!();
        }
        
        last_state = current_state;
    }
    
    println!("Monitoring stopped.");
}

fn print_help() {
    println!("AudioControl MPRIS State Monitor");
    println!("");
    println!("USAGE:");
    println!("    audiocontrol_monitor_mpris_state <PLAYER_IDENTIFIER>");
    println!("");
    println!("ARGUMENTS:");
    println!("    <PLAYER_IDENTIFIER>    Bus name or partial name of the MPRIS player");
    println!("");
    println!("OPTIONS:");
    println!("    -h, --help    Print this help message");
    println!("");
    println!("DESCRIPTION:");
    println!("    Monitors MPRIS property changes for a specified media player in real-time.");
    println!("    Similar to running dbus-monitor but filtered for a specific MPRIS player.");
    println!("    Press Ctrl+C to stop monitoring.");
    println!("");
    println!("    This tool listens for D-Bus PropertyChanged signals from the specified");
    println!("    player and displays the changes as they occur. It's equivalent to:");
    println!("");
    println!("    dbus-monitor --system \"type='signal',\\");
    println!("      interface='org.freedesktop.DBus.Properties',\\");
    println!("      member='PropertiesChanged',\\");
    println!("      sender='org.mpris.MediaPlayer2.YourPlayer',\\");
    println!("      path='/org/mpris/MediaPlayer2'\"");
    println!("");
    println!("    But with the convenience of using the same player selection as");
    println!("    audiocontrol_get_mpris_state and formatted output.");
    println!("");
    println!("    Note: This tool uses polling (every 500ms) to detect changes rather");
    println!("    than true signal monitoring for better compatibility.");
    println!("");
    println!("EXAMPLES:");
    println!("    audiocontrol_monitor_mpris_state org.mpris.MediaPlayer2.vlc");
    println!("        Monitor VLC player property changes");
    println!("");
    println!("    audiocontrol_monitor_mpris_state spotify");
    println!("        Monitor Spotify player changes (partial name match)");
    println!("");
    println!("    audiocontrol_monitor_mpris_state shairport");
    println!("        Monitor Shairport Sync player changes");
}

#[derive(Debug, Clone, PartialEq)]
struct PlayerState {
    playback_status: String,
    title: String,
    artist: String,
    album: String,
    position: Option<i64>,
    volume: Option<f64>,
    shuffle: Option<bool>,
    loop_status: Option<String>,
    can_play: Option<bool>,
    can_pause: Option<bool>,
    can_go_next: Option<bool>,
    can_go_previous: Option<bool>,
}

fn get_current_state(proxy: &Proxy<'_, &Connection>) -> PlayerState {
    let playback_status = get_dbus_property(proxy, "org.mpris.MediaPlayer2.Player", "PlaybackStatus")
        .and_then(|v| v.as_str().map(|s| s.to_string()))
        .unwrap_or_else(|| "Unknown".to_string());
    
    let (title, artist, album) = if let Some(metadata_variant) = retrieve_mpris_metadata(proxy) {
        if let Some(song) = extract_song_from_mpris_metadata(&metadata_variant) {
            (
                song.title.unwrap_or_else(|| "Unknown Title".to_string()),
                song.artist.unwrap_or_else(|| "Unknown Artist".to_string()),
                song.album.unwrap_or_else(|| "".to_string()),
            )
        } else {
            ("Unknown Title".to_string(), "Unknown Artist".to_string(), "".to_string())
        }
    } else {
        ("Unknown Title".to_string(), "Unknown Artist".to_string(), "".to_string())
    };
    
    let position = get_dbus_property(proxy, "org.mpris.MediaPlayer2.Player", "Position")
        .and_then(|v| v.as_i64());
    
    let volume = get_dbus_property(proxy, "org.mpris.MediaPlayer2.Player", "Volume")
        .and_then(|v| v.as_f64());
    
    let shuffle = get_dbus_property(proxy, "org.mpris.MediaPlayer2.Player", "Shuffle")
        .and_then(|v| v.as_u64().map(|u| u != 0).or_else(|| v.as_i64().map(|i| i != 0)));
    
    let loop_status = get_dbus_property(proxy, "org.mpris.MediaPlayer2.Player", "LoopStatus")
        .and_then(|v| v.as_str().map(|s| s.to_string()));
        
    let can_play = get_dbus_property(proxy, "org.mpris.MediaPlayer2.Player", "CanPlay")
        .and_then(|v| v.as_u64().map(|u| u != 0).or_else(|| v.as_i64().map(|i| i != 0)));
        
    let can_pause = get_dbus_property(proxy, "org.mpris.MediaPlayer2.Player", "CanPause")
        .and_then(|v| v.as_u64().map(|u| u != 0).or_else(|| v.as_i64().map(|i| i != 0)));
        
    let can_go_next = get_dbus_property(proxy, "org.mpris.MediaPlayer2.Player", "CanGoNext")
        .and_then(|v| v.as_u64().map(|u| u != 0).or_else(|| v.as_i64().map(|i| i != 0)));
        
    let can_go_previous = get_dbus_property(proxy, "org.mpris.MediaPlayer2.Player", "CanGoPrevious")
        .and_then(|v| v.as_u64().map(|u| u != 0).or_else(|| v.as_i64().map(|i| i != 0)));
    
    PlayerState {
        playback_status,
        title,
        artist,
        album,
        position,
        volume,
        shuffle,
        loop_status,
        can_play,
        can_pause,
        can_go_next,
        can_go_previous,
    }
}

fn detect_changes(old_state: &PlayerState, new_state: &PlayerState) -> Vec<String> {
    let mut changes = Vec::new();
    
    if old_state.playback_status != new_state.playback_status {
        changes.push(format!("PlaybackStatus: {} → {}", old_state.playback_status, new_state.playback_status));
    }
    
    if old_state.title != new_state.title {
        changes.push(format!("Title: {} → {}", old_state.title, new_state.title));
    }
    
    if old_state.artist != new_state.artist {
        changes.push(format!("Artist: {} → {}", old_state.artist, new_state.artist));
    }
    
    if old_state.album != new_state.album && !new_state.album.is_empty() {
        changes.push(format!("Album: {} → {}", old_state.album, new_state.album));
    }
    
    if old_state.position != new_state.position {
        if let (Some(old_pos), Some(new_pos)) = (old_state.position, new_state.position) {
            // Only report position changes if they're significant (more than 2 seconds)
            let diff = (new_pos - old_pos).abs();
            if diff > 2_000_000 { // 2 seconds in microseconds
                let old_seconds = old_pos / 1_000_000;
                let new_seconds = new_pos / 1_000_000;
                changes.push(format!("Position: {}:{:02} → {}:{:02}", 
                    old_seconds / 60, old_seconds % 60,
                    new_seconds / 60, new_seconds % 60));
            }
        }
    }
    
    if old_state.volume != new_state.volume {
        if let (Some(old_vol), Some(new_vol)) = (old_state.volume, new_state.volume) {
            changes.push(format!("Volume: {:.1}% → {:.1}%", old_vol * 100.0, new_vol * 100.0));
        }
    }
    
    if old_state.shuffle != new_state.shuffle {
        if let (Some(old_shuffle), Some(new_shuffle)) = (old_state.shuffle, new_state.shuffle) {
            changes.push(format!("Shuffle: {} → {}", old_shuffle, new_shuffle));
        }
    }
    
    if old_state.loop_status != new_state.loop_status {
        match (&old_state.loop_status, &new_state.loop_status) {
            (Some(old_loop), Some(new_loop)) => {
                changes.push(format!("LoopStatus: {} → {}", old_loop, new_loop));
            },
            _ => {}
        }
    }
    
    changes
}

fn print_current_state(state: &PlayerState) {
    println!("=== Current State ===");
    println!("Status: {} | Title: {} | Artist: {}", state.playback_status, state.title, state.artist);
    
    if !state.album.is_empty() {
        println!("Album: {}", state.album);
    }
    
    if let Some(pos) = state.position {
        let seconds = pos / 1_000_000;
        let minutes = seconds / 60;
        let secs = seconds % 60;
        println!("Position: {}:{:02}", minutes, secs);
    }
    
    if let Some(vol) = state.volume {
        println!("Volume: {:.1}%", vol * 100.0);
    }
    
    let mut capabilities = Vec::new();
    if let Some(true) = state.can_play { capabilities.push("Play"); }
    if let Some(true) = state.can_pause { capabilities.push("Pause"); }
    if let Some(true) = state.can_go_next { capabilities.push("Next"); }
    if let Some(true) = state.can_go_previous { capabilities.push("Previous"); }
    if !capabilities.is_empty() {
        println!("Capabilities: {}", capabilities.join(", "));
    }
    
    if let Some(shuffle) = state.shuffle {
        println!("Shuffle: {}", shuffle);
    }
    
    if let Some(ref loop_status) = state.loop_status {
        println!("Loop: {}", loop_status);
    }
}

fn find_player(identifier: &str) -> Option<PlayerInfo> {
    // Try both session and system buses
    for bus_type in [BusType::Session, BusType::System] {
        if let Ok(players) = find_mpris_players(bus_type.clone()) {
            for player in players {
                // Match by full bus name
                if player.bus_name == identifier {
                    return Some(PlayerInfo {
                        bus_name: player.bus_name,
                        bus_type: player.bus_type,
                    });
                }
                
                // Match by partial bus name
                if player.bus_name.contains(identifier) {
                    return Some(PlayerInfo {
                        bus_name: player.bus_name,
                        bus_type: player.bus_type,
                    });
                }
                
                // Match by identity
                if let Some(identity) = &player.identity {
                    if identity.to_lowercase().contains(&identifier.to_lowercase()) {
                        return Some(PlayerInfo {
                            bus_name: player.bus_name,
                            bus_type: player.bus_type,
                        });
                    }
                }
            }
        }
    }
    
    None
}

fn list_available_players() {
    for bus_type in [BusType::Session, BusType::System] {
        if let Ok(players) = find_mpris_players(bus_type.clone()) {
            if !players.is_empty() {
                println!("  {} bus:", bus_type);
                for player in players {
                    println!("    - {}", player.bus_name);
                    if let Some(identity) = &player.identity {
                        println!("      ({})", identity);
                    }
                }
            }
        }
    }
}

#[derive(Debug)]
struct PlayerInfo {
    bus_name: String,
    bus_type: BusType,
}
