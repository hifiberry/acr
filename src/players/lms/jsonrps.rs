use reqwest::{Client, Error as ReqwestError};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;
use std::time::Duration;
use log::{debug, error, warn};

/// The standard JSON-RPC path for Lyrion Music Server
const JSONRPC_PATH: &str = "/jsonrpc.js";

/// Default timeout for HTTP requests in seconds
const DEFAULT_TIMEOUT_SECS: u64 = 5;

/// Errors that can occur when interacting with the LMS JSON-RPC API
#[derive(Debug, thiserror::Error)]
pub enum LmsRpcError {
    #[error("HTTP request error: {0}")]
    RequestError(#[from] ReqwestError),

    #[error("Failed to parse response: {0}")]
    ParseError(String),

    #[error("LMS server error: {0}")]
    ServerError(String),

    #[error("Empty response from server")]
    EmptyResponse,
}

/// Request structure for LMS JSON-RPC API
#[derive(Debug, Serialize)]
struct JsonRpcRequest {
    id: u32,
    method: String,
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
    #[allow(dead_code)]
    method: String,
    #[allow(dead_code)]
    params: Vec<Value>,
}

/// LMS JSON-RPC client for communicating with a Lyrion Music Server
#[derive(Debug, Clone)]
pub struct LmsRpcClient {
    /// Base URL of the LMS server (e.g., "http://192.168.1.100:9000")
    base_url: String,
    
    /// HTTP client for making requests
    client: Arc<Client>,
    
    /// Request counter for unique IDs
    request_id: u32,
}

impl LmsRpcClient {
    /// Create a new LMS JSON-RPC client
    /// 
    /// # Arguments
    /// * `host` - Hostname or IP address of the LMS server
    /// * `port` - HTTP port of the LMS server (typically 9000)
    pub fn new(host: &str, port: u16) -> Self {
        let base_url = format!("http://{}:{}", host, port);
        
        let client = Client::builder()
            .timeout(Duration::from_secs(DEFAULT_TIMEOUT_SECS))
            .build()
            .unwrap_or_else(|_| Client::new());
            
        LmsRpcClient {
            base_url,
            client: Arc::new(client),
            request_id: 1,
        }
    }
    
    /// Set a custom timeout for the client
    pub fn with_timeout(mut self, timeout_secs: u64) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(timeout_secs))
            .build()
            .unwrap_or_else(|_| Client::new());
            
