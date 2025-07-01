use clap::Parser;
use audiocontrol::data::{PlayerUpdate, Song, PlaybackState, LoopMode};
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

    #[clap(long, default_value = "http://localhost:8000")]
    acr_host: String,
}

fn main() -> Result<(), Box<dyn Error>> {
    let args = Args::parse();
    let mut updates: Vec<PlayerUpdate> = Vec::new();

    let mut song_changed = false;
    let mut current_song = Song::default(); // Assuming Song has a Default impl or a new()

    if let Some(artist) = args.artist {
        current_song.artist = Some(artist);
        song_changed = true;
    }
    if let Some(title) = args.title {
        current_song.title = Some(title);
        song_changed = true;
    }
    if let Some(album) = args.album {
        current_song.album = Some(album);
        song_changed = true;
    }
    if let Some(length) = args.length {
        current_song.duration = Some(length);
        song_changed = true;
    }

    if song_changed {
        updates.push(PlayerUpdate::SongChanged(Some(current_song)));
    }

    if let Some(pos) = args.position {
        updates.push(PlayerUpdate::PositionChanged(Some(pos)));
    }

    if let Some(state) = args.state {
        updates.push(PlayerUpdate::StateChanged(state));
    }

    if let Some(loop_mode) = args.loop_mode {
        updates.push(PlayerUpdate::LoopModeChanged(loop_mode));
    }

    if let Some(shuffle) = args.shuffle {
        updates.push(PlayerUpdate::ShuffleChanged(shuffle));
    }

    if updates.is_empty() {
        println!("No updates to send.");
        return Ok(());
    }

    let client = ureq::agent(); // Using ureq as it's simpler for sync CLI
    let url = format!("{}/player/{}/update", args.acr_host, args.player_name);

    println!("Sending update to: {}", url);
    println!("Payload: {}", serde_json::to_string_pretty(&updates)?);

    let response = client.post(&url)
        .set("Content-Type", "application/json")
        .send_string(&serde_json::to_string(&updates)?);

    match response {
        Ok(resp) => {
            if resp.status() >= 200 && resp.status() < 300 {
                println!("Update sent successfully. Status: {}", resp.status());
            } else {
                eprintln!("Failed to send update. Status: {}", resp.status());
                eprintln!("Response: {}", resp.into_string()?);
            }
        }
        Err(e) => {
            eprintln!("Error sending request: {}", e);
            return Err(Box::new(e));
        }
    }

    Ok(())
}
