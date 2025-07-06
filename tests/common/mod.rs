#![allow(dead_code)]
// Common helpers for integration tests

use serde_json::json;
use std::fs;
use std::io::{self, Write};
use std::process::Command;
use std::time::Duration;

// Helper function to kill any existing audiocontrol processes (cross-platform)
pub fn kill_existing_processes() {
    println!("[CLEANUP] Killing existing audiocontrol processes...");
    
    if cfg!(target_os = "windows") {
        // Use taskkill with /F for force and /T for killing child processes
        let _ = Command::new("taskkill")
            .args(&["/F", "/T", "/IM", "audiocontrol.exe"])
            .output();
        
        // Also try PowerShell approach for more thorough cleanup
        let _ = Command::new("powershell")
            .args(&["-Command", "Get-Process -Name 'audiocontrol' -ErrorAction SilentlyContinue | Stop-Process -Force"])
            .output();
            
        // Also try using wmic as a fallback
        let _ = Command::new("wmic")
            .args(&["process", "where", "name='audiocontrol.exe'", "delete"])
            .output();
    } else {
        // For Unix-like systems, use pkill
        let _ = Command::new("pkill")
            .args(&["-KILL", "-f", "audiocontrol"])
            .output();
        
        // Also try killall as a fallback
        let _ = Command::new("killall")
            .args(&["-KILL", "audiocontrol"])
            .output();
    }
    
    // Wait longer for processes to die
    std::thread::sleep(Duration::from_millis(1000));
    
    // Verify cleanup worked
    if cfg!(target_os = "windows") {
        let output = Command::new("tasklist")
            .args(&["/FI", "IMAGENAME eq audiocontrol.exe"])
            .output();
        
        if let Ok(output) = output {
            let stdout = String::from_utf8_lossy(&output.stdout);
            if stdout.contains("audiocontrol.exe") {
                println!("[CLEANUP] Warning: audiocontrol.exe processes may still be running");
                println!("[CLEANUP] Trying one more aggressive cleanup...");
                
                // Try one more time with different approach
                let _ = Command::new("cmd")
                    .args(&["/C", "taskkill /F /T /IM audiocontrol.exe"])
                    .output();
                    
                std::thread::sleep(Duration::from_millis(500));
            } else {
                println!("[CLEANUP] Successfully killed all audiocontrol processes");
            }
        }
    }
}

// Helper function to wait for the server to be ready
pub async fn wait_for_server(base_url: &str, timeout_seconds: u64) -> Result<(), Box<dyn std::error::Error>> {
    let client = reqwest::Client::new();
    let timeout = Duration::from_secs(timeout_seconds);
    let start = std::time::Instant::now();
    let health_url = format!("{}/api/version", base_url);
    while start.elapsed() < timeout {
        match client.get(&health_url).send().await {
            Ok(response) => {
                if response.status().is_success() {
                    return Ok(());
                }
            }
            Err(_) => {
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
        }
    }
    Err("Server did not start within timeout".into())
}

// Helper function to get now playing info
pub async fn get_now_playing(base_url: &str) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let client = reqwest::Client::new();
    let url = format!("{}/api/now-playing", base_url);
    let response = client.get(&url).send().await?;
    let status = response.status();
    let text = response.text().await?;
    if !status.is_success() {
        return Err(format!("API call failed with status {}: {}", status, text).into());
    }
    let json: serde_json::Value = serde_json::from_str(&text)?;
    Ok(json)
}

// Helper function to get player state
pub async fn get_player_state(base_url: &str, player_name: &str) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let client = reqwest::Client::new();
    let url = format!("{}/api/players", base_url);
    let response = client.get(&url).send().await?;
    let status = response.status();
    let text = response.text().await?;
    if !status.is_success() {
        return Err(format!("API call failed with status {}: {}", status, text).into());
    }
    let json: serde_json::Value = serde_json::from_str(&text)?;
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

