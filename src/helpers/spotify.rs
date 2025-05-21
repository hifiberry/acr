// Spotify helper functions for ACR
// This module provides functionality for authenticating with Spotify
// and managing tokens through the OAuth2 flow

use log::{debug, error, info};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use once_cell::sync::Lazy;
use std::sync::Mutex;

use crate::helpers::security_store::SecurityStore;

// Constants for token storage
const SPOTIFY_ACCESS_TOKEN_KEY: &str = "spotify_access_token";
const SPOTIFY_REFRESH_TOKEN_KEY: &str = "spotify_refresh_token";
const SPOTIFY_TOKEN_EXPIRY_KEY: &str = "spotify_token_expiry";
const SPOTIFY_USER_ID_KEY: &str = "spotify_user_id";
const SPOTIFY_DISPLAY_NAME_KEY: &str = "spotify_display_name";

// Constants for service configuration
#[allow(dead_code)]
// Make sure the default URL has the proper format with trailing slash
const SPOTIFY_DEFAULT_OAUTH_URL: &str = "https://oauth.hifiberry.com/spotify/";

// Global singleton instance of Spotify client
pub(crate) static SPOTIFY_CLIENT: Lazy<Mutex<Option<Spotify>>> = Lazy::new(|| Mutex::new(None));

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

impl Spotify {
    /// Create a new Spotify helper instance with default configuration
    pub fn new() -> Self {
        // Attempt to get instance from global client first
        if let Ok(spotify) = Self::get_instance() {
            return spotify;
        }
        
        // Otherwise create with default configuration
        Spotify {
            config: SpotifyConfig {
                oauth_url: default_spotify_oauth_url(),
                proxy_secret: default_spotify_proxy_secret(),
            }
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
        };
        
        let spotify = Spotify { config };
        
        let mut client_guard = SPOTIFY_CLIENT.lock().unwrap();
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
        let client_guard = SPOTIFY_CLIENT.lock().unwrap();
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
    
    /// Refresh the access token using the refresh token via OAuth proxy (only method)
    pub fn refresh_token(&self) -> Result<SpotifyTokens> {
        use crate::helpers::http_client::new_http_client;
        // Get the current tokens
        let current_tokens = self.get_tokens()?;

        // Create an HTTP client for token refresh
        let http_client = new_http_client(10);

        // Construct the token refresh endpoint URL, adding proxy_secret as a query parameter
        let refresh_url = format!("{}refresh?proxy_secret={}", self.config.oauth_url, self.config.proxy_secret);

        // Prepare the request payload
        let payload = serde_json::json!({
            "refresh_token": current_tokens.refresh_token
        });

        info!("Refreshing Spotify access token via OAuth proxy (proxy_secret as query param)");

        // Make the refresh request (no custom headers needed)
        let response = match http_client.post_json_value(&refresh_url, payload) {
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
        let response = match http_client.get_json_with_headers(&endpoint_url, &headers) {
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
        use crate::helpers::http_client::new_http_client;
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
            _ => Err(crate::helpers::http_client::HttpClientError::RequestError("Not implemented".to_string())),
        };
        match result {
            Ok(_) => Ok(()),
            Err(e) => Err(SpotifyError::ApiError(format!("Command failed: {}", e))),
        }
    }
}
