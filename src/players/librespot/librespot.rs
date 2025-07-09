use crate::players::player_controller::{BasePlayerController, PlayerController};
use crate::data::{PlayerCapability, PlayerCapabilitySet, Song, LoopMode, PlaybackState, PlayerCommand, PlayerState, Track};
use crate::data::stream_details::StreamDetails;
use crate::helpers::PlayerProgress;
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
    
    /// Playback progress tracker
    progress: PlayerProgress,
    
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
            progress: self.progress.clone(),
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
            progress: PlayerProgress::new(),
            on_pause_event: None,
        };
        
        // Set default capabilities - only Killable is available
        player.set_default_capabilities();
        
        player
    }
    


    /// Create a new Librespot player controller with fully custom settings and systemd unit check
    pub fn with_config_and_systemd(process_name: &str, systemd_unit: Option<&str>) -> Self {
        Self::with_full_config(process_name, systemd_unit)
    }
    
    /// Create a new Librespot player controller with full configuration options
    pub fn with_full_config(
        process_name: &str,
        systemd_unit: Option<&str>
    ) -> Self {
        debug!("Creating new LibrespotPlayerController with process_name: {}, systemd_unit: {:?}", 
               process_name, systemd_unit);
        
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
            progress: PlayerProgress::new(),
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
        log::debug!("update_from_event called: state={:?}, song={:?}, duration={:?}", 
                  player_state.state, song.title, song.duration);
        
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
                    log::debug!("Song changed: title={:?}, artist={:?}, album={:?}, duration={:?}", 
                              song.title, song.artist, song.album, song.duration);
                    
                    // Reset position progress for new song
                    self.progress.reset();
                    
                    if let Some(ref metadata) = song.metadata.get("DURATION_MS") {
                        log::debug!("Song has DURATION_MS in metadata: {:?}", metadata);
                    }
                    
                    // Ensure we have a properly populated song object
                    let mut enhanced_song = song.clone();
                    
                    // Make sure duration is set
                    if enhanced_song.duration.is_none() {
                        log::warn!("Song duration is missing in update_from_event!");
                        
                        // Check metadata for duration
                        if let Some(duration_ms) = enhanced_song.metadata.get("DURATION_MS").and_then(|v| v.as_str()).and_then(|s| s.parse::<u64>().ok()) {
                            let duration_seconds = duration_ms as f64 / 1000.0;
                            log::debug!("Retrieved duration from metadata: {} ms -> {} seconds", duration_ms, duration_seconds);
                            enhanced_song.duration = Some(duration_seconds);
                        } else if let Some(duration_ms) = enhanced_song.metadata.get("duration_ms").and_then(|v| v.as_u64()) {
                            let duration_seconds = duration_ms as f64 / 1000.0;
                            log::debug!("Retrieved duration from metadata duration_ms: {} ms -> {} seconds", duration_ms, duration_seconds);
                            enhanced_song.duration = Some(duration_seconds);
                        }
                    } else {
                        // If duration is already set, make sure it's also in the metadata
                        if let Some(duration) = enhanced_song.duration {
                            if !enhanced_song.metadata.contains_key("DURATION_MS") {
                                let duration_ms = (duration * 1000.0) as u64;
                                log::debug!("Adding duration to metadata: {} seconds -> {} ms", duration, duration_ms);
                                enhanced_song.metadata.insert("DURATION_MS".to_string(), json!(duration_ms.to_string()));
                            }
                        }
                    }
                    
                    // Store the updated song
                    *current_song = Some(enhanced_song.clone());
                    song_to_notify = Some(enhanced_song);
                }
            } else {
                debug!("Ignoring song update with no meaningful data");
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
                
                // Update progress playing state
                self.progress.set_playing(new_state == PlaybackState::Playing);
            }
            if new_state == PlaybackState::Playing || state_changed {
                log::info!("[API DEBUG] Notifying state changed: {:?}", new_state);
                self.base.notify_state_changed(new_state);
            }
            
            // Notify about position changes
            if position_changed {
                if let Some(position) = player_state.position {
                    log::info!("[API DEBUG] Notifying position changed: {}", position);
                    // Update progress position
                    self.progress.set_position(position);
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
        // Return a clone of the stored song with enhanced metadata if needed
        if let Ok(song_lock) = self.current_song.read() {
            // Clone the song if it exists
            if let Some(ref song) = *song_lock {
                log::debug!("Original song data: title={:?}, artist={:?}, album={:?}, duration={:?}, cover={:?}", 
                    song.title, song.artist, song.album, song.duration, song.cover_art_url);
                
                // Log the full metadata for debugging
                log::debug!("Original song metadata: {:?}", song.metadata);
                
                // Create a new song object with the same fields
                let mut enhanced_song = song.clone();
                
                // Make sure essential fields are populated, even if stored as metadata
                if song.duration.is_none() {
                    log::warn!("Song duration is missing, attempting to retrieve from metadata");
                    
                    // Try different metadata keys for duration
                    if let Some(duration) = song.metadata.get("duration")
                        .and_then(|v| v.as_f64()) {
                        log::debug!("Found duration in metadata 'duration' field: {} seconds", duration);
                        enhanced_song.duration = Some(duration);
                    } else if let Some(duration_ms) = song.metadata.get("DURATION_MS")
                        .and_then(|v| v.as_str())
                        .and_then(|s| s.parse::<u64>().ok()) {
                        let duration_sec = duration_ms as f64 / 1000.0;
                        log::debug!("Found DURATION_MS in metadata: {} ms -> {} seconds", duration_ms, duration_sec);
                        enhanced_song.duration = Some(duration_sec);
                    } else if let Some(duration_ms) = song.metadata.get("duration_ms")
                        .and_then(|v| v.as_u64()) {
                        let duration_sec = duration_ms as f64 / 1000.0;
                        log::debug!("Found duration_ms in metadata: {} ms -> {} seconds", duration_ms, duration_sec);
                        enhanced_song.duration = Some(duration_sec);
                    } else {
                        log::warn!("No duration found in any metadata field");
                    }
                }

                // If we don't have a source URI set but it's in the metadata, add it
                if enhanced_song.stream_url.is_none() || enhanced_song.stream_url.as_ref().map_or(true, |url| url.trim().is_empty()) {
                    if let Some(uri) = song.metadata.get("uri").and_then(|v| v.as_str()) {
                        enhanced_song.stream_url = Some(uri.to_string());
                        log::debug!("Found URI in metadata: {}", uri);
                    }
                }
                
                // Log the song details for debugging
                log::debug!("Returning song: title={:?}, artist={:?}, album={:?}, duration={:?}, cover={:?}, uri={:?}", 
                    enhanced_song.title, enhanced_song.artist, enhanced_song.album, 
                    enhanced_song.duration, enhanced_song.cover_art_url, enhanced_song.stream_url);
                
                return Some(enhanced_song);
            } else {
                return None;
            }
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
        // Get position from the progress tracker which handles automatic incrementing
        Some(self.progress.get_position())
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
        true // API events are always enabled
    }
    
    fn process_api_event(&self, event_data: &serde_json::Value) -> bool {
        log::info!("[DEBUG] Librespot process_api_event called with: {}", event_data);
        debug!("Processing API event for Librespot player: {}", event_data);
        
        // Check if this is a generic API event format (with "type" field)
        if let Some(event_type) = event_data.get("type").and_then(|t| t.as_str()) {
            return self.process_generic_api_event(event_type, event_data);
        }
        
        log::warn!("[DEBUG] Librespot process_api_event: unknown event format - only 'type' field events are supported");
        false
    }

    fn receive_update(&self, update: PlayerUpdate) -> bool {
        
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
    /// Process generic API events directly without conversion
    fn process_generic_api_event(&self, event_type: &str, event_data: &serde_json::Value) -> bool {
        log::info!("[DEBUG] Processing generic API event: type={}", event_type);
        
        match event_type {
            "ping" => {
                // Mark player as alive
                self.base.alive();
                true
            },
            "state_changed" => {
                if let Some(state_str) = event_data.get("state").and_then(|s| s.as_str()) {
                    let state = match state_str {
                        "playing" => PlaybackState::Playing,
                        "paused" => PlaybackState::Paused,
                        "stopped" => PlaybackState::Stopped,
                        "killed" => PlaybackState::Killed,
                        "disconnected" => PlaybackState::Disconnected,
                        _ => PlaybackState::Unknown,
                    };
                    
                    // Update internal state
                    if let Ok(mut current_state) = self.current_state.write() {
                        let state_changed = current_state.state != state;
                        current_state.state = state;
                        
                        if state_changed {
                            log::info!("[API DEBUG] State changed to: {:?}", state);
                            // Update progress playing state
                            self.progress.set_playing(state == PlaybackState::Playing);
                            self.base.notify_state_changed(state);
                        }
                    }
                    
                    // Update position if provided
                    if let Some(position) = event_data.get("position").and_then(|p| p.as_f64()) {
                        if let Ok(mut current_state) = self.current_state.write() {
                            current_state.position = Some(position);
                            log::info!("[API DEBUG] Position updated to: {}", position);
                            // Update progress position
                            self.progress.set_position(position);
                            self.base.notify_position_changed(position);
                        }
                    }
                    
                    self.base.alive();
                    true
                } else {
                    false
                }
            },
            "song_changed" => {
                if let Some(song_data) = event_data.get("song") {
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
                    if let Some(duration) = song_data.get("duration").and_then(|d| d.as_f64()) {
                        song.duration = Some(duration);
                    }
                    if let Some(uri) = song_data.get("uri").and_then(|u| u.as_str()) {
                        song.stream_url = Some(uri.to_string());
                    }
                    if let Some(cover) = song_data.get("cover_art_url").and_then(|c| c.as_str()) {
                        song.cover_art_url = Some(cover.to_string());
                    }
                    
                    // Store metadata if present
                    if let Some(metadata) = song_data.get("metadata").and_then(|m| m.as_object()) {
                        for (key, value) in metadata {
                            song.metadata.insert(key.clone(), value.clone());
                        }
                    }
                    
                    // Update internal song
                    if let Ok(mut current_song) = self.current_song.write() {
                        let song_changed = match (&*current_song, &song) {
                            (Some(old), new) => old.title != new.title || old.artist != new.artist || old.album != new.album,
                            (None, _) => true,
                        };
                        
                        if song_changed {
                            log::info!("[API DEBUG] Song changed: {:?} - {:?}", song.artist, song.title);
                            // Reset position progress for new song
                            self.progress.reset();
                            *current_song = Some(song.clone());
                            self.base.notify_song_changed(Some(&song));
                        }
                    }
                    
                    self.base.alive();
                    true
                } else {
                    false
                }
            },
            "position_changed" => {
                if let Some(position) = event_data.get("position").and_then(|p| p.as_f64()) {
                    if let Ok(mut current_state) = self.current_state.write() {
                        current_state.position = Some(position);
                        log::info!("[API DEBUG] Position changed to: {}", position);
                        // Update progress position
                        self.progress.set_position(position);
                        self.base.notify_position_changed(position);
                        self.base.alive();
                        true
                    } else {
                        false
                    }
                } else {
                    false
                }
            },
            "loop_mode_changed" => {
                if let Some(mode_str) = event_data.get("mode").and_then(|m| m.as_str()) {
                    let loop_mode = match mode_str {
                        "song" | "track" => LoopMode::Track,
                        "playlist" | "all" => LoopMode::Playlist,
                        "none" => LoopMode::None,
                        _ => return false,
                    };
                    
                    if let Ok(mut current_state) = self.current_state.write() {
                        let mode_changed = current_state.loop_mode != loop_mode;
                        current_state.loop_mode = loop_mode;
                        
                        if mode_changed {
                            log::info!("[API DEBUG] Loop mode changed to: {:?}", loop_mode);
                            self.base.notify_loop_mode_changed(loop_mode);
                        }
                    }
                    
                    self.base.alive();
                    true
                } else {
                    false
                }
            },
            "shuffle_changed" => {
                let shuffle = event_data.get("enabled").and_then(|e| e.as_bool()).unwrap_or(false);
                
                if let Ok(mut current_state) = self.current_state.write() {
                    let shuffle_changed = current_state.shuffle != shuffle;
                    current_state.shuffle = shuffle;
                    
                    if shuffle_changed {
                        log::info!("[API DEBUG] Shuffle changed to: {}", shuffle);
                        self.base.notify_random_changed(shuffle);
                    }
                }
                
                self.base.alive();
                true
            },
            _ => {
                debug!("Unknown generic event type for Librespot: {}", event_type);
                false
            }
        }
    }

    fn receive_update(&self, update: PlayerUpdate) -> bool {
        
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

impl LibrespotPlayerController {
    /// Get the current playback progress
    pub fn get_progress(&self) -> &PlayerProgress {
        &self.progress
    }
    
    /// Get the current playback position from the progress tracker
    pub fn get_tracked_position(&self) -> f64 {
        self.progress.get_position()
    }
    
    /// Check if the player is currently playing according to the progress tracker
    pub fn is_playing(&self) -> bool {
        self.progress.is_playing()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn test_librespot_progress_integration() {
        let player = LibrespotPlayerController::new();
        
        // Initially position should be 0 and not playing
        assert_eq!(player.get_tracked_position(), 0.0);
        assert!(!player.is_playing());
        
        // Simulate a state change to playing
        let event = json!({
            "type": "state_changed",
            "state": "playing",
            "position": 10.0
        });
        
        let result = player.process_api_event(&event);
        assert!(result);
        
        // Position should be set to 10.0 and player should be playing
        let current_position = player.get_tracked_position();
        assert!(current_position >= 10.0); // Position might have incremented slightly
        assert!(current_position < 10.1); // But not too much
        assert!(player.is_playing());
        
        // Wait a bit and position should have incremented
        thread::sleep(Duration::from_millis(100));
        let position_after_wait = player.get_tracked_position();
        assert!(position_after_wait > current_position);
        assert!(position_after_wait < current_position + 1.0); // Should be less than 1 second later
        
        // Simulate a pause
        let pause_event = json!({
            "type": "state_changed",
            "state": "paused"
        });
        
        let result = player.process_api_event(&pause_event);
        assert!(result);
        
        // Player should not be playing anymore
        assert!(!player.is_playing());
        
        // Position should not increment while paused
        let position_at_pause = player.get_tracked_position();
        thread::sleep(Duration::from_millis(100));
        let position_after_pause = player.get_tracked_position();
        assert!((position_after_pause - position_at_pause).abs() < 0.01); // Should be approximately the same
    }
    
    #[test]
    fn test_librespot_song_change_resets_position() {
        let player = LibrespotPlayerController::new();
        
        // Set some initial position and playing state
        let event = json!({
            "type": "state_changed",
            "state": "playing",
            "position": 30.0
        });
        player.process_api_event(&event);
        
        // Position should be 30.0 and player should be playing
        let current_position = player.get_tracked_position();
        assert!(current_position >= 30.0); // Position might have incremented slightly
        assert!(current_position < 30.1); // But not too much
        assert!(player.is_playing());
        
        // Simulate a song change
        let song_event = json!({
            "type": "song_changed",
            "song": {
                "title": "New Song",
                "artist": "New Artist",
                "album": "New Album",
                "duration": 180.0
            }
        });
        
        let result = player.process_api_event(&song_event);
        assert!(result);
        
        // Position should be reset to 0, but player should still be playing
        assert_eq!(player.get_tracked_position(), 0.0);
        assert!(!player.is_playing()); // Reset also sets playing to false
    }
    
    #[test]
    fn test_librespot_position_updates() {
        let player = LibrespotPlayerController::new();
        
        // Set playing state first
        let state_event = json!({
            "type": "state_changed",
            "state": "playing"
        });
        player.process_api_event(&state_event);
        
        // Send position update
        let position_event = json!({
            "type": "position_changed",
            "position": 45.5
        });
        
        let result = player.process_api_event(&position_event);
        assert!(result);
        
        // Position should be updated
        let current_position = player.get_tracked_position();
        assert!(current_position >= 45.5); // Position might have incremented slightly
        assert!(current_position < 45.6); // But not too much
        assert!(player.is_playing());
        
        // Wait a bit and position should have incremented
        thread::sleep(Duration::from_millis(100));
        let position_after_wait = player.get_tracked_position();
        assert!(position_after_wait > current_position);
        assert!(position_after_wait < current_position + 1.0); // Should be less than 1 second later
    }
}
