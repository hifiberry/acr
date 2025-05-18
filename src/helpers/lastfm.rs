use crate::helpers::ratelimit;
use log::{debug, info};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::error::Error;
use std::fmt;
use std::time::SystemTime;
use ureq;
use once_cell::sync::Lazy;
use std::sync::Mutex;

const LASTFM_API_ROOT: &str = "https://ws.audioscrobbler.com/2.0/";
const LASTFM_AUTH_URL: &str = "https://www.last.fm/api/auth/";

// Error types for Last.fm API
#[derive(Debug)]
pub enum LastfmError {
    ApiError(String, i32),
    NetworkError(String),
    ParsingError(String),
    AuthError(String),
    ConfigError(String),
}

impl fmt::Display for LastfmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LastfmError::ApiError(msg, code) => write!(f, "Last.fm API error ({}): {}", code, msg),
            LastfmError::NetworkError(msg) => write!(f, "Network error: {}", msg),
            LastfmError::ParsingError(msg) => write!(f, "Parsing error: {}", msg),
            LastfmError::AuthError(msg) => write!(f, "Authentication error: {}", msg),
            LastfmError::ConfigError(msg) => write!(f, "Configuration error: {}", msg),
        }
    }
}

impl Error for LastfmError {}

// Auth token response
#[derive(Debug, Deserialize)]
struct TokenResponse {
    token: String,
}

// Session response
#[derive(Debug, Deserialize)]
struct SessionResponse {
    session: Session,
}

#[derive(Debug, Deserialize)]
struct Session {
    name: String,
    key: String,
    subscriber: i32, // Last.fm returns 0 or 1
}

// Credentials storage
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LastfmCredentials {
    pub api_key: String,
    pub api_secret: String,
    pub session_key: Option<String>,
    pub username: Option<String>,
    pub auth_token: Option<String>,
    pub token_created: Option<u64>, // Unix timestamp
}

// Singleton instance of LastfmClient
static LASTFM_CLIENT: Lazy<Mutex<Option<LastfmClient>>> = Lazy::new(|| Mutex::new(None));

pub struct LastfmClient {
    credentials: LastfmCredentials,
    client: ureq::Agent,
}

impl LastfmClient {
    /// Initialize the Last.fm client with API credentials
    pub fn initialize(api_key: String, api_secret: String) -> Result<(), LastfmError> {
        if api_key.is_empty() || api_secret.is_empty() {
            return Err(LastfmError::ConfigError(
                "API key and secret are required".to_string(),
            ));
        }

        // Register with rate limiter - 1 request per second is a safe default
        ratelimit::register_service("lastfm", 1000);

        let credentials = LastfmCredentials {
            api_key,
            api_secret,
            session_key: None,
            username: None,
            auth_token: None,
            token_created: None,
        };

        let client = ureq::agent();

        let mut lastfm_guard = LASTFM_CLIENT.lock().unwrap();
        *lastfm_guard = Some(LastfmClient {
            credentials,
            client,
        });

        info!("Last.fm client initialized");
        Ok(())
    }

    /// Get the singleton instance of LastfmClient
    pub fn get_instance() -> Result<LastfmClient, LastfmError> {
        let lastfm_guard = LASTFM_CLIENT.lock().unwrap();
        match &*lastfm_guard {
            Some(client) => Ok(client.clone()),
            None => Err(LastfmError::ConfigError(
                "Last.fm client has not been initialized".to_string(),
            )),
        }
    }

    /// Get authentication URL for user to authorize application
    pub fn get_auth_url(&mut self) -> Result<String, LastfmError> {
        // Get an auth token first
        let token = self.get_auth_token()?;
        
        // Build the auth URL
        let auth_url = format!("{}?api_key={}&token={}", 
            LASTFM_AUTH_URL, 
            self.credentials.api_key,
            token
        );
        
        Ok(auth_url)
    }

    /// Get an authentication token from Last.fm
    pub fn get_auth_token(&mut self) -> Result<String, LastfmError> {
        // Check if we already have a valid token
        if let Some(token) = &self.credentials.auth_token {
            if let Some(created) = self.credentials.token_created {
                let now = SystemTime::now()
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .unwrap()
                    .as_secs();
                
                // Tokens are valid for 60 minutes
                if now - created < 3600 {
                    debug!("Reusing existing auth token");
                    return Ok(token.clone());
                }
            }
        }

        ratelimit::rate_limit("lastfm");

        let params = [
            ("method", "auth.getToken"),
            ("api_key", &self.credentials.api_key),
            ("format", "json"),
        ];

        debug!("Requesting Last.fm auth token");
        let response = self.make_api_request(params, false)?;
        
        let token_response: TokenResponse = serde_json::from_str(&response)
            .map_err(|e| LastfmError::ParsingError(format!("Failed to parse token response: {}", e)))?;
        
        // Store the token
        self.credentials.auth_token = Some(token_response.token.clone());
        self.credentials.token_created = Some(
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        );
        
        debug!("Received new Last.fm auth token");
        Ok(token_response.token)
    }

