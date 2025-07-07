use crate::players::player_controller::{BasePlayerController, PlayerController};
use crate::data::{PlayerCapability, PlayerCapabilitySet, Song, LoopMode, PlaybackState, PlayerCommand, PlayerState, Track};
use crate::data::stream_details::StreamDetails;
use delegate::delegate;
use std::sync::{Arc, RwLock};
use log::{debug, info, warn, error, trace};
use std::any::Any;
use crate::data::PlayerUpdate;
use serde_json::json;

/// Librespot player controller implementation
/// This controller interfaces with Spotify/librespot via API endpoints
pub struct LibrespotPlayerController {
    /// Base controller
    base: BasePlayerController,
    
    /// Path to the librespot executable
    process_name: String,
    
    /// Current song information
    current_song: Arc<RwLock<Option<Song>>>,

    /// Current player state
    current_state: Arc<RwLock<PlayerState>>,
    
    /// Current stream details
    stream_details: Arc<RwLock<Option<StreamDetails>>>,
    
    /// Whether to enable API updates/events
    enable_api_updates: bool,
    
    /// What to do when receiving pause/stop commands: "systemd", "kill", or None
    on_pause_event: Option<String>,
}

// Manually implement Clone for LibrespotPlayerController
impl Clone for LibrespotPlayerController {
    fn clone(&self) -> Self {
        LibrespotPlayerController {
            // Share the BasePlayerController instance to maintain listener registrations
            base: self.base.clone(),
            process_name: self.process_name.clone(),
            current_song: Arc::clone(&self.current_song),
            current_state: Arc::clone(&self.current_state),
            stream_details: Arc::clone(&self.stream_details),
            enable_api_updates: self.enable_api_updates,
            on_pause_event: self.on_pause_event.clone(),
        }
    }
}



impl LibrespotPlayerController {
    /// Create a new Librespot player controller with default settings
    #[allow(dead_code)]
    pub fn new() -> Self {
        debug!("Creating new LibrespotPlayerController with default settings");
        let process = "/usr/bin/librespot"; // Default process path
        
        // Create a base controller with player name and ID
        let base = BasePlayerController::with_player_info("spotify", "librespot");
        
        let player = Self {
            base,
            process_name: process.to_string(),
            current_song: Arc::new(RwLock::new(None)),
            current_state: Arc::new(RwLock::new(PlayerState::new())),
            stream_details: Arc::new(RwLock::new(None)),
            enable_api_updates: true, // Default to enabled
            on_pause_event: None,
        };
        
        // Set default capabilities - only Killable is available
        player.set_default_capabilities();
        
        player
    }
    
    /// Create a new Librespot player controller with API updates setting
    pub fn with_api_updates(enable_api_updates: bool) -> Self {
        debug!("Creating new LibrespotPlayerController with enable_api_updates: {}", enable_api_updates);
        let process = "/usr/bin/librespot"; // Default process path
        
        // Create a base controller with player name and ID
        let base = BasePlayerController::with_player_info("spotify", "librespot");
        
        let player = Self {
            base,
            process_name: process.to_string(),
            current_song: Arc::new(RwLock::new(None)),
            current_state: Arc::new(RwLock::new(PlayerState::new())),
            stream_details: Arc::new(RwLock::new(None)),
            enable_api_updates,
            on_pause_event: None,
        };
        
        // Set default capabilities - only Killable is available
        player.set_default_capabilities();
        
        player
    }

    /// Create a new Librespot player controller with fully custom settings and systemd unit check
    pub fn with_config_and_systemd(process_name: &str, systemd_unit: Option<&str>) -> Self {
        Self::with_full_config(process_name, systemd_unit, true)
    }
    
