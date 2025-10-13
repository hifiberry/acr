use clap::Parser;
use log::{info, debug};
use serde_json::Value;
use std::error::Error;
use audiocontrol::helpers::bluez::BlueZManager;

#[derive(Parser, Debug)]
#[clap(author, version, about = "Show Bluetooth player information via AudioControl API or direct D-Bus", long_about = None)]
struct Args {
    /// AudioControl host URL (used when --direct is not specified)
    #[clap(long, default_value = "http://localhost:1080")]
    host: String,

    /// Request timeout in seconds
    #[clap(long, default_value = "5")]
    timeout: u64,

    /// Show detailed output
    #[clap(short, long)]
    verbose: bool,

    /// Use direct D-Bus access instead of AudioControl API
    #[clap(long)]
    direct: bool,
}

fn main() -> Result<(), Box<dyn Error>> {
    let args = Args::parse();
    
    // Initialize logging
    let log_level = if args.verbose { "debug" } else { "info" };
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or(log_level)).init();

    info!("AudioControl Bluetooth Info Tool");
    
    if args.direct {
        info!("Using direct D-Bus access to BlueZ");
        run_direct_mode(&args)
    } else {
        info!("Connecting to AudioControl at: {}", args.host);
        run_api_mode(&args)
    }
}

/// Run in direct D-Bus mode
fn run_direct_mode(args: &Args) -> Result<(), Box<dyn Error>> {
    let bluez_manager = BlueZManager::new()
        .map_err(|e| format!("Failed to create BlueZ manager: {}", e))?;

    // Discover all Bluetooth audio devices
    let devices = bluez_manager.discover_audio_devices()
        .map_err(|e| format!("Failed to discover Bluetooth devices: {}", e))?;

    // Find the active (playing) device
    let active_device = bluez_manager.get_active_device()
        .map_err(|e| format!("Failed to get active device: {}", e))?;

    // Display active player information
    println!("=== Currently Active Player ===");
    match &active_device {
        Some(device) => {
            println!("Player ID: bluetooth:{}", device.device_address);
            if let Some(ref name) = device.device_name {
                println!("Player Name: {}", name);
            }
            println!("Device Address: {}", device.device_address);
            println!("Connected: {}", if device.is_connected { "Yes" } else { "No" });
            println!("State: {}", if device.is_playing { "Playing" } else { "Not Playing" });
        }
        None => {
            println!("No active Bluetooth player found");
        }
    }

    // Display current track information
    println!("\n=== Current Track Information ===");
    if let Some(active) = &active_device {
        match bluez_manager.get_track_info(&active.player_path) {
            Ok(track_info) => {
                if track_info.title.is_none() && track_info.artist.is_none() && track_info.album.is_none() {
                    println!("No track information available");
                } else {
                    if let Some(ref title) = track_info.title {
                        println!("Title: {}", title);
                    }
                    if let Some(ref artist) = track_info.artist {
                        println!("Artist: {}", artist);
                    }
                    if let Some(ref album) = track_info.album {
                        println!("Album: {}", album);
                    }
                    if let Some(duration) = track_info.duration {
                        println!("Duration: {:.1} seconds", duration as f64 / 1000.0);
                    }
                    if let Some(position) = track_info.position {
                        println!("Position: {:.1} seconds", position as f64 / 1000.0);
                    }
                }
            }
            Err(e) => {
                println!("Failed to get track information: {}", e);
            }
        }
    } else {
        println!("No active player to show track information");
    }

    // Display all Bluetooth players information
    println!("\n=== All Bluetooth Players ===");
    if devices.is_empty() {
        println!("No Bluetooth audio devices found");
    } else {
        println!("Found {} Bluetooth audio device(s):", devices.len());
        for (index, device) in devices.iter().enumerate() {
            println!("\nBluetooth Device {}:", index + 1);
            println!("  Device Address: {}", device.device_address);
            if let Some(ref name) = device.device_name {
                println!("  Device Name: {}", name);
            }
            println!("  Player Path: {}", device.player_path);
            println!("  Connected: {}", if device.is_connected { "Yes" } else { "No" });
            println!("  Playing: {}", if device.is_playing { "Yes" } else { "No" });
            
            // Show current track for this device if available and connected
            if device.is_connected {
                match bluez_manager.get_track_info(&device.player_path) {
                    Ok(track_info) => {
                        if let Some(ref title) = track_info.title {
                            println!("  Current Track: {}", title);
                            if let Some(ref artist) = track_info.artist {
                                println!("  Artist: {}", artist);
                            }
                        }
                    }
                    Err(e) => {
                        debug!("Failed to get track info for {}: {}", device.player_path, e);
                    }
                }
            }
        }
    }

    // Show additional verbose information if requested
    if args.verbose {
        println!("\n=== Verbose Information ===");
        println!("D-Bus system bus access successful");
        println!("BlueZ service communication working");
        
        if let Some(active) = &active_device {
            println!("\nActive device details:");
            println!("  Player Path: {}", active.player_path);
            println!("  Connection Status: {}", active.is_connected);
            println!("  Playback Status: {}", active.is_playing);
        }
    }

    Ok(())
}