    /// Get a session key after user has authorized the application
    pub fn get_session(&mut self) -> Result<(String, String), LastfmError> {
        // Check if we have a token
        let token = match &self.credentials.auth_token {
            Some(t) => t.clone(),
            None => return Err(LastfmError::AuthError("No auth token available".to_string())),
        };

        ratelimit::rate_limit("lastfm");

        let params = [
            ("method", "auth.getSession"),
            ("api_key", &self.credentials.api_key),
            ("token", &token),
        ];

        // This request needs to be signed
        let response = self.make_api_request(params, true)?;
        
        let session_response: SessionResponse = serde_json::from_str(&response)
            .map_err(|e| LastfmError::ParsingError(format!("Failed to parse session response: {}", e)))?;
        
        // Store the session
        self.credentials.session_key = Some(session_response.session.key.clone());
        self.credentials.username = Some(session_response.session.name.clone());
        
        // Clear the token since it's been used
        self.credentials.auth_token = None;
        self.credentials.token_created = None;
        
        info!("Successfully authenticated with Last.fm as user: {}", session_response.session.name);
        Ok((session_response.session.key, session_response.session.name))
    }

    /// Check if user is authenticated
    pub fn is_authenticated(&self) -> bool {
        self.credentials.session_key.is_some() && self.credentials.username.is_some()
    }

    /// Get the username if authenticated
    pub fn get_username(&self) -> Option<String> {
        self.credentials.username.clone()
    }

    /// Make an API request to Last.fm
    fn make_api_request<'a>(&self, params: impl IntoIterator<Item = (&'a str, &'a str)>, sign: bool) -> Result<String, LastfmError> {
        let param_map: HashMap<String, String> = params
            .into_iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();

        // We need to sign the API call if required
        let api_sig = if sign {
            self.generate_signature(&param_map)
        } else {
            String::new()
        };

        // Build the request URL
        let mut request = self.client.post(LASTFM_API_ROOT);

        // Add all parameters
        for (key, value) in &param_map {
            request = request.query(key, value);
        }

        // Add the signature if needed
        if sign {
            request = request.query("api_sig", &api_sig);
        }

        // Always add format=json
        request = request.query("format", "json");

        // Send the request
        let response = request.call()
            .map_err(|e| LastfmError::NetworkError(e.to_string()))?;

        let response_text = response.into_string()
            .map_err(|e| LastfmError::NetworkError(format!("Failed to read response: {}", e)))?;

        // Check for error in response
        if response_text.contains("\"error\":") {
            if let Ok(error_response) = serde_json::from_str::<serde_json::Value>(&response_text) {
                if let (Some(code), Some(message)) = (
                    error_response.get("error").and_then(|e| e.as_i64()),
                    error_response.get("message").and_then(|m| m.as_str()),
                ) {
                    return Err(LastfmError::ApiError(
                        message.to_string(),
                        code as i32,
                    ));
                }
            }
            
            // Generic error if we couldn't parse the specifics
            return Err(LastfmError::ApiError(
                format!("Unknown API error: {}", response_text),
                -1,
            ));
        }

