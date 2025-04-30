use std::io::{self, BufRead, BufReader, ErrorKind, Read};
use log::{warn, error, debug};
use std::thread;
use std::time::Duration;
use std::collections::HashMap;
use serde_json::Value;

// Import the stream helper and data structs
use crate::helpers::stream_helper::{open_stream, AccessMode};
use crate::data::song::Song;
use crate::data::player::PlayerState;
use crate::data::player::PlaybackState;
use crate::data::capabilities::{PlayerCapability, PlayerCapabilitySet};
use crate::data::stream_details::StreamDetails;
use crate::data::loop_mode::LoopMode;

/// Type definition for the event callback function
pub type EventCallback = Box<dyn Fn(Song, PlayerState, PlayerCapabilitySet, StreamDetails) + Send + Sync>;

/// A reader for Spotify/librespot events from a named pipe or network connection
#[allow(dead_code)]
pub struct EventPipeReader {
    source: String,
    callback: Option<EventCallback>,
    reopen: bool,
}

impl EventPipeReader {
    /// Create a new event pipe reader
    #[allow(dead_code)]
    pub fn new(source: &str) -> Self {
        Self {
            source: source.to_string(),
            callback: None,
            reopen: true, // Default to reopen behavior
        }
    }
    
    /// Create a new event pipe reader with both callback and reopen settings
    #[allow(dead_code)]
    pub fn with_callback_and_reopen(source: &str, callback: EventCallback, reopen: bool) -> Self {
        Self {
            source: source.to_string(),
            callback: Some(callback),
            reopen,
        }
    }

    /// Set a callback function to be called when events are parsed
    #[allow(dead_code)]
    pub fn set_callback(&mut self, callback: EventCallback) {
        self.callback = Some(callback);
    }
    
    /// Set whether the pipe should be reopened when closed
    #[allow(dead_code)]
    pub fn set_reopen(&mut self, reopen: bool) {
        self.reopen = reopen;
    }
    
    /// Get whether the pipe will reopen when closed
    #[allow(dead_code)]
    pub fn get_reopen(&self) -> bool {
        self.reopen
    }

    /// Open the source and read it line by line until it's closed
    /// Each line is logged using debug!
    #[allow(dead_code)]
    pub fn read_and_log_pipe(&self) -> io::Result<()> {
        warn!("Opening Spotify event source: {}", self.source);

        // Use the helper function with read-only access mode
        let mut stream_wrapper = open_stream(&self.source, AccessMode::Read)?;
        
        // Get a reader from the stream wrapper
        let reader = match stream_wrapper.as_reader() {
            Ok(reader) => BufReader::new(reader),
            Err(e) => return Err(e),
        };

        warn!("Started reading from Spotify event source");

        // Keep reading until explicitly told to stop
        let result = self.read_stream(reader);
        
        // Check if we should exit or reopen
        if !self.reopen {
            return result;
        }
        
        // If we get here and reopen is true, return Ok so the calling
        // code knows it can try to reopen
        Ok(())
    }

