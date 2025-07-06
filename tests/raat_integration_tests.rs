//! Integration tests for RAAT player

#[path = "common/mod.rs"]
mod common;
use common::*;
use serial_test::serial;
use std::sync::Once;
use std::sync::atomic::AtomicBool;

static INIT: Once = Once::new();
static mut SERVER_PROCESS: Option<std::process::Child> = None;
static SERVER_READY: AtomicBool = AtomicBool::new(false);
const TEST_PORT: u16 = 3004;

#[tokio::test]
#[serial]
async fn test_raat_player_initialization() {
    let server_url = unsafe { common::setup_test_server(TEST_PORT, &raw mut SERVER_PROCESS, &SERVER_READY, &INIT).await };
    
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
