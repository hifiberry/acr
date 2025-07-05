//! Full integration tests for the AudioControl system
//! These tests start the AudioControl server and test the CLI tool against it

use std::process::{Command, Stdio};
use std::time::Duration;
use std::fs;
use serde_json::json;
use reqwest;
use tokio;
use serial_test::serial;

/// Helper function to kill any existing audiocontrol processes (cross-platform)
fn kill_existing_processes() {
    println!("Killing any existing audiocontrol processes...");
    
    // Cross-platform process killing
    if cfg!(target_os = "windows") {
        // On Windows, use taskkill to kill processes by name
        let _ = Command::new("taskkill")
            .args(&["/F", "/IM", "audiocontrol.exe"])
            .output();
    } else {
        // On Linux/Unix, use pkill to kill processes by name with SIGKILL
        let _ = Command::new("pkill")
            .args(&["-KILL", "-f", "audiocontrol"])
            .output();
    }
    
    // Wait a moment for processes to be killed and ports to be released
    std::thread::sleep(Duration::from_millis(1000));
    
    println!("Process cleanup complete");
}

/// Helper function to wait for server to be ready
async fn wait_for_server(base_url: &str, timeout_seconds: u64) -> Result<(), Box<dyn std::error::Error>> {
    let client = reqwest::Client::new();
    let timeout = Duration::from_secs(timeout_seconds);
    let start = std::time::Instant::now();
    let health_url = format!("{}/api/version", base_url);
    
    println!("Waiting for server to be ready at {}", health_url);
    
    while start.elapsed() < timeout {
        match client.get(&health_url).send().await {
            Ok(response) => {
                println!("Health check response: status={}, url={}", response.status(), health_url);
                if response.status().is_success() {
                    println!("Server is ready!");
                    return Ok(());
                }
            }
            Err(e) => {
                println!("Health check failed: {}", e);
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
        }
    }
    
    Err("Server did not start within timeout".into())
}

/// Helper function to get now playing information from API
async fn get_now_playing(base_url: &str) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let client = reqwest::Client::new();
    let url = format!("{}/api/now-playing", base_url);
    
    let response = client.get(&url).send().await?;
    let status = response.status();
    let text = response.text().await?;
    
    println!("Now playing API response: status={}, url={}, body={}", status, url, text);
    
    if !status.is_success() {
        return Err(format!("API call failed with status {}: {}", status, text).into());
    }
    
    let json: serde_json::Value = serde_json::from_str(&text)?;
    Ok(json)
}

/// Helper function to get player state from API
async fn get_player_state(base_url: &str, player_name: &str) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let client = reqwest::Client::new();
    let url = format!("{}/api/players", base_url);
    
    let response = client.get(&url).send().await?;
    let status = response.status();
    let text = response.text().await?;
    
    println!("Players list API response: status={}, url={}, body={}", status, url, text);
    
    if !status.is_success() {
        return Err(format!("API call failed with status {}: {}", status, text).into());
    }
    
    let json: serde_json::Value = serde_json::from_str(&text)?;
    
    // Find the specific player in the players list
    if let Some(players) = json.get("players").and_then(|p| p.as_array()) {
        for player in players {
            if let Some(name) = player.get("name").and_then(|n| n.as_str()) {
                if name == player_name {
                    return Ok(player.clone());
                }
            }
        }
        return Err(format!("Player '{}' not found in players list", player_name).into());
    }
    
    Err("Invalid players list response format".into())
}