    /// Parse a JSON block of event data
    #[allow(dead_code)]
    pub fn parse_block(block: &str) -> Option<(Song, PlayerState, PlayerCapabilitySet, StreamDetails)> {
        debug!("Parsing Spotify event: {}", block);
        
        // Parse the JSON string
        match serde_json::from_str::<Value>(block) {
            Ok(json) => {
                // Initialize objects with default values
                let mut song = Song::default();
                let mut player = PlayerState::new();
                let mut capabilities = PlayerCapabilitySet::empty();
                let mut stream_details = StreamDetails::new();
                
                // Get the event type
                let event_type = match json.get("event").and_then(|v| v.as_str()) {
                    Some(event) => event,
                    None => {
                        warn!("No event type found in Spotify event");
                        return None;
                    }
                };
                
                debug!("Processing Spotify event type: {}", event_type);
                
                // Process event based on type
                match event_type {
                    "playing" => {
                        // Set player state to playing
                        player.state = PlaybackState::Playing;
                        
                        // Extract position if available
                        if let Some(position_ms) = json.get("POSITION_MS").and_then(|v| v.as_str()).and_then(|s| s.parse::<u64>().ok()) {
                            player.position = Some(position_ms as f64 / 1000.0); // Convert ms to seconds
                        }
                        
                        // Add the track_id to player metadata for tracking
                        if let Some(track_id) = json.get("TRACK_ID").and_then(|v| v.as_str()) {
                            let mut metadata = HashMap::new();
                            metadata.insert("track_id".to_string(), serde_json::Value::String(track_id.to_string()));
                            player.metadata = metadata;
                        }
                    },
                    "paused" => {
                        // Set player state to paused
                        player.state = PlaybackState::Paused;
                        
                        // Extract position if available (same as playing)
                        if let Some(position_ms) = json.get("POSITION_MS").and_then(|v| v.as_str()).and_then(|s| s.parse::<u64>().ok()) {
                            player.position = Some(position_ms as f64 / 1000.0);
                        }
                        
                        // Add the track_id to player metadata
                        if let Some(track_id) = json.get("TRACK_ID").and_then(|v| v.as_str()) {
                            let mut metadata = HashMap::new();
                            metadata.insert("track_id".to_string(), serde_json::Value::String(track_id.to_string()));
                            player.metadata = metadata;
                        }
                    },
                    "stopped" => {
                        // Set player state to stopped
                        player.state = PlaybackState::Stopped;
                        
                        // Add the track_id to player metadata if available
                        if let Some(track_id) = json.get("TRACK_ID").and_then(|v| v.as_str()) {
                            let mut metadata = HashMap::new();
                            metadata.insert("track_id".to_string(), serde_json::Value::String(track_id.to_string()));
                            player.metadata = metadata;
                        }
                    },
                    "track_changed" => {
                        // This event has full metadata about the track
                        // Fill in the Song struct with available data
                        
                        // Basic metadata
                        if let Some(title) = json.get("NAME").and_then(|v| v.as_str()) {
                            song.title = Some(title.to_string());
                        }
                        
                        if let Some(artist) = json.get("ARTISTS").and_then(|v| v.as_str()) {
                            song.artist = Some(artist.to_string());
                        }
                        
                        if let Some(album) = json.get("ALBUM").and_then(|v| v.as_str()) {
                            song.album = Some(album.to_string());
                        }
                        
                        if let Some(album_artist) = json.get("ALBUM_ARTISTS").and_then(|v| v.as_str()) {
                            song.album_artist = Some(album_artist.to_string());
                        }
                        
                        // Track number and duration
                        if let Some(num) = json.get("NUMBER").and_then(|v| v.as_str()).and_then(|s| s.parse::<i32>().ok()) {
                            song.track_number = Some(num);
                        }
                        
                        if let Some(duration_ms) = json.get("DURATION_MS").and_then(|v| v.as_str()).and_then(|s| s.parse::<u64>().ok()) {
                            song.duration = Some(duration_ms as f64 / 1000.0); // Convert ms to seconds
                        }
                        
                        // Cover art URL
                        if let Some(covers) = json.get("COVERS").and_then(|v| v.as_str()) {
                            song.cover_art_url = Some(covers.to_string());
                        }
                        
                        // Add track_id to song metadata
                        if let Some(track_id) = json.get("TRACK_ID").and_then(|v| v.as_str()) {
                            let mut metadata = HashMap::new();
                            metadata.insert("track_id".to_string(), serde_json::Value::String(track_id.to_string()));
                            metadata.insert("source".to_string(), serde_json::Value::String("spotify".to_string()));
                            
                            if let Some(uri) = json.get("URI").and_then(|v| v.as_str()) {
                                metadata.insert("uri".to_string(), serde_json::Value::String(uri.to_string()));
                            }
                            
                            if let Some(popularity) = json.get("POPULARITY").and_then(|v| v.as_str()).and_then(|s| s.parse::<i32>().ok()) {
                                metadata.insert("popularity".to_string(), serde_json::Value::Number(serde_json::Number::from(popularity)));
                            }
                            
                            if let Some(is_explicit) = json.get("IS_EXPLICIT").and_then(|v| v.as_str()) {
                                let explicit_bool = is_explicit.to_lowercase() == "true";
                                metadata.insert("explicit".to_string(), serde_json::Value::Bool(explicit_bool));
                            }
                            
                            song.metadata = metadata;
                        }
                        
                        // Set stream source as Spotify
                        song.source = Some("spotify".to_string());
                        
                        // For track_changed events, assume high quality audio
                        stream_details.sample_rate = Some(44100);
                        stream_details.bits_per_sample = Some(16);
                        stream_details.channels = Some(2);
                        stream_details.sample_type = Some("ogg".to_string());
                        stream_details.lossless = Some(false); // Spotify is lossy
                    },
                    "volume_changed" => {
                        // Update volume information
                        if let Some(volume_raw) = json.get("VOLUME").and_then(|v| v.as_str()).and_then(|s| s.parse::<i32>().ok()) {
                            // Spotify volume is 0-65536, we need to convert to 0-100
                            let volume = (volume_raw * 100) / 65536;
                            player.volume = Some(volume);
                        }
                    },
                    "repeat_changed" => {
                        // Update repeat/loop mode
                        let repeat_track = json.get("REPEAT_TRACK")
                            .and_then(|v| v.as_str())
                            .map_or(false, |s| s.to_lowercase() == "true");
                        
                        let repeat = json.get("REPEAT")
                            .and_then(|v| v.as_str())
                            .map_or(false, |s| s.to_lowercase() == "true");
                        
                        player.loop_mode = if repeat_track {
                            LoopMode::Track
                        } else if repeat {
                            LoopMode::Playlist
                        } else {
                            LoopMode::None
                        };
                    },
                    "shuffle_changed" => {
                        // Update shuffle status
                        let shuffle = json.get("SHUFFLE")
                            .and_then(|v| v.as_str())
                            .map_or(false, |s| s.to_lowercase() == "true");
                        
                        player.shuffle = shuffle;
                    },
                    "seeked" => {
                        // Handle position change from seek event
                        debug!("Processing seek position change");
                        
                        // Extract position if available
                        if let Some(position_ms) = json.get("POSITION_MS").and_then(|v| v.as_str()).and_then(|s| s.parse::<u64>().ok()) {
                            player.position = Some(position_ms as f64 / 1000.0); // Convert ms to seconds
                            debug!("Updated position to {:.2}s from seek event", player.position.unwrap());
                        }
                        
                        // Add the track_id to player metadata for tracking
                        if let Some(track_id) = json.get("TRACK_ID").and_then(|v| v.as_str()) {
                            let mut metadata = HashMap::new();
                            metadata.insert("track_id".to_string(), serde_json::Value::String(track_id.to_string()));
                            player.metadata = metadata;
                        }
                    },
                    "loading" | "play_request_id_changed" | "preloading" => {
                        // don't use for anything
                        debug!("Ignoring event type: {}", event_type);
                        return None
                    },
                    _ => {
                        // Unknown event type, log but don't try to process it
                        warn!("Unknown Spotify event type: {}", event_type);
                        return None;
                    }
                }
                
                // For all events, set the seek capability if we have duration
                if song.duration.is_some() {
                    capabilities.add_capability(PlayerCapability::Seek);
                }
                
                // Set the capabilities in the player state
                player.capabilities = capabilities;
                
                debug!("Successfully parsed Spotify event: {} - state: {:?}", 
                       event_type, player.state);
                
                Some((song, player, capabilities, stream_details))
            },
            Err(e) => {
                warn!("Failed to parse Spotify event JSON: {}", e);
                None
            }
        }
    }

