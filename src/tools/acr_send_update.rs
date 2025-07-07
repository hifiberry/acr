use clap::{Parser, Subcommand};
use audiocontrol::data::{PlaybackState, LoopMode};
use serde_json::{json, Value};
use std::error::Error;

#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
struct Args {
    /// Name of the player
    player_name: String,

    #[clap(long, default_value = "http://localhost:1080/api")]
    baseurl: String,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Update song information and optionally playback state
    /// 
    /// Example: audiocontrol_send_update player1 song --title "Song Title" --artist "Artist Name" --album "Album Name"
    Song {
        #[clap(long, help = "Artist name")]
        artist: Option<String>,

        #[clap(long, help = "Song title")]
        title: Option<String>,

        #[clap(long, help = "Album name")]
        album: Option<String>,

        #[clap(long, help = "Song duration in seconds")]
        length: Option<f64>, // Duration in seconds

        #[clap(long, help = "Stream URI or track identifier")]
        uri: Option<String>, // Stream URI

        /// Playback state to set with the song (default: Playing)
        #[clap(long, default_value = "Playing", help = "Playback state (Playing, Paused, Stopped, etc.)")]
        state: PlaybackState,
    },

    /// Update playback state
    /// 
    /// Example: audiocontrol_send_update player1 state Playing
    State {
        /// Playback state (Playing, Paused, Stopped, etc.)
        #[clap(help = "Playback state (Playing, Paused, Stopped, Killed, Disconnected, Unknown)")]
        state: PlaybackState,
    },

    /// Update shuffle setting
    /// 
    /// Example: audiocontrol_send_update player1 shuffle true
    Shuffle {
        /// Enable or disable shuffle (true/false)
        #[clap(help = "Enable shuffle (true) or disable shuffle (false)")]
        enabled: String,
    },

    /// Update loop mode
    /// 
    /// Example: audiocontrol_send_update player1 loop Playlist
    Loop {
        /// Loop mode (None, Track, Playlist)
        #[clap(help = "Loop mode: None (no looping), Track (repeat current track), Playlist (repeat playlist)")]
        mode: LoopMode,
    },

    /// Update playback position
    /// 
    /// Example: audiocontrol_send_update player1 position 45.5
    Position {
        /// Current playback position in seconds
        #[clap(help = "Playback position in seconds (e.g., 45.5 for 45.5 seconds)")]
        position: f64,
    },
}

fn main() -> Result<(), Box<dyn Error>> {
    let args = Args::parse();
    let client = ureq::agent();

    match args.command {
        Commands::Song { artist, title, album, length, uri, state } => {
            // Send song change event first
            let mut song = json!({});
            
            if let Some(artist) = artist {
                song["artist"] = json!(artist);
            }
            if let Some(title) = title {
                song["title"] = json!(title);
            }
            if let Some(album) = album {
                song["album"] = json!(album);
            }
            if let Some(length) = length {
                song["duration"] = json!(length);
            }
            if let Some(uri) = uri {
                song["uri"] = json!(uri);
            }
            
            let song_event = json!({
                "type": "song_changed",
                "song": song
            });

            send_event(&client, &args.baseurl, &args.player_name, &song_event)?;

            // Send state change event (default to Playing)
            let state_str = match state {
                PlaybackState::Playing => "playing",
                PlaybackState::Paused => "paused",
                PlaybackState::Stopped => "stopped",
                PlaybackState::Killed => "killed",
                PlaybackState::Disconnected => "disconnected",
                PlaybackState::Unknown => "unknown",
            };

            let state_event = json!({
                "type": "state_changed",
                "state": state_str
            });

            send_event(&client, &args.baseurl, &args.player_name, &state_event)?;
        }

        Commands::State { state } => {
            let state_str = match state {
                PlaybackState::Playing => "playing",
                PlaybackState::Paused => "paused",
                PlaybackState::Stopped => "stopped",
                PlaybackState::Killed => "killed",
                PlaybackState::Disconnected => "disconnected",
                PlaybackState::Unknown => "unknown",
            };
            
            let event = json!({
                "type": "state_changed",
                "state": state_str
            });

            send_event(&client, &args.baseurl, &args.player_name, &event)?;
        }

        Commands::Shuffle { enabled } => {
            let enabled_bool = enabled.to_lowercase() == "true";
            let event = json!({
                "type": "shuffle_changed",
                "enabled": enabled_bool
            });

            send_event(&client, &args.baseurl, &args.player_name, &event)?;
        }

        Commands::Loop { mode } => {
            let mode_str = match mode {
                LoopMode::Track => "track",
                LoopMode::Playlist => "playlist",
                LoopMode::None => "none",
            };
            
            let event = json!({
                "type": "loop_mode_changed",
                "loop_mode": mode_str
            });

            send_event(&client, &args.baseurl, &args.player_name, &event)?;
        }

        Commands::Position { position } => {
            let event = json!({
                "type": "position_changed",
                "position": position
            });

            send_event(&client, &args.baseurl, &args.player_name, &event)?;
        }
    }

    Ok(())
}

fn send_event(
    client: &ureq::Agent,
    baseurl: &str,
    player_name: &str,
    event: &Value,
) -> Result<(), Box<dyn Error>> {
    let url = format!("{}/player/{}/update", baseurl, player_name);
    
    println!("Sending event to: {}", url);
    println!("Payload: {}", serde_json::to_string_pretty(&event)?);

    let response = client.post(&url)
        .set("Content-Type", "application/json")
        .send_string(&serde_json::to_string(&event)?);

    match response {
        Ok(resp) => {
            if resp.status() >= 200 && resp.status() < 300 {
                println!("Event sent successfully. Status: {}", resp.status());
            } else {
                let status = resp.status();
                let response_body = resp.into_string().unwrap_or_else(|_| "Failed to read response body".to_string());
                eprintln!("Failed to send event. Status: {}", status);
                eprintln!("Response: {}", response_body);
                return Err(format!("HTTP error: {}", status).into());
            }
        }
        Err(e) => {
            eprintln!("Error sending request: {}", e);
            return Err(Box::new(e));
        }
    }

    Ok(())
}
