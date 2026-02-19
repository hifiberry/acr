use clap::{Parser, Subcommand};
use log::{info, warn};
use std::error::Error;

use audiocontrol::helpers::macaddress::normalize_mac_address;
use audiocontrol::players::lms::jsonrps::LmsRpcClient;
use audiocontrol::players::lms::lmsserver::{find_local_servers, get_local_mac_addresses};

/// Command line client for interacting with a Lyrion Music Server (LMS)
#[derive(Parser)]
#[clap(author, version, about)]
struct Cli {
    /// LMS server hostname or IP address
    #[clap(short = 'H', long)]
    host: Option<String>,

    /// LMS server port
    #[clap(short, long, default_value_t = 9000)]
    port: u16,

    /// Player ID (MAC address) to control
    /// If not provided, the first available player will be used
    #[clap(short = 'i', long)]
    player_id: Option<String>,

    /// Number of items to display in list commands
    #[clap(short, long, default_value_t = 20)]
    limit: u32,

    /// Display extended information with all fields returned by LMS
    #[clap(short = 'e', long)]
    extended: bool,

    /// Timeout in seconds for auto-discovery (default: 2)
    #[clap(short = 't', long, default_value_t = 2)]
    timeout: u64,

    /// Enable debug logging for troubleshooting
    #[clap(long)]
    debug: bool,

    #[clap(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// List all available LMS players
    ListPlayers,

    /// Show current player status
    Status,

    /// Check if this device is connected to the server
    /// Uses MAC addresses of local network interfaces
    IsConnected {
        /// Optional MAC address to check (in format XX:XX:XX:XX:XX:XX)
        #[clap(long)]
        mac: Option<String>,
    },

    /// Play the current track
    Play,

    /// Pause the current track
    Pause,

    /// Resume playback
    Resume,

    /// Stop playback
    Stop,

    /// Skip to the next track
    Next,

    /// Skip to the previous track
    Previous,

    /// Set the volume
    Volume {
        /// Volume level (0-100)
        #[clap(value_parser = clap::value_parser!(u8).range(0..=100))]
        level: u8,
    },

    /// Mute the player
    Mute,

    /// Unmute the player
    Unmute,

    /// Search for content in the library
    Search {
        /// Search query
        query: String,
    },

    /// List artists in the library
    ListArtists,

    /// List albums by a specific artist
    ListAlbums {
        /// Artist ID
        artist: String,
    },

    /// List tracks from a specific album
    ListTracks {
        /// Album ID
        album: String,
    },

    /// Set the repeat mode
    Repeat {
        /// Repeat mode (0=off, 1=song, 2=playlist)
        #[clap(value_parser = clap::value_parser!(u8).range(0..=2))]
        mode: u8,
    },

    /// Set the shuffle mode
    Shuffle {
        /// Shuffle mode (0=off, 1=songs, 2=albums)
        #[clap(value_parser = clap::value_parser!(u8).range(0..=2))]
        mode: u8,
    },
}

