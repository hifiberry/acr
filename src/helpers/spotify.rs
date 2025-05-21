// Spotify helper functions for ACR
// This module provides functionality for authenticating with Spotify
// and managing tokens through the OAuth2 flow

use log::{error, info};
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
                Err(SpotifyError::ConfigError(format!("OAuth server unreachable: {}", e)))
            }
        }
    }
}