        Ok(response_text)
    }

    /// Generate a signature for API call as per Last.fm requirements
    fn generate_signature(&self, params: &HashMap<String, String>) -> String {
        // Sort params alphabetically and concatenate
        let mut keys: Vec<&String> = params.keys().collect();
        keys.sort();

        let mut signature_base = String::new();
        for key in keys {
            signature_base.push_str(key);
            signature_base.push_str(params.get(key).unwrap());
        }
        
        // Append the secret
        signature_base.push_str(&self.credentials.api_secret);
        
        // Calculate MD5 hash
        let digest = md5::compute(signature_base);
        format!("{:x}", digest)
    }

    // Clone implementation for the client
    fn clone(&self) -> Self {
        LastfmClient {
            credentials: self.credentials.clone(),
            client: ureq::agent(),
        }
    }

    // Create an instance from credentials
    fn with_credentials(credentials: LastfmCredentials) -> Self {
        // Register with rate limiter
        ratelimit::register_service("lastfm", 1000);

        LastfmClient {
            credentials,
            client: ureq::agent(),
        }
    }

    // Get credentials (useful for persisting them)
    pub fn get_credentials(&self) -> LastfmCredentials {
        self.credentials.clone()
    }

    // Create a new instance from stored credentials
    pub fn from_credentials(credentials: LastfmCredentials) -> Result<(), LastfmError> {
        if credentials.api_key.is_empty() || credentials.api_secret.is_empty() {
            return Err(LastfmError::ConfigError(
                "API key and secret are required".to_string(),
            ));
        }

        // Register with rate limiter
        ratelimit::register_service("lastfm", 1000);

        let client = LastfmClient {
            credentials,
            client: ureq::agent(),
        };

        let mut lastfm_guard = LASTFM_CLIENT.lock().unwrap();
        *lastfm_guard = Some(client);

        info!("Last.fm client initialized from stored credentials");
        Ok(())
    }

    /// Submit a track scrobble to Last.fm
    /// 
    /// # Arguments
    /// * `artist` - The track artist name
    /// * `track` - The track title
    /// * `album` - Optional album name
    /// * `album_artist` - Optional album artist (if different from track artist)
    /// * `timestamp` - Unix timestamp when the track was started playing
    /// * `track_number` - Optional track number
    /// * `duration` - Optional track duration in seconds
    /// 
    /// # Returns
    /// Result indicating success or failure
    pub fn scrobble(
        &self,
        artist: &str,
        track: &str,
        album: Option<&str>,
        album_artist: Option<&str>,
        timestamp: u64,
        track_number: Option<u32>,
        duration: Option<u32>,
    ) -> Result<(), LastfmError> {
        // Check if we're authenticated
        if !self.is_authenticated() {
            return Err(LastfmError::AuthError("Not authenticated with Last.fm".to_string()));
        }

        ratelimit::rate_limit("lastfm");

        // Convert all parameters to owned strings
        let api_key = self.credentials.api_key.clone();
        let session_key = self.credentials.session_key.as_ref().unwrap().clone();
        let timestamp_str = timestamp.to_string();
        
        // Optional parameters
        let track_num_str = track_number.map(|n| n.to_string());
        let duration_str = duration.map(|d| d.to_string());
          // Create a vector to hold owned strings
        let mut param_vec = Vec::new();
        
        // Add required parameters
        param_vec.push(("method", "track.scrobble".to_string()));
        param_vec.push(("api_key", api_key));
        param_vec.push(("sk", session_key));
        param_vec.push(("artist", artist.to_string()));
        param_vec.push(("track", track.to_string()));
        param_vec.push(("timestamp", timestamp_str));
        
        // Add optional parameters
        if let Some(album_name) = album {
            param_vec.push(("album", album_name.to_string()));
        }
        
        if let Some(album_artist_name) = album_artist {
            param_vec.push(("albumArtist", album_artist_name.to_string()));
        }
        
        if let Some(track_num) = track_num_str {
            param_vec.push(("trackNumber", track_num));
        }
        
        if let Some(dur) = duration_str {
            param_vec.push(("duration", dur));
        }
        
        // Create a temporary vector of string references for the API call
        let params: Vec<(&str, &str)> = param_vec.iter()
            .map(|(k, v)| (*k, v.as_str()))
            .collect();

        // This request needs to be signed
        let _response = self.make_api_request(params, true)?;
        
        // Check for error in the response (handled by make_api_request)
        debug!("Scrobble successful for track: {} - {}", artist, track);
        Ok(())
    }

    /// Update "now playing" status on Last.fm
    /// 
    /// # Arguments
    /// * `artist` - The track artist name
    /// * `track` - The track title
    /// * `album` - Optional album name
    /// * `album_artist` - Optional album artist (if different from track artist)
    /// * `track_number` - Optional track number
    /// * `duration` - Optional track duration in seconds
    /// 
    /// # Returns
    /// Result indicating success or failure
    pub fn update_now_playing(
        &self,
        artist: &str,
        track: &str,
        album: Option<&str>,
        album_artist: Option<&str>,
        track_number: Option<u32>,
        duration: Option<u32>,
    ) -> Result<(), LastfmError> {
        // Check if we're authenticated
        if !self.is_authenticated() {
            return Err(LastfmError::AuthError("Not authenticated with Last.fm".to_string()));
        }

        ratelimit::rate_limit("lastfm");

        // Convert all parameters to owned strings
        let api_key = self.credentials.api_key.clone();
        let session_key = self.credentials.session_key.as_ref().unwrap().clone();
        
        // Optional parameters
        let track_num_str = track_number.map(|n| n.to_string());
        let duration_str = duration.map(|d| d.to_string());
          // Create a vector to hold owned strings
        let mut param_vec = Vec::new();
        
        // Add required parameters
        param_vec.push(("method", "track.updateNowPlaying".to_string()));
        param_vec.push(("api_key", api_key));
        param_vec.push(("sk", session_key));
        param_vec.push(("artist", artist.to_string()));
        param_vec.push(("track", track.to_string()));
        
        // Add optional parameters
        if let Some(album_name) = album {
            param_vec.push(("album", album_name.to_string()));
        }
        
        if let Some(album_artist_name) = album_artist {
            param_vec.push(("albumArtist", album_artist_name.to_string()));
        }
        
        if let Some(track_num) = track_num_str {
            param_vec.push(("trackNumber", track_num));
        }
        
        if let Some(dur) = duration_str {
            param_vec.push(("duration", dur));
        }
        
        // Create a temporary vector of string references for the API call
        let params: Vec<(&str, &str)> = param_vec.iter()
            .map(|(k, v)| (*k, v.as_str()))
            .collect();

        // This request needs to be signed
        let _response = self.make_api_request(params, true)?;
        
        // Check for error in the response (handled by make_api_request)
        debug!("Now playing updated for track: {} - {}", artist, track);
        Ok(())
    }
}
