use crate::helpers::ratelimit;
use log::{debug, info, error};
use md5;
use once_cell::sync::Lazy;
use regex::Regex;
use serde::{de::{self, Deserializer, Unexpected}, Deserialize, Serialize}; // Ensure full serde de import
use std::collections::HashMap;
use std::error::Error;
use std::fmt;
use std::time::SystemTime;
use ureq;
use parking_lot::Mutex;
// Import SecurityStore and its error type
use crate::helpers::security_store::{SecurityStore, SecurityStoreError};

const LASTFM_API_ROOT: &str = "https://ws.audioscrobbler.com/2.0/";
const LASTFM_AUTH_URL: &str = "http://www.last.fm/api/auth/";

const LASTFM_SESSION_KEY_STORE: &str = "lastfm_session_key";
const LASTFM_USERNAME_STORE: &str = "lastfm_username";

// Default Last.fm API credentials compiled from secrets.txt at build time
// These are used as fallbacks if no credentials are provided
#[cfg(not(test))]
pub fn default_lastfm_api_key() -> String {
    crate::secrets::lastfm_api_key()
}

#[cfg(not(test))]
pub fn default_lastfm_api_secret() -> String {
    crate::secrets::lastfm_api_secret()
}

// Test credentials (placeholders for tests)
#[cfg(test)]
pub fn default_lastfm_api_key() -> String {
    "test_api_key".to_string()
}

#[cfg(test)]
pub fn default_lastfm_api_secret() -> String {
    "test_api_secret".to_string()
}


// Error types for Last.fm API
#[derive(Debug)]
pub enum LastfmError {
    ApiError(String, i32), // message, code
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

// Added to parse Last.fm's own error responses
#[derive(Debug, Deserialize)]
struct LastfmErrorResponse {
    error: i32,
    message: String,
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
    #[allow(dead_code)] // Field from Last.fm API, not currently used
    subscriber: i32, // Last.fm returns 0 or 1
}

// Helper function to deserialize "0" or "1" string to bool
fn deserialize_string_to_bool<'de, D>(deserializer: D) -> Result<bool, D::Error>
where
    D: Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    match s.as_str() {
        "1" => Ok(true),
        "0" => Ok(false),
        _ => Err(de::Error::invalid_value(Unexpected::Str(&s), &"a string '0' or '1'")),
    }
}

#[derive(Deserialize, Debug, Clone)]
pub struct LastfmTrackInfoArtist {
    pub name: String,
    pub mbid: Option<String>,
    pub url: String,
}

#[derive(Deserialize, Debug, Clone)]
pub struct LastfmTrackInfoAlbumImage {
    #[serde(rename = "#text")]
    pub url: String,
    pub size: String,
}

