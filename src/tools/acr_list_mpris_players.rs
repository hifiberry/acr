#![cfg(unix)]

use std::env;
use log::info;
use dbus::blocking::{Connection, Proxy};
use dbus::{arg::RefArg};
use std::collections::HashMap;
use std::time::Duration;

/// MPRIS player information
#[derive(Debug, Clone)]
pub struct MprisPlayer {
    pub bus_name: String,
    pub bus_type: BusType,
    pub identity: Option<String>,
    pub desktop_entry: Option<String>,
    pub can_control: Option<bool>,
    pub can_play: Option<bool>,
    pub can_pause: Option<bool>,
    pub can_seek: Option<bool>,
    pub can_go_next: Option<bool>,
    pub can_go_previous: Option<bool>,
    pub playback_status: Option<String>,
    pub current_track: Option<String>,
    pub current_artist: Option<String>,
}

/// Bus type enumeration
#[derive(Debug, Clone, PartialEq)]
pub enum BusType {
    Session,
    System,
}

impl std::fmt::Display for BusType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BusType::Session => write!(f, "session"),
            BusType::System => write!(f, "system"),
        }
    }
}

fn main() {
    env_logger::init();
    
    let args: Vec<String> = env::args().collect();
    
    if args.len() > 1 && (args[1] == "--help" || args[1] == "-h") {
        print_help();
        return;
    }
    
    println!("AudioControl MPRIS Player Scanner");
    println!("==================================");
    
    // Find all MPRIS players on both session and system buses
    let mut all_players = Vec::new();
    
    // Try session bus first (most common)
    println!("Scanning session bus for MPRIS players...");
    match find_mpris_players(BusType::Session) {
        Ok(players) => {
            println!("Found {} MPRIS player(s) on session bus", players.len());
            all_players.extend(players);
        }
        Err(e) => {
            println!("Warning: Failed to scan session bus: {}", e);
        }
    }
    
    // Try system bus (for system services like ShairportSync)
    println!("Scanning system bus for MPRIS players...");
    match find_mpris_players(BusType::System) {
        Ok(players) => {
            println!("Found {} MPRIS player(s) on system bus", players.len());
            all_players.extend(players);
        }
        Err(e) => {
            println!("Warning: Failed to scan system bus: {}", e);
        }
    }
    
    if all_players.is_empty() {
        println!("\nNo MPRIS players found on either session or system bus.");
        println!("\nTip: Make sure media players that support MPRIS are running.");
        println!("Common MPRIS-enabled players include: VLC, Spotify, Rhythmbox, Audacious, etc.");
        return;
    }
    
    println!("\nTotal: Found {} MPRIS player(s):\n", all_players.len());
    
    for (i, player) in all_players.iter().enumerate() {
        print_player_info(i + 1, player);
    }
    
    println!("\nSample Configuration:");
    println!("====================");
    if let Some(first_player) = all_players.first() {
        print_sample_config(first_player);
    }
}

fn print_help() {
    println!("AudioControl MPRIS Player Scanner");
    println!("");
    println!("USAGE:");
    println!("    audiocontrol_list_mpris_players [OPTIONS]");
    println!("");
    println!("OPTIONS:");
    println!("    -h, --help    Print this help message");
    println!("");
    println!("DESCRIPTION:");
    println!("    Scans the system D-Bus for MPRIS-compatible media players and displays");
    println!("    their capabilities and bus names. Use this tool to identify players");
    println!("    that can be controlled via the MPRIS interface.");
    println!("");
    println!("EXAMPLES:");
    println!("    audiocontrol_list_mpris_players");
    println!("        List all available MPRIS players");
}

/// Find MPRIS players on the specified bus
fn find_mpris_players(bus_type: BusType) -> Result<Vec<MprisPlayer>, Box<dyn std::error::Error>> {
    info!("Scanning for MPRIS players on {} bus", bus_type);
    
    let conn = match bus_type {
        BusType::Session => Connection::new_session()?,
        BusType::System => Connection::new_system()?,
    };
    
    // Get list of all services on the bus
    let proxy = Proxy::new("org.freedesktop.DBus", "/org/freedesktop/DBus", Duration::from_millis(5000), &conn);
    let (services,): (Vec<String>,) = proxy.method_call("org.freedesktop.DBus", "ListNames", ())?;
    
    let mut players = Vec::new();
    
    // Filter for MPRIS players
    for service in services {
        if service.starts_with("org.mpris.MediaPlayer2.") && service != "org.mpris.MediaPlayer2" {
            info!("Found potential MPRIS player: {}", service);
            
            match get_player_info(&conn, &service, bus_type.clone()) {
                Ok(player) => players.push(player),
                Err(e) => {
                    info!("Failed to get info for player {}: {}", service, e);
                    // Still add a basic entry even if we can't get full info
                    players.push(MprisPlayer {
                        bus_name: service,
                        bus_type: bus_type.clone(),
                        identity: None,
                        desktop_entry: None,
                        can_control: None,
                        can_play: None,
                        can_pause: None,
                        can_seek: None,
                        can_go_next: None,
                        can_go_previous: None,
                        playback_status: None,
                        current_track: None,
                        current_artist: None,
                    });
                }
            }
        }
    }
    
    info!("Found {} MPRIS players on {} bus", players.len(), bus_type);
    Ok(players)
}