/// Run in API mode (original functionality)
fn run_api_mode(args: &Args) -> Result<(), Box<dyn Error>> {

    // Get list of all players
    let players_url = format!("{}/api/players", args.host);
    debug!("Fetching players from: {}", players_url);
    
    let players_response = ureq::get(&players_url)
        .timeout(std::time::Duration::from_secs(args.timeout))
        .call()
        .map_err(|e| format!("Failed to fetch players: {}", e))?;

    let players_text = players_response.into_string()
        .map_err(|e| format!("Failed to read players response: {}", e))?;
    
    let players: Value = serde_json::from_str(&players_text)
        .map_err(|e| format!("Failed to parse players JSON: {}", e))?;

    debug!("Players response: {}", serde_json::to_string_pretty(&players)?);

    // Find active player and Bluetooth players
    let mut active_player: Option<&Value> = None;
    let mut bluetooth_players = Vec::new();
    
    if let Some(players_array) = players.as_array() {
        for player in players_array {
            if let Some(player_obj) = player.as_object() {
                // Check if this is a Bluetooth player
                if let Some(player_id) = player_obj.get("player_id").and_then(|v| v.as_str()) {
                    if player_id.starts_with("bluetooth:") {
                        bluetooth_players.push(player);
                    }
                }
                
                // Check if this is the active player
                if let Some(is_active) = player_obj.get("active").and_then(|v| v.as_bool()) {
                    if is_active {
                        active_player = Some(player);
                    }
                }
            }
        }
    }

    // Display active player information
    println!("=== Currently Active Player ===");
    match active_player {
        Some(player) => {
            if let Some(player_obj) = player.as_object() {
                if let Some(player_id) = player_obj.get("player_id").and_then(|v| v.as_str()) {
                    println!("Player ID: {}", player_id);
                }
                if let Some(player_name) = player_obj.get("player_name").and_then(|v| v.as_str()) {
                    println!("Player Name: {}", player_name);
                }
                if let Some(state) = player_obj.get("state").and_then(|v| v.as_str()) {
                    println!("State: {}", state);
                }
            }
        }
        None => {
            println!("No active player found");
        }
    }

    // Display current track information
    println!("\n=== Current Track Information ===");
    if let Some(active) = active_player {
        if let Some(current_song) = active.get("current_song") {
            if current_song.is_null() {
                println!("No track currently playing");
            } else {
                if let Some(song_obj) = current_song.as_object() {
                    if let Some(title) = song_obj.get("title").and_then(|v| v.as_str()) {
                        println!("Title: {}", title);
                    }
                    if let Some(artist) = song_obj.get("artist").and_then(|v| v.as_str()) {
                        println!("Artist: {}", artist);
                    }
                    if let Some(album) = song_obj.get("album").and_then(|v| v.as_str()) {
                        println!("Album: {}", album);
                    }
                    if let Some(duration) = song_obj.get("duration").and_then(|v| v.as_f64()) {
                        println!("Duration: {:.1} seconds", duration);
                    }
                    if let Some(position) = song_obj.get("position").and_then(|v| v.as_f64()) {
                        println!("Position: {:.1} seconds", position);
                    }
                }
            }
        } else {
            println!("No track information available");
        }
    } else {
        println!("No active player to show track information");
    }

    // Display Bluetooth players information
    println!("\n=== Bluetooth Players ===");
    if bluetooth_players.is_empty() {
        println!("No Bluetooth players found");
    } else {
        println!("Found {} Bluetooth player(s):", bluetooth_players.len());
        for (index, player) in bluetooth_players.iter().enumerate() {
            println!("\nBluetooth Player {}:", index + 1);
            if let Some(player_obj) = player.as_object() {
                if let Some(player_id) = player_obj.get("player_id").and_then(|v| v.as_str()) {
                    println!("  Player ID: {}", player_id);
                }
                if let Some(player_name) = player_obj.get("player_name").and_then(|v| v.as_str()) {
                    println!("  Player Name: {}", player_name);
                }
                if let Some(state) = player_obj.get("state").and_then(|v| v.as_str()) {
                    println!("  State: {}", state);
                }
                if let Some(is_active) = player_obj.get("active").and_then(|v| v.as_bool()) {
                    println!("  Active: {}", if is_active { "Yes" } else { "No" });
                }
                
                // Show current song for this Bluetooth player if available
                if let Some(current_song) = player_obj.get("current_song") {
                    if !current_song.is_null() {
                        if let Some(song_obj) = current_song.as_object() {
                            if let Some(title) = song_obj.get("title").and_then(|v| v.as_str()) {
                                println!("  Current Track: {}", title);
                                if let Some(artist) = song_obj.get("artist").and_then(|v| v.as_str()) {
                                    println!("  Artist: {}", artist);
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // Show additional verbose information if requested
    if args.verbose {
        println!("\n=== Verbose Information ===");
        println!("API Endpoint: {}", players_url);
        println!("Response received successfully");
        
        if let Some(active) = active_player {
            println!("\nActive player JSON:");
            println!("{}", serde_json::to_string_pretty(active)?);
        }
    }

    Ok(())
}