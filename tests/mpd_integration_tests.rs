//! Integration tests for MPD player

#[path = "common/mod.rs"]
mod common;
use common::*;
use serial_test::serial;
use std::sync::Once;
use std::sync::atomic::AtomicBool;

static INIT: Once = Once::new();
static mut SERVER_PROCESS: Option<std::process::Child> = None;
static SERVER_READY: AtomicBool = AtomicBool::new(false);
const TEST_PORT: u16 = 3005;

#[tokio::test]
#[serial]
async fn test_mpd_player_initialization() {
    let server_url = unsafe { common::setup_test_server(TEST_PORT, &raw mut SERVER_PROCESS, &SERVER_READY, &INIT).await };
    
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
                    assert!(false, "[INFO] MPD player not found");
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
