use serde::{Deserialize, Serialize, Deserializer};
use serde_json::Value;
use log::{debug, error};
use crate::helpers::macaddress::normalize_mac_address;
use crate::helpers::http_client::{HttpClient, HttpClientError, new_http_client, post_json};
use std::sync::Arc;
use parking_lot::Mutex;
use std::sync::atomic::{AtomicU32, Ordering};

/// The standard JSON-RPC path for Lyrion Music Server
const JSONRPC_PATH: &str = "/jsonrpc.js";

/// Default timeout for HTTP requests in seconds
const DEFAULT_TIMEOUT_SECS: u64 = 15;

/// Errors that can occur when interacting with the LMS JSON-RPC API
#[derive(Debug, thiserror::Error)]
pub enum LmsRpcError {
    #[error("HTTP request error: {0}")]
    RequestError(String),

    #[error("Failed to parse response: {0}")]
    ParseError(String),

    #[error("LMS server error: {0}")]
    ServerError(String),

    #[error("Empty response from server")]
    EmptyResponse,
}

// Convert from HttpClientError to LmsRpcError
impl From<HttpClientError> for LmsRpcError {
    fn from(error: HttpClientError) -> Self {
        match error {
            HttpClientError::RequestError(msg) => LmsRpcError::RequestError(msg),
            HttpClientError::ParseError(msg) => LmsRpcError::ParseError(msg),
            HttpClientError::ServerError(msg) => LmsRpcError::ServerError(msg),
            HttpClientError::EmptyResponse => LmsRpcError::EmptyResponse,
        }
    }
}

/// Request structure for LMS JSON-RPC API
/// LMS uses a non-standard JSON-RPC format
#[derive(Debug, Serialize)]
struct JsonRpcRequest {
    id: u32,
    method: String,
    #[serde(rename = "params")]
    params: Vec<Value>,
}

/// Response structure for LMS JSON-RPC API
#[derive(Debug, Deserialize)]
struct JsonRpcResponse {
    #[allow(dead_code)]
    id: Value,
    #[serde(default)]
    result: Value,
    #[serde(default)]
    #[allow(dead_code)]
    error: Option<Value>,
}

/// LMS JSON-RPC client for communicating with a Lyrion Music Server
#[derive(Debug, Clone)]
pub struct LmsRpcClient {
    /// Base URL of the LMS server (e.g., "http://192.168.1.100:9000")
    base_url: String,
    
    /// HTTP client for making requests
    client: Arc<dyn HttpClient>, // Changed from Box<dyn HttpClient>
    
    /// Request counter for unique IDs
    request_id: Arc<AtomicU32>, // Changed from u32
    
    /// Lock to serialize requests
    request_lock: Arc<Mutex<()>>, // New field
}

impl LmsRpcClient {
    /// Create a new LMS JSON-RPC client
    /// 
    /// # Arguments
    /// * `host` - Hostname or IP address of the LMS server
    /// * `port` - HTTP port of the LMS server (typically 9000)
    pub fn new(host: &str, port: u16) -> Self {
        let base_url = format!("http://{}:{}", host, port);
        let client = Arc::from(new_http_client(DEFAULT_TIMEOUT_SECS)); // Wrapped in Arc
            
        LmsRpcClient {
            base_url,
            client,
            request_id: Arc::new(AtomicU32::new(1)), // Initialize shared counter
            request_lock: Arc::new(Mutex::new(())), // Initialize shared lock
        }
    }
    
    /// Set a custom timeout for the client
    pub fn with_timeout(self, timeout_secs: u64) -> Self {
        Self {
            base_url: self.base_url,
            client: Arc::from(new_http_client(timeout_secs)), // Create new client, wrap in Arc
            request_id: self.request_id, // Clone Arc, shares counter
            request_lock: self.request_lock, // Clone Arc, shares lock
        }
    }
    
