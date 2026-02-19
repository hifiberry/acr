// Spotify helper functions for ACR
// This module provides functionality for authenticating with Spotify
// and managing tokens through the OAuth2 flow

/// Spotify scopes required for full playback and library control
pub const SPOTIFY_REQUIRED_SCOPES: &str = "user-read-private user-read-email user-read-playback-state user-modify-playback-state user-read-currently-playing app-remote-control playlist-read-private playlist-read-collaborative playlist-modify-private playlist-modify-public user-read-playback-position user-top-read user-read-recently-played user-library-modify user-library-read";

use log::{debug, error, info};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use once_cell::sync::{Lazy, OnceCell};
use parking_lot::Mutex;

use crate::helpers::security_store::SecurityStore;
use crate::helpers::sanitize;

// Constants for token storage
const SPOTIFY_ACCESS_TOKEN_KEY: &str = "spotify_access_token";
const SPOTIFY_REFRESH_TOKEN_KEY: &str = "spotify_refresh_token";
const SPOTIFY_TOKEN_EXPIRY_KEY: &str = "spotify_token_expiry";
const SPOTIFY_USER_ID_KEY: &str = "spotify_user_id";
const SPOTIFY_DISPLAY_NAME_KEY: &str = "spotify_display_name";

// Global singleton instance of Spotify client
pub(crate) static SPOTIFY_CLIENT: Lazy<Mutex<Option<Spotify>>> = Lazy::new(|| Mutex::new(None));

// Global singleton for Spotify config
static GLOBAL_SPOTIFY_CONFIG: OnceCell<SpotifyConfig> = OnceCell::new();

// Default Spotify OAuth URL and proxy secret compiled from secrets.txt at build time
#[cfg(not(test))]
pub fn default_spotify_oauth_url() -> String {
    crate::secrets::spotify_oauth_url()
}

#[cfg(not(test))]
pub fn default_spotify_proxy_secret() -> String {
    crate::secrets::spotify_proxy_secret()
}

// Test credentials (placeholders for tests)
#[cfg(test)]
pub fn default_spotify_oauth_url() -> String {
    "https://test.oauth.example.com/spotify/".to_string()
}

#[cfg(test)]
pub fn default_spotify_proxy_secret() -> String {
    "test_proxy_secret".to_string()
}

// Spotify API error types
#[derive(Error, Debug)]
pub enum SpotifyError {
    #[error("Authentication error: {0}")]
    AuthError(String),
    
    #[error("API error: {0}")]
    ApiError(String),
    
    #[error("Token not found")]
    TokenNotFound,
    
    #[error("Security store error: {0}")]
    SecurityStoreError(#[from] crate::helpers::security_store::SecurityStoreError),
    
    #[error("Serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),
    
    #[error("Configuration error: {0}")]
    ConfigError(String),
}

pub type Result<T> = std::result::Result<T, SpotifyError>;

// Spotify token data structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpotifyTokens {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at: u64,  // Unix timestamp when the token expires
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
    token_type: String,
    scope: Option<String>,
    expires_in: u64,
    refresh_token: Option<String>,
}

// Spotify user profile data
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

/// Spotify helper class for managing authentication and tokens
pub struct Spotify {
    config: SpotifyConfig,
}

