//! A minimal Spotify Web API client for this crate's two providers.
//!
//! **This module holds no account.** It stores no token, refreshes none, and
//! never touches the security store. Every call here takes a bearer token as
//! an argument, and [`access_token`] is where that token comes from: the
//! player daemon, over `GET /api/spotify/access_token`, cached for 60 s by
//! [`crate::core_client::CoreClient`].
//!
//! The account itself lives in the player daemon
//! (`src/players/librespot/spotify_account.rs`), together with the OAuth flow
//! and every route that manages it. It moved because its primary consumer is
//! playback control -- the librespot backend turns `PlayerCommand::Play` and
//! its siblings into Spotify Web API calls -- and fetching the bearer token
//! for those *across the seam* meant a metadata half that was down stopped
//! playback. What is left here is the secondary consumer: two providers that
//! look things up and contribute nothing when there is no token, which is the
//! correct failure mode for metadata.
//!
//! The search request is therefore duplicated between the two daemons rather
//! than shared or proxied. Proxying -- having these providers call the player
//! daemon's `POST /api/spotify/search` -- was considered and rejected: it
//! would put rate-limited provider network work on the player daemon's Rocket
//! workers and make a cover-art lookup two hops. Provider I/O belongs in the
//! provider daemon. What is duplicated is one documented GET, not an
//! abstraction.

use acr_types::token::AccessTokenSource;
use log::{debug, error};
use once_cell::sync::OnceCell;
use std::sync::Arc;
use thiserror::Error;

/// Where a Spotify bearer token comes from: the player daemon, through
/// `CoreClient`. Installed once, from `startup::start_after_core_is_listening`.
static TOKEN_SOURCE: OnceCell<Arc<dyn AccessTokenSource>> = OnceCell::new();

/// Install the token source. The first call wins; later ones are ignored.
///
/// Set once, before any provider can be asked for an image: for the same
/// reason the player side's injected implementations are set once, a caller
/// that already asked one source for a token must not silently start asking
/// another.
pub fn set_token_source(source: Arc<dyn AccessTokenSource>) {
    let _ = TOKEN_SOURCE.set(source);
}

/// A Spotify bearer token, or `None` when no account is linked, no source has
/// been installed, or the player daemon cannot be reached.
///
/// Every caller in this crate treats `None` the same way: contribute nothing.
/// That is what these providers already did when no Spotify account was
/// linked, so an absent player daemon is not a new failure mode for them.
pub fn access_token() -> Option<String> {
    #[cfg(test)]
    if let Some(t) = testing::current() {
        return Some(t);
    }
    TOKEN_SOURCE.get().and_then(|s| s.access_token())
}

/// What can go wrong in a lookup here. Deliberately smaller than the account's
/// error type: nothing in this module can fail to *refresh* a token, because
/// nothing in it refreshes one.
#[derive(Error, Debug)]
pub enum SpotifyError {
    #[error("Authentication error: {0}")]
    AuthError(String),

    #[error("API error: {0}")]
    ApiError(String),

    #[error("No Spotify access token")]
    NoToken,
}

pub type Result<T> = std::result::Result<T, SpotifyError>;

/// The Spotify Web API root, overridable under `cfg(test)` so a test can
/// answer these requests from a stub on loopback rather than the network.
const SPOTIFY_API_ROOT: &str = "https://api.spotify.com/v1";

fn api_root() -> String {
    #[cfg(test)]
    if let Some(root) = testing::current_root() {
        return root;
    }
    SPOTIFY_API_ROOT.to_string()
}