// Helper function to get all players from API
pub async fn get_all_players(base_url: &str) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let client = reqwest::Client::new();
    let url = format!("{}/api/players", base_url);
    let response = client.get(&url).send().await?;
    let status = response.status();
    let text = response.text().await?;
    if !status.is_success() {
        return Err(format!("API call failed with status {}: {}", status, text).into());
    }
    let json: serde_json::Value = serde_json::from_str(&text)?;
    Ok(json)
}

// Helper function to create a test config file
pub fn create_test_config(port: u16) -> Result<String, Box<dyn std::error::Error>> {
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
        "action_plugins": [
            {
                "active-monitor": {
                    "enabled": true
                }
            }
        ]
    });
    let config_path = format!("test_config_{}.json", port);
    fs::write(&config_path, serde_json::to_string_pretty(&config)?)?;
    std::fs::create_dir_all(format!("./test_cache_{}/attributes", port))?;
    std::fs::create_dir_all(format!("./test_cache_{}/images", port))?;
    Ok(config_path)
}

pub fn create_test_pipes() -> Result<(), Box<dyn std::error::Error>> {
    if cfg!(not(target_os = "windows")) {
        std::fs::create_dir_all("/tmp")?;
        let _ = std::fs::write("/tmp/test_raat_metadata", "");
        let _ = std::fs::write("/tmp/test_raat_control", "");
        let _ = std::fs::write("/tmp/test_librespot_event", "");
    } else {
        if let Some(temp_dir) = std::env::temp_dir().to_str() {
            let _ = std::fs::write(format!("{}\\test_raat_metadata", temp_dir), "");
            let _ = std::fs::write(format!("{}\\test_raat_control", temp_dir), "");
            let _ = std::fs::write(format!("{}\\test_librespot_event", temp_dir), "");
        }
    }
    Ok(())
}

pub fn ensure_binaries_built() -> Result<(), Box<dyn std::error::Error>> {
    let target_dir = std::env::var("CARGO_TARGET_DIR").unwrap_or_else(|_| "target".to_string());
    let server_binary_name = if cfg!(target_os = "windows") { "audiocontrol.exe" } else { "audiocontrol" };
    let server_binary_path = std::path::PathBuf::from(&target_dir).join("debug").join(server_binary_name);
    let cli_binary_name = if cfg!(target_os = "windows") { "audiocontrol_player_event_client.exe" } else { "audiocontrol_player_event_client" };
    let cli_binary_path = std::path::PathBuf::from(&target_dir).join("debug").join(cli_binary_name);
    let server_exists = server_binary_path.exists();
    let cli_exists = cli_binary_path.exists();
    if !server_exists || !cli_exists {
        eprintln!("[BUILD] Building required binaries...");
        eprintln!("   Server binary exists: {}", server_exists);
        eprintln!("   CLI binary exists: {}", cli_exists);
        let _ = io::stderr().flush();
        eprintln!("[BUILD] Running: cargo build --bin audiocontrol --bin audiocontrol_player_event_client");
        let _ = io::stderr().flush();
        let build_output = Command::new("cargo")
            .args(&["build", "--bin", "audiocontrol", "--bin", "audiocontrol_player_event_client"])
            .output()?;
        if !build_output.status.success() {
            let stderr = String::from_utf8_lossy(&build_output.stderr);
            return Err(format!("Failed to build binaries: {}", stderr).into());
        }
        eprintln!("[OK] Binaries built successfully");
        let _ = io::stderr().flush();
    } else {
        eprintln!("[OK] Required binaries already exist, skipping build");
        let _ = io::stderr().flush();
    }
    Ok(())
}

pub fn get_cli_binary_path() -> Result<std::path::PathBuf, Box<dyn std::error::Error>> {
    let target_dir = std::env::var("CARGO_TARGET_DIR").unwrap_or_else(|_| "target".to_string());
    let binary_name = if cfg!(target_os = "windows") { "audiocontrol_player_event_client.exe" } else { "audiocontrol_player_event_client" };
    let binary_path = std::path::PathBuf::from(target_dir).join("debug").join(binary_name);
    if !binary_path.exists() {
        return Err(format!("CLI binary not found at {:?}. Make sure to run 'cargo build --bin audiocontrol_player_event_client' first.", binary_path).into());
    }
    Ok(binary_path)
}