#[derive(Deserialize, Debug, Clone)]
pub struct LastfmTrackInfoAlbum {
    pub artist: String,
    pub title: String,
    pub mbid: Option<String>,
    pub url: String,
    #[serde(default)] // image array can be missing
    pub image: Vec<LastfmTrackInfoAlbumImage>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct LastfmTag {
    pub name: String,
    pub url: String,
}

#[derive(Deserialize, Debug, Clone)]
pub struct LastfmTopTags {
    #[serde(default, rename = "tag")] // tag array can be missing or not an array if empty
    pub tags: Vec<LastfmTag>,
}


#[derive(Deserialize, Debug, Clone)]
pub struct LastfmWiki {
    pub published: String,
    pub summary: String,
    pub content: String,
}

#[derive(Deserialize, Debug, Clone)]
pub struct LastfmArtistImage {
    #[serde(rename = "#text")]
    pub url: String,
    pub size: String,
}

#[derive(Deserialize, Debug, Clone)]
pub struct LastfmSimilarArtist {
    pub name: String,
    pub url: String,
    #[serde(default)]
    pub image: Vec<LastfmArtistImage>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct LastfmSimilar {
    #[serde(default, rename = "artist")]
    pub artists: Vec<LastfmSimilarArtist>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct LastfmArtistDetails {
    pub name: String,
    pub mbid: Option<String>,
    pub url: String,
    #[serde(default)]
    pub image: Vec<LastfmArtistImage>,
    pub streamable: String,
    pub stats: Option<serde_json::Value>, // Contains playcount, listeners
    pub similar: Option<LastfmSimilar>,
    #[serde(rename = "tags")]
    pub tags: Option<LastfmTopTags>,
    pub bio: Option<LastfmWiki>,
}

#[derive(Deserialize, Debug)]
struct LastfmArtistInfoResponse {
    artist: LastfmArtistDetails,
}

#[derive(Deserialize, Debug, Clone)]
pub struct LastfmTrackInfoDetails {
    pub name: String,
    pub mbid: Option<String>,
    pub url: String,
    pub duration: String, // Duration in milliseconds, or "0"
    pub listeners: String,
    pub playcount: String,
    pub artist: LastfmTrackInfoArtist,
    pub album: Option<LastfmTrackInfoAlbum>,
    #[serde(rename = "toptags")]
    pub tags: Option<LastfmTopTags>, // Changed from TopTags to Option<LastfmTopTags>
    pub wiki: Option<LastfmWiki>,
    #[serde(deserialize_with = "deserialize_string_to_bool", default)] // userloved might be missing if not authenticated for the call
    pub userloved: bool,
    #[serde(rename = "userplaycount")]
    pub user_playcount: Option<String>,
}

#[derive(Deserialize, Debug)]
struct LastfmTrackInfoResponse {
    track: LastfmTrackInfoDetails,
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
// Make it pub(crate) to be accessible within the crate (e.g., by api module)
pub(crate) static LASTFM_CLIENT: Lazy<Mutex<Option<LastfmClient>>> = Lazy::new(|| Mutex::new(None));

#[derive(Clone)] // Added derive(Clone)
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

        let mut lastfm_guard = LASTFM_CLIENT.lock();
        *lastfm_guard = Some(LastfmClient {
            credentials,
            client,
        });

        // Attempt to load credentials from security store
        if let Some(client_ref) = lastfm_guard.as_mut() {
            client_ref.load_credentials_from_store();
        }

        info!("Last.fm client initialized");
        Ok(())
    }    
    
    /// Initialize the Last.fm client with default API credentials from secrets.txt
    /// 
    /// This will use the credentials compiled in from the secrets.txt file at build time.
    /// If no secrets.txt file was available, placeholder values will be used.
    /// 
    /// # Returns
    /// Result indicating success or failure    
    pub fn initialize_with_defaults() -> Result<(), LastfmError> {
        let api_key = default_lastfm_api_key();
        let api_secret = default_lastfm_api_secret();
        
        if api_key != "YOUR_API_KEY_HERE" && api_secret != "YOUR_API_SECRET_HERE" {
            info!("Using default secrets for Last.fm");
        }
        
        Self::initialize(
            api_key.to_string(),
            api_secret.to_string()
        )
    }

    /// Get the singleton instance of LastfmClient
    pub fn get_instance() -> Result<LastfmClient, LastfmError> {
        let lastfm_guard = LASTFM_CLIENT.lock();
        match &*lastfm_guard {
            Some(client) => Ok(client.clone()),
            None => Err(LastfmError::ConfigError(
                "Last.fm client has not been initialized".to_string(),
            )),
        }
    }    /// Get authentication URL for user to authorize application
    pub fn get_auth_url(&mut self) -> Result<(String, String), LastfmError> { // Ensure return type is (String, String)
        // Get an auth token first
        let token = self.get_auth_token()?; // Removed .await
        
        let auth_url = format!(
            "{}?api_key={}&token={}", 
            LASTFM_AUTH_URL, 
            self.credentials.api_key,
            &token // token is already a String here
        );
        
        Ok((auth_url, token)) // Return the auth_url and the token itself
    }    
    
    pub fn disconnect(&mut self) -> Result<(), String> {
        debug!("Disconnecting Last.fm client: clearing session key and username from memory and secure store.");
        
        // Clear in-memory credentials
        self.credentials.session_key = None;
        self.credentials.username = None;
        self.credentials.auth_token = None;
        self.credentials.token_created = None;
        
        // Remove credentials from secure store
        if let Err(e) = SecurityStore::remove(LASTFM_SESSION_KEY_STORE) {
            debug!("Error removing Last.fm session key from security store: {}", e);
            // Continue with disconnect even if removal fails
        } else {
            debug!("Successfully removed Last.fm session key from security store");
        }
        
        if let Err(e) = SecurityStore::remove(LASTFM_USERNAME_STORE) {
            debug!("Error removing Last.fm username from security store: {}", e);
            // Continue with disconnect even if removal fails
        } else {
            debug!("Successfully removed Last.fm username from security store");
        }
        
        debug!("Last.fm credentials cleared from memory and secure store.");
        Ok(())
    }

    /// Get an authentication token from Last.fm
    pub fn get_auth_token(&mut self) -> Result<String, LastfmError> { // Made synchronous
        // REMOVED: Caching logic for auth_token.
        // Always fetch a new token when this method is called to start an auth flow.
        // The old logic was:
        // if let Some(token) = &self.credentials.auth_token {
        //     if let Some(created) = self.credentials.token_created {
        //         let now = SystemTime::now()
        //             .duration_since(SystemTime::UNIX_EPOCH)
        //             .unwrap()
        //             .as_secs();
        //         // Tokens are valid for 60 minutes
        //         if now - created < 3600 {
        //             debug!("(get_auth_token) Reusing existing auth token: {:?}", token);
        //             return Ok(token.clone());
        //         }
        //     }
        // }

        ratelimit::rate_limit("lastfm");

        let params = [("method", "auth.getToken")];

        debug!("(get_auth_token) Requesting new Last.fm auth token");
        let response_body = self.make_api_request(params.iter().copied(), false)?;
        
        let token_response: TokenResponse = serde_json::from_str(&response_body)
            .map_err(|e| LastfmError::ParsingError(format!("Failed to parse token response: {}, body: {}", e, response_body)))?;
        
        // Store the newly fetched token
        debug!("(get_auth_token) Received new token: {}. Storing it.", token_response.token);
        self.credentials.auth_token = Some(token_response.token.clone());
        self.credentials.token_created = Some(
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        );
        
        debug!("(get_auth_token) Stored new auth token: {:?}, created: {:?}", self.credentials.auth_token, self.credentials.token_created);
        Ok(token_response.token)
    }    
    
    /// Get a session key after user has authorized the application
    pub fn get_session(&mut self) -> Result<(String, String), LastfmError> {
        debug!("(get_session) Attempting to get session. Current auth_token: {:?}", self.credentials.auth_token);
        // Check if we have an auth_token (this should be the initial request token)
        let token = match &self.credentials.auth_token {
            Some(t) => t.clone(),
            None => {
                // If there's no auth_token, it means either get_auth_token was never called,
                // or the token was already successfully used and cleared.
                // Check if we are already authenticated.
                // Check if we are already authenticated.
                if self.is_authenticated() {
                    if let Some(username) = self.get_username() {
                         if let Some(session_key) = self.credentials.session_key.clone() {
                            info!("Already authenticated as {}. Re-confirming session.", username);
                            return Ok((session_key, username));
                         }
                    }
                }
                return Err(LastfmError::AuthError(
                    "No auth token available to attempt session retrieval. Please initiate authentication first.".to_string(),
                ));
            }
        };

        ratelimit::rate_limit("lastfm");

        let params = [
            ("method", "auth.getSession"),
            ("token", &token),
        ];

        debug!("Attempting to get Last.fm session with token: {}", token);
        // Pass the array directly, or a slice of it.
        let response_body = self.make_api_request(params.iter().copied(), true)?;
        
        // make_api_request now directly returns ApiError if Last.fm sends one.
        // If we reach here, it means Last.fm didn't return a JSON error object at the top level.
        // We can attempt to parse SessionResponse.

        let session_response: SessionResponse = serde_json::from_str(&response_body)
            .map_err(|e| {
                error!("Failed to parse session response: {}, body: {}", e, response_body);
                LastfmError::ParsingError(format!("Failed to parse session response: {}", e))
            })?;
        
        // Store the session
        self.credentials.session_key = Some(session_response.session.key.clone());
        self.credentials.username = Some(session_response.session.name.clone());
        
        // Clear the auth_token as it has been successfully used
        self.credentials.auth_token = None;
        self.credentials.token_created = None;
        
        // Store the session in security store
        self.store_credentials_to_store();

        info!("Successfully authenticated with Last.fm as user: {}", session_response.session.name);
        Ok((session_response.session.key, session_response.session.name))
    }
    
    /// Set authentication token for Last.fm
    /// Used in the auth callback to set the token received from Last.fm
    pub fn set_auth_token(&mut self, token: String) -> Result<(), LastfmError> {
        debug!("(set_auth_token) Attempting to set token. Current auth_token: {:?}. New token: {}", self.credentials.auth_token, token);
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_secs();
            
        self.credentials.auth_token = Some(token.clone()); // Clone token for logging too
        self.credentials.token_created = Some(now);
        
        debug!("(set_auth_token) Successfully set Last.fm auth token to: {}, created: {}. Current state: {:?}", 
               token, now, self.credentials.auth_token);
        Ok(())
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
    fn make_api_request<'a>(
        &self, 
        params: impl IntoIterator<Item = (&'a str, &'a str)> + Clone, 
        sign: bool
    ) -> Result<String, LastfmError> {
        let mut param_map: HashMap<String, String> = params
            .clone() // Clone params here if needed for logging before modification
            .into_iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();

        // Always add api_key and format, make_api_request is internal
        param_map.insert("api_key".to_string(), self.credentials.api_key.clone());
        param_map.insert("format".to_string(), "json".to_string());


        if sign {
            // Create signature string
            // Sort params alphabetically by key
            let mut sorted_params: Vec<(&String, &String)> = param_map.iter().collect();
            sorted_params.sort_by_key(|&(k, _)| k);

            let mut sig_string = String::new();
            for (k, v) in sorted_params {
                if k != "format" { // format is not included in signature base string
                    sig_string.push_str(k);
                    sig_string.push_str(v);
                }
            }
            sig_string.push_str(&self.credentials.api_secret);

            let digest = md5::compute(sig_string.as_bytes());
            param_map.insert("api_sig".to_string(), format!("{:x}", digest));
        }
        
        let method_for_log = param_map.get("method").cloned().unwrap_or_else(|| "unknown_method".to_string());
        // Log params, excluding api_secret if it were ever in param_map (it's not, but good practice)
        let log_params: HashMap<String, String> = param_map.iter()
            .filter(|(k, _)| k.as_str() != "api_secret" && k.as_str() != "api_key" && k.as_str() != "token") // also hide api_key and token from general logs
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        debug!("Last.fm API call: method={}, params={:?}", method_for_log, log_params);


        let request_url = LASTFM_API_ROOT;
        
        // Use POST for all requests, Last.fm API generally accepts this
        let request = self.client.post(request_url);
        let form_params: Vec<(&str, &str)> = param_map.iter().map(|(k,v)| (k.as_str(), v.as_str())).collect();

        let response = request.send_form(&form_params);

        match response {
            Ok(res) => {
                let _status = res.status(); // Mark as unused if not needed
                let body = res.into_string().map_err(|e| LastfmError::NetworkError(format!("Failed to read response body: {}", e)))?;
                
                // Log raw body for debugging if necessary, be careful with sensitive data in production logs
                // debug!("Last.fm API response: status={}, body={}", status, body);

                // Try to parse as Last.fm error first, even on 200 OK
                if let Ok(error_response) = serde_json::from_str::<LastfmErrorResponse>(&body) {
                    // It's a Last.fm API error (e.g. token not authorized, invalid params)
                    debug!("Last.fm API returned an error: code={}, message='{}'", error_response.error, error_response.message);
                    return Err(LastfmError::ApiError(error_response.message, error_response.error));
                }

                // If not a Last.fm error response, assume it's a success payload
                // The caller will then try to parse it into its expected struct (e.g., TokenResponse, SessionResponse)
                Ok(body)
            }
            Err(ureq::Error::Status(code, response)) => {
                let error_body = response.into_string().unwrap_or_else(|_| "<empty response body>".to_string());
                error!("Last.fm API HTTP error: {} - Body: {}", code, error_body);
                // Try to parse error_body as LastfmErrorResponse as well, as Last.fm might return structured errors on HTTP error codes
                if let Ok(error_response) = serde_json::from_str::<LastfmErrorResponse>(&error_body) {
                     Err(LastfmError::ApiError(error_response.message, error_response.error))
                } else {
                     Err(LastfmError::NetworkError(format!("HTTP error {} with unparseable body: {}", code, error_body)))
                }
            }
            Err(e) => { // Other errors like transport errors
                error!("Last.fm API request failed (ureq error): {}", e.to_string());
                Err(LastfmError::NetworkError(e.to_string()))
            }
        }
    }

    /// Store credentials to security store
    fn store_credentials_to_store(&self) {
        if let Some(session_key) = &self.credentials.session_key {
            if let Err(e) = SecurityStore::set(LASTFM_SESSION_KEY_STORE, session_key) {
                log::warn!("Failed to store Last.fm session key: {}", e);
            } else {
                debug!("Stored Last.fm session key in security store");
            }
        }
        
        if let Some(username) = &self.credentials.username {
            if let Err(e) = SecurityStore::set(LASTFM_USERNAME_STORE, username) {
                log::warn!("Failed to store Last.fm username: {}", e);
            } else {
                debug!("Stored Last.fm username in security store");
            }
        }
    }

    // Clone implementation for the client
    fn clone(&self) -> Self {
        LastfmClient {
            credentials: self.credentials.clone(),
            client: ureq::agent(),
        }
    }    fn load_credentials_from_store(&mut self) {
        // Try to get session key from security store
        match SecurityStore::get(LASTFM_SESSION_KEY_STORE) {
            Ok(session_key) => {
                self.credentials.session_key = Some(session_key);
                debug!("Loaded Last.fm session key from store");
            }
            Err(e) => {
                if let SecurityStoreError::KeyNotFound(_) = e {
                    debug!("No Last.fm session key found in security store");
                } else {
                    debug!("Error loading Last.fm session key from security store: {}", e);
                }
            }
        }
        
        // Try to get username from security store
        match SecurityStore::get(LASTFM_USERNAME_STORE) {
            Ok(username) => {
                self.credentials.username = Some(username);
                debug!("Loaded Last.fm username from store");
            }
            Err(e) => {
                if let SecurityStoreError::KeyNotFound(_) = e {
                    debug!("No Last.fm username found in security store");
                } else {
                    debug!("Error loading Last.fm username from security store: {}", e);
                }
            }
        }
    }

    // Create an instance from credentials
    #[allow(dead_code)] // Function not currently used
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

        let mut lastfm_guard = LASTFM_CLIENT.lock();
        *lastfm_guard = Some(client);

        info!("Last.fm client initialized from stored credentials");
        Ok(())
    }