    /// Set a specific HTTP client implementation
    pub fn with_client(self, client: Box<dyn HttpClient>) -> Self {
        Self {
            base_url: self.base_url,
            client: Arc::from(client), // Convert Box to Arc
            request_id: self.request_id, // Clone Arc, shares counter
            request_lock: self.request_lock, // Clone Arc, shares lock
        }
    }
    
    /// Get the next request ID
    fn next_id(&self) -> u32 {
        self.request_id.fetch_add(1, Ordering::SeqCst) // Use atomic counter
    }
    
    /// Send a database query command
    /// 
    /// # Arguments
    /// * `command` - Command name (e.g., "artists", "albums", "titles")
    /// * `start` - Start index for pagination (0-based)
    /// * `items_per_response` - Number of items to return per response
    /// * `params` - Tagged parameters as key-value pairs (e.g., ("tags", "a"))
    /// 
    /// # Returns
    /// The result field of the response as a JSON Value
    pub fn database_request(&self, command: &str, start: u32, items_per_response: u32, 
                  params: Vec<(&str, &str)>) -> Result<Value, LmsRpcError> {
        debug!("Command: {}, start: {}, items: {}, params: {:?}", 
               command, start, items_per_response, params);
        
        // Build command with proper format: command start itemsPerResponse tag1:value1 tag2:value2...
        let mut command_values = vec![
            Value::String(command.to_string()),
            Value::String(start.to_string()), // Encode as string instead of number
            Value::String(items_per_response.to_string()), // Encode as string instead of number
        ];
        
        // Add tagged parameters
        for (tag, value) in params {
            // Special case for query parameters denoted by "?"
            if tag == "?" {
                // Just add "?" directly without the colon and value
                command_values.push(Value::String("?".to_string()));
            } else if tag == "" {
                // If the tag is empty, just add the value
                command_values.push(Value::String(value.to_string()));
            } else {
                // For normal parameters, format as "tag:value"
                // But if the value is empty, just use the tag (for toggles)
                let tagged_param = if value.is_empty() {
                    tag.to_string()
                } else {
                    format!("{}:{}", tag, value)
                };
                command_values.push(Value::String(tagged_param));
            }
        }

        // Pass None for player_id to use the default "0" for database commands
        self.request_raw(None, command_values)
    }
    
    /// Send a raw command to a specific player with mixed parameter types
    /// 
    /// # Arguments
    /// * `player_id` - Optional MAC address of player. If None, "0" will be used for database commands
    /// * `command` - Command array as JSON Values for mixed types
    pub fn request_raw(&self, player_id: Option<&str>, command: Vec<Value>) -> Result<Value, LmsRpcError> {
        // Acquire the lock before proceeding with the request.
        // The lock is released when `_guard` goes out of scope.
        let _guard = self.request_lock.lock();

        // The LMS jsonrpc.js API expects params to be an array with:
        // 1. The player_id as the first element (or "0" for command that doesn't require a player)
        // 2. A nested array containing the command and parameters as the second element
        
        // Debug log the command before creating the request
        let url = format!("{}{}", self.base_url, JSONRPC_PATH);
        debug!("LMS command to {}: player_id={:?}, command={:?}", url, player_id, command);
        
        // Create the nested command array
        let command_array = Value::Array(command.clone());
        
        // Create params array with player_id followed by the command array
        let params = vec![
            Value::String(player_id.unwrap_or("0").to_string()),
            command_array
        ];
        
        let request = JsonRpcRequest {
            id: self.next_id(),
            method: "slim.request".to_string(),
            params,
        };
        
        debug!("Sending LMS request to {}: {:?}", url, request);

        match post_json(self.client.as_ref(), &url, &request) {
            Ok(response) => {
                // Parse the response as a JsonRpcResponse
                match serde_json::from_value::<JsonRpcResponse>(response) {
                    Ok(json_response) => {
                        debug!("LMS response: {:?}", json_response.result);
                        Ok(json_response.result)
                    },
                    Err(e) => {
                        error!("Failed to parse LMS response: {}", e);
                        Err(LmsRpcError::ParseError(e.to_string()))
                    }
                }
            },
            Err(e) => {
                error!("Request failed: {}", e);
                Err(LmsRpcError::RequestError(e.to_string()))
            }
        }
    }
    
