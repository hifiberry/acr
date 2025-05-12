// filepath: c:\Users\danie\devel\hifiberry-os\packages\acr\src\players\lms\lmspplayer.rs
use std::sync::Arc;
use log::{debug, info, warn};
use std::collections::HashMap;
use tokio::join;

use crate::players::lms::jsonrps::LmsRpcClient;
use crate::players::lms::lmsserver::get_local_mac_addresses;
use crate::helpers::macaddress::normalize_mac_address;
use crate::data::song::Song;

/// Represents a Logitech Media Server player with its client connection
#[derive(Debug, Clone)]
pub struct LMSPlayer {
    /// LMS RPC client for API calls
    client: Arc<LmsRpcClient>,
    
    /// Player ID (MAC address)
    player_id: String,
}

impl LMSPlayer {
    /// Create a new LMS player
    /// 
    /// # Arguments
    /// * `client` - LMS RPC client for communication with server
    /// * `player_id` - Player ID (MAC address) to connect to
    pub fn new(client: LmsRpcClient, player_id: &str) -> Self {
        Self {
            client: Arc::new(client),
            player_id: player_id.to_string(),
        }
    }
    
    /// Get player ID
    pub fn get_player_id(&self) -> &str {
        &self.player_id
    }
    
    /// Get client reference
    pub fn get_client(&self) -> Arc<LmsRpcClient> {
        self.client.clone()
    }
    
    /// Check if the player is connected to the LMS server
    /// 
    /// This method determines if the current device is registered as a player with
    /// the configured LMS server.
    /// 
    /// # Returns
    /// `true` if connected, `false` otherwise
    pub async fn is_connected(&self) -> bool {
        // Get the local MAC addresses
        let mac_addresses = match get_local_mac_addresses() {
            Ok(addresses) => addresses,
            Err(e) => {
                warn!("Failed to get local MAC addresses: {}", e);
                return false;
            }
        };
        
        // Normalize all local MAC addresses for comparison
        let normalized_local_macs: Vec<mac_address::MacAddress> = mac_addresses
            .iter()
            .map(|mac| mac.clone())
            .collect();
        
        debug!("Local MAC addresses: {:?}", normalized_local_macs);
        
        // Use the client (which is now cloneable) to get players
        let mut client_clone = (*self.client).clone();
        match client_clone.get_players().await {
            Ok(players) => {
                debug!("Found {} players on LMS server", players.len());
                
                // Check if any player matches our MAC address
                for player in players {
                    // The playerid field contains the MAC address
                    match normalize_mac_address(&player.playerid) {
                        Ok(player_mac) => {
                            debug!("Checking player MAC: {:?}", player_mac);
                            
                            // Check if this player's MAC matches any of our local MACs
                            if normalized_local_macs.contains(&player_mac) {
                                info!("Found matching player: {} ({:?})", 
                                     player.name, 
                                     player_mac);
                                return true;
                            }
                        },
                        Err(e) => {
                            debug!("Failed to normalize player MAC: {}", e);
                        }
                    }
                }
                
                debug!("No matching players found on the server");
                false
            },
            Err(e) => {
                warn!("Failed to get players from LMS server: {}", e);
                false
            }
        }
    }
    
    /// Get the current title for remote streams or the song title as formatted for the player
    /// 
    /// # Returns
    /// The current title as a String if available, or an error
    async fn current_title(&self) -> Result<String, String> {
        let mut client_clone = (*self.client).clone();
        match client_clone.request(&self.player_id, "current_title", 0, 1, vec![("?", "")]).await {
            Ok(response) => {
                // Extract the current_title field from the response
                match response.as_str() {
                    Some(title) => Ok(title.to_string()),
                    None => Err("Failed to parse current_title response".to_string()),
                }
            },
            Err(e) => Err(format!("Failed to get current_title: {}", e)),
        }
    }
    
    /// Check if current song is a remote stream
    /// 
    /// # Returns
    /// true if remote stream, false if local, or an error
    async fn remote(&self) -> Result<bool, String> {
        let mut client_clone = (*self.client).clone();
        match client_clone.request(&self.player_id, "remote", 0, 1, vec![("?", "")]).await {
            Ok(response) => {
                // Get the remote value (0 = local, 1 = remote)
                match response.as_i64() {
                    Some(value) => Ok(value == 1),
                    None => Err("Failed to parse remote response".to_string()),
                }
            },
            Err(e) => Err(format!("Failed to get remote status: {}", e)),
        }
    }
    
    /// Get the genre of the current song
    /// 
    /// # Returns
    /// The genre as a String if available, or an error
    async fn genre(&self) -> Result<String, String> {
        let mut client_clone = (*self.client).clone();
        match client_clone.request(&self.player_id, "genre", 0, 1, vec![("?", "")]).await {
            Ok(response) => {
                // Extract the genre field from the response
                match response.as_str() {
                    Some(genre) => Ok(genre.to_string()),
                    None => Err("Failed to parse genre response".to_string()),
                }
            },
            Err(e) => Err(format!("Failed to get genre: {}", e)),
        }
    }
    
    /// Get the artist of the current song
    /// 
    /// # Returns
    /// The artist as a String if available, or an error
    async fn artist(&self) -> Result<String, String> {
        let mut client_clone = (*self.client).clone();
        match client_clone.request(&self.player_id, "artist", 0, 1, vec![("?", "")]).await {
            Ok(response) => {
                // Extract the artist field from the response
                match response.as_str() {
                    Some(artist) => Ok(artist.to_string()),
                    None => Err("Failed to parse artist response".to_string()),
                }
            },
            Err(e) => Err(format!("Failed to get artist: {}", e)),
        }
    }
    
