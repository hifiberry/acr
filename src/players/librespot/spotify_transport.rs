//! The Spotify Web API requests this daemon makes, and where they are
//! addressed.
//!
//! The token comes from `spotify_account`, the account this daemon owns since
//! the one-way seam; the requests below are exactly the ones
//! `audiocontrol_metadata::spotify::Spotify::send_command` issued in place
//! before this backend stopped owning the OAuth client. Method, URL, query
//! parameters, body and headers, and the 204-is-success rule, are copied from
//! that function, not from a survey of it -- see `spotify.rs:609-660` in the
//! `audiocontrol-metadata` crate as it stood before the account moved.
//!
//! Both callers build their requests here: the librespot backend, which turns
//! a `PlayerCommand` into one of them, and `spotify_account`, whose
//! `/playback`, `/currently_playing`, `/command/<c>` and `/search` routes
//! address the same API. One module owns the URLs, so the route and the
//! backend cannot drift apart.

use acr_http::http_client::{new_http_client, HttpClientError};
use log::debug;

/// The Spotify Web API root.
const SPOTIFY_API_ROOT: &str = "https://api.spotify.com/v1";

/// Where the Spotify Web API is, which under test is a stub on loopback.
///
/// Overridable only under `cfg(test)`. A test that proved a play command
/// succeeded by actually reaching `api.spotify.com` would be asserting the
/// network; one that could not reach it at all could not tell "the token was
/// refused" from "no token was found", and that distinction is exactly what
/// the playback test guarding this seam turns on.
fn api_root() -> String {
    #[cfg(test)]
    if let Some(root) = testing::current_root() {
        return root;
    }
    SPOTIFY_API_ROOT.to_string()
}

/// The player endpoints' common base, `<root>/me/player`.
pub(crate) fn player_api_base() -> String {
    format!("{}/me/player", api_root())
}

/// The search URL for `query`, with the filters the `/search` route accepts
/// folded into the query string the way Spotify's API expects them.
///
/// The six filter names, and the `field:value` form, are what
/// `Spotify::search` built before the account moved.
pub(crate) fn search_url(
    query: &str,
    types: &[&str],
    filters: Option<&serde_json::Value>,
) -> String {
    let mut q = query.to_string();
    if let Some(filters) = filters {
        for field in ["artist", "year", "album", "genre", "isrc", "track"] {
            if let Some(value) = filters.get(field).and_then(|v| v.as_str()) {
                q.push_str(&format!(" {}:{}", field, value));
            }
        }
    }
    let type_param = types.join(",");
    format!(
        "{}/search?q={}&type={}",
        api_root(),
        urlencoding::encode(&q),
        urlencoding::encode(&type_param)
    )
}

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
    let base = player_api_base();
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

/// Pointing these requests at a stub for the duration of one test.
#[cfg(test)]
pub mod testing {
    use std::cell::RefCell;

    thread_local! {
        static ROOT: RefCell<Option<String>> = const { RefCell::new(None) };
    }

    pub(super) fn current_root() -> Option<String> {
        ROOT.with(|slot| slot.borrow().clone())
    }

    /// Restores the real API root when dropped, so a test cannot leave the
    /// next one on this thread addressing a stub that has gone.
    pub struct Redirected;

    impl Drop for Redirected {
        fn drop(&mut self) {
            ROOT.with(|slot| *slot.borrow_mut() = None);
        }
    }

    /// Address every Spotify Web API request from this thread at `root`
    /// (an API root, i.e. what `https://api.spotify.com/v1` is) until the
    /// returned guard is dropped.
    #[must_use]
    pub fn redirect_api(root: &str) -> Redirected {
        ROOT.with(|slot| *slot.borrow_mut() = Some(root.to_string()));
        Redirected
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

    /// The search URL is what `Spotify::search` built: filters appended to
    /// the query as `field:value` pairs, then the whole query and the type
    /// list URL-encoded separately.
    #[test]
    fn search_urls_match_what_the_account_built_before_the_move() {
        assert_eq!(
            search_url("Nightwish", &["artist"], None),
            "https://api.spotify.com/v1/search?q=Nightwish&type=artist"
        );
        assert_eq!(
            search_url(
                "Wishmaster",
                &["album", "track"],
                Some(&serde_json::json!({ "artist": "Nightwish", "year": "2000" }))
            ),
            "https://api.spotify.com/v1/search?q=Wishmaster%20artist%3ANightwish%20year%3A2000&type=album%2Ctrack"
        );
    }

    /// A filter key the route does not accept is ignored rather than
    /// appended, which is what keeps an arbitrary JSON body out of the query
    /// string.
    #[test]
    fn an_unknown_filter_is_not_folded_into_the_query() {
        assert_eq!(
            search_url(
                "Wishmaster",
                &["album"],
                Some(&serde_json::json!({ "nonsense": "value" }))
            ),
            "https://api.spotify.com/v1/search?q=Wishmaster&type=album"
        );
    }

    /// With the API redirected, every URL follows -- and the guard puts it
    /// back, so the tests above are not at the mercy of running order.
    #[test]
    fn redirecting_the_api_moves_every_url_and_is_undone() {
        {
            let _redirect = testing::redirect_api("http://127.0.0.1:9/v1");
            assert_eq!(
                request_for("play", &serde_json::json!({})),
                Some(("PUT", "http://127.0.0.1:9/v1/me/player/play".into()))
            );
            assert_eq!(
                search_url("q", &["track"], None),
                "http://127.0.0.1:9/v1/search?q=q&type=track"
            );
        }
        assert_eq!(
            request_for("play", &serde_json::json!({})),
            Some(("PUT", "https://api.spotify.com/v1/me/player/play".into()))
        );
    }
}
