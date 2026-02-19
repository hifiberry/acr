use std::io::{self, BufRead, BufReader, Read};
use log::{warn, error, debug, info};
use std::collections::HashMap;
use serde_json::Value;
use std::thread;
use std::time::Duration;

// Import the stream helper and data structs
use crate::helpers::stream_helper::{open_stream, AccessMode};
use crate::data::song::Song;
use crate::data::player::PlayerState;
use crate::data::player::PlaybackState;  
use crate::data::capabilities::{PlayerCapability, PlayerCapabilitySet};
use crate::data::stream_details::StreamDetails;
use crate::data::loop_mode::LoopMode;

/// Type definition for the metadata callback function
pub type MetadataCallback = Box<dyn Fn(Song, PlayerState, PlayerCapabilitySet, StreamDetails) + Send + Sync>;

/// A reader for metadata from a named pipe or network connection
pub struct MetadataPipeReader {
    source: String,
    callback: Option<MetadataCallback>,
    reopen: bool,
}

impl MetadataPipeReader {
    /// Create a new metadata pipe reader
    pub fn new(source: &str) -> Self {
        Self {
            source: source.to_string(),
            callback: None,
            reopen: true, // Default to reopen behavior
        }
    }
    
    /// Create a new metadata pipe reader with both callback and reopen settings
    pub fn with_callback_and_reopen(source: &str, callback: MetadataCallback, reopen: bool) -> Self {
        Self {
            source: source.to_string(),
            callback: Some(callback),
            reopen,
        }
    }

    /// Set a callback function to be called when metadata is parsed
    pub fn set_callback(&mut self, callback: MetadataCallback) {
        self.callback = Some(callback);
    }
    
    /// Set whether the pipe should be reopened when closed
    pub fn set_reopen(&mut self, reopen: bool) {
        self.reopen = reopen;
    }
    
    /// Get whether the pipe will reopen when closed
    pub fn get_reopen(&self) -> bool {
        self.reopen
    }    /// Open the source and read it line by line until it's closed
    /// Uses auto-reopen pattern similar to the provided example
    pub fn read_and_log_pipe(&self) -> io::Result<()> {
        info!("Opening metadata source: {}", self.source);
        let mut first_open = true;

        loop {
            // Open the pipe/source (with retry logic for initial open)
            let mut stream_wrapper = loop {
                match open_stream(&self.source, AccessMode::Read) {
                    Ok(wrapper) => break wrapper,
                    Err(e) => {
                        if first_open {
                            warn!("Waiting for metadata source '{}': {}", self.source, e);
                            first_open = false;
                        }
                        thread::sleep(Duration::from_secs(1));
                    }
                }
            };
            
            // Get a reader from the stream wrapper
            let reader = match stream_wrapper.as_reader() {
                Ok(reader) => BufReader::new(reader),
                Err(e) => return Err(e),
            };

            if first_open {
                info!("Started reading from metadata source");
                first_open = false;
            }

            // Read until EOF (i.e., writer disconnects) 
            let read_result = self.read_until_eof(reader);
            
            // Check if we should exit or reopen
            if !self.reopen {
                return read_result;
            }
            
            // If reopen is enabled, the outer loop will automatically reopen
            // Optional: wait a bit before reopening to avoid hammering
            thread::sleep(Duration::from_millis(100));
        }
    }

    /// Read from a stream until EOF, processing each line
    fn read_until_eof<R: Read>(&self, mut reader: BufReader<R>) -> io::Result<()> {
        let mut buffer = String::new();
        let mut line_number = 1;

        loop {
            match reader.read_line(&mut buffer) {
                Ok(0) => {
                    // EOF — writer closed pipe, this is normal for FIFO behavior
                    debug!("Writer closed connection (EOF)");
                    return Ok(());
                },
                Ok(_) => {
                    // Successfully read some data
                    if buffer.ends_with('\n') {
                        buffer.pop(); // Remove trailing newline
                    }
                    
                    if !buffer.is_empty() {
                        self.process_line(&buffer, line_number);
                        line_number += 1;
                    }
                    
                    buffer.clear();
                },
                Err(e) => {
                    error!("Read error from metadata source: {}", e);
                    return Err(e);
                }
            }
        }
    }

    /// Process a single line of metadata
    fn process_line(&self, line: &str, line_number: u32) {
        debug!("Metadata [{}]: Processing...", line_number);
        
        match Self::parse_line(line) {
            Some((song, player, capabilities, stream_details)) => {
                // Log the structured data
                debug!("Parsed metadata [{}]:", line_number);
                debug!("  Song: '{} - {}' from album '{}'", 
                       song.title.as_deref().unwrap_or("Unknown"),
                       song.artist.as_deref().unwrap_or("Unknown"),
                       song.album.as_deref().unwrap_or("Unknown"));
                
                debug!("  Player state: {:?}", player.state);
                debug!("  Loop mode: {:?}, Shuffle: {}", player.loop_mode, player.shuffle);
                
                if !capabilities.is_empty() {
                    debug!("  Capabilities: {:?}", capabilities);
                }
                
                if let Some(_sample_rate) = stream_details.sample_rate {
                    debug!("  Audio format: {}", stream_details.format_description());
                }
                
                // If we have a callback function, call it with the parsed metadata
                if let Some(callback) = &self.callback {
                    callback(song, player, capabilities, stream_details);
                }
            },
            None => {
                warn!("Metadata [{}]: Failed to parse JSON data", line_number);
            }
        }
    }