/// The search URL for `query`, with the six filters Spotify's search syntax
/// takes folded into the query string as `field:value` pairs.
fn search_url(query: &str, types: &[&str], filters: Option<&serde_json::Value>) -> String {
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

/// Search Spotify for albums, artists or tracks, with `access_token` as the
/// bearer.
///
/// See: <https://developer.spotify.com/documentation/web-api/reference/search>
pub fn search(
    access_token: &str,
    query: &str,
    types: &[&str],
    filters: Option<&serde_json::Value>,
) -> Result<serde_json::Value> {
    use acr_http::http_client::new_http_client;

    let http_client = new_http_client(10);
    let url = search_url(query, types, filters);
    let headers = [
        ("Authorization", &format!("Bearer {}", access_token)[..]),
        ("Content-Type", "application/json"),
    ];
    http_client
        .get_json_with_headers(&url, &headers)
        .map_err(|e| SpotifyError::ApiError(format!("Failed to search: {}", e)))
}

/// Whether any of `track_ids` is in the user's saved tracks.
///
/// A lookup, not control, so it stays on this side with the favourites
/// provider that asks for it. Uses
/// <https://developer.spotify.com/documentation/web-api/reference/check-users-saved-tracks>,
/// which takes at most 50 ids per request.
pub fn check_saved_tracks(access_token: &str, track_ids: &[String]) -> Result<Option<bool>> {
    use acr_http::http_client::new_http_client;

    if track_ids.is_empty() {
        return Ok(Some(false));
    }

    let mut any_saved = false;

    for chunk in track_ids.chunks(50) {
        let http_client = new_http_client(10);
        let ids_param = chunk.join(",");
        let url = format!(
            "{}/me/tracks/contains?ids={}",
            api_root(),
            urlencoding::encode(&ids_param)
        );

        debug!("Checking saved tracks for IDs: {:?}", chunk);

        let headers = [
            ("Authorization", &format!("Bearer {}", access_token)[..]),
            ("Content-Type", "application/json"),
        ];

        let response = match http_client.get_json_with_headers(&url, &headers) {
            Ok(value) => value,
            Err(e) => {
                error!("Failed to check saved tracks: {}", e);
                return Err(SpotifyError::ApiError(format!(
                    "Failed to check saved tracks: {}",
                    e
                )));
            }
        };

        if let Some(saved_array) = response.as_array() {
            for is_saved in saved_array {
                if is_saved.as_bool() == Some(true) {
                    any_saved = true;
                }
            }
        } else {
            error!(
                "Unexpected response format from saved tracks API: {}",
                response
            );
            return Err(SpotifyError::ApiError(
                "Unexpected response format".to_string(),
            ));
        }
    }

    debug!("Final result: at least one track is saved = {}", any_saved);
    Ok(Some(any_saved))
}

/// Whether a song is in the user's saved tracks.
///
/// `Ok(None)` means Spotify knows no such track, which is a different answer
/// from "found it and it is not saved".
pub fn is_song_favourite(access_token: &str, artist: &str, title: &str) -> Result<Option<bool>> {
    debug!("Checking if song is favourite: '{}' by '{}'", title, artist);

    let query = format!("track:\"{}\" artist:\"{}\"", title, artist);
    let search_result = search(access_token, &query, &["track"], None)?;

    let mut track_ids = Vec::new();
    if let Some(tracks) = search_result
        .get("tracks")
        .and_then(|t| t.get("items"))
        .and_then(|i| i.as_array())
    {
        for track in tracks {
            if let Some(id) = track.get("id").and_then(|i| i.as_str()) {
                track_ids.push(id.to_string());
            }
        }
    }

    debug!("Found {} track IDs on Spotify", track_ids.len());

    if track_ids.is_empty() {
        debug!("No tracks found on Spotify for '{}' by '{}'", title, artist);
        return Ok(None);
    }

    check_saved_tracks(access_token, &track_ids)
}

/// Spotify Favourite Provider for integration with the favourites system
pub struct SpotifyFavouriteProvider;

impl Default for SpotifyFavouriteProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl SpotifyFavouriteProvider {
    pub fn new() -> Self {
        Self
    }
}

impl crate::favourites::FavouriteProvider for SpotifyFavouriteProvider {
    fn is_favourite(
        &self,
        song: &acr_types::song::Song,
    ) -> std::result::Result<bool, crate::favourites::FavouriteError> {
        let artist = song.artist.as_ref().ok_or_else(|| {
            crate::favourites::FavouriteError::InvalidSong("Artist is required".to_string())
        })?;
        let title = song.title.as_ref().ok_or_else(|| {
            crate::favourites::FavouriteError::InvalidSong("Title is required".to_string())
        })?;

        debug!("Checking if Spotify favourite: '{}' by '{}'", title, artist);

        let Some(token) = access_token() else {
            debug!("No Spotify access token available");
            return Err(crate::favourites::FavouriteError::NotConfigured(
                "No Spotify account is linked".to_string(),
            ));
        };

        match is_song_favourite(&token, artist, title) {
            Ok(Some(is_favourite)) => {
                debug!("Spotify favourite check result: {}", is_favourite);
                Ok(is_favourite)
            }
            Ok(None) => {
                debug!("Song not found on Spotify: '{}' by '{}'", title, artist);
                // Song not found on Spotify - treat as not favourite
                Ok(false)
            }
            Err(SpotifyError::AuthError(msg)) => {
                debug!("Spotify authentication error: {}", msg);
                Err(crate::favourites::FavouriteError::AuthError(msg))
            }
            Err(SpotifyError::ApiError(msg)) => {
                debug!("Spotify API error: {}", msg);
                Err(crate::favourites::FavouriteError::NetworkError(msg))
            }
            Err(SpotifyError::NoToken) => Err(crate::favourites::FavouriteError::NotConfigured(
                "No Spotify account is linked".to_string(),
            )),
        }
    }

    fn add_favourite(
        &self,
        _song: &acr_types::song::Song,
    ) -> std::result::Result<(), crate::favourites::FavouriteError> {
        // Spotify Web API doesn't provide an endpoint to add songs to saved tracks programmatically
        // The user would need to do this manually through the Spotify app or web player
        Err(crate::favourites::FavouriteError::Other("Adding songs to Spotify favourites is not supported via API - use Spotify app".to_string()))
    }

    fn remove_favourite(
        &self,
        _song: &acr_types::song::Song,
    ) -> std::result::Result<(), crate::favourites::FavouriteError> {
        // Spotify Web API doesn't provide an endpoint to remove songs from saved tracks programmatically
        // The user would need to do this manually through the Spotify app or web player
        Err(crate::favourites::FavouriteError::Other("Removing songs from Spotify favourites is not supported via API - use Spotify app".to_string()))
    }

    fn get_favourite_count(&self) -> Option<usize> {
        // Spotify API doesn't provide an efficient way to get total count of saved tracks
        // Would require paginating through all saved tracks which could be thousands
        None
    }

    fn provider_name(&self) -> &'static str {
        "spotify"
    }

    fn display_name(&self) -> &'static str {
        "Spotify"
    }

    /// Whether a Spotify account is linked, which this side can only learn by
    /// asking the player daemon.
    ///
    /// Before the account moved this refreshed the token itself; now it is a
    /// read of the 60 s-cached answer, so it costs at most one HTTP call a
    /// minute rather than one per question.
    fn is_enabled(&self) -> bool {
        access_token().is_some()
    }

    fn is_active(&self) -> bool {
        // For Spotify, active means we have valid authentication tokens and can make API calls
        // This is the same as is_enabled for Spotify
        self.is_enabled()
    }
}

