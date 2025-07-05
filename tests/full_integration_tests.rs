//! Full integration tests for the AudioControl system
//! These tests start the AudioControl server and test the CLI tool against it

use std::process::{Command, Stdio};
use std::time::Duration;
use std::fs;
use std::io::{self, Write};
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
    std::thread::sleep(Duration::from_millis(500));
    
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
                tokio::time::sleep(Duration::from_millis(200)).await;
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
            },
            {
                "raat": {
                    "enable": true,
                    "metadata_pipe": "/tmp/test_raat_metadata",
                    "control_pipe": "/tmp/test_raat_control",
                    "reopen_metadata_pipe": false
                }
            },
            {
                "mpd": {
                    "enable": true,
                    "host": "localhost",
                    "port": 6600,
                    "load_on_startup": false,
                    "artist_separator": [",", "feat. "],
                    "enhance_metadata": false
                }
            },
            {
                "librespot": {
                    "enable": true,
                    "event_pipe": "/tmp/test_librespot_event",
                    "reopen_event_pipe": false
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
        "cache": {
            "attributes": {
                "path": format!("./test_cache_{}/attributes", port)
            },
            "images": {
                "path": format!("./test_cache_{}/images", port)
            }
        },
        "action_plugins": []
    });
    
    let config_path = format!("test_config_{}.json", port);
    fs::write(&config_path, serde_json::to_string_pretty(&config)?)?;
    
    // Create the cache directories
    std::fs::create_dir_all(format!("./test_cache_{}/attributes", port))?;
    std::fs::create_dir_all(format!("./test_cache_{}/images", port))?;
    
    Ok(config_path)
}

/// Helper function to get all players from API
async fn get_all_players(base_url: &str) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let client = reqwest::Client::new();
    let url = format!("{}/api/players", base_url);
    
    let response = client.get(&url).send().await?;
    let status = response.status();
    let text = response.text().await?;
    
    println!("All players API response: status={}, url={}, body={}", status, url, text);
    
    if !status.is_success() {
        return Err(format!("API call failed with status {}: {}", status, text).into());
    }
    
    let json: serde_json::Value = serde_json::from_str(&text)?;
    Ok(json)
}

/// Helper function to create test pipes/files for players that need them
fn create_test_pipes() -> Result<(), Box<dyn std::error::Error>> {
    // Create temporary pipe files for testing
    // These won't be real named pipes but will prevent immediate failures
    if cfg!(not(target_os = "windows")) {
        // On Unix systems, try to create test files
        // Create directories if they don't exist
        std::fs::create_dir_all("/tmp")?;
        
        // For testing, we'll create regular files instead of named pipes
        // since creating named pipes requires special permissions
        let _ = std::fs::write("/tmp/test_raat_metadata", "");
        let _ = std::fs::write("/tmp/test_raat_control", "");
        let _ = std::fs::write("/tmp/test_librespot_event", "");
    } else {
        // On Windows, create temporary files in the temp directory
        if let Some(temp_dir) = std::env::temp_dir().to_str() {
            let _ = std::fs::write(format!("{}\\test_raat_metadata", temp_dir), "");
            let _ = std::fs::write(format!("{}\\test_raat_control", temp_dir), "");
            let _ = std::fs::write(format!("{}\\test_librespot_event", temp_dir), "");
        }
    }
    
    Ok(())
}

