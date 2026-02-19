#![cfg(unix)]

use std::env;
use dbus::blocking::{Connection, Proxy};
use dbus::arg::RefArg;
use std::time::Duration;
use audiocontrol::helpers::mpris::{find_mpris_players, BusType, get_dbus_property, retrieve_mpris_metadata, extract_song_from_mpris_metadata};

fn main() {
    env_logger::init();
    
    let args: Vec<String> = env::args().collect();
    
    if args.len() < 2 || args[1] == "--help" || args[1] == "-h" {
        print_help();
        return;
    }
    
    let player_identifier = &args[1];
    
    println!("AudioControl MPRIS State Inspector");
    println!("==================================");
    println!("Player: {}", player_identifier);
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
    println!();
    
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
    
    // Create proxy for the player
    let proxy = Proxy::new(&player.bus_name, "/org/mpris/MediaPlayer2", Duration::from_millis(5000), &conn);
    
    // Display current song info first
    print_current_song(&proxy);
    
    // Get and display all MPRIS properties
    print_mpris_state(&proxy);
}

fn print_help() {
    println!("AudioControl MPRIS State Inspector");
    println!();
    println!("USAGE:");
    println!("    audiocontrol_get_mpris_state <PLAYER_IDENTIFIER> [OPTIONS]");
    println!();
    println!("ARGUMENTS:");
    println!("    <PLAYER_IDENTIFIER>    Bus name or partial name of the MPRIS player");
    println!();
    println!("OPTIONS:");
    println!("    -h, --help    Print this help message");
    println!();
    println!("DESCRIPTION:");
    println!("    Displays all available MPRIS metadata and properties for a specified");
    println!("    media player. Use audiocontrol_list_mpris_players to see available players.");
    println!();
    println!("EXAMPLES:");
    println!("    audiocontrol_get_mpris_state org.mpris.MediaPlayer2.vlc");
    println!("        Show full MPRIS state for VLC player");
    println!();
    println!("    audiocontrol_get_mpris_state spotify");
    println!("        Show MPRIS state for Spotify (partial name match)");
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

fn print_current_song(proxy: &Proxy<'_, &Connection>) {
    println!("Current Song:");
    println!("=============");
    
    // Get playback status first
    let status = if let Some(status_variant) = get_dbus_property(proxy, "org.mpris.MediaPlayer2.Player", "PlaybackStatus") {
        status_variant.as_str().unwrap_or("Unknown").to_string()
    } else {
        "Unknown".to_string()
    };
    
    // Get metadata
    if let Some(metadata_variant) = retrieve_mpris_metadata(proxy) {
        if let Some(song) = extract_song_from_mpris_metadata(&metadata_variant) {
            let title = song.title.as_deref().unwrap_or("Unknown Title");
            let artist = song.artist.as_deref().unwrap_or("Unknown Artist");
            
            println!("  Status: {}", status);
            println!("  Title:  {}", title);
            println!("  Artist: {}", artist);
            
            if let Some(album) = &song.album {
                if !album.is_empty() {
                    println!("  Album:  {}", album);
                }
            }
            
            // Show track number if available
            if let Some(track_number) = song.track_number {
                if track_number > 0 {
                    println!("  Track:  #{}", track_number);
                }
            }
            
            // Show duration if available
            if let Some(duration) = song.duration {
                let total_seconds = duration as i64;
                let minutes = total_seconds / 60;
                let seconds = total_seconds % 60;
                println!("  Length: {}:{:02}", minutes, seconds);
            }
            
            // Show genres if available
            if !song.genres.is_empty() {
                if song.genres.len() == 1 {
                    println!("  Genre:  {}", song.genres[0]);
                } else {
                    println!("  Genres: {}", song.genres.join(", "));
                }
            }
        } else {
            println!("  Status: {} - No track information available", status);
        }
    } else {
        println!("  Status: {} - No metadata available", status);
    }
    
    println!();
}

fn print_mpris_state(proxy: &Proxy<'_, &Connection>) {
    println!("MPRIS MediaPlayer2 Interface:");
    println!("============================");
    print_mediaplayer2_properties(proxy);
    
    println!();
    println!("MPRIS MediaPlayer2.Player Interface:");
    println!("===================================");
    print_player_properties(proxy);
    
    println!();
    println!("Current Track Metadata:");
    println!("======================");
    print_metadata(proxy);
    
    println!();
    println!("Track List Interface (if supported):");
    println!("===================================");
    print_tracklist_properties(proxy);
    
    println!();
    println!("Playlists Interface (if supported):");
    println!("==================================");
    print_playlists_properties(proxy);
}

fn print_mediaplayer2_properties(proxy: &Proxy<'_, &Connection>) {
    let properties = [
        "CanQuit",
        "Fullscreen",
        "CanSetFullscreen",
        "CanRaise",
        "HasTrackList",
        "Identity",
        "DesktopEntry",
        "SupportedUriSchemes",
        "SupportedMimeTypes",
    ];
    
    for prop in properties {
        if let Some(value) = get_dbus_property(proxy, "org.mpris.MediaPlayer2", prop) {
            print!("  {}: ", prop);
            print_property_value(&value);
            println!();
        }
    }
}

fn print_player_properties(proxy: &Proxy<'_, &Connection>) {
    let properties = [
        "PlaybackStatus",
        "LoopStatus", 
        "Rate",
        "Shuffle",
        "Volume",
        "Position",
        "MinimumRate",
        "MaximumRate",
        "CanGoNext",
        "CanGoPrevious",
        "CanPlay",
        "CanPause",
        "CanSeek",
        "CanControl",
    ];
    
    for prop in properties {
        if let Some(value) = get_dbus_property(proxy, "org.mpris.MediaPlayer2.Player", prop) {
            print!("  {}: ", prop);
            print_property_value(&value);
            println!();
        }
    }
}

fn print_metadata(proxy: &Proxy<'_, &Connection>) {
    if let Some(metadata_variant) = retrieve_mpris_metadata(proxy) {
        if let Some(song) = extract_song_from_mpris_metadata(&metadata_variant) {
            println!("  == Structured Song Data ==");
            if let Some(title) = &song.title {
                println!("  Title: {}", title);
            }
            if let Some(artist) = &song.artist {
                println!("  Artist: {}", artist);
            }
            if let Some(album) = &song.album {
                println!("  Album: {}", album);
            }
            if let Some(album_artist) = &song.album_artist {
                println!("  Album Artist: {}", album_artist);
            }
            if let Some(track_number) = song.track_number {
                println!("  Track Number: {}", track_number);
            }
            if let Some(duration) = song.duration {
                println!("  Duration: {:.1} seconds", duration);
            }
            if let Some(year) = song.year {
                println!("  Year: {}", year);
            }
            if !song.genres.is_empty() {
                println!("  Genres: [{}]", song.genres.join(", "));
            }
            if let Some(cover_art_url) = &song.cover_art_url {
                println!("  Cover Art: {}", cover_art_url);
            }
            
            // Show raw metadata if there are additional fields
            if !song.metadata.is_empty() {
                println!("  == Raw MPRIS Metadata ==");
                let mut sorted_keys: Vec<_> = song.metadata.keys().collect();
                sorted_keys.sort();
                
                for key in sorted_keys {
                    if let Some(value) = song.metadata.get(key) {
                        println!("  {}: {}", key, value);
                    }
                }
            }
        } else {
            println!("  No metadata available");
        }
    } else {
        println!("  Metadata property not available");
    }
}

fn print_tracklist_properties(proxy: &Proxy<'_, &Connection>) {
    let properties = [
        "Tracks",
        "CanEditTracks",
    ];
    
    let mut found_any = false;
    for prop in properties {
        if let Some(value) = get_dbus_property(proxy, "org.mpris.MediaPlayer2.TrackList", prop) {
            if !found_any {
                found_any = true;
            }
            print!("  {}: ", prop);
            print_property_value(&value);
            println!();
        }
    }
    
    if !found_any {
        println!("  TrackList interface not supported or no properties available");
    }
}

fn print_playlists_properties(proxy: &Proxy<'_, &Connection>) {
    let properties = [
        "PlaylistCount",
        "Orderings",
        "ActivePlaylist",
    ];
    
    let mut found_any = false;
    for prop in properties {
        if let Some(value) = get_dbus_property(proxy, "org.mpris.MediaPlayer2.Playlists", prop) {
            if !found_any {
                found_any = true;
            }
            print!("  {}: ", prop);
            print_property_value(&value);
            println!();
        }
    }
    
    if !found_any {
        println!("  Playlists interface not supported or no properties available");
    }
}

fn print_property_value(value: &dbus::arg::Variant<Box<dyn RefArg>>) {
    // Try different types
    if let Some(s) = value.as_str() {
        print!("{}", s);
    } else if let Some(b) = value.as_u64().map(|v| v != 0).or_else(|| value.as_i64().map(|v| v != 0)) {
        print!("{}", b);
    } else if let Some(i) = value.as_i64() {
        print!("{}", i);
    } else if let Some(u) = value.as_u64() {
        print!("{}", u);
    } else if let Some(f) = value.as_f64() {
        print!("{}", f);
    } else if let Some(mut iter) = value.as_iter() {
        print!("[");
        let mut first = true;
        for item in iter {
            if !first {
                print!(", ");
            }
            first = false;
            
            if let Some(s) = item.as_str() {
                print!("\"{}\"", s);
            } else {
                print!("{:?}", item);
            }
        }
        print!("]");
    } else {
        print!("<complex value: {:?}>", value);
    }
}

#[derive(Debug)]
struct PlayerInfo {
    bus_name: String,
    bus_type: BusType,
}