    /// Create a new Librespot player controller with full configuration options
    pub fn with_full_config(
        process_name: &str,
        systemd_unit: Option<&str>,
        enable_api_updates: bool
    ) -> Self {
        debug!("Creating new LibrespotPlayerController with process_name: {}, systemd_unit: {:?}, enable_api_updates: {}", 
               process_name, systemd_unit, enable_api_updates);
        
        // Check systemd unit if specified
        if let Some(unit_name) = systemd_unit {
            if !unit_name.is_empty() {
                match crate::helpers::systemd::SystemdHelper::new().is_unit_active(unit_name) {
                    Ok(true) => {
                        debug!("Systemd unit '{}' is active", unit_name);
                    }
                    Ok(false) => {
                        warn!("Systemd unit '{}' is not active - librespot player may not work correctly", unit_name);
                    }
                    Err(e) => {
                        warn!("Could not check systemd unit '{}': {} - continuing anyway", unit_name, e);
                    }
                }
            }
        }
        
        // Create a base controller with player name and ID
        let base = BasePlayerController::with_player_info("spotify", "librespot");
        
        let player = Self {
            base,
            process_name: process_name.to_string(),
            current_song: Arc::new(RwLock::new(None)),
            current_state: Arc::new(RwLock::new(PlayerState::new())),
            stream_details: Arc::new(RwLock::new(None)),
            enable_api_updates,
            on_pause_event: None,
        };
        
        // Set default capabilities - only Killable is available
        player.set_default_capabilities();
        
        player
    }
    
    /// Set the default capabilities for this player
    fn set_default_capabilities(&self) {
        debug!("Setting default LibrespotPlayerController capabilities");
        
        // Only the Killable capability is available (previously incorrectly named Kill)
        self.base.set_capabilities(vec![
            PlayerCapability::Killable,
        ], false); // Don't notify on initialization
    }
    
    /// Set whether to enable API updates
    #[allow(dead_code)]
    pub fn set_enable_api_updates(&mut self, enable: bool) {
        debug!("Setting Librespot API updates to: {}", enable);
        self.enable_api_updates = enable;
    }
    
    /// Get whether API updates are enabled
    #[allow(dead_code)]
    pub fn get_enable_api_updates(&self) -> bool {
        self.enable_api_updates
    }
    
    /// Set the path to the librespot executable
    #[allow(dead_code)]
    pub fn set_process_name(&mut self, process_name: &str) {
        debug!("Setting Librespot process name to: {}", process_name);
        self.process_name = process_name.to_string();
    }
    
    /// Get the path to the librespot executable
    #[allow(dead_code)]
    pub fn get_process_name(&self) -> &str {
        &self.process_name
    }
    
    /// Set the on_pause_event action
    #[allow(dead_code)]
    pub fn set_on_pause_event(&mut self, on_pause_event: Option<String>) {
        debug!("Setting Librespot on_pause_event to: {:?}", on_pause_event);
        self.on_pause_event = on_pause_event;
    }
    
