#[cfg(not(windows))]
use std::env;
#[cfg(not(windows))]
use mpris::{PlayerFinder, Player};
#[cfg(not(windows))]
use log::{info, error};

#[cfg(not(windows))]
fn main() {
    env_logger::init();
    
    let args: Vec<String> = env::args().collect();
    
    if args.len() > 1 && (args[1] == "--help" || args[1] == "-h") {
        print_help();
        return;
    }
    
    println!("AudioControl MPRIS Player Scanner");
    println!("==================================");
    
    // Find all MPRIS players
    match find_mpris_players() {
        Ok(players) => {
            if players.is_empty() {
                println!("No MPRIS players found on the system bus.");
                println!("\nTip: Make sure media players that support MPRIS are running.");
                println!("Common MPRIS-enabled players include: VLC, Spotify, Rhythmbox, Audacious, etc.");
            } else {
                println!("Found {} MPRIS player(s):\n", players.len());
                
                for (i, player) in players.iter().enumerate() {
                    print_player_info(i + 1, player);
                }
                
                println!("\nSample Configuration:");
                println!("====================");
                if let Some(first_player) = players.first() {
                    print_sample_config(first_player);
                }
            }
        }
        Err(e) => {
            error!("Failed to scan for MPRIS players: {}", e);
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    }
}

#[cfg(not(windows))]
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

#[cfg(not(windows))]
fn find_mpris_players() -> Result<Vec<Player>, Box<dyn std::error::Error>> {
    info!("Scanning for MPRIS players on system bus");
    
    let finder = PlayerFinder::new()
        .map_err(|e| format!("Failed to create PlayerFinder: {}", e))?;
    
    let players = finder.find_all()
        .map_err(|e| format!("Failed to find MPRIS players: {}", e))?;
    
    info!("Found {} MPRIS players", players.len());
    Ok(players)
}

#[cfg(not(windows))]
fn print_player_info(index: usize, player: &Player) {
    println!("{}. Player Information:", index);
    println!("   Bus Name: {}", player.bus_name_player_name_part());
    
    // Try to get identity (player name)
    match player.identity() {
        Ok(identity) => println!("   Identity: {}", identity),
        Err(e) => println!("   Identity: <error getting identity: {}>", e),
    }
    
    // Try to get desktop entry
    match player.desktop_entry() {
        Ok(entry) => println!("   Desktop Entry: {}", entry),
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

#[cfg(not(windows))]
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

#[cfg(windows)]
fn main() {
    eprintln!("Error: MPRIS support is not available on Windows.");
    eprintln!("MPRIS is a Linux/Unix D-Bus based media player interface.");
    eprintln!("This tool can only be used on Linux, macOS, and other Unix-like systems.");
    std::process::exit(1);
}