pub fn create_librespot_event(event_type: &str, title: Option<&str>, artist: Option<&str>) -> String {
    let mut event = json!({ "event": event_type });
    if let Some(title) = title { event["NAME"] = json!(title); }
    if let Some(artist) = artist { event["ARTISTS"] = json!(artist); }
    match event_type {
        "playing" | "paused" | "stopped" => { event["POSITION_MS"] = json!(30000); },
        "track_changed" => {
            event["ALBUM"] = json!("Test Album");
            event["DURATION_MS"] = json!(240000);
            event["TRACK_ID"] = json!("spotify:track:test123");
            event["URI"] = json!("spotify:track:test123");
        },
        _ => {}
    }
    event.to_string()
}

pub fn create_generic_api_event(event_type: &str, title: Option<&str>, artist: Option<&str>) -> serde_json::Value {
    let mut event = json!({ "type": event_type });
    match event_type {
        "state_changed" => {
            event["state"] = json!("playing");
            event["position"] = json!(30.0);
        },
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
        },
        "position_changed" => { event["position"] = json!(45.0); },
        "loop_mode_changed" => { event["mode"] = json!("none"); },
        "shuffle_changed" => { event["enabled"] = json!(false); },
        _ => {}
    }
    event
}

pub async fn send_librespot_api_event(server_url: &str, event: &serde_json::Value) -> Result<(), Box<dyn std::error::Error>> {
    let client = reqwest::Client::new();
    let url = format!("{}/api/player/librespot/update", server_url);
    let response = client.post(&url).json(event).send().await?;
    if !response.status().is_success() {
        return Err(format!("API request failed with status: {}", response.status()).into());
    }
    Ok(())
}

pub async fn get_librespot_player_state(server_url: &str) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
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

pub fn write_librespot_events_to_pipe(events: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let pipe_path = if cfg!(target_os = "windows") {
        let temp_dir = std::env::temp_dir();
        temp_dir.join("test_librespot_event").to_string_lossy().to_string()
    } else {
        "/tmp/test_librespot_event".to_string()
    };
    let mut file = std::fs::OpenOptions::new().create(true).write(true).append(true).open(&pipe_path)?;
    for event in events {
        writeln!(file, "{}", event)?;
    }
    file.flush()?;
    Ok(())
}

pub fn get_librespot_pipe_path() -> String {
    if cfg!(target_os = "windows") {
        let temp_dir = std::env::temp_dir();
        temp_dir.join("test_librespot_event").to_string_lossy().to_string()
    } else {
        "/tmp/test_librespot_event".to_string()
    }
}

pub fn wait_for_librespot_pipe(timeout_ms: u64) -> bool {
    use std::time::{Duration, Instant};
    use std::thread::sleep;
    use std::fs;
    let pipe_path = get_librespot_pipe_path();
    let start = Instant::now();
    let poll_interval = Duration::from_millis(50);
    while start.elapsed().as_millis() < timeout_ms as u128 {
        if fs::metadata(&pipe_path).is_ok() {
            return true;
        }
        sleep(poll_interval);
    }
    false
}