    /// Send a control command to a specific player (without pagination parameters)
    /// 
    /// # Arguments
    /// * `player_id` - MAC address of player (e.g., "00:04:20:ab:cd:ef")
    /// * `command` - Command name (e.g., "pause", "play", "stop")
    /// * `args` - Command arguments as simple values without tags
    /// 
    /// # Returns
    /// The result field of the response as a JSON Value
    pub fn control_request(&self, player_id: &str, command: &str, 
                          args: Vec<&str>) -> Result<Value, LmsRpcError> {
        debug!("Control command: {}, args: {:?}", command, args);
        
        // Build command with proper format: command arg1 arg2...
        let mut command_values = vec![
            Value::String(command.to_string()),
        ];
        
        // Add arguments as simple values
        for arg in args {
            command_values.push(Value::String(arg.to_string()));
        }

        debug!("Control command values: {:?}", command_values);

        self.request_raw(Some(player_id), command_values)
    }
    
    /// Get a list of available players
    pub fn get_players(&self) -> Result<Vec<Player>, LmsRpcError> {
        let result = self.control_request("0:0:0:0:0:0:0:0", "players", vec!["0", "100"])?;
        
        // Extract the players array
        match result.get("players_loop") {
            Some(players_array) => {
                match serde_json::from_value::<Vec<Player>>(players_array.clone()) {
                    Ok(players) => Ok(players),
                    Err(e) => Err(LmsRpcError::ParseError(format!("Failed to parse players: {}", e))),
                }
            },
            None => Ok(Vec::new()), // No players available
        }
    }
    
    /// Get player status including current track info
    pub fn get_player_status(&self, player_id: &str) -> Result<PlayerStatus, LmsRpcError> {
        // Use control_request since we need to address a specific player
        let result = self.control_request(player_id, "status", vec!["0", "1", "tags:abcltiqyKo"])?;
        
        match serde_json::from_value::<PlayerStatus>(result.clone()) {
            Ok(status) => Ok(status),
            Err(e) => {
                error!("Failed to parse player status: {}", e);
                error!("Status data: {:?}", result);
                Err(LmsRpcError::ParseError(format!("Failed to parse player status: {}", e)))
            }
        }
    }
    
    /// Get the server address (hostname or IP) from the base URL
    /// 
    /// # Returns
    /// The server address as a String if it can be extracted
    pub fn get_server_address(&self) -> Result<String, LmsRpcError> {
        // Parse the base URL to extract the server address
        if let Some(stripped) = self.base_url.strip_prefix("http://") {
            if let Some(index) = stripped.find(':') {
                return Ok(stripped[..index].to_string());
            }
            return Ok(stripped.to_string());
        }
        
        Err(LmsRpcError::ParseError("Could not extract server address from base URL".to_string()))
    }
    
    /// Get the server port from the base URL
    /// 
    /// # Returns
    /// The server port number
    pub fn get_server_port(&self) -> u16 {
        // Parse the base URL to extract the port
        if let Some(stripped) = self.base_url.strip_prefix("http://") {
            if let Some(index) = stripped.find(':') {
                if let Some(port_str) = stripped.get((index + 1)..) {
                    if let Ok(port) = port_str.parse::<u16>() {
                        return port;
                    }
                }
            }
        }
        
        // Default LMS port if we couldn't extract it
        9000
    }
    
    /// Play the current track
    pub fn play(&self, player_id: &str) -> Result<Value, LmsRpcError> {
        self.control_request(player_id, "play", vec![])
    }
    
    /// Pause the current track
    pub fn pause(&self, player_id: &str) -> Result<Value, LmsRpcError> {
        self.control_request(player_id, "pause", vec!["1"])
    }
    