    /// Get the on_pause_event action
    #[allow(dead_code)]
    pub fn get_on_pause_event(&self) -> &Option<String> {
        &self.on_pause_event
    }


    
    /// Process event updates from the pipe reader
    fn update_from_event(&self, song: Song, player_state: PlayerState, 
                       capabilities: PlayerCapabilitySet, stream_details: StreamDetails) {
        log::info!("[API DEBUG] update_from_event called: state={:?}, song={:?}, capabilities={:?}, stream_details={:?}", player_state.state, song.title, capabilities, stream_details);
        
        // Store the new song if different from current and if there's actual song data
        let mut song_to_notify: Option<Song> = None;
        {
            let mut current_song = self.current_song.write().unwrap();
            
            // Only update song if the incoming song has meaningful data (title, artist, or album)
            let has_song_data = song.title.is_some() || song.artist.is_some() || song.album.is_some();
            
            if has_song_data {
                let song_changed = match (&*current_song, &song) {
                    (Some(old), new) => old.title != new.title || old.artist != new.artist || old.album != new.album,
                    (None, _) => true,
                };
                if song_changed {
                    debug!("[API DEBUG] Song changed: {:?} -> {:?}", current_song.as_ref().map(|s| &s.title), song.title);
                    *current_song = Some(song.clone());
                    song_to_notify = Some(song);
                }
            } else {
                debug!("[API DEBUG] Ignoring song update with no meaningful data");
            }
        }
        
        // Update stored player state
        if let Ok(mut current_state) = self.current_state.write() {
            let new_state = player_state.state;
            let state_changed = current_state.state != new_state;
            let position_changed = current_state.position != player_state.position;
            let shuffle_changed = current_state.shuffle != player_state.shuffle;
            let loop_mode_changed = current_state.loop_mode != player_state.loop_mode;
            
            if state_changed {
                log::info!("[API DEBUG] Librespot state change: {:?} -> {:?}", current_state.state, new_state);
            }
            if new_state == PlaybackState::Playing || state_changed {
                log::info!("[API DEBUG] Notifying state changed: {:?}", new_state);
                self.base.notify_state_changed(new_state);
            }
            
            // Notify about position changes
            if position_changed {
                if let Some(position) = player_state.position {
                    log::info!("[API DEBUG] Notifying position changed: {}", position);
                    self.base.notify_position_changed(position);
                }
            }
            
            // Notify about shuffle changes
            if shuffle_changed {
                log::info!("[API DEBUG] Notifying shuffle changed: {}", player_state.shuffle);
                self.base.notify_random_changed(player_state.shuffle);
            }
            
            // Notify about loop mode changes
            if loop_mode_changed {
                log::info!("[API DEBUG] Notifying loop mode changed: {:?}", player_state.loop_mode);
                self.base.notify_loop_mode_changed(player_state.loop_mode);
            }
            
            // Update the stored state
            *current_state = player_state;
        } else {
            warn!("[API DEBUG] Failed to acquire lock on current state");
        }
        
        // Update stored capabilities - although capabilities are fixed for Librespot
        let capabilities_changed = self.base.set_capabilities_set(capabilities, false);
        if capabilities_changed {
            let current_caps = self.base.get_capabilities();
            log::info!("[API DEBUG] Capabilities changed: {:?}", current_caps);
            self.base.notify_capabilities_changed(&current_caps);
        }
        
        // Update stored stream details
        if let Ok(mut details) = self.stream_details.write() {
            log::info!("[API DEBUG] Stream details updated: {:?}", stream_details);
            *details = Some(stream_details);
        }
        
        // Now notify listeners of song change if needed
        if let Some(song) = song_to_notify {
            log::info!("[API DEBUG] Notifying song changed: {:?}", song.title);
            self.base.notify_song_changed(Some(&song));
        }
        
        // Mark the player as alive since we got data
        self.base.alive();
    }
    
    /// Convert generic API event format to Librespot event format
    fn convert_generic_to_librespot_event(&self, event_data: &serde_json::Value) -> Option<serde_json::Value> {
        log::info!("[API DEBUG] convert_generic_to_librespot_event called: event_data={:?}", event_data);
        // Get the event type from the generic format
        let event_type = event_data.get("type").and_then(|t| t.as_str())?;
        
        match event_type {
            "state_changed" => {
                let state = event_data.get("state").and_then(|s| s.as_str())?;
                let librespot_event = match state {
                    "playing" => "playing",
                    "paused" => "paused", 
                    "stopped" => "stopped",
                    _ => return None,
                };
                
                let mut result = json!({ "event": librespot_event });
                
                // Add position if available
                if let Some(position) = event_data.get("position").and_then(|p| p.as_f64()) {
                    result["POSITION_MS"] = json!((position * 1000.0) as u64);
                }
                
                Some(result)
            },
            "song_changed" => {
                let mut result = json!({ "event": "track_changed" });
                
                if let Some(song) = event_data.get("song") {
                    if let Some(title) = song.get("title").and_then(|t| t.as_str()) {
                        result["NAME"] = json!(title);
                    }
                    if let Some(artist) = song.get("artist").and_then(|a| a.as_str()) {
                        result["ARTISTS"] = json!(artist);
                    }
                    if let Some(album) = song.get("album").and_then(|a| a.as_str()) {
                        result["ALBUM"] = json!(album);
                    }
                    if let Some(duration) = song.get("duration").and_then(|d| d.as_f64()) {
                        result["DURATION_MS"] = json!((duration * 1000.0) as u64);
                    }
                    if let Some(track_number) = song.get("track_number").and_then(|t| t.as_i64()) {
                        result["NUMBER"] = json!(track_number.to_string());
                    }
                    if let Some(cover_url) = song.get("cover_art_url").and_then(|c| c.as_str()) {
                        result["COVERS"] = json!(cover_url);
                    }
                    
                    // Try to extract track_id from metadata
                    if let Some(metadata) = song.get("metadata") {
                        if let Some(track_id) = metadata.get("track_id").and_then(|t| t.as_str()) {
                            result["TRACK_ID"] = json!(track_id);
                        }
                        if let Some(uri) = metadata.get("uri").and_then(|u| u.as_str()) {
                            result["URI"] = json!(uri);
                        }
                    }
                }
                
                Some(result)
            },
            "position_changed" => {
                if let Some(position) = event_data.get("position").and_then(|p| p.as_f64()) {
                    Some(json!({
                        "event": "seeked",
                        "POSITION_MS": (position * 1000.0) as u64
                    }))
                } else {
                    None
                }
            },
            "loop_mode_changed" => {
                if let Some(mode) = event_data.get("mode").and_then(|m| m.as_str()) {
                    let (repeat, repeat_track) = match mode {
                        "song" | "track" => ("false", "true"),
                        "playlist" | "all" => ("true", "false"),
                        "none" => ("false", "false"),
                        _ => return None,
                    };
                    
                    Some(json!({
                        "event": "repeat_changed",
                        "REPEAT": repeat,
                        "REPEAT_TRACK": repeat_track
                    }))
                } else {
                    None
                }
            },
            "shuffle_changed" => {
                let shuffle = event_data.get("enabled").and_then(|e| e.as_bool()).unwrap_or(false);
                Some(json!({
                    "event": "shuffle_changed",
                    "SHUFFLE": if shuffle { "true" } else { "false" }
                }))
            },
            _ => {
                debug!("Unknown generic event type for Librespot conversion: {}", event_type);
                None
            }
        }
    }
}

