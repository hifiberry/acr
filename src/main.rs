use acr::data::{Song, Player, PlayerState, LoopMode, PlayerCapability};
use std::collections::HashMap;

fn main() {
    println!("AudioControl3 (ACR) Sample Usage\n");
    
    // Sample 1: Creating and using a Song object
    println!("=== Sample 1: Creating and using a Song ===");
    let mut song = Song::default();
    song.title = Some("Bohemian Rhapsody".to_string());
    song.artist = Some("Queen".to_string());
    song.album = Some("A Night at the Opera".to_string());
    song.duration = Some(354.0); // 5:54 in seconds
    song.year = Some(1975);
    
    // Adding some metadata
    let mut metadata = HashMap::new();
    metadata.insert("sample_rate".to_string(), serde_json::json!(44100));
    metadata.insert("bit_depth".to_string(), serde_json::json!(16));
    song.metadata = metadata;
    
    println!("Song: {:?}", song);
    println!("Song as JSON: {}", song.to_json());
    
    // Sample 2: Creating and using a Player
    println!("\n=== Sample 2: Creating and using a Player ===");
    let mut player = Player::new("MPD Player".to_string());
    player.player_id = Some("mpd-001".to_string());
    player.type_ = Some("mpd".to_string());
    player.state = PlayerState::Playing;
    player.volume = Some(75);
    player.muted = Some(false);
    player.position = Some(120.5); // 2:00.5 into the song
    
    // Add capabilities
    player.capabilities = Some(vec![
        PlayerCapability::Play,
        PlayerCapability::Pause,
        PlayerCapability::Stop,
        PlayerCapability::Next,
        PlayerCapability::Previous
    ]);
    
    println!("Player: {:?}", player);
    println!("Player as JSON: {}", player.to_json());
    println!("Has play capability: {}", player.has_capability(PlayerCapability::Play));
    println!("Has shuffle capability: {}", player.has_capability(PlayerCapability::Shuffle));
    
    // Sample 3: Demonstrate different player states
    println!("\n=== Sample 3: Demonstrating player states ===");
    let states = vec![
        PlayerState::Playing,
        PlayerState::Paused,
        PlayerState::Stopped,
        PlayerState::Killed,
        PlayerState::Unknown
    ];
    
    for state in states {
        let mut player = Player::new("Test Player".to_string());
        player.state = state;
        println!("Player state: {}", player.state);
    }
    
    // Sample 4: Demonstrate loop modes
    println!("\n=== Sample 4: Demonstrating loop modes ===");
    let loop_modes = vec![
        LoopMode::None,
        LoopMode::Track,
        LoopMode::Playlist
    ];
    
    for mode in loop_modes {
        println!("Loop mode: {}", mode);
    }
}