/// Get detailed information about an MPRIS player
fn get_player_info(conn: &Connection, bus_name: &str, bus_type: BusType) -> Result<MprisPlayer, Box<dyn std::error::Error>> {
    let proxy = Proxy::new(bus_name, "/org/mpris/MediaPlayer2", Duration::from_millis(2000), conn);
    
    let mut player = MprisPlayer {
        bus_name: bus_name.to_string(),
        bus_type,
        identity: None,
        desktop_entry: None,
        can_control: None,
        can_play: None,
        can_pause: None,
        can_seek: None,
        can_go_next: None,
        can_go_previous: None,
        playback_status: None,
        current_track: None,
        current_artist: None,
    };
    
    // Helper function to get a property safely
    let get_property = |interface: &str, property: &str| -> Option<dbus::arg::Variant<Box<dyn RefArg>>> {
        proxy.method_call("org.freedesktop.DBus.Properties", "Get", (interface, property))
            .map(|(variant,): (dbus::arg::Variant<Box<dyn RefArg>>,)| variant)
            .ok()
    };
    
    // Get MediaPlayer2 properties
    if let Some(identity_variant) = get_property("org.mpris.MediaPlayer2", "Identity") {
        if let Some(identity) = identity_variant.as_str() {
            player.identity = Some(identity.to_string());
        }
    }
    
    if let Some(desktop_entry_variant) = get_property("org.mpris.MediaPlayer2", "DesktopEntry") {
        if let Some(desktop_entry) = desktop_entry_variant.as_str() {
            player.desktop_entry = Some(desktop_entry.to_string());
        }
    }
    
    // Get Player properties  
    if let Some(can_control_variant) = get_property("org.mpris.MediaPlayer2.Player", "CanControl") {
        if let Some(can_control) = can_control_variant.as_u64().map(|v| v != 0)
            .or_else(|| can_control_variant.as_i64().map(|v| v != 0)) {
            player.can_control = Some(can_control);
        }
    }
    
    if let Some(can_play_variant) = get_property("org.mpris.MediaPlayer2.Player", "CanPlay") {
        if let Some(can_play) = can_play_variant.as_u64().map(|v| v != 0)
            .or_else(|| can_play_variant.as_i64().map(|v| v != 0)) {
            player.can_play = Some(can_play);
        }
    }
    
    if let Some(can_pause_variant) = get_property("org.mpris.MediaPlayer2.Player", "CanPause") {
        if let Some(can_pause) = can_pause_variant.as_u64().map(|v| v != 0)
            .or_else(|| can_pause_variant.as_i64().map(|v| v != 0)) {
            player.can_pause = Some(can_pause);
        }
    }
    
    if let Some(can_seek_variant) = get_property("org.mpris.MediaPlayer2.Player", "CanSeek") {
        if let Some(can_seek) = can_seek_variant.as_u64().map(|v| v != 0)
            .or_else(|| can_seek_variant.as_i64().map(|v| v != 0)) {
            player.can_seek = Some(can_seek);
        }
    }
    
    if let Some(can_go_next_variant) = get_property("org.mpris.MediaPlayer2.Player", "CanGoNext") {
        if let Some(can_go_next) = can_go_next_variant.as_u64().map(|v| v != 0)
            .or_else(|| can_go_next_variant.as_i64().map(|v| v != 0)) {
            player.can_go_next = Some(can_go_next);
        }
    }
    
    if let Some(can_go_previous_variant) = get_property("org.mpris.MediaPlayer2.Player", "CanGoPrevious") {
        if let Some(can_go_previous) = can_go_previous_variant.as_u64().map(|v| v != 0)
            .or_else(|| can_go_previous_variant.as_i64().map(|v| v != 0)) {
            player.can_go_previous = Some(can_go_previous);
        }
    }
    
    if let Some(playback_status_variant) = get_property("org.mpris.MediaPlayer2.Player", "PlaybackStatus") {
        if let Some(playback_status) = playback_status_variant.as_str() {
            player.playback_status = Some(playback_status.to_string());
        }
    }
    
    // Get metadata
    if let Some(metadata_variant) = get_property("org.mpris.MediaPlayer2.Player", "Metadata") {
        if let Some(metadata_iter) = metadata_variant.as_iter() {
            let mut metadata_map = HashMap::new();
            
            // Parse the metadata dictionary
            let mut iter = metadata_iter;
            while let (Some(key), Some(value)) = (iter.next(), iter.next()) {
                if let Some(key_str) = key.as_str() {
                    metadata_map.insert(key_str.to_string(), value);
                }
            }
            
            // Extract title
            if let Some(title_variant) = metadata_map.get("xesam:title") {
                if let Some(title) = title_variant.as_str() {
                    player.current_track = Some(title.to_string());
                }
            }
            
            // Extract artist (usually an array)
            if let Some(artist_variant) = metadata_map.get("xesam:artist") {
                if let Some(mut artists) = artist_variant.as_iter() {
                    if let Some(first_artist) = artists.next() {
                        if let Some(artist) = first_artist.as_str() {
                            player.current_artist = Some(artist.to_string());
                        }
                    }
                } else if let Some(artist) = artist_variant.as_str() {
                    // Some implementations might return a single string instead of array
                    player.current_artist = Some(artist.to_string());
                }
            }
        }
    }
    
    Ok(player)
}