        self.client = Arc::new(client);
        self
    }
    
    /// Get the next request ID
    fn next_id(&mut self) -> u32 {
        let id = self.request_id;
        self.request_id = self.request_id.wrapping_add(1);
        id
    }
    
    /// Send a command to a specific player
    /// 
    /// # Arguments
    /// * `player_id` - MAC address of player (e.g., "00:04:20:ab:cd:ef") or "0" for server-level commands
    /// * `command` - Command name (e.g., "mixer")
    /// * `start` - Start index for pagination (0-based)
    /// * `items_per_response` - Number of items to return per response
    /// * `params` - Tagged parameters as key-value pairs (e.g., ("volume", "50"))
    /// 
    /// # Returns
    /// The result field of the response as a JSON Value
    pub async fn request(&mut self, player_id: &str, command: &str, start: u32, items_per_response: u32, 
                        params: Vec<(&str, &str)>) -> Result<Value, LmsRpcError> {
        debug!("Command: {}, start: {}, items: {}, params: {:?}", 
               command, start, items_per_response, params);
        
        // Build command with proper format: command start itemsPerResponse tag1:value1 tag2:value2...
        let mut command_values = vec![
            Value::String(command.to_string()),
            Value::String(start.to_string()),
            Value::String(items_per_response.to_string()),
        ];
        
        // Add tagged parameters
        for (tag, value) in params {
            let tagged_param = format!("{}:{}", tag, value);
            command_values.push(Value::String(tagged_param));
        }

        self.request_raw(player_id, command_values).await
    }
    
    /// Send a raw command to a specific player with mixed parameter types
    /// 
    /// # Arguments
    /// * `player_id` - MAC address of player or "0" for server-level commands
    /// * `command` - Command array as JSON Values for mixed types
    pub async fn request_raw(&mut self, player_id: &str, command: Vec<Value>) -> Result<Value, LmsRpcError> {
        // Create params array with player_id and command
        let params = vec![
            Value::String(player_id.to_string()),
            Value::Array(command.clone()),
        ];
        
        let request = JsonRpcRequest {
            id: self.next_id(),
            method: "slim.request".to_string(),
            params,
        };
        
        let url = format!("{}{}", self.base_url, JSONRPC_PATH);
        
        debug!("Sending LMS request to {}: {:?}", url, request);
        // Add a warning log with the full command details
        warn!("LMS command to {}: player_id={}, command={:?}", 
              url, player_id, command);
        
        let response = self.client
            .post(&url)
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await?;
            
        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
            return Err(LmsRpcError::ServerError(format!("HTTP error {}: {}", status, error_text)));
        }
        
        let response_text = response.text().await?;
        if response_text.is_empty() {
            return Err(LmsRpcError::EmptyResponse);
        }
        
        match serde_json::from_str::<JsonRpcResponse>(&response_text) {
            Ok(json_response) => {
                debug!("LMS response: {:?}", json_response.result);
                Ok(json_response.result)
            },
            Err(e) => {
                error!("Failed to parse LMS response: {}", e);
                error!("Response text: {}", response_text);
                Err(LmsRpcError::ParseError(e.to_string()))
            }
        }
    }
    
    /// Get a list of available players
    pub async fn get_players(&mut self) -> Result<Vec<Player>, LmsRpcError> {
        let result = self.request("0", "players", 0, 100, vec![]).await?;
        
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
    pub async fn get_player_status(&mut self, player_id: &str) -> Result<PlayerStatus, LmsRpcError> {
        let result = self.request(player_id, "status", 0, 1, vec![("tags", "abcltiqyKo")]).await?;
        
        match serde_json::from_value::<PlayerStatus>(result.clone()) {
            Ok(status) => Ok(status),
            Err(e) => {
                error!("Failed to parse player status: {}", e);
                error!("Status data: {:?}", result);
                Err(LmsRpcError::ParseError(format!("Failed to parse player status: {}", e)))
            }
        }
    }
    
    /// Play the current track
    pub async fn play(&mut self, player_id: &str) -> Result<Value, LmsRpcError> {
        self.request(player_id, "play", 0, 0, vec![]).await
    }
    
    /// Pause the current track
    pub async fn pause(&mut self, player_id: &str) -> Result<Value, LmsRpcError> {
        self.request(player_id, "pause", 0, 0, vec![("1", "")]).await
    }
    
    /// Toggle pause/play
    pub async fn toggle_pause(&mut self, player_id: &str) -> Result<Value, LmsRpcError> {
        self.request(player_id, "pause", 0, 0, vec![]).await
    }
    
    /// Stop playback
    pub async fn stop(&mut self, player_id: &str) -> Result<Value, LmsRpcError> {
        self.request(player_id, "stop", 0, 0, vec![]).await
    }
    
    /// Skip to next track
    pub async fn next(&mut self, player_id: &str) -> Result<Value, LmsRpcError> {
        self.request(player_id, "playlist", 0, 0, vec![("index", "+1")]).await
    }
    
    /// Skip to previous track
    pub async fn previous(&mut self, player_id: &str) -> Result<Value, LmsRpcError> {
        self.request(player_id, "playlist", 0, 0, vec![("index", "-1")]).await
    }
    
    /// Set volume (0-100)
    pub async fn set_volume(&mut self, player_id: &str, volume: u8) -> Result<Value, LmsRpcError> {
        let volume = volume.min(100);
        self.request(player_id, "mixer", 0, 0, vec![("volume", &volume.to_string())]).await
    }
    
    /// Get current volume
    pub async fn get_volume(&mut self, player_id: &str) -> Result<u8, LmsRpcError> {
        let result = self.request(player_id, "mixer", 0, 0, vec![("volume", "?")]).await?;
        
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
    pub async fn set_mute(&mut self, player_id: &str, mute: bool) -> Result<Value, LmsRpcError> {
        let mute_val = if mute { "1" } else { "0" };
        self.request(player_id, "mixer", 0, 0, vec![("muting", mute_val)]).await
    }
    
    /// Toggle mute status
    pub async fn toggle_mute(&mut self, player_id: &str) -> Result<Value, LmsRpcError> {
        self.request(player_id, "mixer", 0, 0, vec![("muting", "")]).await
    }
    
    /// Get mute status
    pub async fn is_muted(&mut self, player_id: &str) -> Result<bool, LmsRpcError> {
        let result = self.request(player_id, "mixer", 0, 0, vec![("muting", "?")]).await?;
        
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
    pub async fn seek(&mut self, player_id: &str, seconds: f32) -> Result<Value, LmsRpcError> {
        // Convert seconds to format expected by LMS
        let time_str = format!("{:.1}", seconds);
        self.request(player_id, "time", 0, 0, vec![("time", &time_str)]).await
    }
    
    /// Set shuffle mode (0=off, 1=songs, 2=albums)
    pub async fn set_shuffle(&mut self, player_id: &str, shuffle_mode: u8) -> Result<Value, LmsRpcError> {
        let mode = shuffle_mode.min(2).to_string();
        self.request(player_id, "playlist", 0, 0, vec![("shuffle", &mode)]).await
    }
    
    /// Get shuffle mode
    pub async fn get_shuffle(&mut self, player_id: &str) -> Result<u8, LmsRpcError> {
        let result = self.request(player_id, "playlist", 0, 0, vec![("shuffle", "?")]).await?;
        
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
    pub async fn set_repeat(&mut self, player_id: &str, repeat_mode: u8) -> Result<Value, LmsRpcError> {
        let mode = repeat_mode.min(2).to_string();
        self.request(player_id, "playlist", 0, 0, vec![("repeat", &mode)]).await
    }
    
    /// Get repeat mode
    pub async fn get_repeat(&mut self, player_id: &str) -> Result<u8, LmsRpcError> {
        let result = self.request(player_id, "playlist", 0, 0, vec![("repeat", "?")]).await?;
        
        match result.get("_repeat") {
            Some(repeat) => {
                repeat.as_u64()
                    .map(|v| v as u8)
                    .ok_or_else(|| LmsRpcError::ParseError("Repeat mode is not a number".to_string()))
            },
            None => Err(LmsRpcError::ParseError("Repeat mode not found in response".to_string())),
        }
    }
    
    /// Search the library for a query string
    pub async fn search(&mut self, player_id: &str, query: &str, limit: u32) -> Result<SearchResults, LmsRpcError> {
        let result = self.request(
            player_id, 
            "search", 
            0, 
            limit, 
            vec![("term", query)]
        ).await?;
        
        // Try to parse the complex search results
        let mut search_results = SearchResults::default();
        
        // Parse tracks
        if let Some(tracks) = result.get("tracks_loop") {
            search_results.tracks = serde_json::from_value(tracks.clone())
                .unwrap_or_default();
        }
        
        // Parse albums
        if let Some(albums) = result.get("albums_loop") {
            search_results.albums = serde_json::from_value(albums.clone())
                .unwrap_or_default();
        }
        
        // Parse artists
        if let Some(artists) = result.get("artists_loop") {
            search_results.artists = serde_json::from_value(artists.clone())
                .unwrap_or_default();
        }
        
        // Parse playlists
        if let Some(playlists) = result.get("playlists_loop") {
            search_results.playlists = serde_json::from_value(playlists.clone())
                .unwrap_or_default();
        }
        
        Ok(search_results)
    }
    
    /// Get album tracks
    pub async fn get_album_tracks(&mut self, player_id: &str, album_id: &str) -> Result<Vec<Track>, LmsRpcError> {
        let result = self.request(
            player_id, 
            "titles", 
            0, 
            100, 
            vec![("album_id", album_id), ("tags", "altqod")]
        ).await?;
        
        match result.get("titles_loop") {
            Some(tracks) => {
                serde_json::from_value::<Vec<Track>>(tracks.clone())
                    .map_err(|e| LmsRpcError::ParseError(format!("Failed to parse album tracks: {}", e)))
            },
            None => Ok(Vec::new()), // No tracks found
        }
    }
    
    /// Add a track to the playlist
    pub async fn add_track(&mut self, player_id: &str, track_id: &str) -> Result<Value, LmsRpcError> {
        self.request(player_id, "playlist", 0, 0, vec![("add", ""), ("track_id", track_id)]).await
    }
    
    /// Add an album to the playlist
    pub async fn add_album(&mut self, player_id: &str, album_id: &str) -> Result<Value, LmsRpcError> {
        self.request(player_id, "playlist", 0, 0, vec![("add", ""), ("album_id", album_id)]).await
    }
    
    /// Clear the playlist
    pub async fn clear_playlist(&mut self, player_id: &str) -> Result<Value, LmsRpcError> {
        self.request(player_id, "playlist", 0, 0, vec![("clear", "")]).await
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
    #[serde(default)]
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

/// Album information
#[derive(Debug, Clone, Deserialize)]
pub struct Album {
    pub id: String,
    pub album: String,
    #[serde(default)]
    pub artist: String,
    #[serde(default)]
    pub artwork_track_id: Option<String>,
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

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::runtime::Runtime;
    
    // Note: These tests require a running LMS instance.
    // You can set the LMS_TEST_HOST environment variable to your LMS host address.
    // Otherwise, these tests will be skipped.
    
    fn get_test_client() -> Option<LmsRpcClient> {
        match std::env::var("LMS_TEST_HOST") {
            Ok(host) => {
                Some(LmsRpcClient::new(&host, 9000))
            }
            Err(_) => None,
        }
    }
    
    #[test]
    fn test_get_players() {
        let client = match get_test_client() {
            Some(c) => c,
            None => return, // Skip test if no test host is configured
        };
        
        let rt = Runtime::new().unwrap();
        let mut client = client;
        
        let result = rt.block_on(async {
            client.get_players().await
        });
        
        match result {
            Ok(players) => {
                println!("Found {} players", players.len());
                for player in players {
                    println!("  Player: {} ({})", player.name, player.playerid);
                }
            }
            Err(e) => {
                panic!("Failed to get players: {:?}", e);
            }
        }
    }
}