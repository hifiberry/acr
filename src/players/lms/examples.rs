use std::error::Error;
use tokio;

use crate::players::lms::jsonrps::LmsRpcClient;

/// Example showing how to use the LMS JSON-RPC client
pub async fn lms_client_example() -> Result<(), Box<dyn Error>> {
    // Create a new client connected to a Lyrion Music Server at 192.168.1.100:9000
    // Replace with your actual server address
    let mut client = LmsRpcClient::new("192.168.1.100", 9000);
    
    // Get all available players
    let players = client.get_players().await?;
    
    println!("Available players: {}", players.len());
    
    for player in &players {
        println!("Player: {} ({})", player.name, player.playerid);
        println!("  Model: {}", player.model);
        println!("  Connected: {}", if player.is_connected != 0 { "Yes" } else { "No" });
        println!("  Power: {}", if player.power != 0 { "On" } else { "Off" });
    }
    
    // Find the first connected player to use for our example
    let player_id = players.iter()
        .find(|p| p.is_connected != 0)
        .map(|p| p.playerid.clone());
    
    if let Some(player_id) = player_id {
        println!("\nUsing player: {}", player_id);
        
        // Get player status including current track information
        let status = client.get_player_status(&player_id).await?;
        
        println!("Player state: {}", status.mode);
        println!("Volume: {}", status.volume);
        println!("Repeat: {}", status.playlist_repeat);
        println!("Shuffle: {}", status.playlist_shuffle);
        
        // Display current track information
        if !status.playlist_loop.is_empty() {
            let current_track = &status.playlist_loop[0];
            println!("\nNow playing:");
            println!("  Title: {}", current_track.title);
            println!("  Artist: {}", current_track.artist);
            println!("  Album: {}", current_track.album);
            
            if let Some(duration) = current_track.duration {
                let minutes = (duration / 60.0) as u32;
                let seconds = (duration % 60.0) as u32;
                println!("  Duration: {}:{:02}", minutes, seconds);
                println!("  Position: {:.1} seconds", status.time);
            }
        } else {
            println!("\nNo track currently playing");
        }
        
        // Control playback (uncomment as needed)
        // println!("\nPausing playback...");
        // client.pause(&player_id).await?;
        
        // println!("Resuming playback...");
        // client.play(&player_id).await?;
        
        // println!("Setting volume to 50%...");
        // client.set_volume(&player_id, 50).await?;
        
        // Search for tracks
        println!("\nSearching for 'beatles'...");
        let search_results = client.search(&player_id, "beatles", 10).await?;
        
        println!("Found {} tracks", search_results.tracks.len());
        println!("Found {} albums", search_results.albums.len());
        println!("Found {} artists", search_results.artists.len());
        
        // Show top 3 tracks
        if !search_results.tracks.is_empty() {
            println!("\nTop tracks:");
            for (i, track) in search_results.tracks.iter().take(3).enumerate() {
                println!("  {}. {} - {}", i+1, track.title, track.artist);
            }
        }
        
        // Show albums
        if !search_results.albums.is_empty() {
            println!("\nTop albums:");
            for (i, album) in search_results.albums.iter().take(3).enumerate() {
                println!("  {}. {} - {}", i+1, album.album, album.artist);
            }
            
            // Get tracks for the first album
            if let Some(first_album) = search_results.albums.first() {
                println!("\nTracks in album '{}':", first_album.album);
                let album_tracks = client.get_album_tracks(&player_id, &first_album.id).await?;
                
                for (i, track) in album_tracks.iter().enumerate() {
                    println!("  {}. {}", i+1, track.title);
                }
            }
        }
    } else {
        println!("No connected players found!");
    }
    
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::runtime::Runtime;
    
    #[test]
    fn test_example() {
        // Skip this test by default since it requires a real LMS server
        if std::env::var("RUN_LMS_EXAMPLE").is_err() {
            println!("Skipping LMS example test. Set RUN_LMS_EXAMPLE=1 to run");
            return;
        }
        
        let rt = Runtime::new().unwrap();
        match rt.block_on(lms_client_example()) {
            Ok(_) => println!("Example completed successfully"),
            Err(e) => panic!("Example failed: {}", e),
        }
    }
}