impl PlayerController for LibrespotPlayerController {
    delegate! {
        to self.base {
            fn get_capabilities(&self) -> PlayerCapabilitySet;
            fn get_last_seen(&self) -> Option<std::time::SystemTime>;
        }
    }
    
    fn get_song(&self) -> Option<Song> {
        debug!("Getting current song from stored value");
        // Return a clone of the stored song
        if let Ok(song) = self.current_song.read() {
            song.clone()
        } else {
            warn!("Failed to acquire read lock for current song");
            None
        }
    }
    
    fn get_loop_mode(&self) -> LoopMode {
        debug!("Getting current loop mode");
        // Loop mode is not supported, always return None
        LoopMode::None
    }
    
    fn get_playback_state(&self) -> PlaybackState {
        trace!("Getting current playback state");
        // Try to get the state from the current state with a timeout
        // Use try_read() to attempt a non-blocking read
        match self.current_state.try_read() {
            Ok(state) => {
                trace!("Got current playback state: {:?}", state.state);
                return state.state;
            },
            Err(_) => {
                warn!("Could not acquire immediate read lock for playback state, returning unknown state");
                return PlaybackState::Unknown;
            }
        }
    }
    
    fn get_position(&self) -> Option<f64> {
        trace!("Getting current playback position");
        // Try to get the position from the current state with a timeout
        match self.current_state.try_read() {
            Ok(state) => {
                trace!("Got current position: {:?}", state.position);
                return state.position;
            },
            Err(_) => {
                warn!("Could not acquire immediate read lock for position, returning None");
                return None;
            }
        }
    }
    
    fn get_shuffle(&self) -> bool {
        debug!("Getting current shuffle state");
        // Shuffle is not supported for direct control
        false
    }
    
    fn get_player_name(&self) -> String {
        "spotify".to_string()
    }
    
    fn get_player_id(&self) -> String {
        "librespot".to_string()
    }
    
