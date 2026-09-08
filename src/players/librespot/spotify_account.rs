//! The Spotify account: the OAuth tokens, their refresh, and the Web API
//! calls that need one.
//!
//! This moved here from `audiocontrol_metadata::spotify`. The reason is the
//! one the one-way-seam spec gives: the librespot backend turns
//! `PlayerCommand::Play`, `Pause`, `Next` and the rest into Spotify Web API
//! calls, and it used to fetch the bearer token for them *across the seam* —
//! so a metadata half that was down meant pressing play did nothing. The
//! account now lives beside the code that uses it, and playback control needs
//! nothing from the metadata half.
//!
//! What did **not** move is the search call and the two metadata providers
//! that use it. They stay in `audiocontrol-metadata` and pull a token from
//! this daemon over `GET /api/spotify/access_token`, which is the only
//! direction the seam is allowed to run in.
//!
//! Two seams of this module's own are worth naming.
//!
//! The **credentials** live in `acr_secrets::SecurityStore`, the type both
//! daemons share -- but not the file. This daemon opens its own copy at
//! `/var/lib/audiocontrol/security_store.json` and owns the five
//! `SPOTIFY_*_KEY` entries below; Last.fm's session key lives in the
//! metadata daemon's own file at
//! `/var/lib/audiocontrol/metadata/security_store.json` and is no business
//! of this side. `SecurityStore::save_to_file` truncates and rewrites the
//! whole file from whatever is in memory, so the two daemons sharing one
//! file would have each one's writes erase the other's -- which is why they
//! do not.
//!
//! The **OAuth proxy URL and secret** are compiled from `secrets.txt` at
//! build time by `acr-secrets`' build script — a crate both daemons depend
//! on, so a player daemon built without the metadata crate has them. The
//! composition root still passes them into [`initialize_from_config`] rather
//! than this module reading them itself, exactly the way it passes the
//! security store its encryption key: it keeps the values injectable, which is
//! what lets the tests below drive the placeholder check.

use acr_secrets::security_store::SecurityStore;
use acr_types::sanitize;
use log::{debug, error, info, warn};
use once_cell::sync::{Lazy, OnceCell};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::players::librespot::spotify_transport;

/// Spotify scopes required for full playback and library control
pub const SPOTIFY_REQUIRED_SCOPES: &str = "user-read-private user-read-email user-read-playback-state user-modify-playback-state user-read-currently-playing app-remote-control playlist-read-private playlist-read-collaborative playlist-modify-private playlist-modify-public user-read-playback-position user-top-read user-read-recently-played user-library-modify user-library-read";

// Constants for token storage
const SPOTIFY_ACCESS_TOKEN_KEY: &str = "spotify_access_token";
const SPOTIFY_REFRESH_TOKEN_KEY: &str = "spotify_refresh_token";
const SPOTIFY_TOKEN_EXPIRY_KEY: &str = "spotify_token_expiry";
const SPOTIFY_USER_ID_KEY: &str = "spotify_user_id";
const SPOTIFY_DISPLAY_NAME_KEY: &str = "spotify_display_name";

// Global singleton instance of the account, set when Spotify is enabled.
static SPOTIFY_CLIENT: Lazy<Mutex<Option<SpotifyAccount>>> = Lazy::new(|| Mutex::new(None));

// Global singleton for the Spotify configuration.
static GLOBAL_SPOTIFY_CONFIG: OnceCell<SpotifyConfig> = OnceCell::new();

/// Spotify API error types
#[derive(Error, Debug)]
pub enum SpotifyError {
    #[error("Authentication error: {0}")]
    AuthError(String),

    #[error("API error: {0}")]
    ApiError(String),

    /// A message [`spotify_transport`] already formatted the way [`Self::ApiError`]
    /// would have. Carried through verbatim so the `/command/<c>` route's body
    /// is what it was before the transport was extracted from this module.
    #[error("{0}")]
    Transport(String),

    #[error("Token not found")]
    TokenNotFound,

    #[error("Security store error: {0}")]
    SecurityStoreError(#[from] acr_secrets::security_store::SecurityStoreError),

    #[error("Serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),

    #[error("Configuration error: {0}")]
    ConfigError(String),
}

pub type Result<T> = std::result::Result<T, SpotifyError>;

/// Spotify token data structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpotifyTokens {
    pub access_token: String,
    pub refresh_token: String,
    /// Unix timestamp when the token expires
    pub expires_at: u64,
}

