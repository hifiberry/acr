use clap::{Parser, Subcommand};
use serde_json::{json, Value};
use std::error::Error;

#[derive(Parser, Debug)]
#[clap(author, version, about = "Client for sending events to the player API", long_about = None)]
struct Args {
    /// Name of the player
    player_name: String,

    /// AudioControl host URL
    #[clap(long, default_value = "http://localhost:3000")]
    host: String,

    /// Event to send
    #[clap(subcommand)]
    event: EventType,
}

#[derive(Subcommand, Debug)]
enum EventType {
    /// Send a state change event
    StateChanged {
        /// New playback state
        #[clap(value_enum)]
        state: PlaybackState,
    },
    /// Send a song change event
    SongChanged {
        /// Song title
        #[clap(long)]
        title: String,
        /// Song artist
        #[clap(long)]
        artist: Option<String>,
        /// Song album
        #[clap(long)]
        album: Option<String>,
        /// Song duration in seconds
        #[clap(long)]
        duration: Option<f64>,
        /// Song URI
        #[clap(long)]
        uri: Option<String>,
    },
    /// Send a position change event
    PositionChanged {
        /// Position in seconds
        position: f64,
    },
    /// Send a shuffle change event
    ShuffleChanged {
        /// Enable shuffle
        #[clap(long)]
        shuffle: bool,
    },
    /// Send a loop mode change event
    LoopModeChanged {
        /// Loop mode
        #[clap(value_enum)]
        loop_mode: LoopMode,
    },
    /// Send a queue change event
    QueueChanged {
        /// JSON file containing queue data
        #[clap(long)]
        file: Option<String>,
        /// Inline queue as JSON string
        #[clap(long)]
        json: Option<String>,
    },
    /// Send a custom event from JSON
    Custom {
        /// JSON string containing the event data
        json: String,
    },
}

#[derive(clap::ValueEnum, Clone, Debug)]
enum PlaybackState {
    Playing,
    Paused,
    Stopped,
    Unknown,
}

#[derive(clap::ValueEnum, Clone, Debug)]
enum LoopMode {
    None,
    Song,
    Track,
    Playlist,
}

fn main() -> Result<(), Box<dyn Error>> {
    let args = Args::parse();

    let event_data = build_event_data(&args.event)?;

    let client = ureq::agent();
    let url = format!("{}/api/player/{}/update", args.host, args.player_name);

    println!("Sending event to: {}", url);
    println!("Event data: {}", serde_json::to_string_pretty(&event_data)?);

    let response = client
        .post(&url)
        .set("Content-Type", "application/json")
        .send_string(&serde_json::to_string(&event_data)?);

    match response {
        Ok(resp) => {
            if resp.status() >= 200 && resp.status() < 300 {
                println!("✓ Event sent successfully. Status: {}", resp.status());
            } else {
                eprintln!("✗ Failed to send event. Status: {}", resp.status());
                if let Ok(body) = resp.into_string() {
                    eprintln!("Response: {}", body);
                }
                std::process::exit(1);
            }
        }
        Err(e) => {
            eprintln!("✗ Error sending request: {}", e);
            std::process::exit(1);
        }
    }

    Ok(())
}

fn build_event_data(event: &EventType) -> Result<Value, Box<dyn Error>> {
    let event_data = match event {
        EventType::StateChanged { state } => {
            let state_str = match state {
                PlaybackState::Playing => "playing",
                PlaybackState::Paused => "paused",
                PlaybackState::Stopped => "stopped",
                PlaybackState::Unknown => "unknown",
            };

            json!({
                "type": "state_changed",
                "state": state_str
            })
        }

        EventType::SongChanged {
            title,
            artist,
            album,
            duration,
            uri,
        } => {
            let mut song = json!({
                "title": title
            });

            if let Some(artist) = artist {
                song["artist"] = json!(artist);
            }
            if let Some(album) = album {
                song["album"] = json!(album);
            }
            if let Some(duration) = duration {
                song["duration"] = json!(duration);
            }
            if let Some(uri) = uri {
                song["uri"] = json!(uri);
            }

            json!({
                "type": "song_changed",
                "song": song
            })
        }

        EventType::PositionChanged { position } => {
            json!({
                "type": "position_changed",
                "position": position
            })
        }

        EventType::ShuffleChanged { shuffle } => {
            json!({
                "type": "shuffle_changed",
                "shuffle": shuffle
            })
        }

        EventType::LoopModeChanged { loop_mode } => {
            let mode_str = match loop_mode {
                LoopMode::None => "none",
                LoopMode::Song | LoopMode::Track => "song",
                LoopMode::Playlist => "playlist",
            };

            json!({
                "type": "loop_mode_changed",
                "loop_mode": mode_str
            })
        }

        EventType::QueueChanged {
            file,
            json: json_str,
        } => {
            let queue_data = if let Some(file_path) = file {
                let file_content = std::fs::read_to_string(file_path)?;
                serde_json::from_str::<Value>(&file_content)?
            } else if let Some(json_str) = json_str {
                serde_json::from_str::<Value>(json_str)?
            } else {
                return Err("Either --file or --json must be provided for queue_changed".into());
            };

            json!({
                "type": "queue_changed",
                "queue": queue_data
            })
        }

        EventType::Custom { json: json_str } => serde_json::from_str::<Value>(json_str)?,
    };

    Ok(event_data)
}
