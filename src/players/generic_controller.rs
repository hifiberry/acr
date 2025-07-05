use std::sync::{Arc, RwLock};
use std::time::SystemTime;
use std::any::Any;
use std::collections::HashMap;
use log::{debug, info, warn};
use serde_json::Value;

use crate::data::{
    PlayerCapability, PlayerCapabilitySet, Song, Track, LoopMode, 
    PlaybackState, PlayerCommand
};
use crate::data::library::LibraryInterface;
use crate::players::player_controller::{BasePlayerController, PlayerController};

/// A generic player controller that can be configured via JSON and accepts API updates
pub struct GenericPlayerController {
    /// Base controller functionality
    base: BasePlayerController,
    
    /// Player configuration name
    player_name: String,
    
    /// Current internal state
    current_song: Arc<RwLock<Option<Song>>>,
    current_state: Arc<RwLock<PlaybackState>>,
    current_loop_mode: Arc<RwLock<LoopMode>>,
    current_shuffle: Arc<RwLock<bool>>,
    current_position: Arc<RwLock<Option<f64>>>,
    current_queue: Arc<RwLock<Vec<Track>>>,
    
    /// Configuration from JSON
    config: Arc<RwLock<HashMap<String, Value>>>,
}

impl GenericPlayerController {
    /// Create a new generic player controller
    pub fn new(player_name: String) -> Self {
        debug!("Creating new GenericPlayerController: {}", player_name);
        
        // Create base controller with the player name
        let base = BasePlayerController::with_player_info(&player_name, &player_name);
        
        let controller = Self {
            base,
            player_name: player_name.clone(),
            current_song: Arc::new(RwLock::new(None)),
            current_state: Arc::new(RwLock::new(PlaybackState::Unknown)),
            current_loop_mode: Arc::new(RwLock::new(LoopMode::None)),
            current_shuffle: Arc::new(RwLock::new(false)),
            current_position: Arc::new(RwLock::new(None)),
            current_queue: Arc::new(RwLock::new(Vec::new())),
            config: Arc::new(RwLock::new(HashMap::new())),
        };
        
        // Set default capabilities - generic player can accept API events and basic commands
        controller.set_default_capabilities();
        
        controller
    }
    
    /// Create a new generic player controller from JSON configuration
    pub fn from_config(config: &Value) -> Result<Self, String> {
        let player_name = config.get("name")
            .and_then(|n| n.as_str())
            .ok_or("Generic player configuration must have a 'name' field")?;
            
        debug!("Creating GenericPlayerController from config: {}", player_name);
        
        let controller = Self::new(player_name.to_string());
        
        // Store the full configuration
        if let Ok(mut config_lock) = controller.config.write() {
            if let Some(obj) = config.as_object() {
                for (key, value) in obj {
                    config_lock.insert(key.clone(), value.clone());
                }
            }
        }
        
        // Apply any specific configuration
        controller.apply_config(config)?;
        
        Ok(controller)
    }
    
    /// Apply configuration from JSON
    fn apply_config(&self, config: &Value) -> Result<(), String> {
        // Set initial state if provided
        if let Some(initial_state) = config.get("initial_state") {
            if let Some(state_str) = initial_state.as_str() {
                let playback_state = match state_str.to_lowercase().as_str() {
                    "playing" => PlaybackState::Playing,
                    "paused" => PlaybackState::Paused,
                    "stopped" => PlaybackState::Stopped,
                    _ => PlaybackState::Unknown,
                };
                
                if let Ok(mut state) = self.current_state.write() {
                    *state = playback_state;
                }
            }
        }
        
        // Set initial shuffle if provided
        if let Some(shuffle) = config.get("shuffle").and_then(|s| s.as_bool()) {
            if let Ok(mut shuffle_lock) = self.current_shuffle.write() {
                *shuffle_lock = shuffle;
            }
        }
        
        // Set initial loop mode if provided
        if let Some(loop_mode_str) = config.get("loop_mode").and_then(|l| l.as_str()) {
            let loop_mode = match loop_mode_str.to_lowercase().as_str() {
                "song" | "track" => LoopMode::Track,
                "playlist" => LoopMode::Playlist,
                _ => LoopMode::None,
            };
            
            if let Ok(mut loop_lock) = self.current_loop_mode.write() {
                *loop_lock = loop_mode;
            }
        }
        
        Ok(())
    }
    
    /// Set default capabilities for the generic player
    fn set_default_capabilities(&self) {
        debug!("Setting default GenericPlayerController capabilities");
        
        // Generic player supports API events and basic playback control
        let mut capabilities = PlayerCapabilitySet::empty();
        capabilities.add_capability(PlayerCapability::Killable);
        capabilities.add_capability(PlayerCapability::Play);
        capabilities.add_capability(PlayerCapability::Pause);
        capabilities.add_capability(PlayerCapability::Stop);
        capabilities.add_capability(PlayerCapability::Next);
        capabilities.add_capability(PlayerCapability::Previous);
        capabilities.add_capability(PlayerCapability::Seek);
        capabilities.add_capability(PlayerCapability::Loop);
        capabilities.add_capability(PlayerCapability::Shuffle);
        
        self.base.set_capabilities(capabilities.to_vec(), true);
    }
    
