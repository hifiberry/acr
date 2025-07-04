use std::collections::HashMap;
use log::{warn, debug};
use serde_json::Value;

// Import the data structs
use crate::data::song::Song;
use crate::data::player::PlayerState;
use crate::data::player::PlaybackState;
use crate::data::capabilities::{PlayerCapability, PlayerCapabilitySet};
use crate::data::stream_details::StreamDetails;
use crate::data::loop_mode::LoopMode;

/// Type definition for the event callback function
/// This is shared between both event pipe reader and event API processor
pub type EventCallback = Box<dyn Fn(Song, PlayerState, PlayerCapabilitySet, StreamDetails) + Send + Sync>;

/// Shared event processing logic for Librespot events
pub struct LibrespotEventProcessor;

impl LibrespotEventProcessor {
    /// Parse a JSON block of Librespot event data
    /// This is the core parsing logic shared between pipe reader and API processor
    pub fn parse_event_json(block: &str) -> Option<(Song, PlayerState, PlayerCapabilitySet, StreamDetails)> {
        debug!("Parsing Librespot event: {}", block);
        
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
                        warn!("No event type found in Librespot event");
                        return None;
                    }
                };
                
                debug!("Processing Librespot event type: {}", event_type);
                
                // Process event based on type
                match event_type {
                    "playing" => {
                        Self::process_playback_event(&json, &mut player, "playing");
                    },
                    "paused" => {
                        Self::process_playback_event(&json, &mut player, "paused");
                    },
                    "stopped" => {
                        Self::process_playback_event(&json, &mut player, "stopped");
                    },
                    "track_changed" => {
                        Self::process_track_changed_event(&json, &mut song, &mut stream_details);
                    },
                    "volume_changed" => {
                        Self::process_volume_changed_event(&json, &mut player);
                    },
                    "repeat_changed" => {
                        Self::process_repeat_changed_event(&json, &mut player);
                    },
                    "shuffle_changed" => {
                        Self::process_shuffle_changed_event(&json, &mut player);
                    },
                    "seeked" => {
                        Self::process_seeked_event(&json, &mut player);
                    },
                    "loading" | "play_request_id_changed" | "preloading" => {
                        // don't use for anything
                        debug!("Ignoring event type: {}", event_type);
                        return None;
                    },
                    _ => {
                        // Unknown event type, log but don't try to process it
                        warn!("Unknown Librespot event type: {}", event_type);
                        return None;
                    }
                }
                
                // For all events, set the seek capability if we have duration
                if song.duration.is_some() {
                    capabilities.add_capability(PlayerCapability::Seek);
                }
                
                // Set the capabilities in the player state
                player.capabilities = capabilities;
                
                debug!("Successfully parsed Librespot event: {} - state: {:?}", 
                       event_type, player.state);
                
                Some((song, player, capabilities, stream_details))
            },
            Err(e) => {
                warn!("Failed to parse Librespot event JSON: {}", e);
                None
            }
        }
    }

    /// Process playback state events (playing, paused, stopped)
    fn process_playback_event(json: &Value, player: &mut PlayerState, event_type: &str) {
        // Set player state based on event type
        player.state = match event_type {
            "playing" => PlaybackState::Playing,
            "paused" => PlaybackState::Paused,
            "stopped" => PlaybackState::Stopped,
            _ => PlaybackState::Unknown,
        };
        
        // Extract position if available (for playing and paused events)
        if event_type == "playing" || event_type == "paused" {
            if let Some(position_ms) = json.get("POSITION_MS").and_then(|v| v.as_str()).and_then(|s| s.parse::<u64>().ok()) {
                player.position = Some(position_ms as f64 / 1000.0); // Convert ms to seconds
            }
        }
        
        // Add the track_id to player metadata for tracking
        if let Some(track_id) = json.get("TRACK_ID").and_then(|v| v.as_str()) {
            let mut metadata = HashMap::new();
            metadata.insert("track_id".to_string(), serde_json::Value::String(track_id.to_string()));
            player.metadata = metadata;
        }
    }

    /// Process track_changed events
    fn process_track_changed_event(json: &Value, song: &mut Song, stream_details: &mut StreamDetails) {
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
    }

    /// Process volume_changed events
    fn process_volume_changed_event(json: &Value, player: &mut PlayerState) {
        if let Some(volume_raw) = json.get("VOLUME").and_then(|v| v.as_str()).and_then(|s| s.parse::<i32>().ok()) {
            // Spotify volume is 0-65536, we need to convert to 0-100
            let volume = (volume_raw * 100) / 65536;
            player.volume = Some(volume);
        }
    }

    /// Process repeat_changed events
    fn process_repeat_changed_event(json: &Value, player: &mut PlayerState) {
        let repeat_track = json.get("REPEAT_TRACK")
            .and_then(|v| v.as_str())
            .is_some_and(|s| s.to_lowercase() == "true");
        
        let repeat = json.get("REPEAT")
            .and_then(|v| v.as_str())
            .is_some_and(|s| s.to_lowercase() == "true");
        
        player.loop_mode = if repeat_track {
            LoopMode::Track
        } else if repeat {
            LoopMode::Playlist
        } else {
            LoopMode::None
        };
    }

    /// Process shuffle_changed events
    fn process_shuffle_changed_event(json: &Value, player: &mut PlayerState) {
        let shuffle = json.get("SHUFFLE")
            .and_then(|v| v.as_str())
            .is_some_and(|s| s.to_lowercase() == "true");
        
        player.shuffle = shuffle;
    }

    /// Process seeked events
    fn process_seeked_event(json: &Value, player: &mut PlayerState) {
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
    }

    /// Common debug logging for processed events
    pub fn log_processed_event(song: &Song, player_state: &PlayerState) {
        debug!("Successfully processed Librespot event");
        debug!("  Player state: {:?}", player_state.state);
        
        if let Some(title) = &song.title {
            debug!("  Song: '{}' by '{}'", 
                   title, 
                   song.artist.as_deref().unwrap_or("Unknown"));
        }
        
        if let Some(position) = player_state.position {
            debug!("  Position: {:.1}s", position);
        }
    }
}