// Helper to setup the test server (moved from full_integration_tests.rs)
pub async fn setup_test_server(
    test_port: u16,
    server_process: *mut Option<std::process::Child>,
    server_ready: &std::sync::atomic::AtomicBool,
    init: &std::sync::Once,
) -> String {
    let server_url = format!("http://localhost:{}", test_port);
    init.call_once(|| {
        ensure_binaries_built().expect("Failed to build required binaries");
        kill_existing_processes();
        let _ = create_test_pipes();
        let ok = wait_for_librespot_pipe(5000);
        assert!(ok, "Librespot event pipe was not created in time");
        let config_path = create_test_config(test_port).expect("Failed to create test config");
        let target_dir = std::env::var("CARGO_TARGET_DIR").unwrap_or_else(|_| "target".to_string());
        let binary_name = if cfg!(target_os = "windows") { "audiocontrol.exe" } else { "audiocontrol" };
        let binary_path = std::path::PathBuf::from(target_dir).join("debug").join(binary_name);
        let process = std::process::Command::new(&binary_path)
            .args(&["-c", &config_path])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .expect("Failed to start AudioControl server");
        unsafe { *server_process = Some(process) };
    });
    if !server_ready.load(std::sync::atomic::Ordering::Relaxed) {
        let server_ready_result = wait_for_server(&server_url, 30).await;
        if server_ready_result.is_err() {
            eprintln!("Server failed to start: {:?}", server_ready_result.err());
            return server_url;
        }
        server_ready.store(true, std::sync::atomic::Ordering::Relaxed);
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
    server_url
}

// Helper to reset player state (moved from full_integration_tests.rs)
pub async fn reset_player_state(server_url: &str) {
    let cli_binary = get_cli_binary_path().expect("Failed to get CLI binary path");
    let reset_commands = vec![
        vec!["--host", server_url, "test_player", "state-changed", "stopped"],
        vec!["--host", server_url, "test_player", "shuffle-changed"],
        vec!["--host", server_url, "test_player", "loop-mode-changed", "none"],
        vec!["--host", server_url, "test_player", "position-changed", "0.0"],
    ];
    for command_args in reset_commands {
        let _ = std::process::Command::new(&cli_binary)
            .args(&command_args)
            .output();
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
}

/// Comprehensive cleanup function for test suites
/// This function should be called after each test suite to ensure proper cleanup
pub unsafe fn cleanup_test_server(
    test_port: u16,
    server_process: *mut Option<std::process::Child>,
    server_ready: &std::sync::atomic::AtomicBool,
) {
    println!("[CLEANUP] Starting comprehensive test server cleanup for port {}...", test_port);
    
    // 1. Kill the server process directly if we have a handle to it
    if let Some(mut process) = (*server_process).take() {
        println!("[CLEANUP] Killing server process directly (PID: {})...", process.id());
        let _ = process.kill();
        let _ = process.wait();
        println!("[CLEANUP] Server process killed directly");
    }
    
    // 2. Kill any existing processes by name (more aggressive)
    println!("[CLEANUP] Killing any remaining audiocontrol processes...");
    kill_existing_processes();
    
    // 3. Clean up test config files and cache directories
    println!("[CLEANUP] Cleaning up test artifacts...");
    let _ = std::fs::remove_file(format!("test_config_{}.json", test_port));
    let _ = std::fs::remove_dir_all(format!("test_cache_{}", test_port));
    
    // 4. Clean up test pipes
    if cfg!(not(target_os = "windows")) {
        let _ = std::fs::remove_file("/tmp/test_raat_metadata");
        let _ = std::fs::remove_file("/tmp/test_raat_control");
        let _ = std::fs::remove_file("/tmp/test_librespot_event");
    } else {
        if let Some(temp_dir) = std::env::temp_dir().to_str() {
            let _ = std::fs::remove_file(format!("{}\\test_raat_metadata", temp_dir));
            let _ = std::fs::remove_file(format!("{}\\test_raat_control", temp_dir));
            let _ = std::fs::remove_file(format!("{}\\test_librespot_event", temp_dir));
        }
    }
    
    // 5. Reset the server ready flag
    server_ready.store(false, std::sync::atomic::Ordering::Relaxed);
    
    // 6. Wait longer to ensure everything is cleaned up
    std::thread::sleep(Duration::from_millis(2000));
    
    // 7. Final verification
    if cfg!(target_os = "windows") {
        let output = Command::new("tasklist")
            .args(&["/FI", "IMAGENAME eq audiocontrol.exe"])
            .output();
        
        if let Ok(output) = output {
            let stdout = String::from_utf8_lossy(&output.stdout);
            if stdout.contains("audiocontrol.exe") {
                println!("[CLEANUP] WARNING: audiocontrol.exe processes are still running after cleanup!");
            } else {
                println!("[CLEANUP] Verified: No audiocontrol.exe processes running");
            }
        }
    }
    
    println!("[CLEANUP] Comprehensive cleanup complete for port {}", test_port);
}

/// Force cleanup function that should be called manually at the end of test modules
/// This is a more reliable approach than relying on Drop traits
pub unsafe fn force_cleanup_test_server(
    test_port: u16,
    server_process: *mut Option<std::process::Child>,
    server_ready: &std::sync::atomic::AtomicBool,
) {
    println!("[FORCE CLEANUP] Starting forced cleanup for port {}...", test_port);
    cleanup_test_server(test_port, server_process, server_ready);
}

/// Simple function to register a cleanup callback that gets called at the end
pub fn register_cleanup_callback(cleanup_fn: Box<dyn Fn() + Send + 'static>) {
    use std::sync::Mutex;
    use std::sync::Arc;
    
    static CLEANUP_CALLBACKS: std::sync::OnceLock<Arc<Mutex<Vec<Box<dyn Fn() + Send + 'static>>>>> = std::sync::OnceLock::new();
    
    let callbacks = CLEANUP_CALLBACKS.get_or_init(|| Arc::new(Mutex::new(Vec::new())));
    
    if let Ok(mut callbacks) = callbacks.lock() {
        callbacks.push(cleanup_fn);
        
        // Register an exit hook if this is the first callback
        if callbacks.len() == 1 {
            std::panic::set_hook(Box::new(|_| {
                println!("[CLEANUP] Panic hook triggered, running cleanup callbacks...");
                run_cleanup_callbacks();
            }));
        }
    }
}

/// Run all registered cleanup callbacks
pub fn run_cleanup_callbacks() {
    use std::sync::Mutex;
    use std::sync::Arc;
    
    static CLEANUP_CALLBACKS: std::sync::OnceLock<Arc<Mutex<Vec<Box<dyn Fn() + Send + 'static>>>>> = std::sync::OnceLock::new();
    
    if let Some(callbacks) = CLEANUP_CALLBACKS.get() {
        if let Ok(callbacks) = callbacks.lock() {
            println!("[CLEANUP] Running {} cleanup callbacks...", callbacks.len());
            for callback in callbacks.iter() {
                callback();
            }
        }
    }
}

/// A safer test wrapper that ensures cleanup runs
pub async fn run_test_with_cleanup<F, Fut>(
    test_port: u16,
    server_process: *mut Option<std::process::Child>,
    server_ready: &std::sync::atomic::AtomicBool,
    test_fn: F,
) where
    F: FnOnce() -> Fut + std::panic::UnwindSafe,
    Fut: std::future::Future<Output = ()>,
{
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        tokio::runtime::Runtime::new().unwrap().block_on(test_fn())
    }));
    
    // Always run cleanup regardless of test result
    unsafe {
        force_cleanup_test_server(test_port, server_process, server_ready);
    }
    
    // Re-throw panic if the test failed
    if let Err(panic) = result {
        std::panic::resume_unwind(panic);
    }
}

/// Test cleanup guard that uses RAII to ensure cleanup runs
pub struct TestCleanupGuard {
    test_port: u16,
    server_process: *mut Option<std::process::Child>,
    server_ready: *const std::sync::atomic::AtomicBool,
}

impl TestCleanupGuard {
    pub unsafe fn new(
        test_port: u16,
        server_process: *mut Option<std::process::Child>,
        server_ready: *const std::sync::atomic::AtomicBool,
    ) -> Self {
        Self {
            test_port,
            server_process,
            server_ready,
        }
    }
}

impl Drop for TestCleanupGuard {
    fn drop(&mut self) {
        println!("[CLEANUP] TestCleanupGuard dropped, running cleanup...");
        unsafe {
            force_cleanup_test_server(self.test_port, self.server_process, &*self.server_ready);
        }
    }
}

// Add any additional helpers from the old test files here as needed