/// Helper function to create a test configuration file
fn create_test_config(port: u16) -> Result<String, Box<dyn std::error::Error>> {
    let config = json!({
        "players": [
            {
                "generic": {
                    "enable": true,
                    "name": "test_player",
                    "display_name": "Test Player",
                    "supports_api_events": true,
                    "capabilities": ["play", "pause", "stop", "next", "previous", "seek", "shuffle", "loop"],
                    "initial_state": "stopped",
                    "shuffle": false,
                    "loop_mode": "none"
                }
            }
        ],
        "services": {
            "webserver": {
                "enable": true,
                "host": "127.0.0.1",
                "port": port
            }
        },
        "action_plugins": []
    });
    
    let config_path = format!("test_config_{}.json", port);
    fs::write(&config_path, serde_json::to_string_pretty(&config)?)?;
    Ok(config_path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Once;
    use std::sync::atomic::{AtomicBool, Ordering};
    
    static INIT: Once = Once::new();
    static mut SERVER_PROCESS: Option<std::process::Child> = None;
    static SERVER_READY: AtomicBool = AtomicBool::new(false);
    
    const TEST_PORT: u16 = 3001;
    
    /// Cleanup guard that ensures server is killed when dropped
    struct ServerCleanupGuard;
    
    impl Drop for ServerCleanupGuard {
        fn drop(&mut self) {
            println!("ServerCleanupGuard: Ensuring server cleanup...");
            kill_existing_processes();
            
            // Clean up config files
            let _ = fs::remove_file(format!("test_config_{}.json", TEST_PORT));
        }
    }
    
    // Global cleanup guard - when tests end, this will be dropped and cleanup the server
    static CLEANUP_GUARD: std::sync::LazyLock<ServerCleanupGuard> = std::sync::LazyLock::new(|| {
        // Register a panic hook to ensure cleanup happens even if tests panic
        let original_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |panic_info| {
            println!("Test panic detected, performing cleanup...");
            kill_existing_processes();
            original_hook(panic_info);
        }));
        
        ServerCleanupGuard
    });
    
    async fn reset_player_state(server_url: &str) {
        // Reset player to a known state
        let reset_commands = vec![
            vec!["test_player", "state-changed", "stopped"],
            vec!["test_player", "shuffle-changed"], // No --shuffle flag = false
            vec!["test_player", "loop-mode-changed", "none"],
            vec!["test_player", "position-changed", "0.0"],
        ];
        
        for command_args in reset_commands {
            let mut full_args = vec![
                "run", "--bin", "audiocontrol_player_event_client", "--",
                "--host", server_url
            ];
            full_args.extend_from_slice(&command_args);
            
            let _ = Command::new("cargo")
                .args(&full_args)
                .output();
                
            // Small delay between reset commands
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        
        // Wait for reset to complete
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    
    async fn setup_test_server() -> String {
        let server_url = format!("http://localhost:{}", TEST_PORT);
        
        // Ensure cleanup guard is initialized
        std::sync::LazyLock::force(&CLEANUP_GUARD);
        
        INIT.call_once(|| {
            // Kill any existing processes first
            kill_existing_processes();
            
            // Setup
            let config_path = create_test_config(TEST_PORT).expect("Failed to create test config");
            
            // Start AudioControl server
            let server_process = Command::new("cargo")
                .args(&["run", "--bin", "audiocontrol", "--", "-c", &config_path])
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
                panic!("Server failed to start: {:?}", server_ready.err());
            }
            SERVER_READY.store(true, Ordering::Relaxed);
            
            // Give server a moment to fully initialize
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
        
        server_url
    }

    #[tokio::test]
    #[serial]
    async fn test_full_integration_state_change() {
        let server_url = setup_test_server().await;
        
        // Reset player to known state
        reset_player_state(&server_url).await;
        
        // Test initial state
        let initial_state = get_player_state(&server_url, "test_player").await;
        match initial_state {
            Ok(state) => {
                println!("Initial player state: {}", serde_json::to_string_pretty(&state).unwrap());
                // Initial state should be "stopped" after reset
                assert_eq!(state["state"], "stopped");
            }
            Err(e) => {
                panic!("Failed to get initial player state: {}", e);
            }
        }
        
        // Send state change event using CLI tool
        let cli_output = Command::new("cargo")
            .args(&[
                "run", "--bin", "audiocontrol_player_event_client", "--",
                "--host", &server_url,
                "test_player", "state-changed", "playing"
            ])
            .output()
            .expect("Failed to execute CLI command");
        
        if !cli_output.status.success() {
            let stderr = String::from_utf8_lossy(&cli_output.stderr);
            panic!("CLI command failed: {}", stderr);
        }
        
        // Wait a moment for the event to be processed
        tokio::time::sleep(Duration::from_millis(200)).await;
        
        // Check that player state has changed
        let updated_state = get_player_state(&server_url, "test_player").await;
        match updated_state {
            Ok(state) => {
                println!("Updated player state: {}", serde_json::to_string_pretty(&state).unwrap());
                assert_eq!(state["state"], "playing");
            }
            Err(e) => {
                panic!("Failed to get updated player state: {}", e);
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
        let cli_output = Command::new("cargo")
            .args(&[
                "run", "--bin", "audiocontrol_player_event_client", "--",
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
            panic!("CLI command failed: {}", stderr);
        }
        
        // Wait a moment for the event to be processed
        tokio::time::sleep(Duration::from_millis(200)).await;
        
        // Check that player song has changed
        let updated_state = get_now_playing(&server_url).await;
        match updated_state {
            Ok(state) => {
                println!("Updated now playing state: {}", serde_json::to_string_pretty(&state).unwrap());
                
                // Check that song information was updated
                if let Some(song) = state.get("song") {
                    assert_eq!(song["title"], "Integration Test Song");
                    assert_eq!(song["artist"], "Test Artist");
                } else {
                    panic!("No song in now playing state");
                }
            }
            Err(e) => {
                panic!("Failed to get updated now playing state: {}", e);
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
        let events = vec![
            // Set song
            vec![
                "test_player", "song-changed",
                "--title", "Multi Test Song",
                "--artist", "Multi Artist"
            ],
            // Set state to playing
            vec!["test_player", "state-changed", "playing"],
            // Set shuffle
            vec!["test_player", "shuffle-changed", "--shuffle"],
            // Set loop mode
            vec!["test_player", "loop-mode-changed", "track"],
            // Set position
            vec!["test_player", "position-changed", "42.5"],
        ];
        
        for event_args in events {
            let mut full_args = vec![
                "run", "--bin", "audiocontrol_player_event_client", "--",
                "--host", &server_url
            ];
            full_args.extend_from_slice(&event_args);
            
            let cli_output = Command::new("cargo")
                .args(&full_args)
                .output()
                .expect("Failed to execute CLI command");
            
            if !cli_output.status.success() {
                let stderr = String::from_utf8_lossy(&cli_output.stderr);
                panic!("CLI command failed: {}", stderr);
            }
            
            // Small delay between events
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        
        // Wait for all events to be processed
        tokio::time::sleep(Duration::from_millis(300)).await;
        
        // Check final state
        let final_player_state = get_player_state(&server_url, "test_player").await;
        let final_now_playing = get_now_playing(&server_url).await;
        
        match (final_player_state, final_now_playing) {
            (Ok(player_state), Ok(now_playing)) => {
                println!("Final player state: {}", serde_json::to_string_pretty(&player_state).unwrap());
                println!("Final now playing: {}", serde_json::to_string_pretty(&now_playing).unwrap());
                
                // Verify player state changes
                assert_eq!(player_state["state"], "playing");
                
                // Verify now playing changes (song and other info)
                if let Some(song) = now_playing.get("song") {
                    assert_eq!(song["title"], "Multi Test Song");
                    assert_eq!(song["artist"], "Multi Artist");
                } else {
                    panic!("No song in now playing state");
                }
                
                // Verify other now playing state
                assert_eq!(now_playing["shuffle"], true);
                assert_eq!(now_playing["loop_mode"], "song");
                assert_eq!(now_playing["position"], 42.5);
            }
            (Err(e), _) => {
                panic!("Failed to get final player state: {}", e);
            }
            (_, Err(e)) => {
                panic!("Failed to get final now playing state: {}", e);
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
        
        let cli_output = Command::new("cargo")
            .args(&[
                "run", "--bin", "audiocontrol_player_event_client", "--",
                "--host", &server_url,
                "test_player", "custom", &custom_event.to_string()
            ])
            .output()
            .expect("Failed to execute CLI command");
        
        if !cli_output.status.success() {
            let stderr = String::from_utf8_lossy(&cli_output.stderr);
            panic!("CLI command failed: {}", stderr);
        }
        
        // Wait a moment for the event to be processed
        tokio::time::sleep(Duration::from_millis(200)).await;
        
        // Check that player state has changed
        let updated_state = get_player_state(&server_url, "test_player").await;
        match updated_state {
            Ok(state) => {
                println!("Updated player state: {}", serde_json::to_string_pretty(&state).unwrap());
                assert_eq!(state["state"], "paused");
            }
            Err(e) => {
                panic!("Failed to get updated player state: {}", e);
            }
        }
    }
    
    // This test should be run last to verify cleanup
    #[tokio::test]
    #[serial]
    async fn test_zzz_cleanup_verification() {
        // This test runs last due to the "zzz" prefix
        // It verifies that we can clean up properly
        println!("Running final cleanup verification test...");
        
        // Force cleanup
        kill_existing_processes();
        
        // Wait a moment and verify server is down
        tokio::time::sleep(Duration::from_millis(1000)).await;
        
        let server_url = format!("http://localhost:{}", TEST_PORT);
        let client = reqwest::Client::new();
        let health_url = format!("{}/api/version", server_url);
        
        // Server should not be reachable after cleanup
        match client.get(&health_url).send().await {
            Ok(_) => {
                println!("Warning: Server still reachable after cleanup");
            }
            Err(_) => {
                println!("✓ Server properly cleaned up");
            }
        }
    }
}
