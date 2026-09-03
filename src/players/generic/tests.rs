//! Tests for the GenericPlayerController

use crate::players::generic::GenericPlayerController;
use crate::players::player_controller::PlayerController;
use crate::data::loop_mode::LoopMode;
use crate::data::player_command::PlayerCommand;
use crate::data::PlaybackState;
use crate::data::PlayerCapability;
use serde_json::json;

#[cfg(test)]
mod tests {
    use super::*;

    /// Bind a loopback listener that accepts exactly one HTTP request and
    /// sends the raw request text back over the channel. Returns the address
    /// to point `command_url` at, and the receiver.
    fn capture_one_post() -> (std::net::SocketAddr, std::sync::mpsc::Receiver<String>) {
        use std::io::{Read, Write};
        use std::net::TcpListener;
        use std::sync::mpsc;
        use std::thread;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                stream
                    .set_read_timeout(Some(std::time::Duration::from_secs(1)))
                    .ok();
                // ureq writes headers and body in separate TCP segments, so keep
                // reading until we have both the request line and the body.
                let mut req = String::new();
                let mut buf = [0u8; 1024];
                loop {
                    match stream.read(&mut buf) {
                        Ok(0) => break,
                        Ok(n) => {
                            req.push_str(&String::from_utf8_lossy(&buf[..n]));
                            if req.contains("\r\n\r\n") && req.contains("\"command\"") {
                                break;
                            }
                        }
                        Err(_) => break,
                    }
                }
                let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n");
                let _ = tx.send(req);
            }
        });
        (addr, rx)
    }

    fn create_test_controller() -> GenericPlayerController {
        let config = json!({
            "name": "test_player",
            "display_name": "Test Player",
            "enable": true,
            "supports_api_events": true,
            "capabilities": ["play", "pause", "stop", "next", "previous", "seek", "shuffle", "loop"],
            "initial_state": "stopped",
            "shuffle": false,
            "loop_mode": "none"
        });
        
        GenericPlayerController::from_config(&config).unwrap()
    }

    #[test]
    fn test_controller_creation() {
        let controller = GenericPlayerController::new("test_player".to_string());
        assert_eq!(controller.get_player_name(), "test_player");
        assert_eq!(controller.get_player_id(), "test_player");
    }

    #[test]
    fn test_controller_from_config() {
        let config = json!({
            "name": "config_player",
            "display_name": "Config Player",
            "initial_state": "playing",
            "shuffle": true,
            "loop_mode": "track"
        });
        
        let controller = GenericPlayerController::from_config(&config).unwrap();
        assert_eq!(controller.get_player_name(), "config_player");
        assert_eq!(controller.get_playback_state(), PlaybackState::Playing);
        assert!(controller.get_shuffle());
        assert_eq!(controller.get_loop_mode(), LoopMode::Track);
    }

    #[test]
    fn test_controller_invalid_config() {
        let config = json!({
            "display_name": "No Name Player"
            // Missing required "name" field
        });
        
        let result = GenericPlayerController::from_config(&config);
        assert!(result.is_err());
    }

    #[test]
    fn test_basic_player_commands() {
        let controller = create_test_controller();
        
        // Test play command
        let play_result = controller.send_command(PlayerCommand::Play);
        assert!(play_result);
        assert_eq!(controller.get_playback_state(), PlaybackState::Playing);
        
        // Test pause command
        let pause_result = controller.send_command(PlayerCommand::Pause);
        assert!(pause_result);
        assert_eq!(controller.get_playback_state(), PlaybackState::Paused);
        
        // Test stop command
        let stop_result = controller.send_command(PlayerCommand::Stop);
        assert!(stop_result);
        assert_eq!(controller.get_playback_state(), PlaybackState::Stopped);
    }

    #[test]
    fn test_loop_mode_command() {
        let controller = create_test_controller();
        
        // Test loop mode change
        let loop_result = controller.send_command(PlayerCommand::SetLoopMode(LoopMode::Track));
        assert!(loop_result);
        assert_eq!(controller.get_loop_mode(), LoopMode::Track);
    }

    #[test]
    fn test_shuffle_command() {
        let controller = create_test_controller();
        
        // Test shuffle enable
        let shuffle_result = controller.send_command(PlayerCommand::SetRandom(true));
        assert!(shuffle_result);
        assert!(controller.get_shuffle());
    }

    #[test]
    fn test_seek_command() {
        let controller = create_test_controller();
        
        // Test seek
        let seek_result = controller.send_command(PlayerCommand::Seek(42.5));
        assert!(seek_result);
        assert_eq!(controller.get_position(), Some(42.5));
    }

    #[test]
    fn test_capabilities() {
        let controller = create_test_controller();
        let capabilities = controller.get_capabilities();
        
        // Check that the generic player has some basic capabilities
        assert!(!capabilities.is_empty());
    }

    #[test]
    fn test_api_event_processing() {
        let controller = create_test_controller();
        
        // Test state change event
        let state_event = json!({
            "type": "state_changed",
            "state": "playing"
        });
        
        let result = controller.process_api_event(&state_event);
        assert!(result);
        assert_eq!(controller.get_playback_state(), PlaybackState::Playing);
        
        // Test song change event
        let song_event = json!({
            "type": "song_changed",
            "song": {
                "title": "Test Song",
                "artist": "Test Artist",
                "album": "Test Album",
                "cover_art_url": "http://example.com/cover.jpg"
            }
        });

        let result = controller.process_api_event(&song_event);
        assert!(result);

        // Check that the current song was updated
        let current_song = controller.get_song();
        assert!(current_song.is_some());
        let song = current_song.unwrap();
        assert_eq!(song.title, Some("Test Song".to_string()));
        assert_eq!(song.artist, Some("Test Artist".to_string()));
        assert_eq!(song.album, Some("Test Album".to_string()));
        assert_eq!(song.cover_art_url, Some("http://example.com/cover.jpg".to_string()));
    }

    #[test]
    fn test_song_changed_artwork_url_alias() {
        let controller = create_test_controller();

        // Senders such as Sendspin/Music Assistant use "artwork_url" — it should
        // map onto the song's cover_art_url just like the native "cover_art_url".
        let song_event = json!({
            "type": "song_changed",
            "song": {
                "title": "Artwork Song",
                "artwork_url": "http://example.com/art.png"
            }
        });

        assert!(controller.process_api_event(&song_event));
        let song = controller.get_song().expect("song should be set");
        assert_eq!(song.cover_art_url, Some("http://example.com/art.png".to_string()));

        // An empty cover string must not set the field.
        let empty_cover = json!({
            "type": "song_changed",
            "song": { "title": "No Art", "cover_art_url": "" }
        });
        assert!(controller.process_api_event(&empty_cover));
        assert_eq!(controller.get_song().unwrap().cover_art_url, None);
    }

    #[test]
    fn test_position_event() {
        let controller = create_test_controller();
        
        let position_event = json!({
            "type": "position_changed",
            "position": 42.5
        });
        
        let result = controller.process_api_event(&position_event);
        assert!(result);
        assert_eq!(controller.get_position(), Some(42.5));
    }

    #[test]
    fn test_stream_info_event() {
        let controller = create_test_controller();

        let ev = json!({
            "type": "stream_info",
            "stream": { "codec": "flac", "sample_rate": 44100,
                        "bits_per_sample": 16, "channels": 2 }
        });
        assert!(controller.process_api_event(&ev));
        let sd = controller.get_stream_details().expect("stream details set");
        assert_eq!(sd.sample_rate, Some(44100));
        assert_eq!(sd.bits_per_sample, Some(16));
        assert_eq!(sd.channels, Some(2));
        assert_eq!(sd.codec, Some("flac".to_string()));
        assert_eq!(sd.lossless, Some(true));

        // Opus is lossy
        let opus = json!({ "type": "stream_info", "stream": { "codec": "opus" } });
        assert!(controller.process_api_event(&opus));
        assert_eq!(controller.get_stream_details().unwrap().lossless, Some(false));

        // stream_info without a stream object clears the details
        let clear = json!({ "type": "stream_info" });
        assert!(controller.process_api_event(&clear));
        assert!(controller.get_stream_details().is_none());
    }

    #[test]
    fn test_shuffle_event() {
        let controller = create_test_controller();
        
        // Initially shuffle should be false
        assert!(!controller.get_shuffle());
        
        let shuffle_event = json!({
            "type": "shuffle_changed",
            "shuffle": true
        });
        
        let result = controller.process_api_event(&shuffle_event);
        assert!(result);
        assert!(controller.get_shuffle());
    }

    #[test]
    fn test_loop_mode_event() {
        let controller = create_test_controller();
        
        // Initially loop mode should be none
        assert_eq!(controller.get_loop_mode(), LoopMode::None);
        
        let loop_event = json!({
            "type": "loop_mode_changed",
            "loop_mode": "track"
        });
        
        let result = controller.process_api_event(&loop_event);
        assert!(result);
        assert_eq!(controller.get_loop_mode(), LoopMode::Track);
    }

    #[test]
    fn test_invalid_event() {
        let controller = create_test_controller();
        
        let invalid_event = json!({
            "type": "invalid_event_type",
            "data": "some data"
        });
        
        let result = controller.process_api_event(&invalid_event);
        assert!(!result); // Should return false for unknown event types
    }

    #[test]
    fn test_event_without_type() {
        let controller = create_test_controller();
        
        let event_without_type = json!({
            "state": "playing"
        });
        
        let result = controller.process_api_event(&event_without_type);
        assert!(!result); // Should return false for events without type
    }

    #[test]
    fn test_supports_api_events() {
        let controller = create_test_controller();
        assert!(controller.supports_api_events());
    }

    #[test]
    fn test_start_stop() {
        let controller = create_test_controller();
        
        let start_result = controller.start();
        assert!(start_result);
        
        let stop_result = controller.stop();
        assert!(stop_result);
    }

    #[test]
    fn test_multiple_instances() {
        let controller1 = GenericPlayerController::new("player1".to_string());
        let controller2 = GenericPlayerController::new("player2".to_string());
        
        assert_eq!(controller1.get_player_name(), "player1");
        assert_eq!(controller2.get_player_name(), "player2");
        assert_ne!(controller1.get_player_name(), controller2.get_player_name());
    }

    #[test]
    fn test_command_url_posts_on_send_command() {
        let (addr, rx) = capture_one_post();
        let config = json!({
            "name": "sendspin",
            "supports_api_events": true,
            "command_url": format!("http://{}/command", addr)
        });
        let controller = GenericPlayerController::from_config(&config).unwrap();
        assert!(controller.send_command(PlayerCommand::Pause));

        let req = rx.recv_timeout(std::time::Duration::from_secs(2)).expect("no POST received");
        assert!(req.contains("POST /command"));
        assert!(req.contains("\"command\":\"pause\""));
    }

    #[test]
    fn test_command_url_posts_seek_with_position() {
        let (addr, rx) = capture_one_post();
        let config = json!({
            "name": "bridge",
            "supports_api_events": true,
            "command_url": format!("http://{}/command", addr)
        });
        let controller = GenericPlayerController::from_config(&config).unwrap();
        assert!(controller.send_command(PlayerCommand::Seek(42.5)));

        let req = rx.recv_timeout(std::time::Duration::from_secs(2)).expect("no POST received");
        assert!(req.contains("\"command\":\"seek\""), "body was: {}", req);
        assert!(req.contains("\"position\":42.5"), "body was: {}", req);
    }

    #[test]
    fn test_command_url_posts_loop_mode_using_display_vocabulary() {
        let (addr, rx) = capture_one_post();
        let config = json!({
            "name": "bridge",
            "supports_api_events": true,
            "command_url": format!("http://{}/command", addr)
        });
        let controller = GenericPlayerController::from_config(&config).unwrap();
        assert!(controller.send_command(PlayerCommand::SetLoopMode(LoopMode::Track)));

        let req = rx.recv_timeout(std::time::Duration::from_secs(2)).expect("no POST received");
        assert!(req.contains("\"command\":\"set_loop_mode\""), "body was: {}", req);
        // LoopMode::Track renders as "song", not "track" -- same vocabulary
        // PlayerCommand's Display impl already uses.
        assert!(req.contains("\"loop_mode\":\"song\""), "body was: {}", req);
    }

    #[test]
    fn test_command_url_posts_shuffle() {
        let (addr, rx) = capture_one_post();
        let config = json!({
            "name": "bridge",
            "supports_api_events": true,
            "command_url": format!("http://{}/command", addr)
        });
        let controller = GenericPlayerController::from_config(&config).unwrap();
        assert!(controller.send_command(PlayerCommand::SetRandom(true)));

        let req = rx.recv_timeout(std::time::Duration::from_secs(2)).expect("no POST received");
        assert!(req.contains("\"command\":\"set_shuffle\""), "body was: {}", req);
        assert!(req.contains("\"shuffle\":true"), "body was: {}", req);
    }

    #[test]
    fn test_queue_changed_event_populates_queue() {
        let controller = create_test_controller();
        let event = json!({
            "type": "queue_changed",
            "queue": [
                { "title": "First", "artist": "A", "uri": "spotify:track:1" },
                { "name": "Second", "uri": "spotify:track:2" }
            ]
        });

        assert!(controller.process_api_event(&event));

        let queue = controller.get_queue();
        assert_eq!(queue.len(), 2);
        assert_eq!(queue[0].name, "First");
        assert_eq!(queue[0].artist, Some("A".to_string()));
        assert_eq!(queue[0].uri, Some("spotify:track:1".to_string()));
        // "name" is accepted as well as "title"
        assert_eq!(queue[1].name, "Second");
        assert_eq!(queue[1].artist, None);
    }

    #[test]
    fn test_queue_changed_event_ignores_unmappable_fields() {
        let controller = create_test_controller();
        let event = json!({
            "type": "queue_changed",
            "queue": [
                { "title": "Song", "album": "Album", "duration": 210.0,
                  "cover_art_url": "http://example.com/a.jpg" }
            ]
        });

        assert!(controller.process_api_event(&event));
        let queue = controller.get_queue();
        assert_eq!(queue.len(), 1);
        assert_eq!(queue[0].name, "Song");
    }

    #[test]
    fn test_queue_changed_event_with_empty_queue_clears_it() {
        let controller = create_test_controller();
        controller.process_api_event(&json!({
            "type": "queue_changed",
            "queue": [{ "title": "Song" }]
        }));
        assert_eq!(controller.get_queue().len(), 1);

        assert!(controller.process_api_event(&json!({
            "type": "queue_changed",
            "queue": []
        })));
        assert!(controller.get_queue().is_empty());
    }

    #[test]
    fn test_queue_changed_event_without_queue_field_is_rejected() {
        let controller = create_test_controller();
        // A malformed event must not silently clear the queue.
        controller.process_api_event(&json!({
            "type": "queue_changed",
            "queue": [{ "title": "Song" }]
        }));
        assert!(!controller.process_api_event(&json!({ "type": "queue_changed" })));
        assert_eq!(controller.get_queue().len(), 1);
    }

    #[test]
    fn test_queue_capability_is_recognised() {
        let config = json!({
            "name": "bridge",
            "capabilities": ["play", "queue"]
        });
        let controller = GenericPlayerController::from_config(&config).unwrap();
        assert!(controller.get_capabilities().has_capability(PlayerCapability::Queue));
    }

    #[test]
    fn test_position_advances_while_playing() {
        let controller = create_test_controller();

        controller.process_api_event(&json!({ "type": "state_changed", "state": "playing" }));
        controller.process_api_event(&json!({ "type": "position_changed", "position": 10.0 }));

        std::thread::sleep(std::time::Duration::from_millis(150));

        let position = controller.get_position().expect("position should be known");
        assert!(
            position > 10.05,
            "position should have advanced past 10.0 while playing, got {}",
            position
        );
    }

    #[test]
    fn test_position_frozen_while_paused() {
        let controller = create_test_controller();

        controller.process_api_event(&json!({ "type": "state_changed", "state": "paused" }));
        controller.process_api_event(&json!({ "type": "position_changed", "position": 10.0 }));

        std::thread::sleep(std::time::Duration::from_millis(150));

        let position = controller.get_position().expect("position should be known");
        assert!(
            (position - 10.0).abs() < 0.01,
            "position should not advance while paused, got {}",
            position
        );
    }

    #[test]
    fn test_position_is_none_until_reported() {
        let controller = create_test_controller();
        assert_eq!(controller.get_position(), None);
    }

    #[test]
    fn test_send_command_pause_freezes_position() {
        let controller = create_test_controller();

        controller.process_api_event(&json!({ "type": "state_changed", "state": "playing" }));
        controller.process_api_event(&json!({ "type": "position_changed", "position": 10.0 }));

        assert!(controller.send_command(PlayerCommand::Pause));
        assert_eq!(controller.get_playback_state(), PlaybackState::Paused);

        let position_immediately = controller.get_position().expect("position should be known");
        std::thread::sleep(std::time::Duration::from_millis(150));
        let position_after_sleep = controller.get_position().expect("position should be known");

        assert!(
            (position_after_sleep - position_immediately).abs() < 0.01,
            "position should not advance after a Pause command, got {} then {}",
            position_immediately,
            position_after_sleep
        );
    }

    #[test]
    fn test_send_command_play_advances_position() {
        let controller = create_test_controller();

        // create_test_controller starts "stopped", so position is frozen at 10.0
        // until the Play command below sets it moving.
        controller.process_api_event(&json!({ "type": "position_changed", "position": 10.0 }));

        assert!(controller.send_command(PlayerCommand::Play));
        assert_eq!(controller.get_playback_state(), PlaybackState::Playing);

        std::thread::sleep(std::time::Duration::from_millis(150));

        let position = controller.get_position().expect("position should be known");
        assert!(
            position > 10.05,
            "position should have advanced past 10.0 after a Play command, got {}",
            position
        );
    }

    #[test]
    fn test_send_command_stop_freezes_position() {
        let controller = create_test_controller();

        controller.process_api_event(&json!({ "type": "state_changed", "state": "playing" }));
        controller.process_api_event(&json!({ "type": "position_changed", "position": 10.0 }));

        assert!(controller.send_command(PlayerCommand::Stop));
        assert_eq!(controller.get_playback_state(), PlaybackState::Stopped);

        let position_immediately = controller.get_position().expect("position should be known");
        std::thread::sleep(std::time::Duration::from_millis(150));
        let position_after_sleep = controller.get_position().expect("position should be known");

        assert!(
            (position_after_sleep - position_immediately).abs() < 0.01,
            "position should not advance after a Stop command, got {} then {}",
            position_immediately,
            position_after_sleep
        );
    }

    #[test]
    fn test_song_change_resets_position_on_genuine_change() {
        let controller = create_test_controller();

        controller.process_api_event(&json!({
            "type": "song_changed",
            "song": { "title": "First Song", "artist": "Artist A", "uri": "spotify:track:1" }
        }));
        controller.process_api_event(&json!({ "type": "state_changed", "state": "playing" }));
        controller.process_api_event(&json!({ "type": "position_changed", "position": 42.0 }));

        assert!((controller.get_position().unwrap() - 42.0).abs() < 1.0);

        // Genuine song change: different title/artist/uri.
        controller.process_api_event(&json!({
            "type": "song_changed",
            "song": { "title": "Second Song", "artist": "Artist B", "uri": "spotify:track:2" }
        }));

        let position = controller.get_position().expect("position should be known");
        assert!(
            position < 1.0,
            "position should reset to ~0 on a genuine song change, got {}",
            position
        );
    }

    #[test]
    fn test_song_change_same_song_does_not_reset_position() {
        let controller = create_test_controller();

        controller.process_api_event(&json!({
            "type": "song_changed",
            "song": { "title": "Same Song", "artist": "Artist A", "uri": "spotify:track:1" }
        }));
        controller.process_api_event(&json!({ "type": "state_changed", "state": "playing" }));
        controller.process_api_event(&json!({ "type": "position_changed", "position": 42.0 }));

        // Metadata-only refresh for the SAME song: late artwork arriving.
        controller.process_api_event(&json!({
            "type": "song_changed",
            "song": {
                "title": "Same Song",
                "artist": "Artist A",
                "uri": "spotify:track:1",
                "cover_art_url": "http://example.com/late-art.jpg"
            }
        }));

        let position = controller.get_position().expect("position should be known");
        assert!(
            position >= 42.0,
            "a metadata-only refresh of the same song must not reset position, got {}",
            position
        );
    }

    /// A same-song refresh must still reach get_song() with its new metadata
    /// -- suppressing the position reset must not also suppress the store.
    /// Late cover art is the case that matters: a client that never sees the
    /// refreshed song never gets the artwork.
    #[test]
    fn test_song_change_same_song_refresh_still_updates_metadata() {
        let controller = create_test_controller();

        controller.process_api_event(&json!({
            "type": "song_changed",
            "song": { "title": "Same Song", "artist": "Artist A", "uri": "spotify:track:1" }
        }));
        controller.process_api_event(&json!({ "type": "state_changed", "state": "playing" }));
        controller.process_api_event(&json!({ "type": "position_changed", "position": 42.0 }));

        // Metadata-only refresh for the SAME song: late artwork arriving.
        controller.process_api_event(&json!({
            "type": "song_changed",
            "song": {
                "title": "Same Song",
                "artist": "Artist A",
                "uri": "spotify:track:1",
                "cover_art_url": "http://example.com/late-art.jpg"
            }
        }));

        let song = controller.get_song().expect("song should be set");
        assert_eq!(
            song.cover_art_url,
            Some("http://example.com/late-art.jpg".to_string()),
            "a metadata-only refresh must still be stored, not dropped"
        );

        let position = controller.get_position().expect("position should be known");
        assert!(
            position >= 42.0,
            "and must still not reset position, got {}",
            position
        );
    }

    #[test]
    fn test_queue_changed_event_skips_entries_without_title() {
        let controller = create_test_controller();
        let event = json!({
            "type": "queue_changed",
            "queue": [
                { "artist": "No Title Here" },
                { "title": "First", "artist": "A" },
                { "title": "Second", "artist": "B" }
            ]
        });

        assert!(controller.process_api_event(&event));
        let queue = controller.get_queue();
        assert_eq!(queue.len(), 2);
        assert_eq!(queue[0].name, "First");
        assert_eq!(queue[1].name, "Second");
    }

    #[test]
    fn test_queue_changed_event_falls_back_to_name_when_title_is_wrong_type() {
        let controller = create_test_controller();
        // {"title": 123, "name": "Real Title"}: .or_else() only fires on None,
        // so a naive title.or_else(name).as_str() would drop this entry even
        // though "name" is documented as a valid alias.
        let event = json!({
            "type": "queue_changed",
            "queue": [
                { "title": 123, "name": "Real Title" }
            ]
        });

        assert!(controller.process_api_event(&event));
        let queue = controller.get_queue();
        assert_eq!(queue.len(), 1);
        assert_eq!(queue[0].name, "Real Title");
    }
}