    /// Toggle pause/play
    pub fn toggle_pause(&self, player_id: &str) -> Result<Value, LmsRpcError> {
        self.control_request(player_id, "pause", vec![])
    }
    
    /// Stop playback
    pub fn stop(&self, player_id: &str) -> Result<Value, LmsRpcError> {
        self.control_request(player_id, "stop", vec![])
    }
    
    /// Skip to next track
    pub fn next(&self, player_id: &str) -> Result<Value, LmsRpcError> {
        self.control_request(player_id, "playlist", vec!["index", "+1"])
    }
    
    /// Skip to previous track
    pub fn previous(&self, player_id: &str) -> Result<Value, LmsRpcError> {
        self.control_request(player_id, "playlist", vec!["index", "-1"])
    }
    
    /// Set volume (0-100)
    pub fn set_volume(&self, player_id: &str, volume: u8) -> Result<Value, LmsRpcError> {
        let volume = volume.min(100);
        self.control_request(player_id, "mixer", vec!["volume", &volume.to_string()])
    }
    
    /// Get current volume
    pub fn get_volume(&self, player_id: &str) -> Result<u8, LmsRpcError> {
        let result = self.control_request(player_id, "mixer", vec!["volume", "?"])?;
        
        match result.get("_volume") {
            Some(volume) => {
                volume.as_u64()
                    .map(|v| v as u8)
                    .ok_or_else(|| LmsRpcError::ParseError("Volume is not a number".to_string()))
            },
            None => Err(LmsRpcError::ParseError("Volume not found in response".to_string())),
        }
    }
    
    /// Set mute status
    pub fn set_mute(&self, player_id: &str, mute: bool) -> Result<Value, LmsRpcError> {
        let mute_val = if mute { "1" } else { "0" };
        self.control_request(player_id, "mixer", vec!["muting", mute_val])
    }
    
    /// Toggle mute status
    pub fn toggle_mute(&self, player_id: &str) -> Result<Value, LmsRpcError> {
        self.control_request(player_id, "mixer", vec!["muting"])
    }
    
    /// Get mute status
    pub fn is_muted(&self, player_id: &str) -> Result<bool, LmsRpcError> {
        let result = self.control_request(player_id, "mixer", vec!["muting", "?"])?;
        
        match result.get("_muting") {
            Some(muting) => {
                muting.as_i64()
                    .map(|v| v != 0)
                    .ok_or_else(|| LmsRpcError::ParseError("Muting value is not a number".to_string()))
            },
            None => Err(LmsRpcError::ParseError("Muting status not found in response".to_string())),
        }
    }

    /// Seek to a position (in seconds) in the current track
    pub fn seek(&self, player_id: &str, seconds: f32) -> Result<Value, LmsRpcError> {
        // Convert seconds to format expected by LMS
        let time_str = format!("{:.1}", seconds);
        self.control_request(player_id, "time", vec![&time_str])
    }
    
    /// Set shuffle mode (0=off, 1=songs, 2=albums)
    pub fn set_shuffle(&self, player_id: &str, shuffle_mode: u8) -> Result<Value, LmsRpcError> {
        let mode = shuffle_mode.min(2).to_string();
        self.control_request(player_id, "playlist", vec!["shuffle", &mode])
    }
    
    /// Get shuffle mode
    pub fn get_shuffle(&self, player_id: &str) -> Result<u8, LmsRpcError> {
        let result = self.control_request(player_id, "playlist", vec!["shuffle", "?"])?;
        
        match result.get("_shuffle") {
            Some(shuffle) => {
                shuffle.as_u64()
                    .map(|v| v as u8)
                    .ok_or_else(|| LmsRpcError::ParseError("Shuffle mode is not a number".to_string()))
            },
            None => Err(LmsRpcError::ParseError("Shuffle mode not found in response".to_string())),
        }
    }
    
    /// Set repeat mode (0=off, 1=song, 2=playlist)
    pub fn set_repeat(&self, player_id: &str, repeat_mode: u8) -> Result<Value, LmsRpcError> {
        let mode = repeat_mode.min(2).to_string();
        self.control_request(player_id, "playlist", vec!["repeat", &mode])
    }
    
