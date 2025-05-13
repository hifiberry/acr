// filepath: c:\Users\danie\devel\hifiberry-os\packages\acr\src\players\lms\lmspplayer.rs
use std::sync::Arc;
use log::{debug, info, warn};

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
    pub fn is_connected(&self) -> bool {
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
        match client_clone.get_players() {
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
    #[allow(dead_code)]
    fn current_title(&self) -> Result<String, String> {
        let mut client_clone = (*self.client).clone();
        match client_clone.request(&self.player_id, "current_title", 0, 1, vec![("?", "")]) {
            Ok(response) => {
                // Extract the _title field from the response object
                if let Some(obj) = response.as_object() {
                    if let Some(title_value) = obj.get("_title") {
                        if let Some(title) = title_value.as_str() {
                            return Ok(title.to_string());
                        }
                    }
                }
                Err("Failed to parse current_title response".to_string())
            },
            Err(e) => Err(format!("Failed to get current_title: {}", e)),
        }
    }
    
    /// Check if current song is a remote stream
    /// 
    /// # Returns
    /// true if remote stream, false if local, or an error
    fn remote(&self) -> Result<bool, String> {
        let mut client_clone = (*self.client).clone();
        match client_clone.request(&self.player_id, "remote", 0, 1, vec![("?", "")]) {
            Ok(response) => {
                // Extract the _remote value from the response object
                if let Some(obj) = response.as_object() {
                    if let Some(remote_value) = obj.get("_remote") {
                        if let Some(value) = remote_value.as_i64() {
                            return Ok(value == 1);
                        }
                    }
                }
                Err("Failed to parse remote response".to_string())
            },
            Err(e) => Err(format!("Failed to get remote status: {}", e)),
        }
    }
    
    /// Get the genre of the current song
    /// 
    /// # Returns
    /// The genre as a String if available, or an error
    fn genre(&self) -> Result<String, String> {
        let mut client_clone = (*self.client).clone();
        match client_clone.request(&self.player_id, "genre", 0, 1, vec![("?", "")]) {
            Ok(response) => {
                // Extract the _genre field from the response object
                if let Some(obj) = response.as_object() {
                    if let Some(genre_value) = obj.get("_genre") {
                        if let Some(genre) = genre_value.as_str() {
                            return Ok(genre.to_string());
                        }
                    }
                }
                Err("Failed to parse genre response".to_string())
            },
            Err(e) => Err(format!("Failed to get genre: {}", e)),
        }
    }
    
    /// Get the artist of the current song
    /// 
    /// # Returns
    /// The artist as a String if available, or an error
    fn artist(&self) -> Result<String, String> {
        let mut client_clone = (*self.client).clone();
        match client_clone.request(&self.player_id, "artist", 0, 1, vec![("?", "")]) {
            Ok(response) => {
                // Extract the _artist field from the response object
                if let Some(obj) = response.as_object() {
                    if let Some(artist_value) = obj.get("_artist") {
                        if let Some(artist) = artist_value.as_str() {
                            return Ok(artist.to_string());
                        }
                    }
                }
                Err("Failed to parse artist response".to_string())
            },
            Err(e) => Err(format!("Failed to get artist: {}", e)),
        }
    }
    
    /// Get the album of the current song
    /// 
    /// # Returns
    /// The album as a String if available, or an error
    fn album(&self) -> Result<String, String> {
        let mut client_clone = (*self.client).clone();
        match client_clone.request(&self.player_id, "album", 0, 1, vec![("?", "")]) {
            Ok(response) => {
                // Extract the _album field from the response object
                if let Some(obj) = response.as_object() {
                    if let Some(album_value) = obj.get("_album") {
                        if let Some(album) = album_value.as_str() {
                            return Ok(album.to_string());
                        }
                    }
                }
                Err("Failed to parse album response".to_string())
            },
            Err(e) => Err(format!("Failed to get album: {}", e)),
        }
    }
    
    /// Get the title of the current song
    /// 
    /// # Returns
    /// The title as a String if available, or an error
    fn title(&self) -> Result<String, String> {
        let mut client_clone = (*self.client).clone();
        match client_clone.request(&self.player_id, "title", 0, 1, vec![("?", "")]) {
            Ok(response) => {
                // Extract the _title field from the response object
                if let Some(obj) = response.as_object() {
                    if let Some(title_value) = obj.get("_title") {
                        if let Some(title) = title_value.as_str() {
                            return Ok(title.to_string());
                        }
                    }
                }
                Err("Failed to parse title response".to_string())
            },
            Err(e) => Err(format!("Failed to get title: {}", e)),
        }
    }
    
    /// Get the duration of the current song in seconds
    /// 
    /// # Returns
    /// The duration as a f32 if available, or an error
    fn duration(&self) -> Result<f32, String> {
        let mut client_clone = (*self.client).clone();
        match client_clone.request(&self.player_id, "duration", 0, 1, vec![("?", "")]) {
            Ok(response) => {
                // Extract the _duration field from the response object
                if let Some(obj) = response.as_object() {
                    if let Some(duration_value) = obj.get("_duration") {
                        if let Some(duration) = duration_value.as_f64() {
                            return Ok(duration as f32);
                        }
                    }
                }
                Err("Failed to parse duration response".to_string())
            },
            Err(e) => Err(format!("Failed to get duration: {}", e)),
        }
    }
    
    /// Get the path of the current song
    /// 
    /// # Returns
    /// The file path as a String if available, or an error
    fn path(&self) -> Result<String, String> {
        let mut client_clone = (*self.client).clone();
        match client_clone.request(&self.player_id, "path", 0, 1, vec![("?", "")]) {
            Ok(response) => {
                // Extract the _path field from the response object
                if let Some(obj) = response.as_object() {
                    if let Some(path_value) = obj.get("_path") {
                        if let Some(path) = path_value.as_str() {
                            return Ok(path.to_string());
                        }
                    }
                }
                Err("Failed to parse path response".to_string())
            },
            Err(e) => Err(format!("Failed to get path: {}", e)),
        }
    }
    
    /// Get information about the currently playing song
    /// 
    /// # Returns
    /// An optional Song object with the currently playing song information
    pub fn get_current_song(&self) -> Option<Song> {

        // Instead of running in parallel with join(), get each piece of data sequentially
        let title_result = self.title();
        let artist_result = self.artist();
        let album_result = self.album();
        let genre_result = self.genre();
        let duration_result = self.duration();
        let path_result = self.path();
        let remote_result = self.remote();

        // Check if we have at least a title or if we're playing a remote stream
        let title = title_result.ok();
        let remote = remote_result.ok().unwrap_or(false);
        
        if title.is_none() && !remote {
            debug!("No song is currently playing (no title and not a remote stream)");
            return None;
        }
        
        // Store path for both metadata and potential stream URL
        let path_str = path_result.ok();
        
        // Create Song struct with the available information
        let song = Song {
            title,
            artist: artist_result.ok(),
            album: album_result.ok(),
            genre: genre_result.ok(),
            duration: duration_result.ok().map(|d| d as f64),
            // Add stream_url if it's a remote stream with an http URL
            stream_url:  path_str,
            source: Some(if remote { "remote".to_string() } else { "lms".to_string() }),
            ..Default::default()
        };
        
        Some(song)
    }
    
    /// Get information about the currently playing song and its position
    /// 
    /// # Returns
    /// An optional tuple containing the Song information and the current position in seconds
    pub fn now_playing(&self) -> Option<(Song, f32)> {
        // Get song and position sequentially
        let song = self.get_current_song();
        
        // If there's no song playing, return None
        if song.is_none() {
            return None;
        }
        
        // Get the position, defaulting to 0.0 if there was an error
        let position = self.get_current_position().unwrap_or(0.0);
        
        // Return the tuple of song and position
        Some((song.unwrap(), position))
    }
    
    /// Get the current playback position in seconds
    /// 
    /// # Returns
    /// The current playback position in seconds, or an error if it couldn't be retrieved
    pub fn get_current_position(&self) -> Result<f32, String> {
        let mut client_clone = (*self.client).clone();
        match client_clone.request(&self.player_id, "time", 0, 1, vec![("?", "")]) {
            Ok(response) => {
                // Extract the _time field from the response object
                if let Some(obj) = response.as_object() {
                    if let Some(time_value) = obj.get("_time") {
                        if let Some(time) = time_value.as_f64() {
                            return Ok(time as f32);
                        }
                    }
                }
                Err("Failed to parse time response".to_string())
            },
            Err(e) => Err(format!("Failed to get current position: {}", e)),
        }
    }

    /// Get the current mode (play, stop, or pause) of the player
    /// 
    /// # Returns
    /// The current mode as a string if available, or an error
    pub fn get_mode(&self) -> Result<String, String> {
        let mut client_clone = (*self.client).clone();
        match client_clone.request(&self.player_id, "mode", 0, 1, vec![("?", "")]) {
            Ok(response) => {
                // Fallback to old parsing method if the object format is not found
                if let Some(mode) = response.as_str() {
                    return Ok(mode.to_string());
                }
                Err("Failed to parse mode response".to_string())
            },
            Err(e) => Err(format!("Failed to get player mode: {}", e)),
        }
    }

    /// Internal helper to send commands to the LMS player
    /// 
    /// # Arguments
    /// * `command` - The command to send (play, pause, stop, etc.)
    /// * `args` - Optional vector of arguments as (name, value) tuples
    /// 
    /// # Returns
    /// `Ok(())` if the command was sent successfully, or an error message
    fn send_command(&self, command: &str, args: Vec<(&str, &str)>) -> Result<(), String> {
        let mut client_clone = (*self.client).clone();
        
        // For logging, extract just the values when tag is empty
        let log_args: Vec<String> = args.iter()
            .map(|(tag, value)| {
                if tag.is_empty() {
                    (*value).to_string()
                } else {
                    // For named params, use tag:value format
                    format!("{}:{}", tag, value)
                }
            })
            .collect();
        
        match client_clone.request(&self.player_id, command, 0, 1, args) {
            Ok(_) => {
                debug!("{} command sent to player {} with args {:?}", command, self.player_id, log_args);
                Ok(())
            },
            Err(e) => Err(format!("Failed to send {} command: {}", command, e)),
        }
    }
    
    /// Internal helper to send commands with simple string values (no named parameters)
    /// 
    /// # Arguments
    /// * `command` - The command to send (play, pause, stop, etc.)
    /// * `args` - Vector of argument values (without parameter names)
    /// 
    /// # Returns
    /// `Ok(())` if the command was sent successfully, or an error message
    fn send_command_with_values(&self, command: &str, args: Vec<&str>) -> Result<(), String> {
        // Log the simple values here before converting to tuples
        warn!("{} command sent to player {} with args {:?}", command, self.player_id, args);
        
        // Convert each value to a tuple with empty tag name
        let tuple_args = args.into_iter().map(|value| ("", value)).collect();
        
        // Call a modified version of the send_command method that doesn't log
        self.send_command_internal(command, tuple_args)
    }
    
    /// Internal version of send_command that doesn't log (used by send_command_with_values)
    /// 
    /// # Arguments
    /// * `command` - The command to send (play, pause, stop, etc.)
    /// * `args` - Optional vector of arguments as (name, value) tuples
    /// 
    /// # Returns
    /// `Ok(())` if the command was sent successfully, or an error message
    fn send_command_internal(&self, command: &str, args: Vec<(&str, &str)>) -> Result<(), String> {
        let mut client_clone = (*self.client).clone();
        
        // Extract values from tuples with empty tags to use with control_request
        let values: Vec<&str> = args.iter()
            .filter_map(|(tag, value)| {
                if tag.is_empty() {
                    Some(*value)
                } else {
                    None
                }
            })
            .collect();
        
        // Use the control_request method that doesn't add pagination parameters
        match client_clone.control_request(&self.player_id, command, values) {
            Ok(_) => Ok(()),
            Err(e) => Err(format!("Failed to send {} command: {}", command, e)),
        }
    }
    
    /// Send a play command to the player
    /// 
    /// The play command starts playing the current playlist.
    /// 
    /// # Arguments
    /// * `fade_in_secs` - Optional fade-in period in seconds
    /// 
    /// # Returns
    /// `Ok(())` if the command was sent successfully, or an error message
    pub fn play(&self, fade_in_secs: Option<f32>) -> Result<(), String> {
        let mut args = vec![];
        
        // Add fade-in parameter if specified
        if let Some(fade) = fade_in_secs {
            args.push(fade.to_string());
        }
        
        // Convert any owned Strings to &str for the function call
        let str_args: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
        
        self.send_command_with_values("play", str_args)
    }
    
    /// Send a stop command to the player
    /// 
    /// The stop command stops playing the current playlist.
    /// 
    /// # Returns
    /// `Ok(())` if the command was sent successfully, or an error message
    pub fn stop(&self) -> Result<(), String> {
        self.send_command_with_values("stop", vec![])
    }
    
    /// Send a pause command to the player
    /// 
    /// The pause command toggles the pause state, or explicitly sets it based on parameters.
    /// 
    /// # Arguments
    /// * `pause_state` - Optional pause state: Some(true) to force pause, Some(false) to force unpause, None to toggle
    /// * `fade_in_secs` - Optional fade-in period in seconds when unpausing
    /// * `suppress_show_briefly` - Optional flag to suppress display of pause icon on the player
    /// 
    /// # Returns
    /// `Ok(())` if the command was sent successfully, or an error message
    pub fn pause(&self, pause_state: Option<bool>, fade_in_secs: Option<f32>, suppress_show_briefly: Option<bool>) -> Result<(), String> {
        let mut args = vec![];
        let mut owned_strings = vec![];
        
        // Add pause state parameter if specified
        if let Some(state) = pause_state {
            args.push(if state { "1" } else { "0" });
        }
        
        // Add fade in parameter if specified
        if let Some(fade) = fade_in_secs {
            owned_strings.push(fade.to_string());
            args.push(owned_strings.last().unwrap().as_str());
        }
        
        // Add suppress show briefly parameter if specified
        if let Some(suppress) = suppress_show_briefly {
            args.push(if suppress { "1" } else { "0" });
        }
        
        self.send_command_with_values("pause", args)
    }

    /// Send a seek command to the player
    /// 
    /// The seek command allows seeking to an absolute position in the current track.
    /// 
    /// # Arguments
    /// * `position_secs` - Position in seconds to seek to
    /// 
    /// # Returns
    /// `Ok(())` if the command was sent successfully, or an error message
    pub fn seek(&self, position_secs: f32) -> Result<(), String> {
        // Convert the position to a string with one decimal place
        let pos_str = format!("{:.1}", position_secs);
        
        // Use the control_request method (via send_command_with_values)
        // to send the time command with the position parameter
        self.send_command_with_values("time", vec![pos_str.as_str()])
    }
}