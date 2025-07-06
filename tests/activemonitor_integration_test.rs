//! Integration test for the active_monitor plugin: ensures that the active player switches correctly

#[path = "common/mod.rs"]
mod common;
use common::*;
use serial_test::serial;
use std::sync::Once;
use std::sync::atomic::AtomicBool;

static INIT: Once = Once::new();
static mut SERVER_PROCESS: Option<std::process::Child> = None;
static SERVER_READY: AtomicBool = AtomicBool::new(false);
const TEST_PORT: u16 = 3003;

/// Integration test for the active_monitor plugin: ensures that the active player switches correctly
#[tokio::test]
#[serial]
async fn test_active_monitor_switches_active_player() {
    let server_url = common::setup_test_server(TEST_PORT, &raw mut SERVER_PROCESS, &SERVER_READY, &INIT).await;

    // 1. Send a 'playing' event to the generic player
    let generic_playing_event = create_generic_api_event("state_changed", None, None);
    if let Err(e) = send_librespot_api_event(&server_url, &generic_playing_event).await {
        assert!(false, "Failed to send API event to generic player: {}", e);
        return;
    }
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    // 2. Check that the generic player is now active
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
            assert_eq!(id, "test_player", "Active player should be test_player after generic event");
        }
        None => {
            assert!(false, "No active player after generic player event");
        }
    }

    // 3. Send a 'playing' event to librespot
    let librespot_playing_event = create_generic_api_event("state_changed", None, None);
    if let Err(e) = send_librespot_api_event(&server_url, &librespot_playing_event).await {
        assert!(false, "Failed to send API event to librespot: {}", e);
        return;
    }
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    // 4. Check that librespot is now active
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
            assert_eq!(id, "librespot", "Active player should be librespot after librespot event");
        }
        None => {
            assert!(false, "No active player after librespot event");
        }
    }
    
    // Clean up after the last test in this module
    unsafe {
        common::force_cleanup_test_server(TEST_PORT, &raw mut SERVER_PROCESS, &SERVER_READY);
    }
}