fn main() -> Result<(), Box<dyn Error>> {
    // Parse command line arguments first to check for debug flag
    let cli = Cli::parse();

    // Initialize logger with appropriate level based on debug flag
    if cli.debug {
        // Set log level to debug when --debug is specified
        env_logger::Builder::from_env(env_logger::Env::default())
            .filter_level(log::LevelFilter::Debug)
            .init();
        info!("Debug logging enabled");
    } else {
        // Use default info level otherwise
        env_logger::init_from_env(
            env_logger::Env::default().filter_or(env_logger::DEFAULT_FILTER_ENV, "info"),
        );
    }

    // Create LMS client - use auto-discovery if no host is specified
    let mut client = match cli.host {
        Some(host) => {
            info!("Using LMS server at {}:{}", host, cli.port);
            LmsRpcClient::new(&host, cli.port)
        }
        None => {
            info!(
                "No server specified, using auto-discovery (timeout: {}s)...",
                cli.timeout
            );

            // Use discovery to find a server
            match find_local_servers(Some(cli.timeout)) {
                Ok(servers) => {
                    if servers.is_empty() {
                        return Err("No LMS servers found on the network. Please specify a server with -H/--host.".into());
                    }

                    // Use the first discovered server
                    let server = &servers[0];
                    info!(
                        "Using auto-discovered LMS server: {} at {}:{}",
                        server.name, server.ip, server.port
                    );

                    // Print all discovered servers
                    if servers.len() > 1 {
                        info!("Multiple servers found:");
                        for (i, s) in servers.iter().enumerate() {
                            info!("  {}. {} at {}:{}", i + 1, s.name, s.ip, s.port);
                        }
                    }

                    // Create a client using the first discovered server
                    LmsRpcClient::new(&server.ip.to_string(), server.port)
                }
                Err(e) => {
                    warn!("Error during auto-discovery: {}", e);
                    return Err(
                        "Failed to discover LMS servers. Please specify a server with -H/--host."
                            .into(),
                    );
                }
            }
        }
    };

    // Check if this command requires a player ID
    let requires_player = !matches!(cli.command,
        Commands::ListPlayers
        | Commands::ListArtists
        | Commands::ListAlbums { .. }
        | Commands::ListTracks { .. });

    // Get the first connected player if player_id is not specified and command requires a player
    let player_id = if !requires_player {
        // Use "0" for server-level commands
        "0".to_string()
    } else {
        match cli.player_id {
            Some(id) => id,
            None => {
                // Get all players
                match client.get_players() {
                    Ok(players) => {
                        if players.is_empty() {
                            if requires_player {
                                return Err("No players found. Is the LMS server running?".into());
                            } else {
                                "0".to_string() // Use a default for server commands
                            }
                        } else {
                            // Find a connected player
                            let connected_player = players.iter().find(|p| p.is_connected != 0);
                            match connected_player {
                                Some(player) => {
                                    info!("Using player: {} ({})", player.name, player.playerid);
                                    player.playerid.clone()
                                }
                                None => {
                                    // If no connected players but command requires one, use the first player anyway
                                    info!(
                                        "No connected players found, using: {} ({})",
                                        players[0].name, players[0].playerid
                                    );
                                    players[0].playerid.clone()
                                }
                            }
                        }
                    }
                    Err(e) => {
                        if requires_player {
                            return Err(format!("Failed to get players: {}", e).into());
                        } else {
                            "0".to_string() // Use a default for server commands
                        }
                    }
                }
            }
        }
    };

    // Execute the appropriate command
    match cli.command {
        Commands::ListPlayers => {
            let players = client.get_players()?;
            println!("Available players ({}):", players.len());

            for (i, player) in players.iter().enumerate() {
                println!("{}. {} ({})", i + 1, player.name, player.playerid);
                println!("   Model: {}", player.model);
                println!(
                    "   Connected: {}",
                    if player.is_connected != 0 {
                        "Yes"
                    } else {
                        "No"
                    }
                );
                println!("   Power: {}", if player.power != 0 { "On" } else { "Off" });
            }
        }

        Commands::Status => {
            let status = client.get_player_status(&player_id)?;

            println!("Player: {}", player_id);
            println!("State: {}", status.mode);
            println!("Volume: {}", status.volume);
            println!(
                "Repeat: {}",
                match status.playlist_repeat {
                    0 => "Off",
                    1 => "Song",
                    2 => "Playlist",
                    _ => "Unknown",
                }
            );
            println!(
                "Shuffle: {}",
                match status.playlist_shuffle {
                    0 => "Off",
                    1 => "Songs",
                    2 => "Albums",
                    _ => "Unknown",
                }
            );

            // Display current track information
            if !status.playlist_loop.is_empty() {
                let current_track = &status.playlist_loop[0];
                println!("\nNow playing:");
                println!("  Title: {}", current_track.title);
                println!("  Artist: {}", current_track.artist);
                println!("  Album: {}", current_track.album);

                let position_mins = (status.time / 60.0) as u32;
                let position_secs = (status.time % 60.0) as u32;

                if let Some(duration) = current_track.duration {
                    let duration_mins = (duration / 60.0) as u32;
                    let duration_secs = (duration % 60.0) as u32;
                    println!(
                        "  Position: {}:{:02} / {}:{:02}",
                        position_mins, position_secs, duration_mins, duration_secs
                    );
                } else {
                    println!("  Position: {}:{:02}", position_mins, position_secs);
                }

                println!(
                    "  Can seek: {}",
                    if status.can_seek != 0 { "Yes" } else { "No" }
                );
            } else {
                println!("\nNo track currently playing");
            }
        }

        Commands::IsConnected { mac } => {
            println!("Checking connection status...");
            // Implementation changed to use get_local_mac_addresses directly
            let is_connected = check_if_connected(&mut client, mac.as_deref())?;
            println!(
                "Connection status: {}",
                if is_connected {
                    "Connected"
                } else {
                    "Not connected"
                }
            );
        }

        Commands::Play => {
            println!("Playing");
            client.play(&player_id)?;
        }

        Commands::Pause => {
            println!("Pausing");
            client.pause(&player_id)?;
        }

        Commands::Resume => {
            println!("Resuming");
            client.play(&player_id)?;
        }

        Commands::Stop => {
            println!("Stopping");
            client.stop(&player_id)?;
        }

        Commands::Next => {
            println!("Skipping to next track");
            client.next(&player_id)?;
        }

        Commands::Previous => {
            println!("Skipping to previous track");
            client.previous(&player_id)?;
        }

        Commands::Volume { level } => {
            println!("Setting volume to: {}", level);
            client.set_volume(&player_id, level)?;
        }

        Commands::Mute => {
            println!("Muting");
            client.set_mute(&player_id, true)?;
        }

        Commands::Unmute => {
            println!("Unmuting");
            client.set_mute(&player_id, false)?;
        }

        Commands::Search { query } => {
            println!("Searching for: {}", query);
            let results = client.search(&query, cli.limit)?;

            if results.tracks.is_empty() && results.albums.is_empty() && results.artists.is_empty()
            {
                println!("No results found for '{}'", query);
                return Ok(());
            }

            // Display artists
            if !results.artists.is_empty() {
                println!("\nArtists:");
                for (i, artist) in results.artists.iter().enumerate() {
                    println!("  {}. {} (id: {})", i + 1, artist.artist, artist.id);
                }
            }

            // Display albums
            if !results.albums.is_empty() {
                println!("\nAlbums:");
                for (i, album) in results.albums.iter().enumerate() {
                    // Use .as_ref().map_or() to safely handle Option<String> values
                    let title = album.title.as_ref().map_or("Unknown", |s| s.as_str());
                    let artist = album.artist.as_ref().map_or("Unknown", |s| s.as_str());
                    let id = album.id.as_ref().map_or("Unknown", |s| s.as_str());

                    println!("  {}. {} - {} (id: {})", i + 1, title, artist, id);
                }
            }

            // Display tracks
            if !results.tracks.is_empty() {
                println!("\nTracks:");
                for (i, track) in results.tracks.iter().enumerate() {
                    println!(
                        "  {}. {} - {} (id: {})",
                        i + 1,
                        track.title,
                        track.artist,
                        track.id
                    );
                }
            }
        }

        Commands::ListArtists => {
            println!("Listing artists (up to {})", cli.limit);

            let results = client.database_request("artists", 0, cli.limit, vec![])?;

            if let Some(artists_array) = results.get("artists_loop") {
                if let Some(artists) = artists_array.as_array() {
                    println!("Artists ({}):", artists.len());

                    for (i, artist) in artists.iter().enumerate() {
                        let name = artist
                            .get("artist")
                            .and_then(|n| n.as_str())
                            .unwrap_or("Unknown");

                        // Handle ID which can be either a string or a number
                        let id = artist
                            .get("id")
                            .map(|id| {
                                if id.is_string() {
                                    id.as_str().unwrap_or("Unknown").to_string()
                                } else if id.is_number() {
                                    id.as_number()
                                        .map(|n| n.to_string())
                                        .unwrap_or("Unknown".to_string())
                                } else {
                                    "Unknown".to_string()
                                }
                            })
                            .unwrap_or("Unknown".to_string());

                        if cli.extended {
                            println!("  {}. Artist: {}", i + 1, name);
                            println!("     ID: {}", id);

                            // Print all other fields in the artist object
                            for (key, value) in artist.as_object().unwrap().iter() {
                                // Skip fields we've already handled
                                if key != "artist" && key != "id" {
                                    println!("     {}: {}", key, value);
                                }
                            }
                            println!(); // Add a blank line between artists
                        } else {
                            println!("  {}. {} (id: {})", i + 1, name, id);
                        }
                    }
                }
            } else {
                println!("No artists found");
            }
        }

        Commands::ListAlbums { artist } => {
            println!(
                "Listing albums for artist ID: {} (up to {})",
                artist, cli.limit
            );

            // Request comprehensive tags when extended mode is enabled
            let tags = if cli.extended {
                // Request all available metadata from LMS
                "aAlTYydJKLNogqrtuv" // Comprehensive tag set
            } else {
                "al" // Basic album and ID info only
            };

            // LMS uses artist_id parameter for listing albums by artist
            let results = client.database_request(
                "albums",
                0,
                cli.limit,
                vec![("artist_id", artist.as_str()), ("tags", tags)],
            )?;

            if let Some(albums_array) = results.get("albums_loop") {
                if let Some(albums) = albums_array.as_array() {
                    println!("Albums ({}):", albums.len());

                    for (i, album) in albums.iter().enumerate() {
                        let title = album
                            .get("album")
                            .and_then(|n| n.as_str())
                            .unwrap_or("Unknown");

                        // Handle ID which is always a number in the response
                        let id = album
                            .get("id")
                            .map(|id| {
                                if id.is_number() {
                                    id.as_number()
                                        .map(|n| n.to_string())
                                        .unwrap_or("Unknown".to_string())
                                } else if id.is_string() {
                                    id.as_str().unwrap_or("Unknown").to_string()
                                } else {
                                    "Unknown".to_string()
                                }
                            })
                            .unwrap_or("Unknown".to_string());

                        if cli.extended {
                            println!("  {}. Album: {}", i + 1, title);
                            println!("     ID: {}", id);

                            // Print ALL fields in the album object, with proper JSON formatting
                            if let Some(object) = album.as_object() {
                                for (key, value) in object {
                                    // Don't skip any fields, we want to show everything in extended mode
                                    // Format the value according to its type for better readability
                                    let formatted_value = match value {
                                        serde_json::Value::String(s) => format!("\"{}\"", s),
                                        serde_json::Value::Number(_)
                                        | serde_json::Value::Bool(_)
                                        | serde_json::Value::Null => value.to_string(),
                                        serde_json::Value::Array(_)
                                        | serde_json::Value::Object(_) => {
                                            serde_json::to_string_pretty(&value)
                                                .unwrap_or_else(|_| value.to_string())
                                        }
                                    };

                                    // For already displayed fields, also show them for consistency
                                    println!("     {}: {}", key, formatted_value);
                                }
                            }
                            println!(); // Add a blank line between albums
                        } else {
                            println!("  {}. {} (id: {})", i + 1, title, id);
                        }
                    }
                }
            } else {
                println!("No albums found for artist ID '{}'", artist);
            }
        }

        Commands::ListTracks { album } => {
            println!(
                "Listing tracks for album ID: {} (up to {})",
                album, cli.limit
            );

            // Request comprehensive tags when extended mode is enabled
            let tags = if cli.extended {
                // Request all available track metadata from LMS
                "acdeitloquyJKNrs" // Comprehensive tag set including artist, album, duration, etc.
            } else {
                "at" // Basic title and ID info only
            };

            // LMS uses album_id parameter for listing tracks by album
            let results = client.database_request(
                "titles",
                0,
                cli.limit,
                vec![("album_id", album.as_str()), ("tags", tags)],
            )?;

            if let Some(tracks_array) = results.get("titles_loop") {
                if let Some(tracks) = tracks_array.as_array() {
                    println!("Tracks ({}):", tracks.len());

                    for (i, track) in tracks.iter().enumerate() {
                        let title = track
                            .get("title")
                            .and_then(|n| n.as_str())
                            .unwrap_or("Unknown");

                        // Handle ID which is always a number in the response
                        let id = track
                            .get("id")
                            .map(|id| {
                                if id.is_number() {
                                    id.as_number()
                                        .map(|n| n.to_string())
                                        .unwrap_or("Unknown".to_string())
                                } else if id.is_string() {
                                    id.as_str().unwrap_or("Unknown").to_string()
                                } else {
                                    "Unknown".to_string()
                                }
                            })
                            .unwrap_or("Unknown".to_string());

                        let track_num = track.get("tracknum").and_then(|n| n.as_i64()).unwrap_or(0);

                        if cli.extended {
                            println!("  {}. Title: {}", i + 1, title);
                            println!("     ID: {}", id);
                            println!("     Track #: {}", track_num);

                            // Print ALL fields in the track object, with proper JSON formatting
                            if let Some(object) = track.as_object() {
                                for (key, value) in object {
                                    // Format the value according to its type for better readability
                                    let formatted_value = match value {
                                        serde_json::Value::String(s) => format!("\"{}\"", s),
                                        serde_json::Value::Number(_)
                                        | serde_json::Value::Bool(_)
                                        | serde_json::Value::Null => value.to_string(),
                                        serde_json::Value::Array(_)
                                        | serde_json::Value::Object(_) => {
                                            serde_json::to_string_pretty(&value)
                                                .unwrap_or_else(|_| value.to_string())
                                        }
                                    };

                                    // For already displayed fields, also show them for consistency
                                    println!("     {}: {}", key, formatted_value);
                                }
                            }
                            println!(); // Add a blank line between tracks
                        } else {
                            println!("  {}. {} (track #{}, id: {})", i + 1, title, track_num, id);
                        }
                    }
                }
            } else {
                println!("No tracks found for album ID '{}'", album);
            }
        }

        Commands::Repeat { mode } => {
            let mode_name = match mode {
                0 => "off",
                1 => "single track",
                2 => "playlist",
                _ => unreachable!(),
            };

            println!("Setting repeat mode to {} ({})", mode, mode_name);
            client.set_repeat(&player_id, mode)?;
        }

        Commands::Shuffle { mode } => {
            let mode_name = match mode {
                0 => "off",
                1 => "songs",
                2 => "albums",
                _ => unreachable!(),
            };

            println!("Setting shuffle mode to {} ({})", mode, mode_name);
            client.set_shuffle(&player_id, mode)?;
        }
    }

    Ok(())
}

