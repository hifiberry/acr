// NOTE: This file has been integrated into full_integration_tests.rs
// The Librespot integration tests are now part of the main test suite
// and use the shared configuration and server instance.
//
// All tests from this file have been moved to the main test suite with
// appropriate error handling for environments where Librespot is not available.

// This file is kept for reference but is no longer used
#[allow(dead_code)]
fn deprecated_notice() {
    println!("This file has been deprecated. Use full_integration_tests.rs instead.");
}

// Integration tests for Librespot/Spotify player

#[path = "common/mod.rs"]
mod common;
use common::*;
use std::process::Command;
use std::time::Duration;
use serde_json::json;
use serial_test::serial;
use std::sync::Once;
use std::sync::atomic::{AtomicBool, Ordering};

static INIT: Once = Once::new();
static mut SERVER_PROCESS: Option<std::process::Child> = None;
static SERVER_READY: AtomicBool = AtomicBool::new(false);
const TEST_PORT: u16 = 3002;

#[tokio::test]
#[serial]
async fn test_librespot_player_initialization() {
    let server_url = unsafe { common::setup_test_server(TEST_PORT, &mut SERVER_PROCESS, &SERVER_READY, &INIT).await };
    // Check if Librespot player is initialized
    let players_response = get_all_players(&server_url).await;
    match players_response {
        Ok(response) => {
            if let Some(players) = response.get("players").and_then(|p| p.as_array()) {
                let librespot_player = players.iter().find(|p| {
                    p.get("id").and_then(|i| i.as_str()).map(|s| s == "librespot").unwrap_or(false)
                });
                if let Some(player) = librespot_player {
                    println!("Librespot player found: {}", serde_json::to_string_pretty::<serde_json::Value>(player).unwrap());
                    assert!(player.get("state").is_some(), "Librespot player missing state");
                    assert!(player.get("is_active").is_some(), "Librespot player missing is_active");
                    println!("[OK] Librespot player initialized successfully");
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
    let server_url = unsafe { common::setup_test_server(TEST_PORT, &mut SERVER_PROCESS, &SERVER_READY, &INIT).await };
    let track_changed_event = create_generic_api_event("song_changed", Some("API Test Song"), Some("API Test Artist"));
    if let Err(e) = send_librespot_api_event(&server_url, &track_changed_event).await {
        assert!(false, "Failed to send API event to Librespot: {}", e);
        return;
    }
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    let player_state = match get_librespot_player_state(&server_url).await {
        Ok(state) => state,
        Err(e) => {
            assert!(false, "Librespot player should be available for testing: {}", e);
            return;
        }
    };
    println!("Librespot player state: {}", serde_json::to_string_pretty::<serde_json::Value>(&player_state).unwrap());
    if let Some(is_active) = player_state.get("is_active").and_then(|a| a.as_bool()) {
        if is_active {
            if let Some(song) = player_state.get("current_song") {
                if song.get("title") != Some(&json!("API Test Song")) {
                    eprintln!("[FAIL] Expected song title 'API Test Song', got {:?}", song.get("title"));
                    assert!(false, "Active Librespot player should have processed the song title");
                    return;
                }
                if song.get("artist") != Some(&json!("API Test Artist")) {
                    eprintln!("[FAIL] Expected artist 'API Test Artist', got {:?}", song.get("artist"));
                    assert!(false, "Active Librespot player should have processed the artist");
                    return;
                }
                println!("[OK] Librespot API event processed successfully");
            } else {
                eprintln!("[FAIL] Active Librespot player has no current_song after sending event");
                assert!(false, "Active Librespot player should have processed the song change event");
                return;
            }
        } else {
            println!("[INFO] Librespot player is not active - this is expected since we only sent a song change event");
            println!("  Players only become active when they receive a state change to 'playing'");
            println!("[OK] Librespot player correctly remained inactive for non-playing event");
        }
    } else {
        eprintln!("[FAIL] Librespot player missing is_active field");
        assert!(false, "Librespot player should have is_active field");
        return;
    }
    println!("[OK] Librespot API event test passed");
}

#[tokio::test]
#[serial]
async fn test_librespot_api_event_activates_player() {
    let server_url = unsafe { common::setup_test_server(TEST_PORT, &mut SERVER_PROCESS, &SERVER_READY, &INIT).await };
    let playing_event = create_generic_api_event("state_changed", None, None);
    if let Err(e) = send_librespot_api_event(&server_url, &playing_event).await {
        assert!(false, "Failed to send API event to Librespot: {}", e);
        return;
    }
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    let all_players = match get_all_players(&server_url).await {
        Ok(json) => json,
        Err(e) => {
            assert!(false, "Failed to get all players: {}", e);
            return;
        }
    };
    let players = all_players.get("players").and_then(|p| p.as_array()).expect("API should return players array");
    let active_player = players.iter().find(|p| p.get("is_active") == Some(&serde_json::Value::Bool(true)));
    match active_player {
        Some(player) => {
            let id = player.get("id").and_then(|v| v.as_str()).unwrap_or("<none>");
            if id != "librespot" {
                eprintln!("[FAIL] Active player is not librespot after 'playing' event. Actual active player: {}", serde_json::to_string_pretty::<serde_json::Value>(player).unwrap());
                assert!(false, "Active player should be librespot after 'playing' event");
            } else {
                println!("[OK] Librespot player became active after API 'playing' event");
            }
        }
        None => {
            eprintln!("[FAIL] No active player found after 'playing' event. Players: {}", serde_json::to_string_pretty(&json!(players)).unwrap());
            assert!(false, "No active player after 'playing' event");
        }
    }
}

#[tokio::test]
#[serial]
async fn test_librespot_pipe_event_activates_player() {
    let server_url = unsafe { common::setup_test_server(TEST_PORT, &mut SERVER_PROCESS, &SERVER_READY, &INIT).await };
    let events = vec![
        create_librespot_event("playing", None, None),
    ];
    let pipe_write_success = match write_librespot_events_to_pipe(&events) {
        Ok(()) => {
            println!("[OK] Successfully wrote 'playing' event to Librespot pipe");
            true
        }
        Err(e) => {
            println!("[INFO] Failed to write to Librespot pipe: {} - pipe may not be available in test environment", e);
            false
        }
    };
    if pipe_write_success {
        tokio::time::sleep(std::time::Duration::from_millis(1000)).await;
    }
    let all_players = match get_all_players(&server_url).await {
        Ok(json) => json,
        Err(e) => {
            assert!(false, "Failed to get all players: {}", e);
            return;
        }
    };
    let players = all_players.get("players").and_then(|p| p.as_array()).expect("API should return players array");
    let active_player = players.iter().find(|p| p.get("is_active") == Some(&serde_json::Value::Bool(true)));
    if pipe_write_success {
        match active_player {
            Some(player) => {
                let id = player.get("id").and_then(|v| v.as_str()).unwrap_or("<none>");
                if id != "librespot" {
                    eprintln!("[FAIL] Active player is not librespot after pipe 'playing' event. Actual active player: {}", serde_json::to_string_pretty::<serde_json::Value>(player).unwrap());
                    assert!(false, "Active player should be librespot after pipe 'playing' event");
                } else {
                    println!("[OK] Librespot player became active after pipe 'playing' event");
                }
            }
            None => {
                eprintln!("[FAIL] No active player found after pipe 'playing' event. Players: {}", serde_json::to_string_pretty(&json!(players)).unwrap());
                assert!(false, "No active player after pipe 'playing' event");
            }
        }
    } else {
        println!("[SKIP] Pipe write did not succeed, skipping activation assertion");
    }
}
