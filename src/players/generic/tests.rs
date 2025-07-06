//! Tests for the GenericPlayerController

use crate::players::generic::GenericPlayerController;
use crate::players::player_controller::PlayerController;
use crate::data::loop_mode::LoopMode;
use crate::data::player_command::PlayerCommand;
use crate::data::PlaybackState;
use serde_json::json;

#[cfg(test)]
mod tests {
    use super::*;

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
                "album": "Test Album"
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
}
