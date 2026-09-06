//! The librespot backend's Spotify Web API calls.
//!
//! The token comes from whatever `AccessTokenSource` `main` injected
//! (`crate::audiocontrol::token`); the requests below are exactly the ones
//! `audiocontrol_metadata::spotify::Spotify::send_command` issued in place
//! before this backend stopped owning the OAuth client. Method, URL, query
//! parameters, body and headers, and the 204-is-success rule, are copied from
//! that function, not from a survey of it -- see `spotify.rs:609-660` in the
//! `audiocontrol-metadata` crate.

use acr_http::http_client::{new_http_client, HttpClientError};
use log::debug;

/// The HTTP method and URL `Spotify::send_command` builds for `command`, or
/// `None` for a command it does not recognize.
///
/// Query parameters default the same way the original does when `args` is
/// missing the field it looks for: `seek` defaults to position `0`, `repeat`
/// to `"off"`, `shuffle` to `false`, rather than skipping the request.
pub(crate) fn request_for(
    command: &str,
    args: &serde_json::Value,
) -> Option<(&'static str, String)> {
    let base = "https://api.spotify.com/v1/me/player";
    match command {
        "play" => Some(("PUT", format!("{}/play", base))),
        "pause" => Some(("PUT", format!("{}/pause", base))),
        "next" => Some(("POST", format!("{}/next", base))),
        "previous" => Some(("POST", format!("{}/previous", base))),
        "seek" => {
            let position_ms = args.get("position_ms").and_then(|v| v.as_u64()).unwrap_or(0);
            Some(("PUT", format!("{}/seek?position_ms={}", base, position_ms)))
        }
        "repeat" => {
            let state = args.get("state").and_then(|v| v.as_str()).unwrap_or("off");
            Some(("PUT", format!("{}/repeat?state={}", base, state)))
        }
        "shuffle" => {
            let state = args.get("state").and_then(|v| v.as_bool()).unwrap_or(false);
            Some(("PUT", format!("{}/shuffle?state={}", base, state)))
        }
        _ => None,
    }
}

/// Send a command to the Spotify Web API (play, pause, next, previous, seek,
/// repeat, shuffle), the same way `Spotify::send_command` did when this
/// backend called it directly.
///
/// `token` is never logged: only its presence matters here.
pub fn send_command(token: &str, command: &str, args: &serde_json::Value) -> Result<(), String> {
    let (method, url) = match request_for(command, args) {
        Some(v) => v,
        None => return Err(format!("API error: Unknown command: {}", command)),
    };

    let client = new_http_client(10);
    let headers = [
        ("Authorization", &format!("Bearer {}", token)[..]),
        ("Content-Type", "application/json"),
    ];

    // `play`, `pause`, `next` and `previous` forward the caller's args as the
    // request body, same as `Spotify::send_command`; `seek`, `repeat` and
    // `shuffle` carry their parameters in the query string and send an empty
    // body.
    let body = match command {
        "play" | "pause" | "next" | "previous" => args.clone(),
        _ => serde_json::json!({}),
    };

    let result = match method {
        "PUT" => client.put_json_value_with_headers(&url, body, &headers),
        _ => client.post_json_value_with_headers(&url, body, &headers),
    };

    match result {
        Ok(_) => Ok(()),
        // Handle empty responses as success for Spotify API commands (204 No Content)
        Err(HttpClientError::EmptyResponse) => {
            debug!(
                "Spotify API command '{}' returned empty response (204 No Content) - treating as success",
                command
            );
            Ok(())
        }
        Err(e) => Err(format!("API error: Command failed: {}", e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_map_to_the_web_api_as_before() {
        assert_eq!(
            request_for("play", &serde_json::json!({})),
            Some(("PUT", "https://api.spotify.com/v1/me/player/play".into()))
        );
        assert_eq!(
            request_for("next", &serde_json::json!({})),
            Some(("POST", "https://api.spotify.com/v1/me/player/next".into()))
        );
        assert_eq!(
            request_for("seek", &serde_json::json!({"position_ms": 30000})),
            Some((
                "PUT",
                "https://api.spotify.com/v1/me/player/seek?position_ms=30000".into()
            ))
        );
        assert_eq!(request_for("volume", &serde_json::json!({})), None);
    }

    #[test]
    fn pause_previous_and_shuffle_match_the_web_api_too() {
        assert_eq!(
            request_for("pause", &serde_json::json!({})),
            Some(("PUT", "https://api.spotify.com/v1/me/player/pause".into()))
        );
        assert_eq!(
            request_for("previous", &serde_json::json!({})),
            Some(("POST", "https://api.spotify.com/v1/me/player/previous".into()))
        );
        assert_eq!(
            request_for("shuffle", &serde_json::json!({"state": true})),
            Some((
                "PUT",
                "https://api.spotify.com/v1/me/player/shuffle?state=true".into()
            ))
        );
        assert_eq!(
            request_for("repeat", &serde_json::json!({"state": "track"})),
            Some((
                "PUT",
                "https://api.spotify.com/v1/me/player/repeat?state=track".into()
            ))
        );
    }

    /// `Spotify::send_command` defaults a missing query field rather than
    /// refusing the command -- `seek` with no `position_ms` still requests
    /// position 0.
    #[test]
    fn missing_query_args_default_instead_of_being_refused() {
        assert_eq!(
            request_for("seek", &serde_json::json!({})),
            Some((
                "PUT",
                "https://api.spotify.com/v1/me/player/seek?position_ms=0".into()
            ))
        );
        assert_eq!(
            request_for("repeat", &serde_json::json!({})),
            Some((
                "PUT",
                "https://api.spotify.com/v1/me/player/repeat?state=off".into()
            ))
        );
        assert_eq!(
            request_for("shuffle", &serde_json::json!({})),
            Some((
                "PUT",
                "https://api.spotify.com/v1/me/player/shuffle?state=false".into()
            ))
        );
    }

    #[test]
    fn an_unsupported_command_is_reported_as_the_original_would() {
        match request_for("volume", &serde_json::json!({})) {
            None => {}
            Some(_) => panic!("expected no request for an unsupported command"),
        }
    }

    /// `send_command` rejects an unsupported command before making any
    /// request, with the same error text `Spotify::send_command` returned
    /// (via its `SpotifyError::ApiError` `Display` impl, "API error: {0}").
    #[test]
    fn send_command_rejects_unsupported_commands_without_a_request() {
        // Not a real credential -- an obvious placeholder for the test.
        assert_eq!(
            send_command("placeholder-token", "volume", &serde_json::json!({})),
            Err("API error: Unknown command: volume".to_string())
        );
    }
}