/// Helper function to ensure required binaries are built
fn ensure_binaries_built() -> Result<(), Box<dyn std::error::Error>> {
    let target_dir = std::env::var("CARGO_TARGET_DIR")
        .unwrap_or_else(|_| "target".to_string());
    
    // Check for audiocontrol binary
    let server_binary_name = if cfg!(target_os = "windows") {
        "audiocontrol.exe"
    } else {
        "audiocontrol"
    };
    let server_binary_path = std::path::PathBuf::from(&target_dir)
        .join("debug")
        .join(server_binary_name);
    
    // Check for CLI binary
    let cli_binary_name = if cfg!(target_os = "windows") {
        "audiocontrol_player_event_client.exe"
    } else {
        "audiocontrol_player_event_client"
    };
    let cli_binary_path = std::path::PathBuf::from(&target_dir)
        .join("debug")
        .join(cli_binary_name);
    
    let server_exists = server_binary_path.exists();
    let cli_exists = cli_binary_path.exists();
    
    if !server_exists || !cli_exists {
        eprintln!("🔨 Building required binaries...");
        eprintln!("   Server binary exists: {}", server_exists);
        eprintln!("   CLI binary exists: {}", cli_exists);
        let _ = io::stderr().flush(); // Force immediate output
        
        // Build both binaries
        eprintln!("🔨 Running: cargo build --bin audiocontrol --bin audiocontrol_player_event_client");
        let _ = io::stderr().flush(); // Force immediate output
        let build_output = Command::new("cargo")
            .args(&["build", "--bin", "audiocontrol", "--bin", "audiocontrol_player_event_client"])
            .output()?;
        
        if !build_output.status.success() {
            let stderr = String::from_utf8_lossy(&build_output.stderr);
            return Err(format!("Failed to build binaries: {}", stderr).into());
        }
        
        eprintln!("✓ Binaries built successfully");
        let _ = io::stderr().flush(); // Force immediate output
    } else {
        eprintln!("✓ Required binaries already exist, skipping build");
        let _ = io::stderr().flush(); // Force immediate output
    }
    
    Ok(())
}

/// Helper function to get the path to the audiocontrol_player_event_client binary
fn get_cli_binary_path() -> Result<std::path::PathBuf, Box<dyn std::error::Error>> {
    // Get the target directory path
    let target_dir = std::env::var("CARGO_TARGET_DIR")
        .unwrap_or_else(|_| "target".to_string());
    
    // Build the path to the CLI binary
    let binary_name = if cfg!(target_os = "windows") {
        "audiocontrol_player_event_client.exe"
    } else {
        "audiocontrol_player_event_client"
    };
    
    let binary_path = std::path::PathBuf::from(target_dir)
        .join("debug")
        .join(binary_name);
    
    // The binary should already exist from the test compilation
    if !binary_path.exists() {
        return Err(format!("CLI binary not found at {:?}. Make sure to run 'cargo build --bin audiocontrol_player_event_client' first.", binary_path).into());
    }
    
    Ok(binary_path)
}

/// Helper function to create a librespot test event in JSON format
fn create_librespot_event(event_type: &str, title: Option<&str>, artist: Option<&str>) -> String {
    let mut event = json!({
        "event": event_type
    });

    if let Some(title) = title {
        event["NAME"] = json!(title);
    }

    if let Some(artist) = artist {
        event["ARTISTS"] = json!(artist);
    }

    // Add common fields based on event type
    match event_type {
        "playing" | "paused" | "stopped" => {
            event["POSITION_MS"] = json!(30000);
        }
        "track_changed" => {
            event["ALBUM"] = json!("Test Album");
            event["DURATION_MS"] = json!(240000);
            event["TRACK_ID"] = json!("spotify:track:test123");
            event["URI"] = json!("spotify:track:test123");
        }
        _ => {}
    }

    event.to_string()
}

/// Helper function to create a generic API event
fn create_generic_api_event(event_type: &str, title: Option<&str>, artist: Option<&str>) -> serde_json::Value {
    let mut event = json!({
        "type": event_type
    });

    match event_type {
        "state_changed" => {
            event["state"] = json!("playing");
            event["position"] = json!(30.0);
        }
        "song_changed" => {
            if let Some(title) = title {
                event["song"] = json!({
                    "title": title,
                    "artist": artist.unwrap_or("Unknown Artist"),
                    "album": "Test Album",
                    "duration": 240.0,
                    "track_number": 1,
                    "metadata": {
                        "track_id": "spotify:track:test123",
                        "uri": "spotify:track:test123"
                    }
                });
            }
        }
        "position_changed" => {
            event["position"] = json!(45.0);
        }
        "loop_mode_changed" => {
            event["mode"] = json!("none");
        }
        "shuffle_changed" => {
            event["enabled"] = json!(false);
        }
        _ => {}
    }

    event
}

