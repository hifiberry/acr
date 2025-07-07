use clap::Parser;
use audiocontrol::data::PlaybackState;
use serde_json::{json, Value};
use std::env;
use std::error::Error;

#[derive(Parser, Debug)]
#[clap(author, version, about = "Send librespot events to audiocontrol API", long_about = None)]
struct Args {
    /// Base URL for the audiocontrol API
    #[clap(long, default_value = "http://127.0.0.1:1080/api")]
    baseurl: String,

    /// Player name to use in API calls
    #[clap(long, default_value = "librespot")]
    player_name: String,

    /// Enable verbose output with full request details
    #[clap(long, short = 'v', help = "Enable verbose output")]
    verbose: bool,

    /// Suppress all output
    #[clap(long, short = 'q', help = "Quiet mode - suppress all output")]
    quiet: bool,
}

fn main() -> Result<(), Box<dyn Error>> {
    let args = Args::parse();
    let client = ureq::agent();

    // Get the player event type from environment
    let player_event = env::var("PLAYER_EVENT").unwrap_or_else(|_| "unknown".to_string());

    if !args.quiet {
        println!("Received event: {}", player_event);
    }

    match player_event.as_str() {
        "track_changed" => {
            handle_track_changed(&client, &args)?;
        }
        "playing" => {
            handle_playback_state(&client, &args, PlaybackState::Playing)?;
        }
        "paused" => {
            handle_playback_state(&client, &args, PlaybackState::Paused)?;
        }
        "seeked" => {
            handle_position_changed(&client, &args)?;
        }
        "shuffle_changed" => {
            handle_shuffle_changed(&client, &args)?;
        }
        "repeat_changed" => {
            handle_repeat_changed(&client, &args)?;
        }
        _ => {
            if !args.quiet {
                eprintln!("Unknown or unsupported event type: {}", player_event);
            }
        }
    }

    Ok(())
}

fn handle_track_changed(client: &ureq::Agent, args: &Args) -> Result<(), Box<dyn Error>> {
    let mut song = json!({});
    
    // Parse track information from environment variables
    if let Ok(title) = env::var("NAME") {
        song["title"] = json!(title);
    }
    
    if let Ok(artist) = env::var("ARTISTS") {
        song["artist"] = json!(artist);
    }
    
    if let Ok(album) = env::var("ALBUM") {
        song["album"] = json!(album);
    }
    
    if let Ok(duration_ms) = env::var("DURATION_MS") {
        if let Ok(duration) = duration_ms.parse::<u64>() {
            song["duration"] = json!(duration as f64 / 1000.0); // Convert to seconds
        }
    }
    
    if let Ok(uri) = env::var("URI") {
        song["uri"] = json!(uri);
    }

    // Add additional metadata if available
    if let Ok(track_number) = env::var("NUMBER") {
        song["track_number"] = json!(track_number);
    }
    
    if let Ok(disc_number) = env::var("DISC_NUMBER") {
        song["disc_number"] = json!(disc_number);
    }
    
    if let Ok(covers) = env::var("COVERS") {
        // Split covers by newline and take the first one
        let cover_urls: Vec<&str> = covers.lines().collect();
        if !cover_urls.is_empty() {
            song["cover_url"] = json!(cover_urls[0]);
        }
    }

    let event = json!({
        "type": "song_changed",
        "song": song
    });

    send_event(client, &args.baseurl, &args.player_name, &event, args.verbose, args.quiet)?;

    // Also send playing state since track_changed usually means we're playing
    let state_event = json!({
        "type": "state_changed",
        "state": "playing"
    });

    send_event(client, &args.baseurl, &args.player_name, &state_event, args.verbose, args.quiet)?;

    Ok(())
}

fn handle_playback_state(client: &ureq::Agent, args: &Args, state: PlaybackState) -> Result<(), Box<dyn Error>> {
    let state_str = match state {
        PlaybackState::Playing => "playing",
        PlaybackState::Paused => "paused",
        PlaybackState::Stopped => "stopped",
        PlaybackState::Killed => "killed",
        PlaybackState::Disconnected => "disconnected",
        PlaybackState::Unknown => "unknown",
    };
    
    let mut event = json!({
        "type": "state_changed",
        "state": state_str
    });

    // Add position if available
    if let Ok(position_ms) = env::var("POSITION_MS") {
        if let Ok(position) = position_ms.parse::<u64>() {
            event["position"] = json!(position as f64 / 1000.0); // Convert to seconds
        }
    }

    send_event(client, &args.baseurl, &args.player_name, &event, args.verbose, args.quiet)?;

    Ok(())
}

fn handle_shuffle_changed(client: &ureq::Agent, args: &Args) -> Result<(), Box<dyn Error>> {
    let shuffle_enabled = env::var("SHUFFLE")
        .unwrap_or_else(|_| "false".to_string())
        .to_lowercase() == "true";

    let event = json!({
        "type": "shuffle_changed",
        "enabled": shuffle_enabled
    });

    send_event(client, &args.baseurl, &args.player_name, &event, args.verbose, args.quiet)?;

    Ok(())
}

fn handle_repeat_changed(client: &ureq::Agent, args: &Args) -> Result<(), Box<dyn Error>> {
    let repeat_enabled = env::var("REPEAT")
        .unwrap_or_else(|_| "false".to_string())
        .to_lowercase() == "true";
    
    let repeat_track = env::var("REPEAT_TRACK")
        .unwrap_or_else(|_| "false".to_string())
        .to_lowercase() == "true";

    let loop_mode = if !repeat_enabled {
        "none"
    } else if repeat_track {
        "track"
    } else {
        "playlist"
    };

    let event = json!({
        "type": "loop_mode_changed",
        "loop_mode": loop_mode
    });

    send_event(client, &args.baseurl, &args.player_name, &event, args.verbose, args.quiet)?;

    Ok(())
}

fn handle_position_changed(client: &ureq::Agent, args: &Args) -> Result<(), Box<dyn Error>> {
    let mut event = json!({
        "type": "position_changed"
    });

    // Add position from environment variable
    if let Ok(position_ms) = env::var("POSITION_MS") {
        if let Ok(position) = position_ms.parse::<u64>() {
            event["position"] = json!(position as f64 / 1000.0); // Convert to seconds
        }
    }

    send_event(client, &args.baseurl, &args.player_name, &event, args.verbose, args.quiet)?;

    Ok(())
}

fn send_event(
    client: &ureq::Agent,
    baseurl: &str,
    player_name: &str,
    event: &Value,
    verbose: bool,
    quiet: bool,
) -> Result<(), Box<dyn Error>> {
    let url = format!("{}/player/{}/update", baseurl, player_name);
    
    if verbose && !quiet {
        println!("Sending event to: {}", url);
        println!("Payload: {}", serde_json::to_string_pretty(&event)?);
    }

    let response = client.post(&url)
        .set("Content-Type", "application/json")
        .send_string(&serde_json::to_string(&event)?);

    match response {
        Ok(resp) => {
            if resp.status() >= 200 && resp.status() < 300 {
                if verbose && !quiet {
                    println!("Event sent successfully. Status: {}", resp.status());
                }
            } else {
                let status = resp.status();
                let response_body = resp.into_string().unwrap_or_else(|_| "Failed to read response body".to_string());
                if !quiet {
                    eprintln!("Failed to send event. Status: {}", status);
                    eprintln!("Response: {}", response_body);
                }
                return Err(format!("HTTP error: {}", status).into());
            }
        }
        Err(e) => {
            if !quiet {
                eprintln!("Error sending request: {}", e);
            }
            return Err(Box::new(e));
        }
    }

    Ok(())
}