    /// Get detailed information for a track, including user-specific data like playcount and loved status.
    ///
    /// # Arguments
    /// * `artist` - The artist name.
    /// * `title` - The track title.
    ///
    /// # Returns
    /// Result containing `LastfmTrackInfoDetails` or an error.
    pub fn get_track_info(&self, artist: &str, title: &str) -> Result<LastfmTrackInfoDetails, LastfmError> {
        if !self.is_authenticated() {
            // While track.getInfo can be called without auth, user specific fields won't be present.
            // The request implies wanting user-specific data.
            return Err(LastfmError::AuthError(
                "Authentication required to fetch user-specific track information (e.g., loved status).".to_string(),
            ));
        }

        let session_key = self.credentials.session_key.as_ref().ok_or_else(|| {
            error!("Session key not found for authenticated user while calling get_track_info.");
            LastfmError::AuthError("Session key not found despite being authenticated.".to_string())
        })?;
        
        // username is not strictly needed if sk is provided, Last.fm infers user from sk.
        // let username = self.credentials.username.as_ref().ok_or_else(|| {
        //     error!("Username not found for authenticated user while calling get_track_info.");
        //     LastfmError::AuthError("Username not found despite being authenticated.".to_string())
        // })?;

        ratelimit::rate_limit("lastfm");

        let params = vec![
            ("method", "track.getInfo"),
            ("artist", artist),
            ("track", title),
            ("sk", session_key.as_str()), // Session key for user-specific data
            ("autocorrect", "1"),       // Enable autocorrection
            // api_key is added by make_api_request
        ];
        
        // If username is available and you want to explicitly pass it (though sk should be enough)
        // if let Some(uname) = self.credentials.username.as_ref() {
        //    params.push(("username", uname.as_str()));
        // }


        // This request should be signed because it uses 'sk'
        let response_body = self.make_api_request(params.into_iter(), true)?;

        match serde_json::from_str::<LastfmTrackInfoResponse>(&response_body) {
            Ok(parsed_response) => Ok(parsed_response.track),
            Err(e) => {
                error!(
                    "Failed to parse track.getInfo response for artist '{}', title '{}'. Error: {}, Body: {}",
                    artist, title, e, response_body
                );
                Err(LastfmError::ParsingError(format!(
                    "Failed to parse track.getInfo response: {}. Body: {}", e, response_body
                )))
            }
        }
    }