/// Installing a token, and a stub API, for the duration of one test.
#[cfg(test)]
pub mod testing {
    use std::cell::RefCell;

    thread_local! {
        static TOKEN: RefCell<Option<String>> = const { RefCell::new(None) };
        static ROOT: RefCell<Option<String>> = const { RefCell::new(None) };
    }

    pub(super) fn current() -> Option<String> {
        TOKEN.with(|slot| slot.borrow().clone())
    }

    pub(super) fn current_root() -> Option<String> {
        ROOT.with(|slot| slot.borrow().clone())
    }

    /// Undoes both overrides when dropped, so nothing leaks into whatever the
    /// harness runs next on the same thread.
    pub struct Installed;

    impl Drop for Installed {
        fn drop(&mut self) {
            TOKEN.with(|slot| *slot.borrow_mut() = None);
            ROOT.with(|slot| *slot.borrow_mut() = None);
        }
    }

    /// Answer [`super::access_token`] with `token` and address the Spotify
    /// API at `root`, until the returned guard is dropped.
    #[must_use]
    pub fn with_token_and_api(token: &str, root: &str) -> Installed {
        TOKEN.with(|slot| *slot.borrow_mut() = Some(token.to_string()));
        ROOT.with(|slot| *slot.borrow_mut() = Some(root.to_string()));
        Installed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::favourites::FavouriteProvider;

    /// With no source installed there is no token, and every caller here
    /// degrades to contributing nothing rather than failing loudly.
    #[test]
    fn with_no_token_source_there_is_no_token() {
        assert_eq!(access_token(), None);
        assert!(!SpotifyFavouriteProvider::new().is_enabled());
    }

    /// The search URL this side builds is the one the account built before
    /// the split. The two are deliberately duplicated, so this is what says
    /// they still agree.
    #[test]
    fn the_search_url_is_the_one_the_account_built() {
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

    /// A token installed for a test does not outlive it.
    #[test]
    fn a_token_installed_for_a_test_does_not_outlive_it() {
        {
            // Not a real credential -- an obvious placeholder for the test.
            let _installed = testing::with_token_and_api("placeholder-token", "http://127.0.0.1:9");
            assert_eq!(access_token().as_deref(), Some("placeholder-token"));
        }
        assert_eq!(access_token(), None);
    }

    /// An empty id list is answered without a request: `Ok(Some(false))`,
    /// which is what the caller reads as "not a favourite".
    #[test]
    fn checking_no_track_ids_makes_no_request() {
        assert!(matches!(
            check_saved_tracks("placeholder-token", &[]),
            Ok(Some(false))
        ));
    }
}