impl Default for Spotify {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for Spotify {
    fn clone(&self) -> Self {
        Spotify {
            config: self.config.clone(),
        }
    }
}

impl SpotifyConfig {
    pub fn from_json(spotify_config: &serde_json::Value) -> Self {
        let oauth_url = spotify_config.get("oauth_url").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let proxy_secret = match spotify_config.get("proxy_secret").and_then(|v| v.as_str()) {
            Some(s) if !s.trim().is_empty() => s.to_string(),
            _ => default_spotify_proxy_secret(),
        };
        let client_id = spotify_config.get("client_id").and_then(|v| v.as_str()).map(|s| s.to_string());
        let client_secret = spotify_config.get("client_secret").and_then(|v| v.as_str()).map(|s| s.to_string());
        SpotifyConfig { oauth_url, proxy_secret, client_id, client_secret }
    }
}

impl Spotify {
    /// Create a new Spotify helper instance with default configuration
    pub fn new() -> Self {
        Spotify {
            config: GLOBAL_SPOTIFY_CONFIG.get().cloned().unwrap_or_else(|| SpotifyConfig {
                oauth_url: crate::helpers::spotify::default_spotify_oauth_url(),
                proxy_secret: crate::helpers::spotify::default_spotify_proxy_secret(),
                client_id: None,
                client_secret: None,
            }),
        }
    }    /// Initialize the Spotify client with OAuth configuration
    pub fn initialize(mut oauth_url: String, proxy_secret: String) -> Result<()> {
        if oauth_url.is_empty() {
            return Err(SpotifyError::ConfigError("OAuth URL is required".to_string()));
        }
        
        if proxy_secret.is_empty() {
            return Err(SpotifyError::ConfigError("Proxy secret is required".to_string()));
        }
        
        // Ensure the OAuth URL has a trailing slash
        if !oauth_url.ends_with('/') {
            info!("Adding trailing slash to OAuth URL: '{}' -> '{}'", oauth_url, format!("{}/", oauth_url));
            oauth_url = format!("{}/", oauth_url);
        }
        
        // Ensure the OAuth URL starts with http:// or https://
        if !oauth_url.starts_with("http://") && !oauth_url.starts_with("https://") {
            return Err(SpotifyError::ConfigError(format!("Invalid OAuth URL: '{}' - must start with http:// or https://", oauth_url)));
        }
        
        let config = SpotifyConfig {
            oauth_url,
            proxy_secret,
            client_id: None,
            client_secret: None,
        };
        
        let spotify = Spotify { config };
        
        let mut client_guard = SPOTIFY_CLIENT.lock();
        *client_guard = Some(spotify);
        
        info!("Spotify client initialized");
        Ok(())
    }    /// Initialize with default values from secrets.txt
    pub fn initialize_with_defaults() -> Result<()> {
        let oauth_url = default_spotify_oauth_url();
        let proxy_secret = default_spotify_proxy_secret();
          
        info!("Default Spotify OAuth URL: '{}'", oauth_url);
        info!("Default Spotify proxy secret length: {} chars", proxy_secret.len());
          // Check for placeholder values that would indicate misconfiguration
        let is_placeholder_url = oauth_url.contains("your-oauth-proxy-url") || 
                                oauth_url.contains("your_spotify_oauth_url") ||
                                oauth_url == "unknown" ||  // Exact match for "unknown"
                                oauth_url.is_empty();
        
        let is_placeholder_secret = proxy_secret.contains("your-spotify-proxy-secret") || 
                                   proxy_secret.contains("your_spotify_proxy_secret") ||
                                   proxy_secret == "unknown" ||  // Exact match for "unknown"
                                   proxy_secret.is_empty();
                                   
        if oauth_url.contains("unknown") {
            info!("OAuth URL contains 'unknown': '{}'", oauth_url);
        }
        
        if is_placeholder_url || is_placeholder_secret {
            info!("Spotify initialization error: URL is placeholder: {}, Secret is placeholder: {}", 
                 is_placeholder_url, is_placeholder_secret);
            return Err(SpotifyError::ConfigError("Default Spotify OAuth credentials are not configured".to_string()));
        }
        
        info!("Initializing Spotify with URL '{}' from secrets.txt", oauth_url);
        Self::initialize(oauth_url, proxy_secret)
    }
      /// Get the singleton instance of the Spotify client
    pub fn get_instance() -> Result<Spotify> {
        let client_guard = SPOTIFY_CLIENT.lock();
        match &*client_guard {
            Some(client) => Ok(client.clone()),
            None => Err(SpotifyError::ConfigError("Spotify client has not been initialized".to_string()))
        }
    }
      /// Get OAuth URL for the authentication process
    pub fn get_oauth_url(&self) -> &str {
        // Log the URL before returning it to help debug issues
        info!("Using OAuth URL: '{}'", &self.config.oauth_url);
        &self.config.oauth_url
    }
      /// Get the proxy secret for authenticating with the OAuth proxy
    pub fn get_proxy_secret(&self) -> &str {
        info!("Using proxy secret length: {} chars", self.config.proxy_secret.len());
        if self.config.proxy_secret.trim().is_empty() {
            error!("Proxy secret is empty or only whitespace - this will cause authentication failure");
        }
        &self.config.proxy_secret
    }
    /// Get the Spotify client_id and client_secret if configured
    pub fn get_client_id(&self) -> Option<&str> {
        self.config.client_id.as_deref()
    }
    pub fn get_client_secret(&self) -> Option<&str> {
        self.config.client_secret.as_deref()
    }
      /// Store Spotify tokens in the security store
    pub fn store_tokens(&self, tokens: &SpotifyTokens) -> Result<()> {
        // Store tokens securely
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
        // Get tokens from the security store
        let access_token = SecurityStore::get(SPOTIFY_ACCESS_TOKEN_KEY)
            .map_err(|_| SpotifyError::TokenNotFound)?;
        
        let refresh_token = SecurityStore::get(SPOTIFY_REFRESH_TOKEN_KEY)
            .map_err(|_| SpotifyError::TokenNotFound)?;
        
        let expires_at_str = SecurityStore::get(SPOTIFY_TOKEN_EXPIRY_KEY)
            .map_err(|_| SpotifyError::TokenNotFound)?;
            
        let expires_at = expires_at_str.parse::<u64>()
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
                // Check if token is still valid (with some buffer)
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
                
                // Token is valid if it expires in the future
                tokens.expires_at > now
            },
            Err(_) => false,
        }
    }
      /// Clear all Spotify tokens and user data
    pub fn clear_tokens(&self) -> Result<()> {
        // Remove all Spotify-related keys
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
        use crate::helpers::http_client::new_http_client;
        
        info!("Checking connectivity to OAuth server: {}", self.config.oauth_url);
        
        // Create an HTTP client with a short timeout for this check
        let http_client = new_http_client(5);
        
        // Try a simple GET request to the base URL
        match http_client.get_text(&self.config.oauth_url) {
            Ok(text) => {
                // Check if the response contains any indication of being the OAuth service
                let is_valid = text.contains("OAuth") || 
                              text.contains("Spotify") || 
                              text.contains("Authentication") ||
                              text.contains("login");
                
                info!("OAuth server is reachable. Response looks valid: {}", is_valid);
                
                // Log a truncated version of the response for debugging
                let truncated = if text.len() > 100 {
                    format!("{}... (truncated)", &text[0..100])
                } else {
                    text.clone()
                };
                info!("OAuth server response: {}", truncated);
                
                Ok(is_valid)
            },
            Err(e) => {
                error!("Failed to connect to OAuth server: {}", e);
                Err(SpotifyError::ConfigError(format!("OAuth server unreachable: {}", e)))            }
        }
    }    
    
    // Build headers for OAuth proxy requests
    pub fn build_oauth_headers(&self) -> Vec<(&str, String)> {
        let mut headers = vec![
            ("X-Proxy-Secret", self.get_proxy_secret().to_string()),
        ];
        if let Some(client_id) = self.get_client_id() {
            if !client_id.is_empty() {
                debug!("Sending X-Spotify-Client-Id: {}... ({} chars)", 
                       sanitize::safe_truncate(client_id, 6), client_id.len());
                headers.push(("X-Spotify-Client-Id", client_id.to_string()));
            } else {
                debug!("Not sending X-Spotify-Client-Id: value is empty");
            }
        } else {
            debug!("Not sending X-Spotify-Client-Id: not set in config");
        }
        if let Some(client_secret) = self.get_client_secret() {
            if !client_secret.is_empty() {
                debug!("Sending X-Spotify-Client-Secret: {}... ({} chars)", 
                       sanitize::safe_truncate(client_secret, 6), client_secret.len());
                headers.push(("X-Spotify-Client-Secret", client_secret.to_string()));
            } else {
                debug!("Not sending X-Spotify-Client-Secret: value is empty");
            }
        } else {
            debug!("Not sending X-Spotify-Client-Secret: not set in config");
        }
        headers
    }
    /// Refresh the access token using the refresh token via OAuth proxy (only method)
    pub fn refresh_token(&self) -> Result<SpotifyTokens> {
        use crate::helpers::http_client::new_http_client;
        let current_tokens = self.get_tokens()?;
        let http_client = new_http_client(10);
        let refresh_url = format!("{}refresh", self.config.oauth_url);
        let payload = serde_json::json!({
            "refresh_token": current_tokens.refresh_token
        });
        info!("Refreshing Spotify access token via OAuth proxy (headers)");
        let mut headers = self.build_oauth_headers();
        headers.push(("Content-Type", "application/json".to_string()));
        let headers_ref: Vec<(&str, &str)> = headers.iter().map(|(k, v)| (*k, v.as_str())).collect();
        let response = match http_client.post_json_value_with_headers(&refresh_url, payload, &headers_ref) {
            Ok(value) => value,
            Err(e) => {
                error!("Failed to refresh Spotify token via proxy: {}", e);
                return Err(SpotifyError::AuthError(format!("Token refresh via proxy failed: {}", e)));
            }
        };

        // Parse the token response
        let token_response: SpotifyTokenResponse = match serde_json::from_value(response) {
            Ok(parsed) => parsed,
            Err(e) => {
                error!("Failed to parse token refresh response from proxy: {}", e);
                return Err(SpotifyError::SerializationError(e));
            }
        };

        // Calculate the new expiration time
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let expires_at = now + token_response.expires_in;

        // Create the new tokens structure
        let new_tokens = SpotifyTokens {
            access_token: token_response.access_token,
            // If we got a new refresh token, use it; otherwise keep the old one
            refresh_token: token_response.refresh_token.unwrap_or(current_tokens.refresh_token),
            expires_at,
        };

        // Store the updated tokens
        self.store_tokens(&new_tokens)?;

        info!("Successfully refreshed Spotify access token via OAuth proxy");
        Ok(new_tokens)
    }
      /// Ensure we have a valid token, refreshing if necessary
    pub fn ensure_valid_token(&self) -> Result<String> {
        match self.get_tokens() {
            Ok(tokens) => {
                // Check if token is expired or about to expire (within 60 seconds)
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
                
                if tokens.expires_at <= now + 60 {
                    // Token is expired or about to expire, refresh it
                    info!("Spotify token is expired or about to expire, refreshing");
                    
                    // Only use direct API refresh, never the OAuth proxy
                    match self.refresh_token() {
                        Ok(new_tokens) => {
                            info!("Token refresh via direct API successful, new token will expire in {} seconds", 
                                  new_tokens.expires_at.saturating_sub(now));
                            Ok(new_tokens.access_token)
                        },
                        Err(e) => {
                            error!("Direct API token refresh failed: {}", e);
                            Err(e)
                        }
                    }
                } else {
                    // Token is still valid
                    debug!("Spotify token is still valid for {} more seconds", tokens.expires_at - now);
                    Ok(tokens.access_token)
                }
            },
            Err(e) => {
                error!("Failed to get Spotify tokens: {}", e);
                Err(e)
            }
        }
    }/// Get the current playback state from Spotify API
    /// 
    /// This method fetches information about the user's current playback state,
    /// including the currently playing track, playback position, and active device.
    /// 
    /// See: https://developer.spotify.com/documentation/web-api/reference/get-information-about-the-users-current-playback
    pub fn get_playback_state(&self) -> Result<Option<SpotifyPlaybackState>> {
        use crate::helpers::http_client::{new_http_client, HttpClientError};
        
        // Ensure we have a valid token
        let access_token = self.ensure_valid_token()?;
        
        // Create an HTTP client
        let http_client = new_http_client(10);
        
        // Use the real Spotify API endpoint, not the OAuth proxy
        let endpoint_url = "https://api.spotify.com/v1/me/player";
          // Set up authorization header
        let headers = [
            ("Authorization", &format!("Bearer {}", access_token)[..]),
            ("Content-Type", "application/json")
        ];
        
        info!("Fetching Spotify playback state");
        
        // Make the API request
        let response = match http_client.get_json_with_headers(endpoint_url, &headers) {
            Ok(value) => {
                // Check if we got a 204 No Content (no active playback)
                if value.is_null() {
                    debug!("No active Spotify playback found");
                    return Ok(None);
                }
                value
            },            Err(e) => {
                match e {
                    // Handle 204 No Content as a legitimate response indicating no active playback
                    HttpClientError::EmptyResponse => {
                        debug!("No active Spotify playback (204 No Content)");
                        return Ok(None);
                    },
                    // Handle auth errors differently
                    HttpClientError::ServerError(msg) if msg.contains("401") || msg.contains("403") => {
                        error!("Authentication error when fetching playback state: {}", msg);
                        return Err(SpotifyError::AuthError("Authentication failed".to_string()));
                    },
                    // Other errors indicate a problem
                    _ => {
                        error!("Failed to fetch Spotify playback state: {}", e);
                        return Err(SpotifyError::ApiError(format!("Failed to fetch playback state: {}", e)));
                    }
                }
            }
        };
        
        // Parse the playback state response
        match serde_json::from_value::<SpotifyPlaybackState>(response) {
            Ok(playback_state) => {
                if let Some(track) = &playback_state.item {
                    debug!("Currently playing: {} by {}", 
                          track.name, 
                          track.artists.iter().map(|a| a.name.clone()).collect::<Vec<_>>().join(", "));
                }
                Ok(Some(playback_state))
            },
            Err(e) => {
                error!("Failed to parse Spotify playback state: {}", e);
                Err(SpotifyError::SerializationError(e))
            }
        }
    }
    /// Send a command to the Spotify Web API (play, pause, next, previous, seek, repeat, shuffle)
    pub fn send_command(&self, command: &str, args: &serde_json::Value) -> Result<()> {
        use crate::helpers::http_client::{new_http_client, HttpClientError};
        let access_token = self.ensure_valid_token()?;
        let http_client = new_http_client(10);
        let api_url = match command {
            "play" => "https://api.spotify.com/v1/me/player/play",
            "pause" => "https://api.spotify.com/v1/me/player/pause",
            "next" => "https://api.spotify.com/v1/me/player/next",
            "previous" => "https://api.spotify.com/v1/me/player/previous",
            "seek" => "https://api.spotify.com/v1/me/player/seek",
            "repeat" => "https://api.spotify.com/v1/me/player/repeat",
            "shuffle" => "https://api.spotify.com/v1/me/player/shuffle",
            _ => return Err(SpotifyError::ApiError(format!("Unknown command: {}", command))),
        };
        let headers = [
            ("Authorization", &format!("Bearer {}", access_token)[..]),
            ("Content-Type", "application/json"),
        ];
        let result = match command {
            // Use PUT for play, pause, seek, repeat, shuffle
            "play" | "pause" => http_client.put_json_value_with_headers(api_url, args.clone(), &headers),
            "seek" => {
                let position_ms = args.get("position_ms").and_then(|v| v.as_u64()).unwrap_or(0);
                let url = format!("{}?position_ms={}", api_url, position_ms);
                http_client.put_json_value_with_headers(&url, serde_json::json!({}), &headers)
            },
            "repeat" => {
                let state = args.get("state").and_then(|v| v.as_str()).unwrap_or("off");
                let url = format!("{}?state={}", api_url, state);
                http_client.put_json_value_with_headers(&url, serde_json::json!({}), &headers)
            },
            "shuffle" => {
                let state = args.get("state").and_then(|v| v.as_bool()).unwrap_or(false);
                let url = format!("{}?state={}", api_url, state);
                http_client.put_json_value_with_headers(&url, serde_json::json!({}), &headers)
            },
            // Use POST for next and previous
            "next" | "previous" => http_client.post_json_value_with_headers(api_url, args.clone(), &headers),
            _ => Err(HttpClientError::RequestError("Not implemented".to_string())),
        };
        match result {
            Ok(_) => Ok(()),
            // Handle empty responses as success for Spotify API commands (204 No Content)
            Err(HttpClientError::EmptyResponse) => {
                debug!("Spotify API command '{}' returned empty response (204 No Content) - treating as success", command);
                Ok(())
            },
            Err(e) => Err(SpotifyError::ApiError(format!("Command failed: {}", e))),
        }
    }
    /// Get the user's currently playing track from Spotify
    pub fn get_currently_playing(&self) -> Result<Option<serde_json::Value>> {
        use crate::helpers::http_client::new_http_client;
        let access_token = self.ensure_valid_token()?;
        let http_client = new_http_client(10);
        let url = "https://api.spotify.com/v1/me/player/currently-playing";
        let headers = [
            ("Authorization", &format!("Bearer {}", access_token)[..]),
            ("Content-Type", "application/json"),
        ];
        let result = http_client.get_json_with_headers(url, &headers);
        match result {
            Ok(json) => {
                if json.is_null() {
                    Ok(None)
                } else {
                    Ok(Some(json))
                }
            },
            Err(e) => Err(SpotifyError::ApiError(format!("Failed to get currently playing: {}", e))),
        }
    }
    /// Search Spotify for albums, artists, or tracks with optional filters
    /// See: https://developer.spotify.com/documentation/web-api/reference/search
    pub fn search(&self, query: &str, types: &[&str], filters: Option<&serde_json::Value>) -> Result<serde_json::Value> {
        use crate::helpers::http_client::new_http_client;
        let access_token = self.ensure_valid_token()?;
        let http_client = new_http_client(10);
        let mut q = query.to_string();
        // Add filters to the query string
        if let Some(filters) = filters {
            if let Some(artist) = filters.get("artist").and_then(|v| v.as_str()) {
                q.push_str(&format!(" artist:{}", artist));
            }
            if let Some(year) = filters.get("year").and_then(|v| v.as_str()) {
                q.push_str(&format!(" year:{}", year));
            }
            if let Some(album) = filters.get("album").and_then(|v| v.as_str()) {
                q.push_str(&format!(" album:{}", album));
            }
            if let Some(genre) = filters.get("genre").and_then(|v| v.as_str()) {
                q.push_str(&format!(" genre:{}", genre));
            }
            if let Some(isrc) = filters.get("isrc").and_then(|v| v.as_str()) {
                q.push_str(&format!(" isrc:{}", isrc));
            }
            if let Some(track) = filters.get("track").and_then(|v| v.as_str()) {
                q.push_str(&format!(" track:{}", track));
            }
        }
        let type_param = types.join(",");
        let url = format!(
            "https://api.spotify.com/v1/search?q={}&type={}",
            urlencoding::encode(&q),
            urlencoding::encode(&type_param)
        );
        let headers = [
            ("Authorization", &format!("Bearer {}", access_token)[..]),
            ("Content-Type", "application/json"),
        ];
        let result = http_client.get_json_with_headers(&url, &headers);
        match result {
            Ok(json) => Ok(json),
            Err(e) => Err(SpotifyError::ApiError(format!("Failed to search: {}", e))),
        }
    }

    /// Construct the OAuth login URL with required scopes as a query parameter
    pub fn build_oauth_login_url(&self) -> String {
        let base_url = self.get_oauth_url();
        let scopes = Self::required_scopes();
        // Ensure no double ? in URL
        let sep = if base_url.contains('?') { "&" } else { "?" };
        format!("{}login{}scope={}", base_url, sep, urlencoding::encode(scopes))
    }
}