    fn send_command(&self, command: PlayerCommand) -> bool {
        info!("Sending command to Librespot player: {}", command);
        
        // Handle pause/stop commands with on_pause_event action
        match command {
            PlayerCommand::Pause | PlayerCommand::Stop => {
                if let Some(ref action) = self.on_pause_event {
                    match action.as_str() {
                        "systemd" => {
                            info!("Received {} command, restarting librespot via systemd", command);
                            match crate::helpers::systemd::SystemdHelper::new().restart_unit("librespot") {
                                Ok(_) => {
                                    info!("Successfully restarted librespot systemd unit");
                                    return true;
                                }
                                Err(e) => {
                                    error!("Failed to restart librespot systemd unit: {}", e);
                                    return false;
                                }
                            }
                        }
                        "kill" => {
                            info!("Received {} command, killing librespot process", command);
                            return self.kill_process();
                        }
                        _ => {
                            debug!("Received {} command, doing nothing (on_pause_event='{}')", command, action);
                            return true;
                        }
                    }
                } else {
                    debug!("Received {} command, doing nothing (on_pause_event not configured)", command);
                    return true;
                }
            }
            PlayerCommand::Kill => {
                return self.kill_process();
            }
            _ => {
                // Any other command is not supported
                warn!("Command not supported by Librespot: {}", command);
                false
            }
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn start(&self) -> bool {
        info!("Starting Librespot player controller (API mode only)");
        
        // No pipe listeners to start
        self.base.alive();
        true
    }
    
    fn stop(&self) -> bool {
        info!("Stopping Librespot player controller");
        
        // Nothing to stop in API-only mode
        true
    }

    fn get_queue(&self) -> Vec<Track> {
        debug!("LibrespotController: get_queue called - returning empty vector");
        Vec::new()
    }

    fn supports_api_events(&self) -> bool {
        self.enable_api_updates
    }
    
    fn process_api_event(&self, event_data: &serde_json::Value) -> bool {
        log::info!("[DEBUG] Librespot process_api_event called with: {}", event_data);
        debug!("Processing API event for Librespot player: {}", event_data);
        
        // Check if API updates are enabled
        if !self.enable_api_updates {
            log::info!("[DEBUG] Librespot API updates disabled, ignoring event");
            return false;
        }
        
        // Check if this is a Librespot-specific event format (with "event" field)
        if event_data.get("event").is_some() {
            // This is the legacy Librespot format - process it directly
            let json_str = event_data.to_string();
            if let Some((song, player_state, capabilities, stream_details)) = 
                super::event_common::LibrespotEventProcessor::parse_event_json(&json_str) {
                log::info!("[DEBUG] Librespot parsed legacy event: state={:?}, song={:?}", player_state.state, song.title);
                self.update_from_event(song, player_state, capabilities, stream_details);
                return true;
            }
        } else {
            // Try to convert from generic format to Librespot format
            if let Some(librespot_event) = self.convert_generic_to_librespot_event(event_data) {
                log::info!("[DEBUG] Librespot converted generic event to: {}", librespot_event);
                let json_str = librespot_event.to_string();
                if let Some((song, player_state, capabilities, stream_details)) = 
                    super::event_common::LibrespotEventProcessor::parse_event_json(&json_str) {
                    log::info!("[DEBUG] Librespot parsed converted event: state={:?}, song={:?}", player_state.state, song.title);
                    self.update_from_event(song, player_state, capabilities, stream_details);
                    return true;
                }
            }
        }
        log::warn!("[DEBUG] Librespot process_api_event: event not processed");
        false
    }

    fn receive_update(&self, update: PlayerUpdate) -> bool {
        // Check if API updates are enabled
        if !self.enable_api_updates {
            log::info!("[DEBUG] Librespot API updates disabled, ignoring update");
            return false;
        }
        
        // Convert PlayerUpdate to serde_json::Value and forward to process_api_event
        match serde_json::to_value(&update) {
            Ok(json_val) => self.process_api_event(&json_val),
            Err(e) => {
                log::warn!("[DEBUG] Librespot receive_update: failed to convert PlayerUpdate to JSON: {}", e);
                false
            }
        }
    }
}

impl LibrespotPlayerController {
    /// Kill the librespot process
    fn kill_process(&self) -> bool {
        info!("Attempting to kill Librespot process: {}", self.process_name);
        
        // Use system kill command
        #[cfg(unix)]
        {
            use std::process::Command;
            
            // Try to kill the process using pkill
            match Command::new("pkill")
                .arg("-f")
                .arg(&self.process_name)
                .status() {
                    Ok(status) => {
                        if status.success() {
                            info!("Successfully killed Librespot process");
                            return true;
                        } else {
                            warn!("Failed to kill Librespot process, exit code: {:?}", status.code());
                            return false;
                        }
                    },
                    Err(e) => {
                        error!("Failed to execute kill command: {}", e);
                        return false;
                    }
                }
        }
        
        #[cfg(not(unix))]
        {
            warn!("System process kill not implemented for this platform");
            return false;
        }
    }
}