    /// Get artist information from Last.fm
    /// 
    /// # Arguments
    /// * `artist` - The artist name.
    ///
    /// # Returns
    /// Result containing `LastfmArtistDetails` or an error.
    pub fn get_artist_info(&self, artist: &str) -> Result<LastfmArtistDetails, LastfmError> {
        ratelimit::rate_limit("lastfm");

        let params = vec![
            ("method", "artist.getInfo"),
            ("artist", artist),
            ("autocorrect", "0"),       // Disable autocorrection
            // api_key is added by make_api_request
        ];

        // This request does not need to be signed (no user-specific data)
        debug!("Requesting artist.getInfo for artist: {}", artist);
        let response_body = self.make_api_request(params.into_iter(), false)?;

        match serde_json::from_str::<LastfmArtistInfoResponse>(&response_body) {
            Ok(parsed_response) => Ok(parsed_response.artist),
            Err(e) => {
                error!(
                    "Failed to parse artist.getInfo response for artist '{}'. Error: {}, Body: {}",
                    artist, e, response_body
                );
                Err(LastfmError::ParsingError(format!(
                    "Failed to parse artist.getInfo response: {}. Body: {}", e, response_body
                )))
            }
        }
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

    /// Love a track on Last.fm
    pub fn love_track(&self, artist: &str, track: &str) -> Result<(), LastfmError> {
        if !self.is_authenticated() {
            return Err(LastfmError::AuthError("Authentication required to love tracks".to_string()));
        }

        let session_key = self.credentials.session_key.as_ref().ok_or_else(|| {
            error!("Session key not found for authenticated user while calling love_track.");
            LastfmError::AuthError("Session key not found despite being authenticated.".to_string())
        })?;

        let params = vec![
            ("method", "track.love"),
            ("artist", artist),
            ("track", track),
            ("sk", session_key.as_str()),
        ];

        // This request needs to be signed
        let _response = self.make_api_request(params, true)?;
        
        debug!("Track loved: {} - {}", artist, track);
        Ok(())
    }

    /// Unlove a track on Last.fm
    pub fn unlove_track(&self, artist: &str, track: &str) -> Result<(), LastfmError> {
        if !self.is_authenticated() {
            return Err(LastfmError::AuthError("Authentication required to unlove tracks".to_string()));
        }

        let session_key = self.credentials.session_key.as_ref().ok_or_else(|| {
            error!("Session key not found for authenticated user while calling unlove_track.");
            LastfmError::AuthError("Session key not found despite being authenticated.".to_string())
        })?;

        let params = vec![
            ("method", "track.unlove"),
            ("artist", artist),
            ("track", track),
            ("sk", session_key.as_str()),
        ];

        // This request needs to be signed
        let _response = self.make_api_request(params, true)?;
        
        debug!("Track unloved: {} - {}", artist, track);
        Ok(())
    }

    /// Check if a track is loved on Last.fm
    pub fn is_track_loved(&self, artist: &str, track: &str) -> Result<bool, LastfmError> {
        if !self.is_authenticated() {
            return Err(LastfmError::AuthError("Authentication required to check love status".to_string()));
        }

        // Use the existing get_track_info method which includes userloved status
        match self.get_track_info(artist, track) {
            Ok(track_info) => Ok(track_info.userloved),
            Err(e) => Err(e),
        }
    }

}


#[derive(Debug, Deserialize, Clone, Serialize)]
pub struct LovedTrackDate {
    pub uts: String,
    #[serde(rename = "#text")]
    pub text: String,
}

#[derive(Debug, Deserialize, Clone, Serialize)]
pub struct LovedTrackArtist {
    pub name: String,
    pub mbid: Option<String>,
    pub url: String,
}

#[derive(Debug, Deserialize, Clone, Serialize)]
pub struct LastfmImage {
    pub size: String,
    #[serde(rename = "#text")]
    pub url: String,
}

#[derive(Debug, Deserialize, Clone, Serialize)]
pub struct LovedTrack {
    pub name: String,
    pub mbid: Option<String>,
    pub url: String,
    pub date: LovedTrackDate,
    pub artist: LovedTrackArtist,
    pub image: Option<Vec<LastfmImage>>,
    // streamable can be complex, omitting for now unless needed
}

/// Last.fm Artist Updater
/// 
/// Implements the ArtistUpdater trait to fetch artist information from Last.fm
pub struct LastfmUpdater;

impl LastfmUpdater {
    pub fn new() -> Self {
        LastfmUpdater
    }
}

impl crate::helpers::ArtistUpdater for LastfmUpdater {
    /// Updates artist information using Last.fm service
    /// 
    /// This function fetches artist bio, tags, and images from Last.fm and adds them to the artist metadata.
    /// Note: Last.fm doesn't require a MusicBrainz ID and works with just the artist name.
    /// 
    /// # Arguments
    /// * `artist` - The artist to update
    /// 
    /// # Returns
    /// The updated artist with Last.fm data
    fn update_artist(&self, mut artist: crate::data::artist::Artist) -> crate::data::artist::Artist {
        debug!("Updating artist {} with Last.fm data", artist.name);
        
        // Get the Last.fm client instance
        let lastfm_client = {
            let guard = LASTFM_CLIENT.lock();
            match guard.as_ref() {
                Some(client) => client.clone(),
                None => {
                    debug!("Last.fm client not initialized, skipping Last.fm lookup for artist {}", artist.name);
                    return artist;
                }
            }
        };
        
        // Get artist info from Last.fm
        match lastfm_client.get_artist_info(&artist.name) {
            Ok(artist_info) => {
                debug!("Successfully retrieved Last.fm data for artist {}", artist.name);
                
                let mut updated_data = Vec::new();
                
                // Ensure we have metadata
                if artist.metadata.is_none() {
                    artist.metadata = Some(crate::data::metadata::ArtistMeta::new());
                }
                
                if let Some(meta) = &mut artist.metadata {
                    // Add biography from Last.fm (use content, which is the full version)
                    if let Some(bio) = &artist_info.bio {
                        if !bio.content.is_empty() {
                            let cleaned_biography = cleanup_biography(&bio.content);
                            meta.biography = Some(cleaned_biography);
                            meta.biography_source = Some("LastFM".to_string());
                            updated_data.push("biography".to_string());
                            debug!("Added Last.fm biography for artist {}", artist.name);
                        }
                    }
                    
                    // Add tags/genres from Last.fm
                    if let Some(tags) = &artist_info.tags {
                        if !tags.tags.is_empty() {
                            for tag in &tags.tags {
                                meta.add_genre(tag.name.clone());
                            }
                            updated_data.push(format!("{} tags", tags.tags.len()));
                            debug!("Added {} Last.fm tags for artist {}", tags.tags.len(), artist.name);
                        }
                    }
                    
                    // Add MusicBrainz ID if available and not already present
                    if let Some(mbid) = &artist_info.mbid {
                        if !mbid.is_empty() && !meta.mbid.contains(mbid) {
                            meta.add_mbid(mbid.clone());
                            updated_data.push("MusicBrainz ID".to_string());
                            debug!("Added Last.fm MusicBrainz ID for artist {}: {}", artist.name, mbid);
                        }
                    }
                }
                
                // Log successful update with summary of what was added
                if !updated_data.is_empty() {
                    info!("Updated artist '{}' with Last.fm data: {}", artist.name, updated_data.join(", "));
                }
            }
            Err(e) => {
                debug!("Failed to get Last.fm data for artist {}: {}", artist.name, e);
            }
        }
        
        artist
    }
}

/// Clean up Last.fm biography text by removing HTML links and unwanted text
/// 
/// This function removes Last.fm specific content like:
/// - "<a href="https://www.last.fm/music/Artist">Read more on Last.fm</a>"
/// - Other similar Last.fm promotional text
pub fn cleanup_biography(biography: &str) -> String {
    // Regex to match Last.fm links and "Read more" text
    static LASTFM_LINK_REGEX: Lazy<Regex> = Lazy::new(|| {
        Regex::new(r#"<a\s+href="https://www\.last\.fm/music/[^"]*">Read more on Last\.fm</a>"#)
            .expect("Failed to compile Last.fm link regex")
    });
    
    // Remove Last.fm links
    let cleaned = LASTFM_LINK_REGEX.replace_all(biography, "");
    
    // Clean up any trailing whitespace and periods that may be left over
    let cleaned = cleaned.trim_end();
    
    // Remove trailing period if it was immediately before the removed link
    let cleaned = if biography.contains(r#". <a href="https://www.last.fm/music/"#) && cleaned.ends_with('.') {
        cleaned.trim_end_matches('.')
    } else {
        cleaned
    };
    
    cleaned.to_string()
}



/// Love a track on Last.fm
/// 
/// # Arguments
/// * `artist` - The artist name
/// * `track` - The track name
/// 
/// # Returns
/// Result indicating success or failure
pub fn love_track(artist: &str, track: &str) -> Result<(), LastfmError> {
    let client = LastfmClient::get_instance()?;
    client.love_track(artist, track)
}

/// Unlove a track on Last.fm
/// 
/// # Arguments
/// * `artist` - The artist name
/// * `track` - The track name
/// 
/// # Returns
/// Result indicating success or failure
pub fn unlove_track(artist: &str, track: &str) -> Result<(), LastfmError> {
    let client = LastfmClient::get_instance()?;
    client.unlove_track(artist, track)
}

/// Check if a track is loved on Last.fm
/// 
/// # Arguments
/// * `artist` - The artist name
/// * `track` - The track name
/// 
/// # Returns
/// Result containing true if loved, false if not
pub fn is_track_loved(artist: &str, track: &str) -> Result<bool, LastfmError> {
    let client = LastfmClient::get_instance()?;
    client.is_track_loved(artist, track)
}

/// Last.fm implementation of FavouriteProvider
pub struct LastfmFavouriteProvider;

impl LastfmFavouriteProvider {
    pub fn new() -> Self {
        Self
    }
}

impl crate::helpers::favourites::FavouriteProvider for LastfmFavouriteProvider {
    fn is_favourite(&self, song: &crate::data::song::Song) -> Result<bool, crate::helpers::favourites::FavouriteError> {
        let artist = song.artist.as_ref()
            .ok_or_else(|| crate::helpers::favourites::FavouriteError::InvalidSong("Artist is required".to_string()))?;
        let title = song.title.as_ref()
            .ok_or_else(|| crate::helpers::favourites::FavouriteError::InvalidSong("Title is required".to_string()))?;

        match is_track_loved(artist, title) {
            Ok(loved) => Ok(loved),
            Err(LastfmError::AuthError(msg)) => Err(crate::helpers::favourites::FavouriteError::AuthError(msg)),
            Err(LastfmError::NetworkError(msg)) => Err(crate::helpers::favourites::FavouriteError::NetworkError(msg)),
            Err(LastfmError::ConfigError(msg)) => Err(crate::helpers::favourites::FavouriteError::NotConfigured(msg)),
            Err(e) => Err(crate::helpers::favourites::FavouriteError::Other(e.to_string())),
        }
    }

    fn add_favourite(&self, song: &crate::data::song::Song) -> Result<(), crate::helpers::favourites::FavouriteError> {
        let artist = song.artist.as_ref()
            .ok_or_else(|| crate::helpers::favourites::FavouriteError::InvalidSong("Artist is required".to_string()))?;
        let title = song.title.as_ref()
            .ok_or_else(|| crate::helpers::favourites::FavouriteError::InvalidSong("Title is required".to_string()))?;

        match love_track(artist, title) {
            Ok(()) => Ok(()),
            Err(LastfmError::AuthError(msg)) => Err(crate::helpers::favourites::FavouriteError::AuthError(msg)),
            Err(LastfmError::NetworkError(msg)) => Err(crate::helpers::favourites::FavouriteError::NetworkError(msg)),
            Err(LastfmError::ConfigError(msg)) => Err(crate::helpers::favourites::FavouriteError::NotConfigured(msg)),
            Err(e) => Err(crate::helpers::favourites::FavouriteError::Other(e.to_string())),
        }
    }

    fn remove_favourite(&self, song: &crate::data::song::Song) -> Result<(), crate::helpers::favourites::FavouriteError> {
        let artist = song.artist.as_ref()
            .ok_or_else(|| crate::helpers::favourites::FavouriteError::InvalidSong("Artist is required".to_string()))?;
        let title = song.title.as_ref()
            .ok_or_else(|| crate::helpers::favourites::FavouriteError::InvalidSong("Title is required".to_string()))?;

        match unlove_track(artist, title) {
            Ok(()) => Ok(()),
            Err(LastfmError::AuthError(msg)) => Err(crate::helpers::favourites::FavouriteError::AuthError(msg)),
            Err(LastfmError::NetworkError(msg)) => Err(crate::helpers::favourites::FavouriteError::NetworkError(msg)),
            Err(LastfmError::ConfigError(msg)) => Err(crate::helpers::favourites::FavouriteError::NotConfigured(msg)),
            Err(e) => Err(crate::helpers::favourites::FavouriteError::Other(e.to_string())),
        }
    }

    fn get_favourite_count(&self) -> Option<usize> {
        // Last.fm API doesn't provide an easy way to get the total count of loved tracks
        // Would require paginating through all loved tracks, which is not efficient
        None
    }

    fn provider_name(&self) -> &'static str {
        "lastfm"
    }

    fn display_name(&self) -> &'static str {
        "Last.fm"
    }

