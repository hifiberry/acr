use std::io::{self, BufRead, BufReader, ErrorKind, Read};
use log::{warn, error, debug};
use std::thread;
use std::time::Duration;

// Import the stream helper and shared event processing
use crate::helpers::stream_helper::{open_stream, AccessMode};
use crate::data::song::Song;
use crate::data::player::PlayerState;
use crate::data::capabilities::PlayerCapabilitySet;
use crate::data::stream_details::StreamDetails;
use super::event_common::{EventCallback, LibrespotEventProcessor};

/// A reader for Spotify/librespot events from a named pipe or network connection
/// 
/// This reader processes JSON events from Spotify/librespot in a specific format.
/// For detailed documentation of the expected event format, see EVENT_PIPE_FORMAT.md
/// in this directory.
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
        // Delegate to the shared event processor
        LibrespotEventProcessor::parse_event_json(block)
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