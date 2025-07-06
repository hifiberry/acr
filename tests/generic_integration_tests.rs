//! Generic integration tests for the AudioControl system
//! These tests start the AudioControl server and test the CLI tool against it

#[path = "common/mod.rs"]
mod common;
use common::*;
use std::process::Command;
use std::time::Duration;
use serde_json::json;
use serial_test::serial;

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Once;
    use std::sync::atomic::AtomicBool;
    
    static INIT: Once = Once::new();
    static mut SERVER_PROCESS: Option<std::process::Child> = None;
    static SERVER_READY: AtomicBool = AtomicBool::new(false);
    
    const TEST_PORT: u16 = 3001;
    
    async fn reset_player_state(server_url: &str) {
        // Get the CLI binary path
        let cli_binary = get_cli_binary_path().expect("Failed to get CLI binary path");
        
        // Reset player to a known state
        let reset_commands = vec![
            vec!["--host", server_url, "test_player", "state-changed", "stopped"],
            vec!["--host", server_url, "test_player", "shuffle-changed"], // No --shuffle flag = false
            vec!["--host", server_url, "test_player", "loop-mode-changed", "none"],
            vec!["--host", server_url, "test_player", "position-changed", "0.0"],
        ];
        
        for command_args in reset_commands {
            let _ = Command::new(&cli_binary)
                .args(&command_args)
                .output();
                
            // Small delay between reset commands
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        
        // Wait for reset to complete
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    #[tokio::test]
    #[serial]
    async fn test_generic_integration_state_change() {
        let server_url = common::setup_test_server(TEST_PORT, &raw mut SERVER_PROCESS, &SERVER_READY, &INIT).await;
        
        // Reset player to known state
        reset_player_state(&server_url).await;
        
        // Test initial state
        let initial_state = get_player_state(&server_url, "test_player").await;
        match initial_state {
            Ok(state) => {
                println!("Initial player state: {}", serde_json::to_string_pretty::<serde_json::Value>(&state).unwrap());
                // Initial state should be "stopped" after reset
                assert_eq!(state["state"], "stopped");
            }
            Err(e) => {
                eprintln!("Failed to get initial player state: {}", e);
                assert!(false, "Failed to get initial player state: {}", e);
                return;
            }
        }
        
        // Send state change event using CLI tool
        let cli_binary = get_cli_binary_path().expect("Failed to get CLI binary path");
        let cli_output = Command::new(&cli_binary)
            .args(&[
                "--host", &server_url,
                "test_player", "state-changed", "playing"
            ])
            .output()
            .expect("Failed to execute CLI command");
        
        if !cli_output.status.success() {
            let stderr = String::from_utf8_lossy(&cli_output.stderr);
            eprintln!("CLI command failed: {}", stderr);
            assert!(false, "CLI command failed: {}", stderr);
            return;
        }
        
        // Wait a moment for the event to be processed
        tokio::time::sleep(Duration::from_millis(100)).await;
        
        // Check that player state has changed
        let updated_state = get_player_state(&server_url, "test_player").await;
        match updated_state {
            Ok(state) => {
                println!("Updated player state: {}", serde_json::to_string_pretty::<serde_json::Value>(&state).unwrap());
                assert_eq!(state["state"], "playing");
            }
            Err(e) => {
                eprintln!("Failed to get updated player state: {}", e);
                assert!(false, "Failed to get updated player state: {}", e);
                return;
            }
        }
    }

    #[tokio::test]
    #[serial]
    async fn test_generic_integration_song_change() {
        let server_url = common::setup_test_server(TEST_PORT, &raw mut SERVER_PROCESS, &SERVER_READY, &INIT).await;
        
        // Reset player to known state
        reset_player_state(&server_url).await;
        
        // Send song change event using CLI tool
        let cli_binary = get_cli_binary_path().expect("Failed to get CLI binary path");
        let cli_output = Command::new(&cli_binary)
            .args(&[
                "--host", &server_url,
                "test_player", "song-changed",
                "--title", "Integration Test Song",
                "--artist", "Test Artist",
                "--album", "Test Album",
                "--duration", "180.5"
            ])
            .output()
            .expect("Failed to execute CLI command");
        
        if !cli_output.status.success() {
            let stderr = String::from_utf8_lossy(&cli_output.stderr);
            assert!(false, "CLI command failed: {}", stderr);
        }
        
        // Wait a moment for the event to be processed
        tokio::time::sleep(Duration::from_millis(100)).await;
        
        // Check that player song has changed
        let updated_state = get_now_playing(&server_url).await;
        match updated_state {
            Ok(state) => {
                println!("Updated now playing state: {}", serde_json::to_string_pretty::<serde_json::Value>(&state).unwrap());
                
                // Check that song information was updated
                if let Some(song) = state.get("song") {
                    assert_eq!(song["title"], "Integration Test Song");
                    assert_eq!(song["artist"], "Test Artist");
                } else {
                    assert!(false, "No song in now playing state");
                }
            }
            Err(e) => {
                assert!(false, "Failed to get updated now playing state: {}", e);
            }
        }
    }

    #[tokio::test]
    #[serial]
    async fn test_generic_integration_multiple_events() {
        let server_url = common::setup_test_server(TEST_PORT, &raw mut SERVER_PROCESS, &SERVER_READY, &INIT).await;
        
        // Reset player to known state
        reset_player_state(&server_url).await;
        
        // Send multiple events
        let cli_binary = get_cli_binary_path().expect("Failed to get CLI binary path");
        let events = vec![
            // Set song
            vec![
                "--host", &server_url,
                "test_player", "song-changed",
                "--title", "Multi Test Song",
                "--artist", "Multi Artist"
            ],
            // Set state to playing
            vec!["--host", &server_url, "test_player", "state-changed", "playing"],
            // Set shuffle
            vec!["--host", &server_url, "test_player", "shuffle-changed", "--shuffle"],
            // Set loop mode
            vec!["--host", &server_url, "test_player", "loop-mode-changed", "track"],
            // Set position
            vec!["--host", &server_url, "test_player", "position-changed", "42.5"],
        ];
        
        for event_args in events {
            let cli_output = Command::new(&cli_binary)
                .args(&event_args)
                .output()
                .expect("Failed to execute CLI command");
            
            if !cli_output.status.success() {
                let stderr = String::from_utf8_lossy(&cli_output.stderr);
                assert!(false, "CLI command failed: {}", stderr);
            }
            
            // Small delay between events
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        
        // Wait for all events to be processed
        tokio::time::sleep(Duration::from_millis(150)).await;
        
        // Check final state
        let final_player_state = get_player_state(&server_url, "test_player").await;
        let final_now_playing = get_now_playing(&server_url).await;
        
        match (final_player_state, final_now_playing) {
            (Ok(player_state), Ok(now_playing)) => {
                println!("Final player state: {}", serde_json::to_string_pretty::<serde_json::Value>(&player_state).unwrap());
                println!("Final now playing: {}", serde_json::to_string_pretty::<serde_json::Value>(&now_playing).unwrap());
                // Verify player state changes
                assert_eq!(player_state["state"], "playing");
                
                // Verify now playing changes (song and other info)
                if let Some(song) = now_playing.get("song") {
                    assert_eq!(song["title"], "Multi Test Song");
                    assert_eq!(song["artist"], "Multi Artist");
                } else {
                    assert!(false, "No song in now playing state");
                }
                
                // Verify other now playing state
                assert_eq!(now_playing["shuffle"], true);
                assert_eq!(now_playing["loop_mode"], "song");
                assert_eq!(now_playing["position"], 42.5);
            }
            (Err(e), _) => {
                assert!(false, "Failed to get final player state: {}", e);
            }
            (_, Err(e)) => {
                assert!(false, "Failed to get final now playing state: {}", e);
            }
        }
    }

    #[tokio::test]
    #[serial]
    async fn test_generic_integration_custom_event() {
        let server_url = common::setup_test_server(TEST_PORT, &raw mut SERVER_PROCESS, &SERVER_READY, &INIT).await;
        
        // Reset player to known state
        reset_player_state(&server_url).await;
        
        // Send custom event using CLI tool
        let custom_event = json!({
            "type": "state_changed",
            "state": "paused"
        });
        
        let cli_binary = get_cli_binary_path().expect("Failed to get CLI binary path");
        let cli_output = Command::new(&cli_binary)
            .args(&[
                "--host", &server_url,
                "test_player", "custom", &custom_event.to_string()
            ])
            .output()
            .expect("Failed to execute CLI command");
        
        if !cli_output.status.success() {
            let stderr = String::from_utf8_lossy(&cli_output.stderr);
            assert!(false, "CLI command failed: {}", stderr);
        }
        
        // Wait a moment for the event to be processed
        tokio::time::sleep(Duration::from_millis(100)).await;
        
        // Check that player state has changed
        let updated_state = get_player_state(&server_url, "test_player").await;
        match updated_state {
            Ok(state) => {
                println!("Updated player state: {}", serde_json::to_string_pretty::<serde_json::Value>(&state).unwrap());
                assert_eq!(state["state"], "paused");
            }
            Err(e) => {
                assert!(false, "Failed to get updated player state: {}", e);
            }
        }
    }
    
    #[tokio::test]
    #[serial]
    async fn test_players_initialization() {
        let server_url = common::setup_test_server(TEST_PORT, &raw mut SERVER_PROCESS, &SERVER_READY, &INIT).await;
        
        // Get all players to verify they are initialized
        let players_response = get_all_players(&server_url).await;
        match players_response {
            Ok(response) => {
                println!("All players response: {}", serde_json::to_string_pretty::<serde_json::Value>(&response).unwrap());
                // Verify we have the expected players
                if let Some(players) = response.get("players").and_then(|p| p.as_array()) {
                    println!("Found {} players", players.len());
                    
                    // Check for expected player types
                    let mut found_players = Vec::new();
                    for player in players {
                        if let Some(name) = player.get("name").and_then(|n| n.as_str()) {
                            found_players.push(name.to_string());
                            println!("Found player: {}", name);
                            // Verify each player has basic required fields
                            assert!(player.get("id").is_some(), "Player {} missing id", name);
                            assert!(player.get("state").is_some(), "Player {} missing state", name);
                            assert!(player.get("is_active").is_some(), "Player {} missing is_active", name);
                        }
                    }
                    
                    // We should have at least our test player, though other players might not initialize
                    // if their dependencies (MPD server, pipes, etc.) are not available
                    assert!(found_players.contains(&"test_player".to_string()), "test_player should be initialized");
                    
                    // Log which players were found
                    println!("Initialized players: {:?}", found_players);
                    
                } else {
                    assert!(false, "Invalid players response format");
                }
            }
            Err(e) => {
                assert!(false, "Failed to get players: {}", e);
            }
        }
        
        // Clean up after the last test in this module
        unsafe {
            common::force_cleanup_test_server(TEST_PORT, &raw mut SERVER_PROCESS, &SERVER_READY);
        }
    }
    
} // end of mod tests