    /// Read from a stream until it's closed
    #[allow(dead_code)]
    fn read_stream<R: Read>(&self, mut reader: BufReader<R>) -> io::Result<()> {
        let mut buffer = String::new();
        let mut current_json = String::new();
        let mut in_json_object = false;
        let mut json_objects_processed = 0;

        loop {
            match reader.read_line(&mut buffer) {
                Ok(0) => {
                    // End of data, exit the loop
                    debug!("End of data received from Spotify event pipe, exiting read loop");
                    return Ok(());
                },
                Ok(_) => {
                    // Successfully read some data
                    if buffer.ends_with('\n') {
                        buffer.pop(); // Remove trailing newline
                    }
                    
                    let trimmed_line = buffer.trim();
                    
                    if trimmed_line == "{" {
                        // Start of a new JSON object
                        in_json_object = true;
                        current_json.clear();
                        current_json.push_str(trimmed_line);
                    } else if trimmed_line == "}" && in_json_object {
                        // End of the current JSON object
                        current_json.push_str(trimmed_line);
                        in_json_object = false;
                        
                        // Process the complete JSON object
                        debug!("Complete JSON object received: {}", current_json);
                        
                        // Parse the JSON string and call callback if successful
                        if let Some(result) = Self::parse_block(&current_json) {
                            if let Some(callback) = &self.callback {
                                let (song, player_state, capabilities, stream_details) = result;
                                
                                // Log parsing results
                                debug!("Parsed Spotify event [{}]:", json_objects_processed + 1);
                                debug!("  Player state: {:?}", player_state.state);
                                
                                if let Some(title) = &song.title {
                                    debug!("  Song: '{}' by '{}'", 
                                           title, 
                                           song.artist.as_deref().unwrap_or("Unknown"));
                                }
                                
                                if let Some(position) = player_state.position {
                                    debug!("  Position: {:.1}s", position);
                                }
                                
                                // Call the callback with the parsed data
                                callback(song, player_state, capabilities, stream_details);
                            }
                            
                            json_objects_processed += 1;
                        }
                    } else if in_json_object && !trimmed_line.is_empty() {
                        // Add this line to the current JSON object being built
                        current_json.push_str("\n");
                        current_json.push_str(trimmed_line);
                    }
                    
                    buffer.clear();
                },
                Err(e) => {
                    // For errors, exit the loop
                    error!("Error reading from Spotify event source: {}", e);
                    return Err(e);
                }
            }
        }
    }
    
