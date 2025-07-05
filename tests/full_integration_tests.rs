//! Full integration tests for the AudioControl system
//! These tests start the AudioControl server and test the CLI tool against it

#[path = "common/mod.rs"]
mod common;
use common::*;
use std::process::{Command, Stdio};
use std::time::Duration;
use serde_json::json;
use serial_test::serial;
use std::fs;

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Once;
    use std::sync::atomic::{AtomicBool, Ordering};
    
    static INIT: Once = Once::new();
    static mut SERVER_PROCESS: Option<std::process::Child> = None;
    static SERVER_READY: AtomicBool = AtomicBool::new(false);
    static CLEANUP_REGISTERED: AtomicBool = AtomicBool::new(false);
    
    const TEST_PORT: u16 = 3001;
    
    /// Force cleanup of server and test resources
    fn force_cleanup() {
        println!("[CLEANUP] Force cleanup: Killing server and cleaning up test resources...");
        
        // Kill server process directly if we have a handle to it
        unsafe {
            if let Some(mut process) = SERVER_PROCESS.take() {
                println!("[CLEANUP] Killing server process directly...");
                let _ = process.kill();
                let _ = process.wait();
            }
        }
        
        // Also kill any processes by name
        kill_existing_processes();
        
        // Clean up config files and cache directories
        let _ = fs::remove_file(format!("test_config_{}.json", TEST_PORT));
        let _ = fs::remove_dir_all(format!("test_cache_{}", TEST_PORT));
        
        println!("[CLEANUP] Force cleanup complete");
    }

    
    /// Cleanup guard that ensures server is killed when dropped
    struct ServerCleanupGuard;
    
    impl Drop for ServerCleanupGuard {
        fn drop(&mut self) {
            force_cleanup();
        }
    }
    
    /// Ensures that cleanup is registered only once for the test module
    fn ensure_module_cleanup() {
        use std::sync::atomic::Ordering;
        if !CLEANUP_REGISTERED.swap(true, Ordering::SeqCst) {
            // Register a guard to clean up when the test process exits
            std::thread::spawn(|| {
                // The guard will run force_cleanup() on drop
                let _guard = ServerCleanupGuard;
                // Block forever so the guard lives until process exit
                std::thread::park();
            });
        }
    }
    
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
    
    async fn setup_test_server() -> String {
        let server_url = format!("http://localhost:{}", TEST_PORT);
        INIT.call_once(|| {
            // Ensure binaries are built before running tests
            ensure_binaries_built().expect("Failed to build required binaries");
            // Kill any existing processes first
            kill_existing_processes();
            // Create test pipes for players that need them
            let _ = create_test_pipes();
            // Wait for librespot pipe to exist before starting server
            let ok = wait_for_librespot_pipe(5000);
            assert!(ok, "Librespot event pipe was not created in time");
            // Setup config
            let config_path = create_test_config(TEST_PORT).expect("Failed to create test config");
            
            // Get the path to the pre-built audiocontrol binary
            let target_dir = std::env::var("CARGO_TARGET_DIR")
                .unwrap_or_else(|_| "target".to_string());
            let binary_name = if cfg!(target_os = "windows") {
                "audiocontrol.exe"
            } else {
                "audiocontrol"
            };
            let binary_path = std::path::PathBuf::from(target_dir)
                .join("debug")
                .join(binary_name);
            
            // Start AudioControl server using pre-built binary
            let server_process = Command::new(&binary_path)
                .args(&["-c", &config_path])
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .expect("Failed to start AudioControl server");
            
            unsafe {
                SERVER_PROCESS = Some(server_process);
            }
        });
        
        // Wait for server to be ready if not already
        if !SERVER_READY.load(Ordering::Relaxed) {
            let server_ready = wait_for_server(&server_url, 30).await;
            if server_ready.is_err() {
                eprintln!("Server failed to start: {:?}", server_ready.err());
                return server_url; // Return anyway, let individual tests handle the failure
            }
            SERVER_READY.store(true, Ordering::Relaxed);
            
            // Give server a moment to fully initialize
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
        
        server_url
    }

    #[tokio::test]
    #[serial]
    async fn test_full_integration_state_change() {
        let server_url = setup_test_server().await;
        
        // Ensure module cleanup is initialized
        ensure_module_cleanup();
        
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
    async fn test_full_integration_song_change() {
        let server_url = setup_test_server().await;
        
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
    async fn test_full_integration_multiple_events() {
        let server_url = setup_test_server().await;
        
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
    async fn test_full_integration_custom_event() {
        let server_url = setup_test_server().await;
        
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
        let server_url = setup_test_server().await;
        
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
    }
    
    #[tokio::test]
    #[serial]
    async fn test_raat_player_initialization() {
        let server_url = setup_test_server().await;
        
        // Check if RAAT player is initialized
        let players_response = get_all_players(&server_url).await;
        match players_response {
            Ok(response) => {
                if let Some(players) = response.get("players").and_then(|p| p.as_array()) {
                    let raat_player = players.iter().find(|p| {
                        p.get("name").and_then(|n| n.as_str()).map(|s| s.contains("raat")).unwrap_or(false)
                    });
                    if let Some(player) = raat_player {
                        println!("RAAT player found: {}", serde_json::to_string_pretty::<serde_json::Value>(player).unwrap());
                        // Verify RAAT player has basic state
                        if player.get("state").is_none() {
                            eprintln!("[FAIL] RAAT player missing state field");
                            assert!(false, "RAAT player missing state");
                            return;
                        }
                        if player.get("is_active").is_none() {
                            eprintln!("[FAIL] RAAT player missing is_active field");
                            assert!(false, "RAAT player missing is_active");
                            return;
                        }
                        
                        println!("[OK] RAAT player initialized successfully");
                    } else {
                        println!("[POTENTIAL PROBLEM] RAAT player not found - this may be expected if pipe dependencies are not available");
                        // Don't fail - RAAT player may not be available in test environment
                    }
                } else {
                    assert!(false, "Invalid players response format");
                }
            }
            Err(e) => {
                assert!(false, "Failed to get players: {}", e);
            }
        }
    }
    
    #[tokio::test]
    #[serial]
    async fn test_mpd_player_initialization() {
        let server_url = setup_test_server().await;
        
        // Check if MPD player is initialized
        let players_response = get_all_players(&server_url).await;
        match players_response {
            Ok(response) => {
                if let Some(players) = response.get("players").and_then(|p| p.as_array()) {
                    let mpd_player = players.iter().find(|p| {
                        p.get("name").and_then(|n| n.as_str()).map(|s| s.contains("mpd")).unwrap_or(false)
                    });
                    if let Some(player) = mpd_player {
                        println!("MPD player found: {}", serde_json::to_string_pretty::<serde_json::Value>(player).unwrap());
                        // Verify MPD player has basic state
                        if player.get("state").is_none() {
                            eprintln!("[FAIL] MPD player missing state field");
                            assert!(false, "MPD player missing state");
                            return;
                        }
                        if player.get("is_active").is_none() {
                            eprintln!("[FAIL] MPD player missing is_active field");
                            assert!(false, "MPD player missing is_active");
                            return;
                        }
                        
                        println!("[OK] MPD player initialized successfully");
                    } else {
                        println!("[INFO] MPD player not found - this may be expected if MPD server is not available");
                        // Don't fail - MPD player may not be available in test environment
                    }
                } else {
                    assert!(false, "Invalid players response format");
                }
            }
            Err(e) => {
                assert!(false, "Failed to get players: {}", e);
            }
        }
    }
    
    #[tokio::test]
    #[serial]
    async fn test_librespot_player_initialization() {
        // moved to librespot_integration_tests.rs
    }
    
    #[tokio::test]
    #[serial]
    async fn test_librespot_api_events() {
        // moved to librespot_integration_tests.rs
    }
    
    #[tokio::test]
    #[serial]
    async fn test_librespot_pipe_events() {
        // moved to librespot_integration_tests.rs
    }
    
    #[tokio::test]
    #[serial]
    async fn test_librespot_api_event_activates_player() {
        // moved to librespot_integration_tests.rs
    }
    
    #[tokio::test]
    #[serial]
    async fn test_librespot_pipe_event_activates_player() {
        // moved to librespot_integration_tests.rs
    }
} // end of mod tests