impl Spotify {
    /// Helper to get the required scopes as a string
    pub fn required_scopes() -> &'static str {
        SPOTIFY_REQUIRED_SCOPES
    }

    /// Helper to construct the /create_session URL with required scopes as a query parameter
    pub fn build_create_session_url(&self) -> String {
        let base_url = self.get_oauth_url();
        let scopes = Self::required_scopes();
        // Ensure no double ? in URL
        let sep = if base_url.contains('?') { "&" } else { "?" };
        format!("{}create_session{}scope={}", base_url, sep, urlencoding::encode(scopes))
    }

    /// Helper to construct the OAuth login URL (only needs session_id)
    pub fn build_login_url(&self, session_id: &str) -> String {
        let base_url = self.get_oauth_url();
        format!("{base_url}login/{session_id}")
    }

    /// Check if a song is in the user's favourites/saved tracks
    /// 
    /// This method searches for the song on Spotify using artist and title,
    /// then checks if any of the found track IDs are in the user's saved tracks.
    /// 
    /// # Arguments
    /// * `artist` - The artist name
    /// * `title` - The song title
    /// 
    /// # Returns
    /// * `Ok(Some(true))` - Song is in favourites
    /// * `Ok(Some(false))` - Song is not in favourites
    /// * `Ok(None)` - Song not found on Spotify
    /// * `Err(...)` - API error occurred
    pub fn is_song_favourite(&self, artist: &str, title: &str) -> Result<Option<bool>> {
        debug!("Checking if song is favourite: '{}' by '{}'", title, artist);
        
        // First, search for the song on Spotify
        let query = format!("track:\"{}\" artist:\"{}\"", title, artist);
        let search_result = self.search(&query, &["track"], None)?;
        
        // Extract track IDs from search results
        let mut track_ids = Vec::new();
        if let Some(tracks) = search_result.get("tracks").and_then(|t| t.get("items")).and_then(|i| i.as_array()) {
            for track in tracks {
                if let Some(id) = track.get("id").and_then(|i| i.as_str()) {
                    track_ids.push(id.to_string());
                }
            }
        }
        
        debug!("Found {} track IDs on Spotify: {:?}", track_ids.len(), track_ids);
        
        if track_ids.is_empty() {
            debug!("No tracks found on Spotify for '{}' by '{}'", title, artist);
            return Ok(None);
        }
        
        // Check if any of these track IDs are in the user's saved tracks
        self.check_saved_tracks(&track_ids)
    }
    
    /// Check if track IDs are in the user's saved tracks
    /// 
    /// Uses the Spotify Web API endpoint: https://developer.spotify.com/documentation/web-api/reference/check-users-saved-tracks
    /// 
    /// # Arguments
    /// * `track_ids` - Vector of Spotify track IDs to check (max 50 per request)
    /// 
    /// # Returns
    /// * `Ok(Some(true))` - At least one track ID is saved
    /// * `Ok(Some(false))` - None of the track IDs are saved
    /// * `Err(...)` - API error occurred
    pub fn check_saved_tracks(&self, track_ids: &[String]) -> Result<Option<bool>> {
        use crate::helpers::http_client::new_http_client;
        
        if track_ids.is_empty() {
            return Ok(Some(false));
        }
        
        // Spotify API allows max 50 IDs per request
        let chunk_size = 50;
        let mut any_saved = false;
        
        for chunk in track_ids.chunks(chunk_size) {
            let access_token = self.ensure_valid_token()?;
            let http_client = new_http_client(10);
            
            // Join track IDs with commas
            let ids_param = chunk.join(",");
            let url = format!("https://api.spotify.com/v1/me/tracks/contains?ids={}", 
                             urlencoding::encode(&ids_param));
            
            debug!("Checking saved tracks for IDs: {:?}", chunk);
            
            let headers = [
                ("Authorization", &format!("Bearer {}", access_token)[..]),
                ("Content-Type", "application/json"),
            ];
            
            let response = match http_client.get_json_with_headers(&url, &headers) {
                Ok(value) => value,
                Err(e) => {
                    error!("Failed to check saved tracks: {}", e);
                    return Err(SpotifyError::ApiError(format!("Failed to check saved tracks: {}", e)));
                }
            };
            
            debug!("Saved tracks API response: {}", response);
            
            // Response should be an array of booleans
            if let Some(saved_array) = response.as_array() {
                for (i, is_saved) in saved_array.iter().enumerate() {
                    if let Some(saved) = is_saved.as_bool() {
                        debug!("Track ID '{}' is saved: {}", chunk.get(i).unwrap_or(&"unknown".to_string()), saved);
                        if saved {
                            any_saved = true;
                        }
                    }
                }
            } else {
                error!("Unexpected response format from saved tracks API: {}", response);
                return Err(SpotifyError::ApiError("Unexpected response format".to_string()));
            }
        }
        
        debug!("Final result: at least one track is saved = {}", any_saved);
        Ok(Some(any_saved))
    }
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