    /// Try to reopen the event pipe with a backoff strategy
    #[allow(dead_code)]
    pub fn reopen_with_backoff(&self) -> io::Result<()> {
        // Start with a small delay and increase it on failures
        let mut retry_delay = Duration::from_millis(500);
        let max_delay = Duration::from_secs(10);
        let mut attempt = 1;
        
        loop {
            debug!("Attempting to reopen Spotify event pipe (attempt {})", attempt);
            match self.read_and_log_pipe() {
                Ok(_) => {
                    // If the pipe closed normally and we're not reopening, we're done
                    if !self.reopen {
                        debug!("Spotify event pipe closed and reopening is disabled");
                        return Ok(());
                    }
                    
                    // If we get here, the pipe closed but we're supposed to reopen
                    warn!("Spotify event pipe closed, will reopen after delay");
                    thread::sleep(retry_delay);
                    attempt += 1;
                },
                Err(e) => {
                    // If the error is that the pipe doesn't exist, wait and retry
                    if e.kind() == ErrorKind::NotFound {
                        warn!("Spotify event pipe not found, waiting for it to be created");
                    } else {
                        warn!("Error opening Spotify event pipe: {}", e);
                    }
                    
                    thread::sleep(retry_delay);
                    attempt += 1;
                    
                    // Increase retry delay (with a cap)
                    retry_delay = std::cmp::min(retry_delay * 2, max_delay);
                }
            }
        }
    }
}