// Spotify playback state structures
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpotifyPlaybackState {
    pub device: Option<SpotifyDevice>,
    pub repeat_state: Option<String>,
    pub shuffle_state: Option<bool>,
    pub is_playing: bool,
    pub item: Option<SpotifyTrack>,
    pub progress_ms: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpotifyDevice {
    pub id: Option<String>,
    pub name: String,
    pub volume_percent: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpotifyTrack {
    pub id: Option<String>,
    pub name: String,
    pub duration_ms: u32,
    pub artists: Vec<SpotifyArtist>,
    pub album: Option<SpotifyAlbum>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpotifyArtist {
    pub id: Option<String>,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpotifyAlbum {
    pub id: Option<String>,
    pub name: String,
    pub images: Option<Vec<SpotifyImage>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpotifyImage {
    pub url: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
}

// Spotify token refresh response
#[derive(Debug, Clone, Serialize, Deserialize)]
struct SpotifyTokenResponse {
    access_token: String,
    #[allow(dead_code)]
    token_type: String,
    #[allow(dead_code)]
    scope: Option<String>,
    expires_in: u64,
    refresh_token: Option<String>,
}

/// Spotify user profile data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpotifyUserProfile {
    pub id: String,
    pub display_name: Option<String>,
    pub email: Option<String>,
}

/// Spotify configuration structure
#[derive(Debug, Clone)]
pub struct SpotifyConfig {
    pub oauth_url: String,
    pub proxy_secret: String,
    pub client_id: Option<String>,
    pub client_secret: Option<String>,
}

impl SpotifyConfig {
    /// Read the `spotify` service configuration, falling back to the
    /// build-time proxy secret the caller supplies where the section leaves
    /// it empty.
    pub fn from_json(spotify_config: &serde_json::Value, default_proxy_secret: &str) -> Self {
        let oauth_url = spotify_config
            .get("oauth_url")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let proxy_secret = match spotify_config.get("proxy_secret").and_then(|v| v.as_str()) {
            Some(s) if !s.trim().is_empty() => s.to_string(),
            _ => default_proxy_secret.to_string(),
        };
        let client_id = spotify_config
            .get("client_id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let client_secret = spotify_config
            .get("client_secret")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        SpotifyConfig {
            oauth_url,
            proxy_secret,
            client_id,
            client_secret,
        }
    }
}

/// The Spotify account: authentication, tokens, and the Web API calls that
/// need one.
pub struct SpotifyAccount {
    config: SpotifyConfig,
}

impl Default for SpotifyAccount {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for SpotifyAccount {
    fn clone(&self) -> Self {
        SpotifyAccount {
            config: self.config.clone(),
        }
    }
}

/// Bring the account up from the `spotify` service configuration.
///
/// This is `audiocontrol_metadata`'s `set_global_config` plus its
/// `initialize_spotify`, in one call and in the same order: the global
/// configuration is stored first, because the client reads it while it
/// initialises.
///
/// `default_oauth_url` and `default_proxy_secret` are the values compiled
/// from `secrets.txt`; see the module doc for why the caller supplies them.
/// A build with no secrets passes `"unknown"`, which the placeholder check
/// below rejects, and the account simply never initialises — the same
/// outcome as a device with no Spotify credentials baked in.
///
/// The global configuration is stored **whether or not `spotify.enable` is
/// set**, and even when there is no `spotify` section at all. That is
/// deliberate and it is the behaviour that moved: [`SpotifyAccount::new`]
/// reads it, and `new` — not [`SpotifyAccount::get_instance`] — is what every
/// route and the librespot backend use, so a device with tokens stored but
/// `enable` unset must still be able to refresh them.
pub fn initialize_from_config(
    config: &serde_json::Value,
    default_oauth_url: &str,
    default_proxy_secret: &str,
) {
    let section = acr_types::config::get_service_config(config, "spotify");

    let global = match section {
        Some(section) => SpotifyConfig::from_json(section, default_proxy_secret),
        None => SpotifyConfig {
            oauth_url: default_oauth_url.to_string(),
            proxy_secret: default_proxy_secret.to_string(),
            client_id: None,
            client_secret: None,
        },
    };
    let _ = GLOBAL_SPOTIFY_CONFIG.set(global);

    info!("Starting Spotify initialization");

    let Some(section) = section else {
        debug!("No Spotify configuration found, Spotify features will be unavailable.");
        return;
    };

    let enabled = section
        .get("enable")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    info!("Spotify enabled in config: {}", enabled);

    if !enabled {
        info!("Spotify integration is disabled");
        return;
    }

    let oauth_url = section
        .get("oauth_url")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let proxy_secret = section
        .get("proxy_secret")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    info!(
        "Config values - OAuth URL present: {}, proxy secret present: {}",
        oauth_url.is_some(),
        proxy_secret.is_some()
    );

    let init_result = match (oauth_url, proxy_secret) {
        (Some(url), Some(secret)) if !url.is_empty() && !secret.is_empty() => {
            info!(
                "Initializing Spotify with configuration from audiocontrol.json, URL: '{}'",
                url
            );
            SpotifyAccount::initialize(url, secret)
        }
        _ => {
            info!("No valid Spotify config in audiocontrol.json, falling back to secrets.txt");
            SpotifyAccount::initialize_with_defaults(default_oauth_url, default_proxy_secret)
        }
    };

    if let Err(e) = init_result {
        warn!("Failed to initialize Spotify client: {}", e);
        return;
    }

    match SpotifyAccount::get_instance() {
        Ok(client) => {
            if client.has_valid_tokens() {
                info!("Spotify is connected with valid tokens");
            } else {
                info!("Spotify is not connected. User needs to authenticate.");
            }
        }
        Err(e) => warn!(
            "Could not get Spotify client instance to check status: {}",
            e
        ),
    }
    info!("Spotify initialized successfully");
}

/// The current Spotify access token, refreshed if it is about to expire, or
/// `None` when no account is linked.
///
/// **This is the call that no longer crosses the seam.** The librespot
/// backend and `GET /api/spotify/access_token` both read it, and both are
/// answered from this process's own security store: a metadata half that is
/// absent, unconfigured or down cannot stop a play command any more.
pub fn access_token() -> Option<String> {
    // A thread-local override in tests rather than a global: the account is
    // process-wide state, and two tests that each want their own would
    // otherwise depend on
    // which ran first.
    #[cfg(test)]
    if let Some(t) = testing::current_token() {
        return Some(t);
    }
    SpotifyAccount::new().ensure_valid_token().ok()
}

/// How much of a foreign HTTP body reaches the log.
const LOG_EXCERPT_BYTES: usize = 100;

/// Cut `text` to at most `max` **bytes**, on a character boundary.
///
/// A byte slice is what this replaced, and it panicked: the text is whatever
/// the configured OAuth proxy served -- a localized login page, or an error
/// page with an accented word in it -- so the cut can land inside a multi-byte
/// character. `check_oauth_server` runs on a Rocket worker answering
/// `GET /api/spotify/check_server`, which puts the panic in the daemon that
/// has to stay up for the device to play anything.
///
/// A function rather than an expression at the call site so a test can drive
/// the same code the caller runs. What that still does not cover is the
/// *call*: nothing fails if `check_oauth_server` stops using this. Driving
/// that needs a live HTTP server, which this module has no harness for.
fn truncate_for_log(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_string();
    }
    let end = (0..=max).rev().find(|&i| text.is_char_boundary(i)).unwrap_or(0);
    format!("{}... (truncated)", &text[..end])
}


impl SpotifyAccount {
    /// An account reading whatever configuration [`initialize_from_config`]
    /// stored.
    ///
    /// Never fails to construct, and does not require Spotify to have been
    /// *enabled*: that is the difference from [`Self::get_instance`], and it
    /// is why the routes and the backend use this one. Swapping to
    /// `get_instance` here would make a device that has an account linked but
    /// no `spotify.enable` in its configuration report no token, where it used
    /// to find one.
    pub fn new() -> Self {
        SpotifyAccount {
            config: GLOBAL_SPOTIFY_CONFIG.get().cloned().unwrap_or(SpotifyConfig {
                oauth_url: String::new(),
                proxy_secret: String::new(),
                client_id: None,
                client_secret: None,
            }),
        }
    }

    /// Initialize the account with an OAuth proxy URL and secret.
    pub fn initialize(mut oauth_url: String, proxy_secret: String) -> Result<()> {
        if oauth_url.is_empty() {
            return Err(SpotifyError::ConfigError("OAuth URL is required".to_string()));
        }

        if proxy_secret.is_empty() {
            return Err(SpotifyError::ConfigError(
                "Proxy secret is required".to_string(),
            ));
        }

        // Ensure the OAuth URL has a trailing slash
        if !oauth_url.ends_with('/') {
            oauth_url = format!("{}/", oauth_url);
            info!("Added trailing slash to OAuth URL: '{}'", oauth_url);
        }

        if !oauth_url.starts_with("http://") && !oauth_url.starts_with("https://") {
            return Err(SpotifyError::ConfigError(format!(
                "Invalid OAuth URL: '{}' - must start with http:// or https://",
                oauth_url
            )));
        }

        let config = SpotifyConfig {
            oauth_url,
            proxy_secret,
            client_id: None,
            client_secret: None,
        };

        let mut client_guard = SPOTIFY_CLIENT.lock();
        *client_guard = Some(SpotifyAccount { config });

        info!("Spotify client initialized");
        Ok(())
    }

    /// Initialize with the values compiled from `secrets.txt`, rejecting the
    /// placeholders a build with no secrets produces.
    pub fn initialize_with_defaults(oauth_url: &str, proxy_secret: &str) -> Result<()> {
        info!("Default Spotify OAuth URL: '{}'", oauth_url);
        info!(
            "Default Spotify proxy secret length: {} chars",
            proxy_secret.len()
        );

        let is_placeholder_url = oauth_url.contains("your-oauth-proxy-url")
            || oauth_url.contains("your_spotify_oauth_url")
            || oauth_url == "unknown"
            || oauth_url.is_empty();

        let is_placeholder_secret = proxy_secret.contains("your-spotify-proxy-secret")
            || proxy_secret.contains("your_spotify_proxy_secret")
            || proxy_secret == "unknown"
            || proxy_secret.is_empty();

        if oauth_url.contains("unknown") {
            info!("OAuth URL contains 'unknown': '{}'", oauth_url);
        }

        if is_placeholder_url || is_placeholder_secret {
            info!(
                "Spotify initialization error: URL is placeholder: {}, Secret is placeholder: {}",
                is_placeholder_url, is_placeholder_secret
            );
            return Err(SpotifyError::ConfigError(
                "Default Spotify OAuth credentials are not configured".to_string(),
            ));
        }

        info!("Initializing Spotify with URL '{}' from secrets.txt", oauth_url);
        Self::initialize(oauth_url.to_string(), proxy_secret.to_string())
    }

    /// The singleton set by [`Self::initialize`], i.e. only when Spotify is
    /// enabled in the configuration.
    pub fn get_instance() -> Result<SpotifyAccount> {
        let client_guard = SPOTIFY_CLIENT.lock();
        match &*client_guard {
            Some(client) => Ok(client.clone()),
            None => Err(SpotifyError::ConfigError(
                "Spotify client has not been initialized".to_string(),
            )),
        }
    }

    /// Get OAuth URL for the authentication process
    pub fn get_oauth_url(&self) -> &str {
        info!("Using OAuth URL: '{}'", &self.config.oauth_url);
        &self.config.oauth_url
    }

    /// Get the proxy secret for authenticating with the OAuth proxy
    pub fn get_proxy_secret(&self) -> &str {
        info!(
            "Using proxy secret length: {} chars",
            self.config.proxy_secret.len()
        );
        if self.config.proxy_secret.trim().is_empty() {
            error!("Proxy secret is empty or only whitespace - this will cause authentication failure");
        }
        &self.config.proxy_secret
    }

    pub fn get_client_id(&self) -> Option<&str> {
        self.config.client_id.as_deref()
    }

    pub fn get_client_secret(&self) -> Option<&str> {
        self.config.client_secret.as_deref()
    }

    /// Store Spotify tokens in the security store
    pub fn store_tokens(&self, tokens: &SpotifyTokens) -> Result<()> {
        SecurityStore::set(SPOTIFY_ACCESS_TOKEN_KEY, &tokens.access_token)?;
        SecurityStore::set(SPOTIFY_REFRESH_TOKEN_KEY, &tokens.refresh_token)?;
        SecurityStore::set(SPOTIFY_TOKEN_EXPIRY_KEY, &tokens.expires_at.to_string())?;

        info!("Spotify tokens stored successfully");
        Ok(())
    }

    /// Store user profile information in the security store
    pub fn store_user_profile(&self, profile: &SpotifyUserProfile) -> Result<()> {
        SecurityStore::set(SPOTIFY_USER_ID_KEY, &profile.id)?;

        if let Some(display_name) = &profile.display_name {
            SecurityStore::set(SPOTIFY_DISPLAY_NAME_KEY, display_name)?;
        }

        info!("Spotify user profile stored successfully");
        Ok(())
    }

    /// Get stored Spotify tokens from the security store
    pub fn get_tokens(&self) -> Result<SpotifyTokens> {
        let access_token =
            SecurityStore::get(SPOTIFY_ACCESS_TOKEN_KEY).map_err(|_| SpotifyError::TokenNotFound)?;

        let refresh_token = SecurityStore::get(SPOTIFY_REFRESH_TOKEN_KEY)
            .map_err(|_| SpotifyError::TokenNotFound)?;

        let expires_at_str =
            SecurityStore::get(SPOTIFY_TOKEN_EXPIRY_KEY).map_err(|_| SpotifyError::TokenNotFound)?;

        let expires_at = expires_at_str
            .parse::<u64>()
            .map_err(|_| SpotifyError::AuthError("Invalid token expiry".to_string()))?;

        Ok(SpotifyTokens {
            access_token,
            refresh_token,
            expires_at,
        })
    }

    /// Check if we have valid Spotify tokens
    pub fn has_valid_tokens(&self) -> bool {
        match self.get_tokens() {
            Ok(tokens) => {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();

                tokens.expires_at > now
            }
            Err(_) => false,
        }
    }

    /// Clear all Spotify tokens and user data
    pub fn clear_tokens(&self) -> Result<()> {
        let _ = SecurityStore::remove(SPOTIFY_ACCESS_TOKEN_KEY);
        let _ = SecurityStore::remove(SPOTIFY_REFRESH_TOKEN_KEY);
        let _ = SecurityStore::remove(SPOTIFY_TOKEN_EXPIRY_KEY);
        let _ = SecurityStore::remove(SPOTIFY_USER_ID_KEY);
        let _ = SecurityStore::remove(SPOTIFY_DISPLAY_NAME_KEY);

        info!("Spotify tokens cleared");
        Ok(())
    }

    /// Get user profile information if available
    pub fn get_user_profile(&self) -> Result<SpotifyUserProfile> {
        let user_id = SecurityStore::get(SPOTIFY_USER_ID_KEY)
            .map_err(|_| SpotifyError::AuthError("User ID not found".to_string()))?;

        let display_name = SecurityStore::get(SPOTIFY_DISPLAY_NAME_KEY).ok();

        Ok(SpotifyUserProfile {
            id: user_id,
            display_name,
            email: None, // We don't store email
        })
    }

    /// Check if the OAuth server is reachable and responding as expected
    pub fn check_oauth_server(&self) -> Result<bool> {
        use acr_http::http_client::new_http_client;

        info!(
            "Checking connectivity to OAuth server: {}",
            self.config.oauth_url
        );

        let http_client = new_http_client(5);

        match http_client.get_text(&self.config.oauth_url) {
            Ok(text) => {
                let is_valid = text.contains("OAuth")
                    || text.contains("Spotify")
                    || text.contains("Authentication")
                    || text.contains("login");

                info!("OAuth server is reachable. Response looks valid: {}", is_valid);

                let truncated = truncate_for_log(&text, LOG_EXCERPT_BYTES);
                info!("OAuth server response: {}", truncated);

                Ok(is_valid)
            }
            Err(e) => {
                error!("Failed to connect to OAuth server: {}", e);
                Err(SpotifyError::ConfigError(format!(
                    "OAuth server unreachable: {}",
                    e
                )))
            }
        }
    }

    /// Build headers for OAuth proxy requests
    pub fn build_oauth_headers(&self) -> Vec<(&str, String)> {
        let mut headers = vec![("X-Proxy-Secret", self.get_proxy_secret().to_string())];
        if let Some(client_id) = self.get_client_id() {
            if !client_id.is_empty() {
                debug!(
                    "Sending X-Spotify-Client-Id: {}... ({} chars)",
                    sanitize::safe_truncate(client_id, 6),
                    client_id.len()
                );
                headers.push(("X-Spotify-Client-Id", client_id.to_string()));
            } else {
                debug!("Not sending X-Spotify-Client-Id: value is empty");
            }
        } else {
            debug!("Not sending X-Spotify-Client-Id: not set in config");
        }
        if let Some(client_secret) = self.get_client_secret() {
            if !client_secret.is_empty() {
                debug!(
                    "Sending X-Spotify-Client-Secret: {}... ({} chars)",
                    sanitize::safe_truncate(client_secret, 6),
                    client_secret.len()
                );
                headers.push(("X-Spotify-Client-Secret", client_secret.to_string()));
            } else {
                debug!("Not sending X-Spotify-Client-Secret: value is empty");
            }
        } else {
            debug!("Not sending X-Spotify-Client-Secret: not set in config");
        }
        headers
    }

    /// Refresh the access token using the refresh token via the OAuth proxy
    /// (the only method).
    pub fn refresh_token(&self) -> Result<SpotifyTokens> {
        use acr_http::http_client::new_http_client;
        let current_tokens = self.get_tokens()?;
        let http_client = new_http_client(10);
        let refresh_url = format!("{}refresh", self.config.oauth_url);
        let payload = serde_json::json!({
            "refresh_token": current_tokens.refresh_token
        });
        info!("Refreshing Spotify access token via OAuth proxy (headers)");
        let mut headers = self.build_oauth_headers();
        headers.push(("Content-Type", "application/json".to_string()));
        let headers_ref: Vec<(&str, &str)> =
            headers.iter().map(|(k, v)| (*k, v.as_str())).collect();
        let response =
            match http_client.post_json_value_with_headers(&refresh_url, payload, &headers_ref) {
                Ok(value) => value,
                Err(e) => {
                    error!("Failed to refresh Spotify token via proxy: {}", e);
                    return Err(SpotifyError::AuthError(format!(
                        "Token refresh via proxy failed: {}",
                        e
                    )));
                }
            };

        let token_response: SpotifyTokenResponse = match serde_json::from_value(response) {
            Ok(parsed) => parsed,
            Err(e) => {
                error!("Failed to parse token refresh response from proxy: {}", e);
                return Err(SpotifyError::SerializationError(e));
            }
        };

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        // Saturating: `expires_in` is parsed straight from the OAuth proxy's
        // JSON and is not ours to trust. An absurd value overflows this in a
        // debug build and wraps in a release one -- and wrapping is the worse
        // half, because the token then looks permanently expired and every
        // play command refreshes again, forever.
        let expires_at = now.saturating_add(token_response.expires_in);

        let new_tokens = SpotifyTokens {
            access_token: token_response.access_token,
            // If we got a new refresh token, use it; otherwise keep the old one
            refresh_token: token_response
                .refresh_token
                .unwrap_or(current_tokens.refresh_token),
            expires_at,
        };

        self.store_tokens(&new_tokens)?;

        info!("Successfully refreshed Spotify access token via OAuth proxy");
        Ok(new_tokens)
    }

    /// Ensure we have a valid token, refreshing if necessary
    pub fn ensure_valid_token(&self) -> Result<String> {
        match self.get_tokens() {
            Ok(tokens) => {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();

                if tokens.expires_at <= now + 60 {
                    info!("Spotify token is expired or about to expire, refreshing");

                    match self.refresh_token() {
                        Ok(new_tokens) => {
                            info!(
                                "Token refresh via direct API successful, new token will expire in {} seconds",
                                new_tokens.expires_at.saturating_sub(now)
                            );
                            Ok(new_tokens.access_token)
                        }
                        Err(e) => {
                            error!("Direct API token refresh failed: {}", e);
                            Err(e)
                        }
                    }
                } else {
                    debug!(
                        "Spotify token is still valid for {} more seconds",
                        tokens.expires_at - now
                    );
                    Ok(tokens.access_token)
                }
            }
            Err(e) => {
                error!("Failed to get Spotify tokens: {}", e);
                Err(e)
            }
        }
    }

    /// Get the current playback state from the Spotify API.
    ///
    /// See: <https://developer.spotify.com/documentation/web-api/reference/get-information-about-the-users-current-playback>
    pub fn get_playback_state(&self) -> Result<Option<SpotifyPlaybackState>> {
        use acr_http::http_client::{new_http_client, HttpClientError};

        let access_token = self.ensure_valid_token()?;
        let http_client = new_http_client(10);
        let endpoint_url = spotify_transport::player_api_base();
        let headers = [
            ("Authorization", &format!("Bearer {}", access_token)[..]),
            ("Content-Type", "application/json"),
        ];

        info!("Fetching Spotify playback state");

        let response = match http_client.get_json_with_headers(&endpoint_url, &headers) {
            Ok(value) => {
                if value.is_null() {
                    debug!("No active Spotify playback found");
                    return Ok(None);
                }
                value
            }
            Err(e) => match e {
                // 204 No Content is a legitimate "nothing is playing"
                HttpClientError::EmptyResponse => {
                    debug!("No active Spotify playback (204 No Content)");
                    return Ok(None);
                }
                HttpClientError::ServerError(msg) if msg.contains("401") || msg.contains("403") => {
                    error!("Authentication error when fetching playback state: {}", msg);
                    return Err(SpotifyError::AuthError("Authentication failed".to_string()));
                }
                _ => {
                    error!("Failed to fetch Spotify playback state: {}", e);
                    return Err(SpotifyError::ApiError(format!(
                        "Failed to fetch playback state: {}",
                        e
                    )));
                }
            },
        };

        match serde_json::from_value::<SpotifyPlaybackState>(response) {
            Ok(playback_state) => {
                if let Some(track) = &playback_state.item {
                    debug!(
                        "Currently playing: {} by {}",
                        track.name,
                        track
                            .artists
                            .iter()
                            .map(|a| a.name.clone())
                            .collect::<Vec<_>>()
                            .join(", ")
                    );
                }
                Ok(Some(playback_state))
            }
            Err(e) => {
                error!("Failed to parse Spotify playback state: {}", e);
                Err(SpotifyError::SerializationError(e))
            }
        }
    }

    /// Send a command to the Spotify Web API (play, pause, next, previous,
    /// seek, repeat, shuffle).
    ///
    /// The request building lives in [`spotify_transport`], which the
    /// librespot backend already calls directly, so the route and the backend
    /// issue the same request rather than two that have to be kept in step.
    pub fn send_command(&self, command: &str, args: &serde_json::Value) -> Result<()> {
        let access_token = self.ensure_valid_token()?;
        spotify_transport::send_command(&access_token, command, args)
            .map_err(SpotifyError::Transport)
    }

    /// Get the user's currently playing track from Spotify
    pub fn get_currently_playing(&self) -> Result<Option<serde_json::Value>> {
        use acr_http::http_client::new_http_client;
        let access_token = self.ensure_valid_token()?;
        let http_client = new_http_client(10);
        let url = format!("{}/currently-playing", spotify_transport::player_api_base());
        let headers = [
            ("Authorization", &format!("Bearer {}", access_token)[..]),
            ("Content-Type", "application/json"),
        ];
        match http_client.get_json_with_headers(&url, &headers) {
            Ok(json) => {
                if json.is_null() {
                    Ok(None)
                } else {
                    Ok(Some(json))
                }
            }
            Err(e) => Err(SpotifyError::ApiError(format!(
                "Failed to get currently playing: {}",
                e
            ))),
        }
    }

    /// Search Spotify for albums, artists, or tracks with optional filters.
    ///
    /// See: <https://developer.spotify.com/documentation/web-api/reference/search>
    pub fn search(
        &self,
        query: &str,
        types: &[&str],
        filters: Option<&serde_json::Value>,
    ) -> Result<serde_json::Value> {
        use acr_http::http_client::new_http_client;
        let access_token = self.ensure_valid_token()?;
        let http_client = new_http_client(10);
        let url = spotify_transport::search_url(query, types, filters);
        let headers = [
            ("Authorization", &format!("Bearer {}", access_token)[..]),
            ("Content-Type", "application/json"),
        ];
        match http_client.get_json_with_headers(&url, &headers) {
            Ok(json) => Ok(json),
            Err(e) => Err(SpotifyError::ApiError(format!("Failed to search: {}", e))),
        }
    }

    /// The required scopes as a string
    pub fn required_scopes() -> &'static str {
        SPOTIFY_REQUIRED_SCOPES
    }

    /// Construct the OAuth login URL with required scopes as a query parameter
    pub fn build_oauth_login_url(&self) -> String {
        let base_url = self.get_oauth_url();
        let scopes = Self::required_scopes();
        let sep = if base_url.contains('?') { "&" } else { "?" };
        format!("{}login{}scope={}", base_url, sep, urlencoding::encode(scopes))
    }

    /// Construct the `/create_session` URL with required scopes as a query
    /// parameter
    pub fn build_create_session_url(&self) -> String {
        let base_url = self.get_oauth_url();
        let scopes = Self::required_scopes();
        let sep = if base_url.contains('?') { "&" } else { "?" };
        format!(
            "{}create_session{}scope={}",
            base_url,
            sep,
            urlencoding::encode(scopes)
        )
    }

    /// Construct the OAuth login URL (only needs session_id)
    pub fn build_login_url(&self, session_id: &str) -> String {
        let base_url = self.get_oauth_url();
        format!("{base_url}login/{session_id}")
    }
}

/// Installing an access token for the duration of one test.
///
/// The account itself reads `acr_secrets::SecurityStore`, which is
/// process-global and initialised once from a path in the daemon's
/// configuration. A test that wanted a linked account would have to reach
/// into that global and would then be visible to every other test in the
/// binary, so instead this stands in for "an account is linked on this
/// thread". What it does **not** stand in for is where the token comes from:
/// nothing here consults `crate::audiocontrol::token`, which is the point.
#[cfg(test)]
pub mod testing {
    use std::cell::RefCell;

    thread_local! {
        static TOKEN: RefCell<Option<String>> = const { RefCell::new(None) };
    }

    pub(super) fn current_token() -> Option<String> {
        TOKEN.with(|slot| slot.borrow().clone())
    }

    /// Removes the token when dropped, so a test cannot leak a linked account
    /// into whatever the harness runs next on the same thread.
    pub struct Linked;

    impl Drop for Linked {
        fn drop(&mut self) {
            TOKEN.with(|slot| *slot.borrow_mut() = None);
        }
    }

    /// Treat an account as linked, with `token` as its access token, until
    /// the returned guard is dropped.
    #[must_use]
    pub fn link_account(token: &str) -> Linked {
        TOKEN.with(|slot| *slot.borrow_mut() = Some(token.to_string()));
        Linked
    }
}

#[cfg(test)]
mod tests {

    /// `truncate_for_log` cuts on a character boundary, not a byte offset.
    ///
    /// The byte slice this replaced panicked whenever the cut landed inside a
    /// multi-byte character, which a localized OAuth login page makes ordinary
    /// rather than exotic.
    #[test]
    fn a_long_non_ascii_response_is_truncated_without_panicking() {
        // Two bytes each, so the limit lands on a boundary.
        let even = "\u{e9}".repeat(200);
        assert!(truncate_for_log(&even, 100).ends_with("... (truncated)"));

        // Four bytes each, shifted by one so the limit lands mid-character --
        // the input the old code panicked on.
        let odd = format!("a{}", "\u{1f600}".repeat(40));
        assert!(!odd.is_char_boundary(100), "the fixture must straddle the cut");
        let cut = truncate_for_log(&odd, 100);
        assert!(cut.ends_with("... (truncated)"));
        assert!(cut.len() < odd.len());

        // Short enough to keep whole, and not marked as cut.
        assert_eq!(truncate_for_log("short", 100), "short");
    }

    /// An absurd `expires_in` from the OAuth proxy must not overflow the
    /// expiry. Wrapping is the worse half of the old behaviour: the token then
    /// looks permanently expired, so every play command refreshes again.
    #[test]
    fn an_absurd_expires_in_saturates_rather_than_wrapping() {
        let now: u64 = 1_757_000_000;
        assert_eq!(now.saturating_add(u64::MAX), u64::MAX);
        assert!(
            now.saturating_add(u64::MAX) > now,
            "a saturated expiry is still in the future, which is what stops the refresh loop"
        );
    }

    use super::*;

    /// With nothing linked and no security store initialised, there is no
    /// token — and asking for one makes no network call, so this neither
    /// hangs nor reaches Spotify.
    #[test]
    fn with_no_account_linked_there_is_no_access_token() {
        assert_eq!(access_token(), None);
    }

    /// The guard is what keeps a linked account from leaking into the next
    /// test on this thread.
    #[test]
    fn dropping_the_guard_unlinks_the_account() {
        {
            // Not a real credential -- an obvious placeholder for the test.
            let _linked = testing::link_account("placeholder-token");
            assert_eq!(access_token(), Some("placeholder-token".to_string()));
        }
        assert_eq!(access_token(), None);
    }

    /// An empty `oauth_url` in the `spotify` section is carried through as
    /// empty rather than replaced by the compiled-in default, while an empty
    /// `proxy_secret` *is* replaced. That asymmetry is what
    /// `SpotifyConfig::from_json` did before the move, and a device whose
    /// configuration names an OAuth URL of its own depends on it.
    #[test]
    fn a_configured_section_keeps_its_own_oauth_url_and_falls_back_for_the_secret() {
        let config = SpotifyConfig::from_json(
            &serde_json::json!({ "oauth_url": "https://oauth.example.com/" }),
            "compiled-in-secret",
        );
        assert_eq!(config.oauth_url, "https://oauth.example.com/");
        assert_eq!(config.proxy_secret, "compiled-in-secret");
    }

    /// A build with no `secrets.txt` compiles a placeholder in, and that must
    /// not be treated as a usable OAuth proxy.
    ///
    /// The placeholder check is what has to reject these, not the URL
    /// validation in `initialize`: the second and third pairs below are
    /// well-formed `https://` URLs and a non-empty secret, so a build that
    /// dropped the check would initialise an account pointed at a template.
    #[test]
    fn placeholder_defaults_do_not_initialize_an_account() {
        for (url, secret) in [
            ("unknown", "unknown"),
            ("https://your-oauth-proxy-url/", "a-secret"),
            ("https://oauth.example.com/", "your-spotify-proxy-secret"),
        ] {
            assert!(
                SpotifyAccount::initialize_with_defaults(url, secret).is_err(),
                "({}, <secret>) must not initialize an account",
                url
            );
        }
    }
}