    /// Get repeat mode
    pub fn get_repeat(&self, player_id: &str) -> Result<u8, LmsRpcError> {
        let result = self.control_request(player_id, "playlist", vec!["repeat", "?"])?;
        
        match result.get("_repeat") {
            Some(repeat) => {
                repeat.as_u64()
                    .map(|v| v as u8)
                    .ok_or_else(|| LmsRpcError::ParseError("Repeat mode is not a number".to_string()))
            },
            None => Err(LmsRpcError::ParseError("Repeat mode not found in response".to_string())),
        }
    }
    
    /// Check if a specific MAC address is connected to this LMS server
    /// If no MAC address is provided, it will check all local interfaces
    pub fn is_connected(&self, mac_addr: Option<&str>) -> Result<bool, LmsRpcError> {
        // Get players to check connections
        let players = self.get_players()?;
        
        // Get MAC addresses to check
        let mac_addresses = match mac_addr {
            Some(mac) => {
                match normalize_mac_address(mac) {
                    Ok(mac_address) => vec![mac_address],
                    Err(e) => return Err(LmsRpcError::ServerError(format!("Invalid MAC address: {}", e))),
                }
            },
            None => {
                // Get all local MACs
                match crate::players::lms::lmsserver::get_local_mac_addresses() {
                    Ok(addresses) => {
                        if addresses.is_empty() {
                            return Err(LmsRpcError::ServerError("No MAC addresses found for local interfaces".to_string()));
                        }
                        addresses
                    },
                    Err(e) => return Err(LmsRpcError::ServerError(format!("Failed to get local MAC addresses: {}", e))),
                }
            }
        };
        
        // Check if any player's MAC address matches one of our MAC addresses
        for player in players {
            // Only check connected players
            if player.is_connected == 0 {
                continue;
            }
            
            // Parse the player's MAC address
            match normalize_mac_address(&player.playerid) {
                Ok(player_mac) => {
                    // Check against our MAC addresses
                    for local_mac in &mac_addresses {
                        if player_mac == *local_mac {
                            debug!("Found matching MAC: player {} ({}) matches local interface", 
                                  player.name, player_mac);
                            return Ok(true);
                        }
                    }
                },
                Err(e) => {
                    debug!("Could not parse player MAC address '{}': {}", player.playerid, e);
                }
            }
        }
        
        // No matches found
        Ok(false)
    }

    /// Search for content in the LMS library
    /// 
    /// # Arguments
    /// * `player_id` - MAC address of player
    /// * `query` - Search query string
    /// * `limit` - Maximum number of results to return
    /// 
    /// # Returns
    /// Search results containing tracks, albums, artists, and playlists
    pub fn search(&self, query: &str, limit: u32) -> Result<SearchResults, LmsRpcError> {
        debug!("Searching for '{}' (limit {})", query, limit);
        let mut results = SearchResults::default();
        
        // Search for tracks
        let track_results = self.database_request("search", 0, limit, 
            vec![("term", query), ("type", "track"), ("tags", "aCdtl")])?;
            
        if let Some(tracks_array) = track_results.get("tracks_loop") {
            if let Some(tracks) = tracks_array.as_array() {
                for track_value in tracks {
                    if let Ok(track) = serde_json::from_value::<Track>(track_value.clone()) {
                        results.tracks.push(track);
                    }
                }
            }
        }
        
        // Search for albums
        let album_results = self.database_request("search", 0, limit, 
            vec![("term", query), ("type", "album"), ("tags", "aCdtlyo")])?;
            
        if let Some(albums_array) = album_results.get("albums_loop") {
            if let Some(albums) = albums_array.as_array() {
                for album_value in albums {
                    if let Ok(album) = serde_json::from_value::<Album>(album_value.clone()) {
                        results.albums.push(album);
                    }
                }
            }
        }
        
        // Search for artists
        let artist_results = self.database_request("search", 0, limit, 
            vec![("term", query), ("type", "artist"), ("tags", "a")])?;
            
        if let Some(artists_array) = artist_results.get("artists_loop") {
            if let Some(artists) = artists_array.as_array() {
                for artist_value in artists {
                    if let Ok(artist) = serde_json::from_value::<Artist>(artist_value.clone()) {
                        results.artists.push(artist);
                    }
                }
            }
        }
        
        // Search for playlists
        let playlist_results = self.database_request("search", 0, limit, 
            vec![("term", query), ("type", "playlist"), ("tags", "p")])?;
            
        if let Some(playlists_array) = playlist_results.get("playlists_loop") {
            if let Some(playlists) = playlists_array.as_array() {
                for playlist_value in playlists {
                    if let Ok(playlist) = serde_json::from_value::<Playlist>(playlist_value.clone()) {
                        results.playlists.push(playlist);
                    }
                }
            }
        }
        
        Ok(results)
    }
}

