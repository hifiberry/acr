use std::sync::{Arc, RwLock};
use std::collections::HashMap;
use log::{debug, warn, error};
use serde_json::Value;
use rocket::serde::json::Json;
use rocket::post;

// Import the shared event processing
use crate::data::song::Song;
use crate::data::player::PlayerState;
use crate::data::capabilities::PlayerCapabilitySet;
use crate::data::stream_details::StreamDetails;
use super::event_common::{EventCallback, LibrespotEventProcessor};

/// A processor for Librespot events via API endpoint
pub struct EventApiProcessor {
    callback: Option<EventCallback>,
    enabled: bool,
}

impl EventApiProcessor {
    /// Create a new event API processor
    pub fn new() -> Self {
        Self {
            callback: None,
            enabled: true,
        }
    }
    
    /// Create a new event API processor with callback
    pub fn with_callback(callback: EventCallback) -> Self {
        Self {
            callback: Some(callback),
            enabled: true,
        }
    }

    /// Set a callback function to be called when events are processed
    pub fn set_callback(&mut self, callback: EventCallback) {
        self.callback = Some(callback);
    }
    
    /// Enable or disable the processor
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }
    
    /// Get whether the processor is enabled
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Process a JSON event
    pub fn process_event(&self, json_data: &Value) -> Option<(Song, PlayerState, PlayerCapabilitySet, StreamDetails)> {
        if !self.enabled {
            debug!("Event API processor is disabled, ignoring event");
            return None;
        }
        
        debug!("Processing Librespot event via API: {}", json_data);
        
        // Convert to string and use the shared event processor
        let json_str = json_data.to_string();
        let result = LibrespotEventProcessor::parse_event_json(&json_str);
        
        if let Some((song, player_state, capabilities, stream_details)) = &result {
            // Use the shared debug logging
            LibrespotEventProcessor::log_processed_event(song, player_state);
            
            // Call the callback with the parsed data
            if let Some(callback) = &self.callback {
                callback(song.clone(), player_state.clone(), *capabilities, stream_details.clone());
            }
        }
        
        result
    }
}

/// Global storage for event API processors (one per player instance)
type ProcessorMap = HashMap<String, Arc<RwLock<EventApiProcessor>>>;
static mut PROCESSORS: Option<RwLock<ProcessorMap>> = None;
static PROCESSORS_INIT: std::sync::Once = std::sync::Once::new();

/// Initialize the global processor map
fn get_processors() -> &'static RwLock<ProcessorMap> {
    unsafe {
        PROCESSORS_INIT.call_once(|| {
            PROCESSORS = Some(RwLock::new(HashMap::new()));
        });
        PROCESSORS.as_ref().unwrap()
    }
}

/// Register an event API processor for a player
pub fn register_processor(player_id: &str, processor: Arc<RwLock<EventApiProcessor>>) {
    let processors = get_processors();
    if let Ok(mut map) = processors.write() {
        map.insert(player_id.to_string(), processor);
        debug!("Registered event API processor for player: {}", player_id);
    } else {
        error!("Failed to register event API processor for player: {}", player_id);
    }
}

/// Unregister an event API processor for a player
pub fn unregister_processor(player_id: &str) {
    let processors = get_processors();
    if let Ok(mut map) = processors.write() {
        if map.remove(player_id).is_some() {
            debug!("Unregistered event API processor for player: {}", player_id);
        }
    }
}

/// Get an event API processor for a player
pub fn get_processor(player_id: &str) -> Option<Arc<RwLock<EventApiProcessor>>> {
    let processors = get_processors();
    if let Ok(map) = processors.read() {
        map.get(player_id).cloned()
    } else {
        None
    }
}

/// Response structure for the API endpoint
#[derive(serde::Serialize)]
pub struct EventResponse {
    pub success: bool,
    pub message: String,
}

/// API endpoint to receive Librespot events
#[post("/api/player/librespot/update", data = "<event_data>")]
pub fn librespot_event_update(event_data: Json<Value>) -> Json<EventResponse> {
    debug!("Received Librespot event via API");
    
    // For now, we'll process for the default "spotify" player
    // In the future, this could be extended to support multiple instances
    let player_id = "spotify";
    
    if let Some(processor) = get_processor(player_id) {
        if let Ok(processor_guard) = processor.read() {
            match processor_guard.process_event(&event_data) {
                Some(_) => {
                    Json(EventResponse {
                        success: true,
                        message: "Event processed successfully".to_string(),
                    })
                }
                None => {
                    Json(EventResponse {
                        success: false,
                        message: "Failed to process event or processor disabled".to_string(),
                    })
                }
            }
        } else {
            error!("Failed to acquire read lock on event processor");
            Json(EventResponse {
                success: false,
                message: "Internal error: could not access processor".to_string(),
            })
        }
    } else {
        warn!("No event API processor registered for player: {}", player_id);
        Json(EventResponse {
            success: false,
            message: format!("No event processor registered for player: {}", player_id),
        })
    }
}