    /// Get the album of the current song
    /// 
    /// # Returns
    /// The album as a String if available, or an error
    async fn album(&self) -> Result<String, String> {
        let mut client_clone = (*self.client).clone();
        match client_clone.request(&self.player_id, "album", 0, 1, vec![("?", "")]).await {
            Ok(response) => {
                // Extract the album field from the response
                match response.as_str() {
                    Some(album) => Ok(album.to_string()),
                    None => Err("Failed to parse album response".to_string()),
                }
            },
            Err(e) => Err(format!("Failed to get album: {}", e)),
        }
    }
    
    /// Get the title of the current song
    /// 
    /// # Returns
    /// The title as a String if available, or an error
    async fn title(&self) -> Result<String, String> {
        let mut client_clone = (*self.client).clone();
        match client_clone.request(&self.player_id, "title", 0, 1, vec![("?", "")]).await {
            Ok(response) => {
                // Extract the title field from the response
                match response.as_str() {
                    Some(title) => Ok(title.to_string()),
                    None => Err("Failed to parse title response".to_string()),
                }
            },
            Err(e) => Err(format!("Failed to get title: {}", e)),
        }
    }
    
    /// Get the duration of the current song in seconds
    /// 
    /// # Returns
    /// The duration as a f32 if available, or an error
    async fn duration(&self) -> Result<f32, String> {
        let mut client_clone = (*self.client).clone();
        match client_clone.request(&self.player_id, "duration", 0, 1, vec![("?", "")]).await {
            Ok(response) => {
                // Extract the duration field from the response
                match response.as_f64() {
                    Some(duration) => Ok(duration as f32),
                    None => Err("Failed to parse duration response".to_string()),
                }
            },
            Err(e) => Err(format!("Failed to get duration: {}", e)),
        }
    }
    
    /// Get the path of the current song
    /// 
    /// # Returns
    /// The file path as a String if available, or an error
    async fn path(&self) -> Result<String, String> {
        let mut client_clone = (*self.client).clone();
        match client_clone.request(&self.player_id, "path", 0, 1, vec![("?", "")]).await {
            Ok(response) => {
                // Extract the path field from the response
                match response.as_str() {
                    Some(path) => Ok(path.to_string()),
                    None => Err("Failed to parse path response".to_string()),
                }
            },
            Err(e) => Err(format!("Failed to get path: {}", e)),
        }
    }
    
    /// Get information about the currently playing song
    /// 
    /// # Returns
    /// An optional Song object with the currently playing song information
    pub async fn get_current_song(&self) -> Option<Song> {
        // Run all data retrieving functions in parallel
        let (title_result, artist_result, album_result, genre_result, 
             duration_result, path_result, remote_result) = join!(
            self.title(),
            self.artist(),
            self.album(),
            self.genre(),
            self.duration(),
            self.path(),
            self.remote()
        );
        
        // Check if we have at least a title or if we're playing a remote stream
        let title = title_result.ok();
        let remote = remote_result.ok().unwrap_or(false);
        
        if title.is_none() && !remote {
            debug!("No song is currently playing (no title and not a remote stream)");
            return None;
        }
        
        // Create metadata hashmap with additional information
        let mut metadata = HashMap::new();
        
        // Store path for both metadata and potential stream URL
        let path_str = path_result.ok();
        
        // Add path to metadata if available
        if let Some(path) = &path_str {
            metadata.insert("path".to_string(), serde_json::Value::String(path.clone()));
        }
        
        // Create Song struct with the available information
        let song = Song {
            title,
            artist: artist_result.ok(),
            album: album_result.ok(),
            genre: genre_result.ok(),
            duration: duration_result.ok().map(|d| d as f64),
            // Add stream_url if it's a remote stream with an http URL
            stream_url: if remote {
                path_str.filter(|p| p.starts_with("http"))
            } else {
                None
            },
            source: Some(if remote { "remote".to_string() } else { "lms".to_string() }),
            metadata,
            ..Default::default()
        };
        
        Some(song)
    }
    
    /// Get information about the currently playing song and its position
    /// 
    /// # Returns
    /// An optional tuple containing the Song information and the current position in seconds
    pub async fn now_playing(&self) -> Option<(Song, f32)> {
        // Fetch song and position in parallel
        let (song, position_result) = join!(
            self.get_current_song(),
            self.get_current_position()
        );
        
        // If there's no song playing, return None
        if song.is_none() {
            return None;
        }
        
        // Get the position, defaulting to 0.0 if there was an error
        let position = position_result.unwrap_or(0.0);
        
        // Return the tuple of song and position
        Some((song.unwrap(), position))
    }
    
    /// Get the current playback position in seconds
    /// 
    /// # Returns
    /// The current playback position in seconds, or an error if it couldn't be retrieved
    pub async fn get_current_position(&self) -> Result<f32, String> {
        let mut client_clone = (*self.client).clone();
        match client_clone.request(&self.player_id, "time", 0, 1, vec![("?", "")]).await {
            Ok(response) => {
                // Extract the time value from the response
                match response.as_f64() {
                    Some(time) => Ok(time as f32),
                    None => Err("Failed to parse time response".to_string()),
                }
            },
            Err(e) => Err(format!("Failed to get current position: {}", e)),
        }
    }

    /// Get the current mode (play, stop, or pause) of the player
    /// 
    /// # Returns
    /// The current mode as a string if available, or an error
    pub async fn get_mode(&self) -> Result<String, String> {
        let mut client_clone = (*self.client).clone();
        match client_clone.request(&self.player_id, "mode", 0, 1, vec![("?", "")]).await {
            Ok(response) => {
                // Extract the mode field from the response
                match response.as_str() {
                    Some(mode) => Ok(mode.to_string()),
                    None => Err("Failed to parse mode response".to_string()),
                }
            },
            Err(e) => Err(format!("Failed to get player mode: {}", e)),
        }
    }
}