/// Player information
#[derive(Debug, Clone, Deserialize)]
pub struct Player {
    pub playerid: String,
    pub name: String,
    #[serde(default)]
    pub ip: String,
    #[serde(default)]
    pub model: String,
    #[serde(default = "default_connected", rename = "connected")]
    pub is_connected: u8,
    #[serde(default)]
    pub power: u8,
}

fn default_connected() -> u8 { 0 }

/// Player status and current playing track
#[derive(Debug, Clone, Deserialize)]
pub struct PlayerStatus {
    #[serde(default)]
    pub mode: String,
    #[serde(default = "default_zero", rename = "playlist repeat")]
    pub playlist_repeat: u8,
    #[serde(default = "default_zero", rename = "playlist shuffle")]
    pub playlist_shuffle: u8,
    #[serde(default)]
    pub power: u8,
    #[serde(default = "default_zero", rename = "mixer volume")]
    pub volume: u8,
    #[serde(default)]
    pub duration: f32,
    #[serde(default)]
    pub time: f32,
    #[serde(default = "default_zero")]
    pub can_seek: u8,
    #[serde(default)]
    pub playlist_loop: Vec<Track>,
}

fn default_zero() -> u8 { 0 }

/// Track information
#[derive(Debug, Clone, Deserialize)]
pub struct Track {
    #[serde(default, deserialize_with = "deserialize_id_to_string")]
    pub id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub album: String,
    #[serde(default)]
    pub artist: String,
    #[serde(default)]
    pub coverid: String,
    #[serde(default)]
    pub duration: Option<f32>,
    #[serde(default, rename = "playlist index")]
    pub playlist_index: Option<i32>,
}

/// Custom deserializer for track IDs that can be either strings or integers
fn deserialize_id_to_string<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    // First deserialize to a serde_json::Value which can represent any JSON value
    let value = Value::deserialize(deserializer)?;
    
    // Convert the value to a string regardless of its type
    Ok(match value {
        Value::String(s) => s,
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Null => String::new(),
        _ => String::new(), // Empty string for arrays and objects
    })
}

/// Album information
#[derive(Debug, Deserialize, Clone)]
pub struct Album {
    pub id: Option<String>,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub artwork_url: Option<String>,
    pub year: Option<String>,
    pub genres: Option<String>,
    pub added_time: Option<String>,
    // Add other fields as needed
}

/// Artist information
#[derive(Debug, Clone, Deserialize)]
pub struct Artist {
    pub id: String,
    pub artist: String,
}

/// Playlist information
#[derive(Debug, Clone, Deserialize)]
pub struct Playlist {
    pub id: String,
    pub playlist: String,
}

/// Search results containing various types of matches
#[derive(Debug, Default, Clone)]
pub struct SearchResults {
    pub tracks: Vec<Track>,
    pub albums: Vec<Album>,
    pub artists: Vec<Artist>,
    pub playlists: Vec<Playlist>,
}