impl crate::helpers::favourites::FavouriteProvider for SpotifyFavouriteProvider {
    fn is_favourite(&self, song: &crate::data::song::Song) -> std::result::Result<bool, crate::helpers::favourites::FavouriteError> {
        let artist = song.artist.as_ref()
            .ok_or_else(|| crate::helpers::favourites::FavouriteError::InvalidSong("Artist is required".to_string()))?;
        let title = song.title.as_ref()
            .ok_or_else(|| crate::helpers::favourites::FavouriteError::InvalidSong("Title is required".to_string()))?;

        debug!("Checking if Spotify favourite: '{}' by '{}'", title, artist);

        match Spotify::get_instance() {
            Ok(spotify) => {
                match spotify.is_song_favourite(artist, title) {
                    Ok(Some(is_favourite)) => {
                        debug!("Spotify favourite check result: {}", is_favourite);
                        Ok(is_favourite)
                    },
                    Ok(None) => {
                        debug!("Song not found on Spotify: '{}' by '{}'", title, artist);
                        // Song not found on Spotify - treat as not favourite
                        Ok(false)
                    },
                    Err(SpotifyError::AuthError(msg)) => {
                        debug!("Spotify authentication error: {}", msg);
                        Err(crate::helpers::favourites::FavouriteError::AuthError(msg))
                    },
                    Err(SpotifyError::ApiError(msg)) => {
                        debug!("Spotify API error: {}", msg);
                        Err(crate::helpers::favourites::FavouriteError::NetworkError(msg))
                    },
                    Err(SpotifyError::ConfigError(msg)) => {
                        debug!("Spotify configuration error: {}", msg);
                        Err(crate::helpers::favourites::FavouriteError::NotConfigured(msg))
                    },
                    Err(e) => {
                        debug!("Other Spotify error: {}", e);
                        Err(crate::helpers::favourites::FavouriteError::Other(e.to_string()))
                    }
                }
            },
            Err(e) => {
                debug!("Failed to get Spotify instance: {}", e);
                Err(crate::helpers::favourites::FavouriteError::NotConfigured("Spotify client not initialized".to_string()))
            }
        }
    }