/// Check if this client is connected to an LMS server
fn check_if_connected(
    client: &mut LmsRpcClient,
    mac_override: Option<&str>,
) -> Result<bool, Box<dyn Error>> {
    // Get the local MAC addresses
    let mac_addresses = get_local_mac_addresses()?;

    // If a specific MAC was provided, validate it instead
    let check_macs = if let Some(mac_str) = mac_override {
        match normalize_mac_address(mac_str) {
            Ok(mac) => vec![mac],
            Err(e) => return Err(format!("Invalid MAC address format: {}", e).into()),
        }
    } else {
        mac_addresses
    };

    // Get all players from the LMS server
    let server_players = client.get_players()?;

    // Check if any player has a MAC that matches one of our local interfaces
    for player in &server_players {
        match normalize_mac_address(&player.playerid) {
            Ok(player_mac) => {
                // Convert MAC to string and check if it's all zeros
                let mac_str = player_mac.to_string();
                let is_placeholder = mac_str == "00:00:00:00:00:00"
                    || mac_str == "00-00-00-00-00-00"
                    || mac_str == "000000000000";

                // Skip placeholder MACs
                if is_placeholder {
                    continue;
                }

                // Check if this player's MAC matches any of our local MACs
                for local_mac in &check_macs {
                    if player_mac == *local_mac {
                        return Ok(true);
                    }
                }
            }
            Err(_) => {
                // Invalid MAC format, just skip this player
                continue;
            }
        }
    }

    // No matching player found
    Ok(false)
}