    fn is_enabled(&self) -> bool {
        // Check if Last.fm is configured and authenticated
        match LastfmClient::get_instance() {
            Ok(client) => client.is_authenticated(),
            Err(_) => false,
        }
    }

    fn is_active(&self) -> bool {
        // For Last.fm, active means the user is currently logged in (authenticated)
        // This is the same as is_enabled for now, but conceptually different:
        // - is_enabled: provider is configured/available
        // - is_active: provider is functional (user logged in)
        match LastfmClient::get_instance() {
            Ok(client) => client.is_authenticated(),
            Err(_) => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::cleanup_biography;

    #[test]
    fn test_cleanup_biography_removes_lastfm_links() {
        let test_cases = vec![
            (
                r#"Metallica is an American heavy metal band. The band was formed in 1981 in Los Angeles by vocalist/guitarist James Hetfield and drummer Lars Ulrich. <a href="https://www.last.fm/music/Metallica">Read more on Last.fm</a>"#,
                "Metallica is an American heavy metal band. The band was formed in 1981 in Los Angeles by vocalist/guitarist James Hetfield and drummer Lars Ulrich"
            ),
            (
                r#"The Beatles were an English rock band formed in Liverpool in 1960. <a href="https://www.last.fm/music/The+Beatles">Read more on Last.fm</a>"#,
                "The Beatles were an English rock band formed in Liverpool in 1960"
            ),
            (
                "This is a biography without any Last.fm links.",
                "This is a biography without any Last.fm links."
            ),
            (
                r#"Pink Floyd were an English rock band. <a href="https://www.last.fm/music/Pink+Floyd">Read more on Last.fm</a>    "#,
                "Pink Floyd were an English rock band"
            ),
            (
                r#"Queen were a British rock band that achieved worldwide fame. <a href="https://www.last.fm/music/Queen">Read more on Last.fm</a>"#,
                "Queen were a British rock band that achieved worldwide fame"
            ),
        ];

        for (input, expected) in test_cases {
            let result = cleanup_biography(input);
            assert_eq!(result, expected, "Failed for input: {}", input);
        }
    }

    #[test] 
    fn test_cleanup_biography_handles_empty_string() {
        let result = cleanup_biography("");
        assert_eq!(result, "");
    }

    #[test]
    fn test_cleanup_biography_handles_only_link() {
        let input = r#"<a href="https://www.last.fm/music/TestArtist">Read more on Last.fm</a>"#;
        let result = cleanup_biography(input);
        assert_eq!(result, "");
    }
}