    /// Process an API event and update internal state
    fn process_api_event_internal(&self, event_data: &Value) -> bool {
        debug!("Processing API event for generic player '{}': {:?}", self.player_name, event_data);
        
        // Try to extract event type
        let event_type = match event_data.get("type").and_then(|t| t.as_str()) {
            Some(t) => t,
            None => {
                warn!("API event missing 'type' field");
                return false;
            }
        };
        
        match event_type {
            "state_changed" => self.handle_state_change_event(event_data),
            "song_changed" => self.handle_song_change_event(event_data),
            "position_changed" => self.handle_position_change_event(event_data),
            "loop_mode_changed" => self.handle_loop_mode_change_event(event_data),
            "shuffle_changed" => self.handle_shuffle_change_event(event_data),
            "queue_changed" => self.handle_queue_change_event(event_data),
            _ => {
                debug!("Unknown event type '{}' for generic player", event_type);
                false
            }
        }
    }
    
    /// Handle state change events
    fn handle_state_change_event(&self, event_data: &Value) -> bool {
        if let Some(state_str) = event_data.get("state").and_then(|s| s.as_str()) {
            let playback_state = match state_str.to_lowercase().as_str() {
                "playing" => PlaybackState::Playing,
                "paused" => PlaybackState::Paused,
                "stopped" => PlaybackState::Stopped,
                _ => PlaybackState::Unknown,
            };
            
            if let Ok(mut state) = self.current_state.write() {
                *state = playback_state;
                debug!("Generic player '{}' state changed to: {:?}", self.player_name, playback_state);
                return true;
            }
        }
        false
    }
    
    /// Handle song change events
    fn handle_song_change_event(&self, event_data: &Value) -> bool {
        // Try to parse song information from the event
        let song = if let Some(song_data) = event_data.get("song") {
            self.parse_song_from_json(song_data)
        } else {
            None
        };
        
        if let Ok(mut current_song) = self.current_song.write() {
            *current_song = song;
            debug!("Generic player '{}' song changed", self.player_name);
            return true;
        }
        false
    }
    
    /// Handle position change events
    fn handle_position_change_event(&self, event_data: &Value) -> bool {
        if let Some(position) = event_data.get("position").and_then(|p| p.as_f64()) {
            if let Ok(mut pos) = self.current_position.write() {
                *pos = Some(position);
                debug!("Generic player '{}' position changed to: {}", self.player_name, position);
                return true;
            }
        }
        false
    }
    
    /// Handle loop mode change events
    fn handle_loop_mode_change_event(&self, event_data: &Value) -> bool {
        if let Some(mode_str) = event_data.get("loop_mode").and_then(|m| m.as_str()) {
            let loop_mode = match mode_str.to_lowercase().as_str() {
                "song" | "track" => LoopMode::Track,
                "playlist" => LoopMode::Playlist,
                _ => LoopMode::None,
            };
            
            if let Ok(mut mode) = self.current_loop_mode.write() {
                *mode = loop_mode;
                debug!("Generic player '{}' loop mode changed to: {:?}", self.player_name, loop_mode);
                return true;
            }
        }
        false
    }
    
    /// Handle shuffle change events
    fn handle_shuffle_change_event(&self, event_data: &Value) -> bool {
        if let Some(shuffle) = event_data.get("shuffle").and_then(|s| s.as_bool()) {
            if let Ok(mut shuffle_lock) = self.current_shuffle.write() {
                *shuffle_lock = shuffle;
                debug!("Generic player '{}' shuffle changed to: {}", self.player_name, shuffle);
                return true;
            }
        }
        false
    }
    
    /// Handle queue change events
    fn handle_queue_change_event(&self, event_data: &Value) -> bool {
        if let Some(queue_data) = event_data.get("queue").and_then(|q| q.as_array()) {
            let mut tracks = Vec::new();
            for track_data in queue_data {
                if let Some(track) = self.parse_track_from_json(track_data) {
                    tracks.push(track);
                }
            }
            
            if let Ok(mut queue) = self.current_queue.write() {
                *queue = tracks;
                debug!("Generic player '{}' queue changed ({} tracks)", self.player_name, queue.len());
                return true;
            }
        }
        false
    }
    
    /// Parse a song from JSON data
    fn parse_song_from_json(&self, song_data: &Value) -> Option<Song> {
        let mut song = Song::default();
        
        if let Some(title) = song_data.get("title").and_then(|t| t.as_str()) {
            song.title = Some(title.to_string());
        }
        
        if let Some(artist) = song_data.get("artist").and_then(|a| a.as_str()) {
            song.artist = Some(artist.to_string());
        }
        
        if let Some(album) = song_data.get("album").and_then(|a| a.as_str()) {
            song.album = Some(album.to_string());
        }
        
        // Set optional fields
        if let Some(duration) = song_data.get("duration").and_then(|d| d.as_f64()) {
            song.duration = Some(duration);
        }
        
        if let Some(uri) = song_data.get("uri").and_then(|u| u.as_str()) {
            song.stream_url = Some(uri.to_string());
        }
        
        Some(song)
    }
    