    /// Parse a JSON line of metadata and return a tuple of (Song, PlayerState, PlayerCapabilitySet, StreamDetails) if successful
    pub fn parse_line(line: &str) -> Option<(Song, PlayerState, PlayerCapabilitySet, StreamDetails)> {
        // Parse the JSON string
        debug!("Parsing JSON line: {}", line);
        match serde_json::from_str::<Value>(line) {
            Ok(json) => {
                // Initialize a player with default values
                let mut player = PlayerState::new();
                
                // Initialize empty capabilities set
                let mut capabilities = PlayerCapabilitySet::empty();
                
                // Parse stream details if available
                let mut stream_details = StreamDetails::new();
                if let Some(stream_format) = json.get("stream_format").and_then(|sf| sf.as_object()) {
                    if let Some(sample_rate) = stream_format.get("sample_rate").and_then(|v| v.as_u64()) {
                        stream_details.sample_rate = Some(sample_rate as u32);
                    }
                    
                    if let Some(bits_per_sample) = stream_format.get("bits_per_sample").and_then(|v| v.as_u64()) {
                        stream_details.bits_per_sample = Some(bits_per_sample as u8);
                    }
                    
                    if let Some(channels) = stream_format.get("channels").and_then(|v| v.as_u64()) {
                        stream_details.channels = Some(channels as u8);
                    }
                    
                    if let Some(sample_type) = stream_format.get("sample_type").and_then(|v| v.as_str()) {
                        stream_details.sample_type = Some(sample_type.to_string());
                    }
                    
                    // Assume PCM streams are lossless
                    if let Some(sample_type) = &stream_details.sample_type {
                        if sample_type.eq_ignore_ascii_case("pcm") {
                            stream_details.lossless = Some(true);
                        }
                    }
                }
                
                // Set player state from the JSON data
                if let Some(state_str) = json.get("state").and_then(|v| v.as_str()) {
                    player.state = match state_str {
                        "playing" => PlaybackState::Playing,
                        "paused" => PlaybackState::Paused,
                        "stopped" => PlaybackState::Stopped,
                        _ => PlaybackState::Unknown,
                    };
                }
                
                // Set player position from seek value
                if let Some(seek) = json.get("seek").and_then(|v| v.as_i64()) {
                    let position = seek as f64;
                    // Convert to seconds if in milliseconds (RAAT sometimes provides milliseconds)
                    let position = if position > 10000.0 { position / 1000.0 } else { position };
                    player.position = Some(position);
                }
                
                // Add player capabilities based on JSON data
                let mut player_metadata = HashMap::new();
                
                // Process play capability
                if let Some(is_play_allowed) = json.get("is_play_allowed").and_then(|v| v.as_bool()) {
                    player_metadata.insert("is_play_allowed".to_string(), Value::Bool(is_play_allowed));
                    if is_play_allowed {
                        capabilities.add_capability(PlayerCapability::Play);
                    }
                }
                
                // Process pause capability
                if let Some(is_pause_allowed) = json.get("is_pause_allowed").and_then(|v| v.as_bool()) {
                    player_metadata.insert("is_pause_allowed".to_string(), Value::Bool(is_pause_allowed));
                    if is_pause_allowed {
                        capabilities.add_capability(PlayerCapability::Pause);
                    }
                }
                
                // Process seek capability
                if let Some(is_seek_allowed) = json.get("is_seek_allowed").and_then(|v| v.as_bool()) {
                    player_metadata.insert("is_seek_allowed".to_string(), Value::Bool(is_seek_allowed));
                    if is_seek_allowed {
                        capabilities.add_capability(PlayerCapability::Seek);
                    }
                }
                
                // Process next capability
                if let Some(is_next_allowed) = json.get("is_next_allowed").and_then(|v| v.as_bool()) {
                    player_metadata.insert("is_next_allowed".to_string(), Value::Bool(is_next_allowed));
                    if is_next_allowed {
                        capabilities.add_capability(PlayerCapability::Next);
                    }
                }
                
                // Process previous capability
                if let Some(is_previous_allowed) = json.get("is_previous_allowed").and_then(|v| v.as_bool()) {
                    player_metadata.insert("is_previous_allowed".to_string(), Value::Bool(is_previous_allowed));
                    if is_previous_allowed {
                        capabilities.add_capability(PlayerCapability::Previous);
                    }
                }
                
                // Add shuffle and loop functionality to capabilities if available in metadata
                if json.get("shuffle").is_some() {
                    capabilities.add_capability(PlayerCapability::Shuffle);
                }
                
                if json.get("loop").is_some() {
                    capabilities.add_capability(PlayerCapability::Loop);
                }
                
                // Store shuffle state if available
                if let Some(shuffle) = json.get("shuffle").and_then(|v| v.as_bool()) {
                    player_metadata.insert("shuffle".to_string(), Value::Bool(shuffle));
                    // Set as non-optional boolean
                    player.shuffle = shuffle;
                }
                
                // Store loop mode if available
                if let Some(loop_mode_str) = json.get("loop").and_then(|v| v.as_str()) {
                    player_metadata.insert("loop".to_string(), Value::String(loop_mode_str.to_string()));
                    
                    // Convert string to LoopMode enum and set in the Player struct
                    let loop_mode = match loop_mode_str.to_lowercase().as_str() {
                        "no" | "none" | "off" => LoopMode::None,
                        "song" | "track" | "one" => LoopMode::Track,
                        "playlist" | "all" => LoopMode::Playlist,
                        _ => LoopMode::None, // Default to None for unrecognized values
                    };
                    player.loop_mode = loop_mode;
                }
                
                // Store stream format in player metadata
                if let Some(stream_format) = json.get("stream_format") {
                    player_metadata.insert("stream_format".to_string(), stream_format.clone());
                }
                
                // Add metadata to player
                player.metadata = player_metadata;
                
                // Set capabilities in the player
                player.capabilities = capabilities;

                // Check if "now_playing" field exists in the JSON
                if let Some(now_playing) = json.get("now_playing").and_then(|np| np.as_object()) {
                    let mut song = Song::default();
                    let mut metadata = HashMap::new();
                    
                    // Extract basic fields
                    if let Some(title) = now_playing.get("title").and_then(|v| v.as_str()) {
                        song.title = Some(title.to_string());
                    }
                    
                    if let Some(artist) = now_playing.get("artist").and_then(|v| v.as_str()) {
                        song.artist = Some(artist.to_string());
                    }
                    
                    if let Some(album) = now_playing.get("album").and_then(|v| v.as_str()) {
                        song.album = Some(album.to_string());
                    }
                    
                    if let Some(composer) = now_playing.get("composer").and_then(|v| v.as_str()) {
                        song.album_artist = Some(composer.to_string()); // Using composer as album_artist
                        metadata.insert("composer".to_string(), Value::String(composer.to_string()));
                    }
                    
                    if let Some(length) = now_playing.get("length").and_then(|v| v.as_i64()) {
                        song.duration = Some(length as f64);
                    }
                    
                    if let Some(artwork_url) = now_playing.get("artwork_url").and_then(|v| v.as_str()) {
                        song.cover_art_url = Some(artwork_url.to_string());
                    }
                    
                    // Extract additional fields into metadata map
                    if let Some(one_line) = now_playing.get("one_line").and_then(|v| v.as_str()) {
                        metadata.insert("one_line".to_string(), Value::String(one_line.to_string()));
                    }
                    
                    if let Some(two_line_title) = now_playing.get("two_line_title").and_then(|v| v.as_str()) {
                        metadata.insert("two_line_title".to_string(), Value::String(two_line_title.to_string()));
                    }
                    
                    if let Some(two_line_subtitle) = now_playing.get("two_line_subtitle").and_then(|v| v.as_str()) {
                        metadata.insert("two_line_subtitle".to_string(), Value::String(two_line_subtitle.to_string()));
                    }
                    
                    if let Some(three_line_title) = now_playing.get("three_line_title").and_then(|v| v.as_str()) {
                        metadata.insert("three_line_title".to_string(), Value::String(three_line_title.to_string()));
                    }
                    
                    if let Some(three_line_subtitle) = now_playing.get("three_line_subtitle").and_then(|v| v.as_str()) {
                        metadata.insert("three_line_subtitle".to_string(), Value::String(three_line_subtitle.to_string()));
                    }
                    
                    if let Some(three_line_subsubtitle) = now_playing.get("three_line_subsubtitle").and_then(|v| v.as_str()) {
                        metadata.insert("three_line_subsubtitle".to_string(), Value::String(three_line_subsubtitle.to_string()));
                    }
                    
                    // Set source as RAAT
                    song.source = Some("raat".to_string());
                    
                    // Add metadata to the song
                    song.metadata = metadata;
                    
                    Some((song, player, capabilities, stream_details))
                } else {
                    debug!("No 'now_playing' field found in JSON");
                    None
                }
            },
            Err(e) => {
                warn!("Failed to parse JSON: {}", e);
                None
            }
        }
    }

}