    fn add_favourite(&self, _song: &crate::data::song::Song) -> std::result::Result<(), crate::helpers::favourites::FavouriteError> {
        // Spotify Web API doesn't provide an endpoint to add songs to saved tracks programmatically
        // The user would need to do this manually through the Spotify app or web player
        Err(crate::helpers::favourites::FavouriteError::Other("Adding songs to Spotify favourites is not supported via API - use Spotify app".to_string()))
    }

    fn remove_favourite(&self, _song: &crate::data::song::Song) -> std::result::Result<(), crate::helpers::favourites::FavouriteError> {
        // Spotify Web API doesn't provide an endpoint to remove songs from saved tracks programmatically  
        // The user would need to do this manually through the Spotify app or web player
        Err(crate::helpers::favourites::FavouriteError::Other("Removing songs from Spotify favourites is not supported via API - use Spotify app".to_string()))
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

    fn is_enabled(&self) -> bool {
        // Check if Spotify client is configured and has valid tokens (with auto-refresh)
        match Spotify::get_instance() {
            Ok(spotify) => {
                // Try to ensure we have valid tokens, refreshing if necessary
                // This will automatically refresh expired tokens
                spotify.ensure_valid_token().is_ok()
            },
            Err(_) => false,
        }
    }

    fn is_active(&self) -> bool {
        // For Spotify, active means we have valid authentication tokens and can make API calls
        // This is the same as is_enabled for Spotify
        self.is_enabled()
    }
}

// Add the missing set_global_config method for the Spotify global config singleton
impl Spotify {
    pub fn set_global_config(spotify_config: &serde_json::Value) {
        let config = SpotifyConfig::from_json(spotify_config);
        let _ = GLOBAL_SPOTIFY_CONFIG.set(config);
    }
}