    /// Parse a track from JSON data
    fn parse_track_from_json(&self, track_data: &Value) -> Option<Track> {
        let name = track_data.get("name").and_then(|n| n.as_str()).unwrap_or("Unknown").to_string();
        let track_number = track_data.get("track_number").and_then(|t| t.as_u64()).map(|n| n as u16);
        
        let mut track = Track::new(None, track_number, name);
        
        if let Some(artist) = track_data.get("artist").and_then(|a| a.as_str()) {
            track.artist = Some(artist.to_string());
        }
        
        if let Some(uri) = track_data.get("uri").and_then(|u| u.as_str()) {
            track.uri = Some(uri.to_string());
        }
        
        Some(track)
    }
}

// Implement Clone manually
impl Clone for GenericPlayerController {
    fn clone(&self) -> Self {
        Self {
            base: self.base.clone(),
            player_name: self.player_name.clone(),
            current_song: Arc::clone(&self.current_song),
            current_state: Arc::clone(&self.current_state),
            current_loop_mode: Arc::clone(&self.current_loop_mode),
            current_shuffle: Arc::clone(&self.current_shuffle),
            current_position: Arc::clone(&self.current_position),
            current_queue: Arc::clone(&self.current_queue),
            config: Arc::clone(&self.config),
        }
    }
}

impl PlayerController for GenericPlayerController {
    fn get_capabilities(&self) -> PlayerCapabilitySet {
        self.base.get_capabilities()
    }
    
    fn get_song(&self) -> Option<Song> {
        if let Ok(song) = self.current_song.read() {
            song.clone()
        } else {
            None
        }
    }
    
    fn get_queue(&self) -> Vec<Track> {
        if let Ok(queue) = self.current_queue.read() {
            queue.clone()
        } else {
            Vec::new()
        }
    }
    
    fn get_loop_mode(&self) -> LoopMode {
        if let Ok(mode) = self.current_loop_mode.read() {
            *mode
        } else {
            LoopMode::None
        }
    }
    
    fn get_playback_state(&self) -> PlaybackState {
        if let Ok(state) = self.current_state.read() {
            *state
        } else {
            PlaybackState::Unknown
        }
    }
    
    fn get_position(&self) -> Option<f64> {
        if let Ok(pos) = self.current_position.read() {
            *pos
        } else {
            None
        }
    }
    
    fn get_shuffle(&self) -> bool {
        if let Ok(shuffle) = self.current_shuffle.read() {
            *shuffle
        } else {
            false
        }
    }
    
    fn get_player_name(&self) -> String {
        self.player_name.clone()
    }
    
    fn get_player_id(&self) -> String {
        self.player_name.clone()
    }
    
    fn get_last_seen(&self) -> Option<SystemTime> {
        Some(SystemTime::now())
    }
    
    fn as_any(&self) -> &dyn Any {
        self
    }
    
    fn start(&self) -> bool {
        info!("Starting GenericPlayerController: {}", self.player_name);
        true
    }
    
    fn stop(&self) -> bool {
        info!("Stopping GenericPlayerController: {}", self.player_name);
        true
    }
    
    fn send_command(&self, command: PlayerCommand) -> bool {
        debug!("GenericPlayerController '{}' received command: {:?}", self.player_name, command);
        
        // Generic player just logs commands but doesn't actually do anything
        // In a real implementation, this would send commands to an external player
        match command {
            PlayerCommand::Play => {
                if let Ok(mut state) = self.current_state.write() {
                    *state = PlaybackState::Playing;
                }
                true
            }
            PlayerCommand::Pause => {
                if let Ok(mut state) = self.current_state.write() {
                    *state = PlaybackState::Paused;
                }
                true
            }
            PlayerCommand::Stop => {
                if let Ok(mut state) = self.current_state.write() {
                    *state = PlaybackState::Stopped;
                }
                true
            }
            PlayerCommand::SetLoopMode(mode) => {
                if let Ok(mut loop_mode) = self.current_loop_mode.write() {
                    *loop_mode = mode;
                }
                true
            }
            PlayerCommand::SetRandom(enabled) => {
                if let Ok(mut shuffle) = self.current_shuffle.write() {
                    *shuffle = enabled;
                }
                true
            }
            PlayerCommand::Seek(position) => {
                if let Ok(mut pos) = self.current_position.write() {
                    *pos = Some(position);
                }
                true
            }
            _ => {
                debug!("Command {:?} not implemented for generic player", command);
                false
            }
        }
    }
    
    fn supports_api_events(&self) -> bool {
        true
    }
    
    fn process_api_event(&self, event_data: &serde_json::Value) -> bool {
        self.process_api_event_internal(event_data)
    }
    
    fn get_library(&self) -> Option<Box<dyn LibraryInterface>> {
        None
    }
}
