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
        let normalized_local_macs: Vec<mac_address::MacAddress> = mac_addresses.to_vec();
        
        debug!("Local MAC addresses: {:?}", normalized_local_macs);
        
        // Use the client directly without cloning
        match self.client.get_players() {
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
    
    /// Check if current song is a remote stream
    /// 
    /// # Returns
    /// true if remote stream, false if local, or an error
    fn remote(&self) -> Result<bool, String> {
        match self.client.control_request(&self.player_id, "remote", vec!["?"]) {
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
        match self.client.control_request(&self.player_id, "genre", vec!["?"]) {
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
        match self.client.control_request(&self.player_id, "artist", vec!["?"]) {
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
        match self.client.control_request(&self.player_id, "album", vec!["?"]) {
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
        match self.client.control_request(&self.player_id, "title", vec!["?"]) {
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
        match self.client.control_request(&self.player_id, "duration", vec!["?"]) {
            Ok(response) => {
                // Extract the _duration field from the response object
                if let Some(obj) = response.as_object() {
                    if let Some(duration_value) = obj.get("_duration") {
                        // Try to parse as a float directly
                        if let Some(duration) = duration_value.as_f64() {
                            return Ok(duration as f32);
                        }
                        
                        // If not a float, try parsing as a string
                        if let Some(duration_str) = duration_value.as_str() {
                            // Try to parse the string as a float
                            if let Ok(duration) = duration_str.parse::<f32>() {
                                return Ok(duration);
                            }
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
        match self.client.control_request(&self.player_id, "path", vec!["?"]) {
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
        let track_id = self.get_current_track_id().ok();
        
        // Generate cover art URL from track ID if available
        let mut cover_art_url = None;
        if let Some(id) = &track_id {
            // Create thumbnail URL from track ID using the server address and port
            if let Ok(server_addr) = self.client.get_server_address() {
                let port = self.client.get_server_port();
                cover_art_url = Some(format!("http://{}:{}/music/{}/cover.jpg", server_addr, port, id));
                debug!("Generated cover art URL from track ID: {:?}", cover_art_url);
            } else {
                warn!("Could not get server address for thumbnail URL");
            }
        }
        
 
        // Check if we have at least a title or if we're playing a remote stream
        let title = title_result.ok();
        let remote = remote_result.ok().unwrap_or(false);
        
        if title.is_none() && !remote {
            debug!("No song is currently playing (no title and not a remote stream)");
            return None;
        }
        
        // Store path for both metadata and potential stream URL
        let path_str = path_result.ok();
        
        // Log if we found a thumbnail URL
        if let Some(thumb_url) = &cover_art_url {
            debug!("Found thumbnail URL: {}", thumb_url);
        }        
        
        // Create Song struct with the available information
        let song = Song {
            title,
            artist: artist_result.ok(),
            album: album_result.ok(),
            genre: genre_result.ok(),
            duration: duration_result.ok().map(|d| d as f64),
            // Add stream_url if it's a remote stream with an http URL
            stream_url: path_str,
            source: Some(if remote { "remote".to_string() } else { "lms".to_string() }),
            cover_art_url,
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
        song.as_ref()?;
        
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
        match self.client.control_request(&self.player_id, "time", vec!["?"]) {
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
        match self.client.control_request(&self.player_id, "mode", vec!["?"]) {
            Ok(response) => {
                // First try to extract from object format
                if let Some(obj) = response.as_object() {
                    if let Some(mode_value) = obj.get("_mode") {
                        if let Some(mode) = mode_value.as_str() {
                            return Ok(mode.to_string());
                        }
                    }
                }
                
                // Fallback to old parsing method if the object format is not found
                if let Some(mode) = response.as_str() {
                    return Ok(mode.to_string());
                }
                
                Err("Failed to parse mode response".to_string())
            },
            Err(e) => Err(format!("Failed to get player mode: {}", e)),
        }
    }

    /// Get the current shuffle mode of the player
    /// 
    /// # Returns
    /// The current shuffle mode (0=off, 1=songs, 2=albums), or an error
    pub fn get_shuffle(&self) -> Result<u8, String> {
        // Use the control_request method instead of the paginated request
        match self.client.control_request(&self.player_id, "playlist", vec!["shuffle", "?"]) {
            Ok(result) => {
                debug!("Shuffle response: {:?}", result);
                
                // Try to extract the _shuffle field from the response object
                if let Some(obj) = result.as_object() {
                    if let Some(shuffle_value) = obj.get("_shuffle") {
                        // Handle the case where _shuffle is a string
                        if let Some(shuffle_str) = shuffle_value.as_str() {
                            if let Ok(shuffle_int) = shuffle_str.parse::<u8>() {
                                debug!("Current shuffle mode is {} (from string value)", shuffle_int);
                                return Ok(shuffle_int);
                            }
                        }
                        // Handle the case where _shuffle is an integer
                        else if let Some(shuffle) = shuffle_value.as_u64() {
                            debug!("Current shuffle mode is {}", shuffle);
                            return Ok(shuffle as u8);
                        }
                    }
                }
                
                // Log the full response for debugging
                warn!("Failed to parse shuffle response: {:?}", result);
                Err("Failed to parse shuffle response".to_string())
            },
            Err(e) => Err(format!("Failed to get shuffle mode: {}", e))
        }
    }

    /// Set the shuffle mode of the player
    /// 
    /// # Arguments
    /// * `mode` - Shuffle mode (0=off, 1=songs, 2=albums)
    /// 
    /// # Returns
    /// `Ok(())` if the command was sent successfully, or an error message
    pub fn set_shuffle(&self, mode: u8) -> Result<(), String> {
        // Ensure mode is 0, 1, or 2
        let valid_mode = mode.min(2);
        
        // Convert the mode to a string and use send_command_with_values
        // instead of the paginated request approach
        let mode_str = valid_mode.to_string();
        
        debug!("Setting shuffle mode to {}", valid_mode);
        self.send_command_with_values("playlist", vec!["shuffle", &mode_str])
    }
    
    /// Get the current repeat mode of the player
    /// 
    /// # Returns
    /// The current repeat mode (0=off, 1=song, 2=playlist), or an error
    pub fn get_repeat(&self) -> Result<u8, String> {
        // Use the control_request method instead of the paginated request
        match self.client.control_request(&self.player_id, "playlist", vec!["repeat", "?"]) {
            Ok(result) => {
                debug!("Repeat response: {:?}", result);
                
                if let Some(obj) = result.as_object() {
                    if let Some(repeat_value) = obj.get("_repeat") {
                        // Handle the case where _repeat is a string
                        if let Some(repeat_str) = repeat_value.as_str() {
                            if let Ok(repeat_int) = repeat_str.parse::<u8>() {
                                debug!("Current repeat mode is {} (from string value)", repeat_int);
                                return Ok(repeat_int);
                            }
                        }
                        // Handle the case where _repeat is an integer
                        else if let Some(repeat) = repeat_value.as_u64() {
                            debug!("Current repeat mode is {}", repeat);
                            return Ok(repeat as u8);
                        }
                    }
                }
                
                // Log the full response for debugging
                warn!("Failed to parse repeat response: {:?}", result);
                Err("Failed to parse repeat response".to_string())
            },
            Err(e) => Err(format!("Failed to get repeat mode: {}", e))
        }
    }

    /// Set the repeat mode of the player
    /// 
    /// # Arguments
    /// * `mode` - Repeat mode (0=off, 1=song, 2=playlist)
    /// 
    /// # Returns
    /// `Ok(())` if the command was sent successfully, or an error message
    pub fn set_repeat(&self, mode: u8) -> Result<(), String> {
        // Ensure mode is 0, 1, or 2
        let valid_mode = mode.min(2);
        
        // Convert the mode to a string and use send_command_with_values
        // instead of the paginated request approach
        let mode_str = valid_mode.to_string();
        
        debug!("Setting repeat mode to {}", valid_mode);
        self.send_command_with_values("playlist", vec!["repeat", &mode_str])
    }
    
    /// Send commands with simple string values (no named parameters)
    /// 
    /// # Arguments
    /// * `command` - The command to send (play, pause, stop, etc.)
    /// * `args` - Vector of argument values (without parameter names)
    /// 
    /// # Returns
    /// `Ok(())` if the command was sent successfully, or an error message
    fn send_command_with_values(&self, command: &str, args: Vec<&str>) -> Result<(), String> {
        // Log the simple values here before converting to tuples
        debug!("{} command sent to player {} with args {:?}", command, self.player_id, args);
        
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
        match self.client.control_request(&self.player_id, command, values) {
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
    
    /// Get the ID of the currently playing track
    /// 
    /// This is a two-step process:
    /// 1. Get the current playlist index
    /// 2. Get the track ID at that index
    /// 
    /// # Returns
    /// `Ok(String)` with the track ID if available, or an error message
    pub fn get_current_track_id(&self) -> Result<String, String> {
        // Step 1: Get the current playlist index
        match self.client.control_request(&self.player_id, "status", vec!["0", "0"]) {
            Ok(response) => {
                // Extract the playlist_cur_index field
                if let Some(obj) = response.as_object() {
                    if let Some(index_value) = obj.get("playlist_cur_index") {
                        // The index can be either a string or a number
                        let index_str = if let Some(index) = index_value.as_str() {
                            index.to_string()
                        } else if let Some(index) = index_value.as_u64() {
                            index.to_string()
                        } else {
                            return Err("Failed to parse playlist index".to_string());
                        };
                        
                        debug!("Current playlist index: {}", index_str);
                        
                        // Step 2: Get the track ID at the current index
                        match self.client.control_request(&self.player_id, "status", vec![&index_str, "1", "tags:uK"]) {
                            Ok(track_response) => {
                                // The response contains a playlist_loop array with one item
                                if let Some(obj) = track_response.as_object() {
                                    if let Some(playlist_loop) = obj.get("playlist_loop") {
                                        if let Some(items) = playlist_loop.as_array() {
                                            if !items.is_empty() {
                                                // Get the first playlist item
                                                if let Some(current_item) = items.first().and_then(|i| i.as_object()) {
                                                    // Extract the ID from the item
                                                    if let Some(id_value) = current_item.get("id") {
                                                        // Handle the ID being either a number or string
                                                        let track_id = if let Some(id) = id_value.as_str() {
                                                            id.to_string()
                                                        } else if let Some(id) = id_value.as_u64() {
                                                            id.to_string()
                                                        } else {
                                                            return Err("Failed to parse track ID".to_string());
                                                        };
                                                        
                                                        debug!("Current track ID: {}", track_id);
                                                        return Ok(track_id);
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                                
                                Err("Could not find track ID in playlist response".to_string())
                            },
                            Err(e) => Err(format!("Failed to get track at index {}: {}", index_str, e))
                        }
                    } else {
                        Err("Playlist index not found in response".to_string())
                    }
                } else {
                    Err("Invalid response format".to_string())
                }
            },
            Err(e) => Err(format!("Failed to get player status: {}", e))
        }
    }
    
    /// Skip to the previous song in the playlist
    /// 
    /// Uses the 'button jump_rew' command to go to the previous track.
    /// 
    /// # Returns
    /// `Ok(())` if the command was sent successfully, or an error message
    pub fn previous(&self) -> Result<(), String> {
        debug!("Sending 'playlist index -' command to player {}", self.player_id);
        self.send_command_with_values("button", vec!["jump_rew"])
    }
    
    /// Skip to the next song in the playlist
    /// 
    /// Uses the 'button jump_fwd' command to go to the next track.
    /// 
    /// # Returns
    /// `Ok(())` if the command was sent successfully, or an error message
    pub fn next(&self) -> Result<(), String> {
        debug!("Sending 'playlist index +' command to player {}", self.player_id);
        self.send_command_with_values("button", vec!["jump_fwd"])
    }
    
    /// Clear the entire playlist/queue
    /// 
    /// The playlist clear command removes any song that is on the playlist.
    /// The player is stopped as a side effect of this command.
    /// 
    /// # Returns
    /// `Ok(())` if the command was sent successfully, or an error message
    pub fn clear_queue(&self) -> Result<(), String> {
        debug!("Clearing playlist for player {}", self.player_id);
        self.send_command_with_values("playlist", vec!["clear"])
    }    /// Add a track to the current playlist at the beginning or end
    /// 
    /// Uses the playlistcontrol command with cmd:add (to add at the end) or 
    /// cmd:insert (to insert at the beginning - plays next) with a track_id parameter.
    /// 
    /// # Arguments
    /// * `track_id` - The ID of the track to add to the playlist
    /// * `at_beginning` - If true, inserts the track to play next (after current song).
    ///                   If false, adds the track to the end of the playlist.
    /// 
    /// # Returns
    /// `Ok(())` if the command was sent successfully, or an error message
    pub fn add_to_queue(&self, track_id: &str, at_beginning: bool) -> Result<(), String> {
        // Choose the appropriate command based on at_beginning parameter
        let cmd = if at_beginning { "insert" } else { "add" };
        
        debug!("Adding track {} to {} of playlist for player {}", 
               track_id, 
               if at_beginning { "beginning" } else { "end" }, 
               self.player_id);
        
        // Format command parameters according to the API
        let cmd_param = format!("cmd:{}", cmd);
        let track_param = format!("track_id:{}", track_id);
        
        // Use playlistcontrol with cmd:add/insert and track_id parameters
        match self.client.control_request(
            &self.player_id, 
            "playlistcontrol", 
            vec![&cmd_param, &track_param]
        ) {
            Ok(_) => {
                debug!("Track added successfully to playlist");
                Ok(())
            },
            Err(e) => Err(format!("Failed to add track to playlist: {}", e)),
        }
    }

    /// Get the current playlist/queue as a collection of Track objects
    /// 
    /// Fetches the current playlist from the LMS server and returns it as a vector
    /// of Track objects with ID, track number, name, and artist fields populated.
    /// 
    /// # Returns
    /// A vector of Track objects representing the current queue, or an error message
    pub fn get_queue(&self) -> Result<Vec<crate::data::track::Track>, String> {
        debug!("Fetching playlist for player {}", self.player_id);
        
        // Request status with playlist info and required tags for track information
        // Use tags:adlK to get:
        // - a: artist information
        // - d: track duration
        // - l: title (for track name)
        // - K: track ID
        match self.client.control_request(
            &self.player_id, 
            "status", 
            vec!["0", "100", "tags:adlKitN"]
        ) {
            Ok(response) => {
                debug!("Received playlist response");
                
                // Create a vector to hold the track objects
                let mut tracks = Vec::new();
                
                // Extract the playlist_loop from the response if it exists
                if let Some(obj) = response.as_object() {
                    if let Some(playlist_loop) = obj.get("playlist_loop") {
                        if let Some(items) = playlist_loop.as_array() {
                            debug!("Found {} items in playlist", items.len());
                            
                            // Process each track in the playlist
                            for (index, track_value) in items.iter().enumerate() {
                                if let Some(track_obj) = track_value.as_object() {
                                    // Extract required fields
                                    let title = track_obj.get("title").and_then(|v| v.as_str())
                                        .unwrap_or("Unknown Title").to_string();
                                    
                                    // Create track object with name
                                    let mut track = crate::data::track::Track::with_name(title);
                                    
                                    // Set track ID if available
                                    if let Some(id) = track_obj.get("id").and_then(|v| v.as_str()) {
                                        track = track.with_id(crate::data::Identifier::String(id.to_string()));
                                    } else if let Some(id) = track_obj.get("track_id").and_then(|v| v.as_str()) {
                                        track = track.with_id(crate::data::Identifier::String(id.to_string()));
                                    }
                                    
                                    // Set track number if available
                                    if let Some(track_num) = track_obj.get("tracknum").and_then(|v| v.as_u64()) {
                                        track.track_number = Some(track_num as u16);
                                    } else if let Some(track_num) = track_obj.get("track").and_then(|v| v.as_u64()) {
                                        track.track_number = Some(track_num as u16);
                                    } else {
                                        // Fall back to playlist position if no track number is available
                                        track.track_number = Some((index + 1) as u16);
                                    }
                                    
                                    // Set artist if available
                                    if let Some(artist) = track_obj.get("artist").and_then(|v| v.as_str()) {
                                        track.artist = Some(artist.to_string());
                                    }
                                    
                                    // Set URI if available
                                    if let Some(url) = track_obj.get("url").and_then(|v| v.as_str()) {
                                        track = track.with_uri(url.to_string());
                                    }
                                    
                                    tracks.push(track);
                                }
                            }
                        }
                    }
                }
                
                // Log the number of tracks found
                debug!("Returning {} tracks from playlist", tracks.len());
                
                Ok(tracks)
            },
            Err(e) => {
                warn!("Failed to fetch playlist: {}", e);
                Err(format!("Failed to fetch playlist: {}", e))
            }
        }
    }

    /// Delete a track from the playlist at the specified index
    /// 
    /// Uses the playlistcontrol command with cmd:delete and index parameter
    /// to remove a track at a specific position in the playlist.
    /// 
    /// # Arguments
    /// * `index` - The zero-based index of the track to remove from the playlist
    /// 
    /// # Returns
    /// `Ok(())` if the command was sent successfully, or an error message
    pub fn delete_from_playlist(&self, index: usize) -> Result<(), String> {
        debug!("Deleting track at index {} from playlist for player {}", index, self.player_id);
        
        // Format command parameters according to the API
        let cmd_param = "delete";
        let index_param = format!("index:{}", index);
        
        // Use playlistcontrol with cmd:delete and index parameters
        match self.client.control_request(
            &self.player_id, 
            "playlist", 
            vec![cmd_param, &index_param]
        ) {
            Ok(_) => {
                debug!("Track at index {} successfully deleted from playlist", index);
                Ok(())
            },
            Err(e) => Err(format!("Failed to delete track from playlist: {}", e)),
        }
    }

    /// Play a specific song in the queue by its index
    /// 
    /// Uses the playlist index command to jump to a specific track in the playlist.
    /// 
    /// # Arguments
    /// * `index` - The zero-based index of the track to play
    /// 
    /// # Returns
    /// `Ok(())` if the command was sent successfully, or an error message
    pub fn play_queue_index(&self, index: usize) -> Result<(), String> {
        debug!("Playing track at index {} in playlist for player {}", index, self.player_id);
        
        // Use the playlist index command to jump to the specified track
        let index_str = index.to_string();
        self.send_command_with_values("playlist", vec!["index", &index_str])
    }
}