fn print_player_info(index: usize, player: &MprisPlayer) {
    println!("{}. Player Information:", index);
    println!("   Bus Name: {}", player.bus_name);
    println!("   Bus Type: {} bus", player.bus_type);
    
    // Extract player name from bus name
    let player_name = player.bus_name.strip_prefix("org.mpris.MediaPlayer2.")
        .unwrap_or("Unknown");
    println!("   Player Name: {}", player_name);
    
    // Print identity
    match &player.identity {
        Some(identity) => println!("   Identity: {}", identity),
        None => println!("   Identity: <not available>"),
    }
    
    // Print desktop entry
    match &player.desktop_entry {
        Some(entry) => println!("   Desktop Entry: {}", entry),
        None => println!("   Desktop Entry: <not available>"),
    }
    
    // Print capabilities
    println!("   Capabilities:");
    
    match player.can_control {
        Some(can_control) => println!("     - Can Control: {}", can_control),
        None => println!("     - Can Control: <not available>"),
    }
    
    match player.can_play {
        Some(can_play) => println!("     - Can Play: {}", can_play),
        None => println!("     - Can Play: <not available>"),
    }
    
    match player.can_pause {
        Some(can_pause) => println!("     - Can Pause: {}", can_pause),
        None => println!("     - Can Pause: <not available>"),
    }
    
    match player.can_seek {
        Some(can_seek) => println!("     - Can Seek: {}", can_seek),
        None => println!("     - Can Seek: <not available>"),
    }
    
    match player.can_go_next {
        Some(can_go_next) => println!("     - Can Go Next: {}", can_go_next),
        None => println!("     - Can Go Next: <not available>"),
    }
    
    match player.can_go_previous {
        Some(can_go_previous) => println!("     - Can Go Previous: {}", can_go_previous),
        None => println!("     - Can Go Previous: <not available>"),
    }
    
    // Print current status
    match &player.playback_status {
        Some(status) => println!("   Current Status: {}", status),
        None => println!("   Current Status: <not available>"),
    }
    
    // Print current track info
    match (&player.current_track, &player.current_artist) {
        (Some(track), Some(artist)) => {
            println!("   Current Track: {}", track);
            println!("   Current Artist: {}", artist);
        }
        (Some(track), None) => println!("   Current Track: {}", track),
        (None, _) => println!("   Current Track: <no track loaded>"),
    }
    
    if player.bus_type == BusType::System {
        println!("   Note: This player is on the system bus. Full MPRIS control");
        println!("         may require special configuration or elevated privileges.");
    }
    
    println!();
}

fn print_sample_config(player: &MprisPlayer) {
    println!("{{");
    println!("  \"mpris\": {{");
    println!("    \"enable\": true,");
    println!("    \"bus_name\": \"{}\",", player.bus_name);
    if player.bus_type == BusType::System {
        println!("    \"bus_type\": \"system\"");
    } else {
        println!("    \"bus_type\": \"session\"");
    }
    println!("  }}");
    println!("}}");
    println!();
    println!("Add this configuration to your audiocontrol.json players array to");
    println!("enable control of this MPRIS player through AudioControl.");
    
    if player.bus_type == BusType::System {
        println!();
        println!("Note: System bus MPRIS players may require special configuration");
        println!("      and may not be fully supported by all MPRIS libraries.");
    }
}