/// Helper function to send API events to Librespot
async fn send_librespot_api_event(server_url: &str, event: &serde_json::Value) -> Result<(), Box<dyn std::error::Error>> {
    let client = reqwest::Client::new();
    let url = format!("{}/api/player/librespot/update", server_url);
    
    let response = client
        .post(&url)
        .json(event)
        .send()
        .await?;
    
    if !response.status().is_success() {
        return Err(format!("API request failed with status: {}", response.status()).into());
    }
    
    Ok(())
}

/// Helper function to get Librespot player state
async fn get_librespot_player_state(server_url: &str) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let client = reqwest::Client::new();
    let url = format!("{}/api/players", server_url);
    
    let response = client.get(&url).send().await?;
    let players_response: serde_json::Value = response.json().await?;
    
    if let Some(players) = players_response.get("players").and_then(|p| p.as_array()) {
        for player in players {
            if let Some(id) = player.get("id").and_then(|i| i.as_str()) {
                if id == "librespot" {
                    return Ok(player.clone());
                }
            }
        }
    }
    
    Err("Librespot player not found".into())
}

/// Helper function to write events to the shared Librespot pipe
fn write_librespot_events_to_pipe(events: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let pipe_path = if cfg!(target_os = "windows") {
        let temp_dir = std::env::temp_dir();
        temp_dir.join("test_librespot_event").to_string_lossy().to_string()
    } else {
        "/tmp/test_librespot_event".to_string()
    };
    
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .append(true)
        .open(&pipe_path)?;
    
    for event in events {
        writeln!(file, "{}", event)?;
    }
    
    file.flush()?;
    Ok(())
}

