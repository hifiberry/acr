use clap::Parser;
use audiocontrol::data::{PlaybackState, LoopMode};
use serde_json::{json, Value};
use std::error::Error;

#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
struct Args {
    /// Name of the player
    player_name: String,

    #[clap(long)]
    artist: Option<String>,

    #[clap(long)]
    title: Option<String>,

    #[clap(long)]
    album: Option<String>,

    #[clap(long)]
    length: Option<f64>, // Duration in seconds

    #[clap(long)]
    position: Option<f64>, // Current playback position in seconds

    #[clap(long)]
    state: Option<PlaybackState>, // e.g., Playing, Paused, Stopped

    #[clap(long)]
    loop_mode: Option<LoopMode>, // e.g., None, Track, Playlist

    #[clap(long)]
    shuffle: Option<bool>,

    #[clap(long, default_value = "http://localhost:1080/api")]
    baseurl: String,
}

fn main() -> Result<(), Box<dyn Error>> {
    let args = Args::parse();
    let mut events: Vec<Value> = Vec::new();

    // Check if we have song change data
    if args.artist.is_some() || args.title.is_some() || args.album.is_some() || args.length.is_some() {
        let mut song = json!({});
        
        if let Some(artist) = args.artist {
            song["artist"] = json!(artist);
        }
        if let Some(title) = args.title {
            song["title"] = json!(title);
        }
        if let Some(album) = args.album {
            song["album"] = json!(album);
        }
        if let Some(length) = args.length {
            song["duration"] = json!(length);
        }
        
        events.push(json!({
            "type": "song_changed",
            "song": song
        }));
    }

    // Add position change event
    if let Some(position) = args.position {
        events.push(json!({
            "type": "position_changed",
            "position": position
        }));
    }

    // Add state change event
    if let Some(state) = args.state {
        let state_str = match state {
            PlaybackState::Playing => "playing",
            PlaybackState::Paused => "paused",
            PlaybackState::Stopped => "stopped",
            _ => "unknown"
        };
        
        events.push(json!({
            "type": "state_changed",
            "state": state_str
        }));
    }

    // Add loop mode change event
    if let Some(loop_mode) = args.loop_mode {
        let mode_str = match loop_mode {
            LoopMode::Track => "track",
            LoopMode::Playlist => "playlist",
            LoopMode::None => "none",
        };
        
        events.push(json!({
            "type": "loop_mode_changed",
            "loop_mode": mode_str
        }));
    }

    // Add shuffle change event
    if let Some(shuffle) = args.shuffle {
        events.push(json!({
            "type": "shuffle_changed",
            "shuffle": shuffle
        }));
    }

    if events.is_empty() {
        println!("No updates to send.");
        return Ok(());
    }

    let client = ureq::agent(); // Using ureq as it's simpler for sync CLI
    
    // Send each event individually
    for event in events {
        let url = format!("{}/player/{}/update", args.baseurl, args.player_name);
        
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
                    eprintln!("Failed to send event. Status: {}", resp.status());
                    eprintln!("Response: {}", resp.into_string()?);
                }
            }
            Err(e) => {
                eprintln!("Error sending request: {}", e);
                return Err(Box::new(e));
            }
        }
    }

    Ok(())
}
