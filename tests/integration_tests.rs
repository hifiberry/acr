//! Integration tests for the player event client

use std::process::Command;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cli_help() {
        let output = Command::new("cargo")
            .args(&["run", "--bin", "audiocontrol_player_event_client", "--", "--help"])
            .output()
            .expect("Failed to execute command");

        assert!(output.status.success());
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains("Client for sending events to the player API"));
        assert!(stdout.contains("Commands:"));
        assert!(stdout.contains("state-changed"));
        assert!(stdout.contains("song-changed"));
    }

    #[test]
    fn test_cli_version() {
        let output = Command::new("cargo")
            .args(&["run", "--bin", "audiocontrol_player_event_client", "--", "--version"])
            .output()
            .expect("Failed to execute command");

        assert!(output.status.success());
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains("audiocontrol")); // Version shows as "audiocontrol 0.4.3"
    }

    #[test]
    fn test_cli_invalid_command() {
        let output = Command::new("cargo")
            .args(&["run", "--bin", "audiocontrol_player_event_client", "--", "test_player", "invalid_command"])
            .output()
            .expect("Failed to execute command");

        assert!(!output.status.success());
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("error:") || stderr.contains("unrecognized subcommand"));
    }

    #[test]
    fn test_cli_missing_args() {
        let output = Command::new("cargo")
            .args(&["run", "--bin", "audiocontrol_player_event_client"])
            .output()
            .expect("Failed to execute command");

        assert!(!output.status.success());
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("error:") || stderr.contains("required arguments"));
    }

    #[test]
    fn test_cli_state_changed_help() {
        let output = Command::new("cargo")
            .args(&["run", "--bin", "audiocontrol_player_event_client", "--", "test_player", "state-changed", "--help"])
            .output()
            .expect("Failed to execute command");

        assert!(output.status.success());
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains("Send a state change event"));
        assert!(stdout.contains("playing"));
        assert!(stdout.contains("paused"));
        assert!(stdout.contains("stopped"));
    }

    #[test]
    fn test_cli_song_changed_help() {
        let output = Command::new("cargo")
            .args(&["run", "--bin", "audiocontrol_player_event_client", "--", "test_player", "song-changed", "--help"])
            .output()
            .expect("Failed to execute command");

        if !output.status.success() {
            println!("Command failed with stderr: {}", String::from_utf8_lossy(&output.stderr));
        }
        
        let stdout = String::from_utf8_lossy(&output.stdout);
        
        // Check for the essential parts of the help output
        assert!(stdout.contains("Send a song change event") || stdout.contains("song-changed"));
        assert!(stdout.contains("--title") || stdout.contains("title"));
        assert!(stdout.contains("--artist") || stdout.contains("artist"));
        assert!(stdout.contains("--album") || stdout.contains("album"));
    }

    #[test]
    fn test_cli_custom_help() {
        let output = Command::new("cargo")
            .args(&["run", "--bin", "audiocontrol_player_event_client", "--", "test_player", "custom", "--help"])
            .output()
            .expect("Failed to execute command");

        assert!(output.status.success());
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains("Send a custom event from JSON"));
        assert!(stdout.contains("JSON")); // Uses "JSON" instead of "--json"
    }

    #[test]
    fn test_invalid_host_format() {
        let output = Command::new("cargo")
            .args(&[
                "run", "--bin", "audiocontrol_player_event_client", "--", 
                "--host", "invalid-host",
                "test_player", "state-changed", "playing"
            ])
            .output()
            .expect("Failed to execute command");

        // Should fail due to invalid host or connection error
        assert!(!output.status.success());
    }

    #[test]
    fn test_connection_error() {
        let output = Command::new("cargo")
            .args(&[
                "run", "--bin", "audiocontrol_player_event_client", "--", 
                "--host", "http://localhost:9999",  // Non-existent server
                "test_player", "state-changed", "playing"
            ])
            .output()
            .expect("Failed to execute command");

        // Should fail due to connection error
        assert!(!output.status.success());
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("error") || stderr.contains("connection") || stderr.contains("failed"));
    }
}
