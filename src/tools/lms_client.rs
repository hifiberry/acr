use std::error::Error;
use clap::{Parser, Subcommand};
use log::info;
use log::warn;

use acr::players::lms::jsonrps::LmsRpcClient;

/// Command line client for interacting with a Lyrion Music Server (LMS)
#[derive(Parser)]
#[clap(author, version, about)]
struct Cli {
    /// LMS server hostname or IP address
    #[clap(short = 'H', long, default_value = "127.0.0.1")]
    host: String,

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
    
    #[clap(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// List all available LMS players
    ListPlayers,
    
    /// Show current player status
    Status,
    
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
        /// Artist ID or name
        artist: String,
    },
    
    /// List tracks from a specific album
    ListTracks {
        /// Album ID or name
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

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // Initialize logger
    env_logger::init_from_env(
        env_logger::Env::default().filter_or(env_logger::DEFAULT_FILTER_ENV, "info")
    );
    
    // Parse command line arguments
    let cli = Cli::parse();
    
    // Create LMS client
    let mut client = LmsRpcClient::new(&cli.host, cli.port);
    
    // Check if this command requires a player ID
    let requires_player = match cli.command {
        Commands::ListPlayers | Commands::ListArtists | Commands::ListAlbums { .. } | Commands::ListTracks { .. } => false,
        _ => true,
    };
    
    // Get the first connected player if player_id is not specified and command requires a player
    let player_id = if !requires_player {
        // Use "0" for server-level commands
        "0".to_string()
    } else {
        match cli.player_id {
            Some(id) => id,
            None => {
                // Get all players
                match client.get_players().await {
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
                                },
                                None => {
                                    // If no connected players but command requires one, use the first player anyway
                                    info!("No connected players found, using: {} ({})", 
                                          players[0].name, players[0].playerid);
                                    players[0].playerid.clone()
                                }
                            }
                        }
                    },
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
            let players = client.get_players().await?;
            println!("Available players ({}):", players.len());
            
            for (i, player) in players.iter().enumerate() {
                println!("{}. {} ({})", i + 1, player.name, player.playerid);
                println!("   Model: {}", player.model);
                println!("   Connected: {}", if player.is_connected != 0 { "Yes" } else { "No" });
                println!("   Power: {}", if player.power != 0 { "On" } else { "Off" });
            }
        },
        
        Commands::Status => {
            let status = client.get_player_status(&player_id).await?;
            
            println!("Player: {}", player_id);
            println!("State: {}", status.mode);
            println!("Volume: {}", status.volume);
            println!("Repeat: {}", match status.playlist_repeat {
                0 => "Off",
                1 => "Song",
                2 => "Playlist",
                _ => "Unknown",
            });
            println!("Shuffle: {}", match status.playlist_shuffle {
                0 => "Off",
                1 => "Songs",
                2 => "Albums",
                _ => "Unknown",
            });
            
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
                    println!("  Position: {}:{:02} / {}:{:02}", 
                             position_mins, position_secs, 
                             duration_mins, duration_secs);
                } else {
                    println!("  Position: {}:{:02}", position_mins, position_secs);
                }
                
                println!("  Can seek: {}", if status.can_seek != 0 { "Yes" } else { "No" });
            } else {
                println!("\nNo track currently playing");
            }
        },
        
        Commands::Play => {
            println!("Playing");
            client.play(&player_id).await?;
        },
        
        Commands::Pause => {
            println!("Pausing");
            client.pause(&player_id).await?;
        },
        
        Commands::Resume => {
            println!("Resuming");
            client.play(&player_id).await?;
        },
        
        Commands::Stop => {
            println!("Stopping");
            client.stop(&player_id).await?;
        },
        
        Commands::Next => {
            println!("Skipping to next track");
            client.next(&player_id).await?;
        },
        
        Commands::Previous => {
            println!("Skipping to previous track");
            client.previous(&player_id).await?;
        },
        
        Commands::Volume { level } => {
            println!("Setting volume to: {}", level);
            client.set_volume(&player_id, level).await?;
        },
        
        Commands::Mute => {
            println!("Muting");
            client.set_mute(&player_id, true).await?;
        },
        
        Commands::Unmute => {
            println!("Unmuting");
            client.set_mute(&player_id, false).await?;
        },
        
        Commands::Search { query } => {
            println!("Searching for: {}", query);
            let results = client.search(&player_id, &query, cli.limit).await?;
            
            if results.tracks.is_empty() && 
               results.albums.is_empty() && 
               results.artists.is_empty() {
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
                    println!("  {}. {} - {} (id: {})", 
                             i + 1, album.album, album.artist, album.id);
                }
            }
            
            // Display tracks
            if !results.tracks.is_empty() {
                println!("\nTracks:");
                for (i, track) in results.tracks.iter().enumerate() {
                    println!("  {}. {} - {} (id: {})", 
                             i + 1, track.title, track.artist, track.id);
                }
            }
        },
        
        Commands::ListArtists => {
            println!("Listing artists (up to {})", cli.limit);
            // LMS doesn't have a direct method to list all artists,
            // so we'll search for "" which returns all content
            let results = client.request(&player_id, vec!["artists", "0", &cli.limit.to_string()]).await?;
            
            if let Some(artists_array) = results.get("artists_loop") {
                if let Some(artists) = artists_array.as_array() {
                    println!("Artists ({}):", artists.len());
                    
                    for (i, artist) in artists.iter().enumerate() {
                        let name = artist.get("artist").and_then(|n| n.as_str()).unwrap_or("Unknown");
                        
                        // Handle ID which can be either a string or a number
                        let id = artist.get("id").map(|id| {
                            if id.is_string() {
                                id.as_str().unwrap_or("Unknown").to_string()
                            } else if id.is_number() {
                                id.as_number().map(|n| n.to_string()).unwrap_or("Unknown".to_string())
                            } else {
                                "Unknown".to_string()
                            }
                        }).unwrap_or("Unknown".to_string());
                        
                        println!("  {}. {} (id: {})", i + 1, name, id);
                    }
                }
            } else {
                println!("No artists found");
            }
        },
        
        Commands::ListAlbums { artist } => {
            println!("Listing albums for artist: {} (up to {})", artist, cli.limit);
            
            // Try to determine if the input is an artist ID or a name
            let is_id = artist.parse::<i32>().is_ok();
            
            let results = if is_id {
                client.request(&player_id, vec!["albums", "0", &cli.limit.to_string(), "artist_id:", &artist, "tags:al"]).await?
            } else {
                client.request(&player_id, vec!["albums", "0", &cli.limit.to_string(), "artist:", &artist, "tags:al"]).await?
            };
            
            if let Some(albums_array) = results.get("albums_loop") {
                if let Some(albums) = albums_array.as_array() {
                    println!("Albums ({}):", albums.len());
                    
                    for (i, album) in albums.iter().enumerate() {
                        let title = album.get("album").and_then(|n| n.as_str()).unwrap_or("Unknown");
                        
                        // Handle ID which is always a number in the response
                        let id = album.get("id").map(|id| {
                            if id.is_number() {
                                id.as_number().map(|n| n.to_string()).unwrap_or("Unknown".to_string())
                            } else if id.is_string() {
                                id.as_str().unwrap_or("Unknown").to_string()
                            } else {
                                "Unknown".to_string()
                            }
                        }).unwrap_or("Unknown".to_string());
                        
                        println!("  {}. {} (id: {})", i + 1, title, id);
                    }
                }
            } else {
                println!("No albums found for artist '{}'", artist);
            }
        },
        
        Commands::ListTracks { album } => {
            println!("Listing tracks for album: {} (up to {})", album, cli.limit);
            
            // Try to determine if the input is an album ID or a name
            let is_id = album.parse::<i32>().is_ok();
            
            let results = if is_id {
                client.request(&player_id, vec!["titles", "0", &cli.limit.to_string(), "album_id:", &album, "tags:at"]).await?
            } else {
                client.request(&player_id, vec!["titles", "0", &cli.limit.to_string(), "album:", &album, "tags:at"]).await?
            };
            
            if let Some(tracks_array) = results.get("titles_loop") {
                if let Some(tracks) = tracks_array.as_array() {
                    println!("Tracks ({}):", tracks.len());
                    
                    for (i, track) in tracks.iter().enumerate() {
                        let title = track.get("title").and_then(|n| n.as_str()).unwrap_or("Unknown");
                        
                        // Handle ID which is always a number in the response
                        let id = track.get("id").map(|id| {
                            if id.is_number() {
                                id.as_number().map(|n| n.to_string()).unwrap_or("Unknown".to_string())
                            } else if id.is_string() {
                                id.as_str().unwrap_or("Unknown").to_string()
                            } else {
                                "Unknown".to_string()
                            }
                        }).unwrap_or("Unknown".to_string());
                        
                        let track_num = track.get("tracknum").and_then(|n| n.as_i64()).unwrap_or(0);
                        
                        println!("  {}. {} (track #{}, id: {})", i + 1, title, track_num, id);
                    }
                }
            } else {
                println!("No tracks found for album '{}'", album);
            }
        },
        
        Commands::Repeat { mode } => {
            let mode_name = match mode {
                0 => "off",
                1 => "single track",
                2 => "playlist",
                _ => unreachable!(),
            };
            
            println!("Setting repeat mode to {} ({})", mode, mode_name);
            client.set_repeat(&player_id, mode).await?;
        },
        
        Commands::Shuffle { mode } => {
            let mode_name = match mode {
                0 => "off",
                1 => "songs",
                2 => "albums",
                _ => unreachable!(),
            };
            
            println!("Setting shuffle mode to {} ({})", mode, mode_name);
            client.set_shuffle(&player_id, mode).await?;
        },
    }
    
    Ok(())
}