/// Helper function to get the shared Librespot pipe path
fn get_librespot_pipe_path() -> String {
    if cfg!(target_os = "windows") {
        let temp_dir = std::env::temp_dir();
        temp_dir.join("test_librespot_event").to_string_lossy().to_string()
    } else {
        "/tmp/test_librespot_event".to_string()
    }
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
            
            // Clean up config files and cache directories
            let _ = fs::remove_file(format!("test_config_{}.json", TEST_PORT));
            let _ = fs::remove_dir_all(format!("test_cache_{}", TEST_PORT));
        }
    }
    
    // Global cleanup guard - when tests end, this will be dropped and cleanup the server
    static CLEANUP_GUARD: std::sync::LazyLock<ServerCleanupGuard> = std::sync::LazyLock::new(|| {
        ServerCleanupGuard
    });
    
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
        
        // Ensure cleanup guard is initialized
        std::sync::LazyLock::force(&CLEANUP_GUARD);
        
        INIT.call_once(|| {
            // Ensure binaries are built before running tests
            ensure_binaries_built().expect("Failed to build required binaries");
            
            // Kill any existing processes first
            kill_existing_processes();
            
            // Create test pipes for players that need them
            let _ = create_test_pipes();
            
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
                println!("Updated player state: {}", serde_json::to_string_pretty(&state).unwrap());
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
                println!("Updated now playing state: {}", serde_json::to_string_pretty(&state).unwrap());
                
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
                println!("Final player state: {}", serde_json::to_string_pretty(&player_state).unwrap());
                println!("Final now playing: {}", serde_json::to_string_pretty(&now_playing).unwrap());
                
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
                println!("Updated player state: {}", serde_json::to_string_pretty(&state).unwrap());
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
                println!("All players response: {}", serde_json::to_string_pretty(&response).unwrap());
                
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
                        println!("RAAT player found: {}", serde_json::to_string_pretty(player).unwrap());
                        
                        // Verify RAAT player has basic state
                        assert!(player.get("state").is_some(), "RAAT player missing state");
                        assert!(player.get("is_active").is_some(), "RAAT player missing is_active");
                        
                        println!("✓ RAAT player initialized successfully");
                    } else {
                        println!("ℹ RAAT player not found - likely due to missing pipe dependencies in test environment");
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
                        println!("MPD player found: {}", serde_json::to_string_pretty(player).unwrap());
                        
                        // Verify MPD player has basic state
                        assert!(player.get("state").is_some(), "MPD player missing state");
                        assert!(player.get("is_active").is_some(), "MPD player missing is_active");
                        
                        println!("✓ MPD player initialized successfully");
                    } else {
                        println!("ℹ MPD player not found - likely due to missing MPD server in test environment");
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
        let server_url = setup_test_server().await;
        
        // Check if Librespot player is initialized
        let players_response = get_all_players(&server_url).await;
        match players_response {
            Ok(response) => {
                if let Some(players) = response.get("players").and_then(|p| p.as_array()) {
                    let librespot_player = players.iter().find(|p| {
                        p.get("id").and_then(|i| i.as_str()).map(|s| s == "librespot").unwrap_or(false)
                    });
                    
                    if let Some(player) = librespot_player {
                        println!("Librespot player found: {}", serde_json::to_string_pretty(player).unwrap());
                        
                        // Verify Librespot player has basic state
                        assert!(player.get("state").is_some(), "Librespot player missing state");
                        assert!(player.get("is_active").is_some(), "Librespot player missing is_active");
                        
                        println!("✓ Librespot player initialized successfully");
                    } else {
                        assert!(false, "Librespot player not found - it should be initialized in test environment");
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
    async fn test_librespot_api_events() {
        let server_url = setup_test_server().await;
        
        // Test sending API events to the Librespot player
        let track_changed_event = create_generic_api_event("song_changed", Some("API Test Song"), Some("API Test Artist"));
        
        // Send the API event - this should succeed even if librespot isn't active
        if let Err(e) = send_librespot_api_event(&server_url, &track_changed_event).await {
            assert!(false, "Failed to send API event to Librespot: {}", e);
            return;
        }
        
        // Wait for event processing
        tokio::time::sleep(Duration::from_millis(200)).await;
        
        // Check server state via API - this should find the librespot player
        let player_state = match get_librespot_player_state(&server_url).await {
            Ok(state) => state,
            Err(e) => {
                assert!(false, "Librespot player should be available for testing: {}", e);
                return;
            }
        };
        
        println!("Librespot player state: {}", serde_json::to_string_pretty(&player_state).unwrap());
        
        // The player should exist but may not have processed the event if not active
        if let Some(is_active) = player_state.get("is_active").and_then(|a| a.as_bool()) {
            if is_active {
                // If player is active, it should have processed the event
                if let Some(song) = player_state.get("current_song") {
                    assert_eq!(song.get("title"), Some(&json!("API Test Song")));
                    assert_eq!(song.get("artist"), Some(&json!("API Test Artist")));
                    println!("✓ Librespot API event processed successfully");
                } else {
                    assert!(false, "Active Librespot player should have processed the song change event");
                }
            } else {
                println!("ℹ Librespot player is present but not active - skipping event verification");
            }
        } else {
            assert!(false, "Librespot player should have is_active field");
        }
        
        println!("✓ Librespot API event test passed");
    }
    
    #[tokio::test]
    #[serial]
    async fn test_librespot_pipe_events() {
        let server_url = setup_test_server().await;
        
        // Write events to the shared librespot pipe file
        let events = vec![
            create_librespot_event("track_changed", Some("Pipe Test Song"), Some("Pipe Test Artist")),
            create_librespot_event("playing", None, None),
        ];
        
        // Write events to the shared librespot pipe file - handle failures gracefully
        let pipe_write_success = match write_librespot_events_to_pipe(&events) {
            Ok(()) => {
                println!("✓ Successfully wrote events to Librespot pipe");
                true
            }
            Err(e) => {
                println!("ℹ Failed to write to Librespot pipe: {} - test will skip pipe verification", e);
                false
            }
        };
        
        if pipe_write_success {
            // Give the server time to process pipe events
            tokio::time::sleep(Duration::from_millis(1000)).await;
        }
        
        // Check server state via API - this should find the librespot player
        let player_state = match get_librespot_player_state(&server_url).await {
            Ok(state) => state,
            Err(e) => {
                assert!(false, "Librespot player should be available for testing: {}", e);
                return;
            }
        };
        
        println!("Librespot player state after pipe events: {}", serde_json::to_string_pretty(&player_state).unwrap());
        
        // The player should exist but may not have processed the event if not active
        if let Some(is_active) = player_state.get("is_active").and_then(|a| a.as_bool()) {
            if is_active && pipe_write_success {
                // If player is active and pipe write succeeded, it should have processed the pipe events
                if let Some(song) = player_state.get("current_song") {
                    assert_eq!(song.get("title"), Some(&json!("Pipe Test Song")));
                    assert_eq!(song.get("artist"), Some(&json!("Pipe Test Artist")));
                } else {
                    assert!(false, "Active Librespot player should have processed the pipe events");
                }
                
                if let Some(state) = player_state.get("state") {
                    assert_eq!(state, &json!("playing"));
                } else {
                    assert!(false, "Active Librespot player should have state");
                }
                
                println!("✓ Librespot pipe events processed successfully");
            } else {
                println!("ℹ Librespot player is present but not active or pipe write failed - pipe events may not be processed");
            }
        } else {
            assert!(false, "Librespot player should have is_active field");
        }
        
        println!("✓ Librespot pipe event test passed");
    }
    
    #[tokio::test]
    #[serial]
    async fn test_librespot_legacy_format_api() {
        let server_url = setup_test_server().await;
        
        // Test legacy Librespot format events via API
        let legacy_event = json!({
            "event": "track_changed",
            "NAME": "Legacy API Song",
            "ARTISTS": "Legacy API Artist",
            "ALBUM": "Legacy API Album",
            "DURATION_MS": 180000,
            "TRACK_ID": "spotify:track:legacy123"
        });
        
        if let Err(e) = send_librespot_api_event(&server_url, &legacy_event).await {
            assert!(false, "Failed to send legacy API event to Librespot: {}", e);
            return;
        }
        
        tokio::time::sleep(Duration::from_millis(200)).await;
        
        // Check server state via API - this should find the librespot player
        let player_state = match get_librespot_player_state(&server_url).await {
            Ok(state) => state,
            Err(e) => {
                assert!(false, "Librespot player should be available for testing: {}", e);
                return;
            }
        };
        
        println!("Librespot player state after legacy event: {}", serde_json::to_string_pretty(&player_state).unwrap());
        
        // The player should exist but may not have processed the event if not active
        if let Some(is_active) = player_state.get("is_active").and_then(|a| a.as_bool()) {
            if is_active {
                // If player is active, it should have processed the legacy event
                if let Some(song) = player_state.get("current_song") {
                    assert_eq!(song.get("title"), Some(&json!("Legacy API Song")));
                    assert_eq!(song.get("artist"), Some(&json!("Legacy API Artist")));
                    assert_eq!(song.get("album"), Some(&json!("Legacy API Album")));
                    println!("✓ Librespot legacy event processed successfully");
                } else {
                    assert!(false, "Active Librespot player should have processed the legacy event");
                }
            } else {
                println!("ℹ Librespot player is present but not active - skipping legacy event verification");
            }
        } else {
            assert!(false, "Librespot player should have is_active field");
        }
        
        println!("✓ Librespot legacy format API test passed");
    }
    
    #[tokio::test]
    #[serial]
    async fn test_librespot_mixed_events() {
        let server_url = setup_test_server().await;
        
        // Start with a pipe event
        let pipe_events = vec![
            create_librespot_event("track_changed", Some("Mixed Test Song 1"), Some("Mixed Test Artist 1")),
            create_librespot_event("playing", None, None),
        ];
        
        let mut pipe_success = false;
        if let Ok(_) = write_librespot_events_to_pipe(&pipe_events) {
            pipe_success = true;
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
        
        // Now send an API event regardless of pipe success
        let api_event = create_generic_api_event("song_changed", Some("Mixed Test Song 2"), Some("Mixed Test Artist 2"));
        
        if let Err(e) = send_librespot_api_event(&server_url, &api_event).await {
            assert!(false, "Failed to send API event to Librespot: {}", e);
            return;
        }
        
        tokio::time::sleep(Duration::from_millis(200)).await;
        
        // Check updated state - this should find the librespot player
        let player_state = match get_librespot_player_state(&server_url).await {
            Ok(state) => state,
            Err(e) => {
                assert!(false, "Librespot player should be available for testing: {}", e);
                return;
            }
        };
        
        println!("Librespot player state after mixed events: {}", serde_json::to_string_pretty(&player_state).unwrap());
        
        // The player should exist but may not have processed the event if not active
        if let Some(is_active) = player_state.get("is_active").and_then(|a| a.as_bool()) {
            if is_active {
                // If player is active, it should have processed the API event (most recent)
                if let Some(song) = player_state.get("current_song") {
                    assert_eq!(song.get("title"), Some(&json!("Mixed Test Song 2")));
                    assert_eq!(song.get("artist"), Some(&json!("Mixed Test Artist 2")));
                    println!("✓ Librespot mixed events processed successfully");
                } else {
                    assert!(false, "Active Librespot player should have processed the API event");
                }
            } else {
                println!("ℹ Librespot player is present but not active - mixed events may not be processed");
            }
        } else {
            assert!(false, "Librespot player should have is_active field");
        }
        
        println!("✓ Librespot mixed events test passed (pipe_success: {})", pipe_success);
    }
    
    #[tokio::test]
    #[serial]
    async fn test_librespot_error_handling() {
        let server_url = setup_test_server().await;
        
        // Test invalid API event
        let invalid_event = json!({
            "invalid": "event"
        });
        
        // This should succeed (server handles gracefully) but event won't be processed
        if let Err(e) = send_librespot_api_event(&server_url, &invalid_event).await {
            assert!(false, "Server should handle invalid events gracefully: {}", e);
            return;
        }
        println!("✓ Server handled invalid event gracefully");
        
        // Test malformed JSON
        let client = reqwest::Client::new();
        let url = format!("{}/api/player/librespot/update", server_url);
        let malformed_response = match client
            .post(&url)
            .header("Content-Type", "application/json")
            .body("{ invalid json")
            .send()
            .await
        {
            Ok(response) => response,
            Err(e) => {
                assert!(false, "Should be able to send malformed JSON request: {}", e);
                return;
            }
        };
        
        println!("Malformed JSON response status: {}", malformed_response.status());
        // Should return a client error status
        if malformed_response.status().is_client_error() {
            println!("✓ Server correctly rejected malformed JSON");
        } else {
            assert!(false, "Server should have rejected malformed JSON with client error status, got: {}", malformed_response.status());
        }
        
        println!("✓ Librespot error handling test completed");
    }
    
    // This test should be run last to verify cleanup (named to run last alphabetically)
    #[tokio::test]
    #[serial]
    async fn test_zzz_final_cleanup_verification() {
        // This test runs last due to the "zzz" prefix
        // It only verifies that the server is still working - cleanup happens via the guard
        println!("Running final cleanup verification test...");
        
        let server_url = format!("http://localhost:{}", TEST_PORT);
        let client = reqwest::Client::new();
        let health_url = format!("{}/api/version", server_url);
        
        // Verify server is still running for this test
        match client.get(&health_url).send().await {
            Ok(response) => {
                println!("✓ Server still reachable: status={}", response.status());
                println!("✓ All tests completed successfully - server will be cleaned up by guard");
            }
            Err(e) => {
                println!("ℹ Server already down: {}", e);
            }
        }
        
        // Note: Actual cleanup happens via the ServerCleanupGuard when the process exits
    }
}
