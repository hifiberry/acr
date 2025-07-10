#![cfg(unix)]

use std::env;
use mpris::{PlayerFinder, Player};
use log::info;
use std::process::Command;

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
    let mut session_players = Vec::new();
    let mut system_players = Vec::new();
    
    // Try session bus first (most common)
    println!("Scanning session bus for MPRIS players...");
    match find_mpris_players_session() {
        Ok(mut players) => {
            println!("Found {} MPRIS player(s) on session bus", players.len());
            session_players.append(&mut players);
        }
        Err(e) => {
            println!("Warning: Failed to scan session bus: {}", e);
        }
    }
    
    // Try system bus (for system services like ShairportSync)
    println!("Scanning system bus for MPRIS players...");
    match find_mpris_players_system() {
        Ok(mut players) => {
            println!("Found {} MPRIS player(s) on system bus", players.len());
            system_players.append(&mut players);
        }
        Err(e) => {
            println!("Warning: Failed to scan system bus: {}", e);
        }
    }
    
    let total_players = session_players.len() + system_players.len();
    
    if total_players == 0 {
        println!("\nNo MPRIS players found on either session or system bus.");
        println!("\nTip: Make sure media players that support MPRIS are running.");
        println!("Common MPRIS-enabled players include: VLC, Spotify, Rhythmbox, Audacious, etc.");
        return;
    }
    
    println!("\nTotal: Found {} MPRIS player(s):\n", total_players);
    
    let mut index = 1;
    
    // Display session bus players
    for player in session_players.iter() {
        print_player_info(index, player);
        index += 1;
    }
    
    // Display system bus players
    for bus_name in system_players.iter() {
        print_system_player_info(index, bus_name);
        index += 1;
    }
    
    println!("\nSample Configuration:");
    println!("====================");
    if let Some(first_player) = session_players.first() {
        print_sample_config(first_player);
    } else if let Some(first_system_player) = system_players.first() {
        print_system_sample_config(first_system_player);
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

fn find_mpris_players_session() -> Result<Vec<Player>, Box<dyn std::error::Error>> {
    info!("Scanning for MPRIS players on session bus");
    
    let finder = PlayerFinder::new()
        .map_err(|e| format!("Failed to create PlayerFinder for session bus: {}", e))?;
    
    let players = finder.find_all()
        .map_err(|e| format!("Failed to find MPRIS players on session bus: {}", e))?;
    
    info!("Found {} MPRIS players on session bus", players.len());
    Ok(players)
}

fn find_mpris_players_system() -> Result<Vec<String>, Box<dyn std::error::Error>> {
    info!("Scanning for MPRIS players on system bus");
    
    // Use busctl to find MPRIS players on system bus
    let output = Command::new("busctl")
        .args(&["--system", "list", "--no-pager"])
        .output()
        .map_err(|e| format!("Failed to run busctl: {}", e))?;
    
    if !output.status.success() {
        return Err(format!("busctl failed with exit code: {}", output.status).into());
    }
    
    let output_str = String::from_utf8_lossy(&output.stdout);
    let mut players = Vec::new();
    
    // Look for MPRIS players in the output
    for line in output_str.lines() {
        if line.contains("org.mpris.MediaPlayer2.") && !line.contains("org.mpris.MediaPlayer2 ") {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if let Some(bus_name) = parts.first() {
                if bus_name.starts_with("org.mpris.MediaPlayer2.") {
                    info!("Found MPRIS player on system bus: {}", bus_name);
                    players.push(bus_name.to_string());
                }
            }
        }
    }
    
    info!("Found {} MPRIS players on system bus", players.len());
    Ok(players)
}

fn print_player_info(index: usize, player: &Player) {
    println!("{}. Player Information:", index);
    println!("   Bus Name: {}", player.bus_name_player_name_part());
    
    // Try to get identity (player name)
    let identity = player.identity();
    println!("   Identity: {}", identity);
    
    // Try to get desktop entry
    match player.get_desktop_entry() {
        Ok(Some(entry)) => println!("   Desktop Entry: {}", entry),
        Ok(None) => println!("   Desktop Entry: <not set>"),
        Err(_) => println!("   Desktop Entry: <not available>"),
    }
    
    // Check capabilities
    println!("   Capabilities:");
    
    if let Ok(can_control) = player.can_control() {
        println!("     - Can Control: {}", can_control);
    }
    
    if let Ok(can_play) = player.can_play() {
        println!("     - Can Play: {}", can_play);
    }
    
    if let Ok(can_pause) = player.can_pause() {
        println!("     - Can Pause: {}", can_pause);
    }
    
    if let Ok(can_seek) = player.can_seek() {
        println!("     - Can Seek: {}", can_seek);
    }
    
    if let Ok(can_go_next) = player.can_go_next() {
        println!("     - Can Go Next: {}", can_go_next);
    }
    
    if let Ok(can_go_previous) = player.can_go_previous() {
        println!("     - Can Go Previous: {}", can_go_previous);
    }
    
    // Try to get current status
    match player.get_playback_status() {
        Ok(status) => println!("   Current Status: {:?}", status),
        Err(_) => println!("   Current Status: <not available>"),
    }
    
    // Try to get current metadata
    match player.get_metadata() {
        Ok(metadata) => {
            if let Some(title) = metadata.title() {
                println!("   Current Track: {}", title);
                if let Some(artists) = metadata.artists() {
                    if !artists.is_empty() {
                        println!("   Current Artist: {}", artists.join(", "));
                    }
                }
            } else {
                println!("   Current Track: <no track loaded>");
            }
        }
        Err(_) => println!("   Current Track: <metadata not available>"),
    }
    
    println!();
}

fn print_sample_config(player: &Player) {
    let bus_name = format!("org.mpris.MediaPlayer2.{}", player.bus_name_player_name_part());
    
    println!("{{");
    println!("  \"mpris\": {{");
    println!("    \"enable\": true,");
    println!("    \"bus_name\": \"{}\"", bus_name);
    println!("  }}");
    println!("}}");
    println!();
    println!("Add this configuration to your audiocontrol.json players array to");
    println!("enable control of this MPRIS player through AudioControl.");
}

fn print_system_player_info(index: usize, bus_name: &str) {
    println!("{}. System Bus Player Information:", index);
    println!("   Bus Name: {}", bus_name);
    
    // Extract player name from bus name
    let player_name = bus_name.strip_prefix("org.mpris.MediaPlayer2.")
        .unwrap_or("Unknown");
    println!("   Player Name: {}", player_name);
    println!("   Bus Type: System Bus");
    
    // Try to get basic information using dbus-send
    println!("   Capabilities: <limited - use dbus-send for detailed inspection>");
    
    // Try to get identity using dbus-send
    match get_system_player_property(bus_name, "Identity") {
        Ok(identity) => println!("   Identity: {}", identity),
        Err(_) => println!("   Identity: <not available>"),
    }
    
    // Try to get current status
    match get_system_player_property(bus_name, "PlaybackStatus") {
        Ok(status) => println!("   Current Status: {}", status),
        Err(_) => println!("   Current Status: <not available>"),
    }
    
    println!("   Note: This player is on the system bus. Full MPRIS control");
    println!("         may require special configuration or elevated privileges.");
    println!();
}

fn get_system_player_property(bus_name: &str, property: &str) -> Result<String, Box<dyn std::error::Error>> {
    let output = Command::new("dbus-send")
        .args(&[
            "--system",
            "--print-reply",
            &format!("--dest={}", bus_name),
            "/org/mpris/MediaPlayer2",
            "org.freedesktop.DBus.Properties.Get",
            "string:org.mpris.MediaPlayer2",
            &format!("string:{}", property)
        ])
        .output()
        .map_err(|e| format!("Failed to run dbus-send: {}", e))?;
    
    if output.status.success() {
        let output_str = String::from_utf8_lossy(&output.stdout);
        // Parse the dbus-send output to extract the property value
        // This is a simple parser - in production you'd want something more robust
        for line in output_str.lines() {
            if line.trim().starts_with("variant") || line.trim().starts_with("string") {
                if let Some(value) = extract_dbus_string_value(line) {
                    return Ok(value);
                }
            }
        }
    }
    
    Err("Property not found or not accessible".into())
}

fn extract_dbus_string_value(line: &str) -> Option<String> {
    // Extract string value from dbus-send output
    // Look for patterns like: string "value" or variant string "value"
    if let Some(start) = line.find('"') {
        if let Some(end) = line.rfind('"') {
            if start < end {
                return Some(line[start + 1..end].to_string());
            }
        }
    }
    None
}

fn print_system_sample_config(bus_name: &str) {
    println!("{{");
    println!("  \"mpris\": {{");
    println!("    \"enable\": true,");
    println!("    \"bus_name\": \"{}\",", bus_name);
    println!("    \"bus_type\": \"system\"");
    println!("  }}");
    println!("}}");
    println!();
    println!("Add this configuration to your audiocontrol.json players array to");
    println!("enable control of this system bus MPRIS player through AudioControl.");
    println!();
    println!("Note: System bus MPRIS players may require special configuration");
    println!("      and may not be fully supported by all MPRIS libraries.");
}


