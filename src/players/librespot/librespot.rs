use crate::players::player_controller::{BasePlayerController, PlayerController};
use crate::data::{PlayerCapability, PlayerCapabilitySet, Song, LoopMode, PlaybackState, PlayerCommand, PlayerState, Track};
use crate::data::stream_details::StreamDetails;
use crate::helpers::playback_progress::PlayerProgress;
use delegate::delegate;
use std::sync::{Arc, RwLock};
use log::{debug, info, warn, error, trace};
use std::any::Any;

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
    
    /// Playback progress tracking
    player_progress: Arc<RwLock<PlayerProgress>>,
    
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
            player_progress: Arc::clone(&self.player_progress),
            on_pause_event: self.on_pause_event.clone(),
        }
    }
}


impl LibrespotPlayerController {
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
                match crate::helpers::process_helper::is_systemd_unit_active(unit_name) {
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
            player_progress: Arc::new(RwLock::new(PlayerProgress::new())),
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
    pub fn set_process_name(&mut self, process_name: &str) {
        debug!("Setting Librespot process name to: {}", process_name);
        self.process_name = process_name.to_string();
    }
    
    /// Get the path to the librespot executable
    pub fn get_process_name(&self) -> &str {
        &self.process_name
    }
    
    /// Set the on_pause_event action
    pub fn set_on_pause_event(&mut self, on_pause_event: Option<String>) {
        debug!("Setting Librespot on_pause_event to: {:?}", on_pause_event);
        self.on_pause_event = on_pause_event;
    }
    
    /// Get the on_pause_event action
    pub fn get_on_pause_event(&self) -> &Option<String> {
        &self.on_pause_event
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
        // Return the actual loop mode from the stored state
        match self.current_state.try_read() {
            Ok(state) => {
                debug!("Got current loop mode: {:?}", state.loop_mode);
                state.loop_mode
            },
            Err(_) => {
                warn!("Could not acquire immediate read lock for loop mode, returning None");
                LoopMode::None
            }
        }
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
        // Get position from PlayerProgress for accurate tracking
        match self.player_progress.try_read() {
            Ok(progress) => {
                let position = progress.get_position();
                trace!("Got current position from PlayerProgress: {:?}", position);
                Some(position)
            },
            Err(_) => {
                warn!("Could not acquire immediate read lock for PlayerProgress, falling back to stored position");
                // Fall back to stored position if PlayerProgress is not available
                match self.current_state.try_read() {
                    Ok(state) => {
                        trace!("Got current position from state: {:?}", state.position);
                        state.position
                    },
                    Err(_) => {
                        warn!("Could not acquire immediate read lock for position, returning None");
                        None
                    }
                }
            }
        }
    }
    
    fn get_shuffle(&self) -> bool {
        debug!("Getting current shuffle state");
        // Return the actual shuffle state from the stored state
        match self.current_state.try_read() {
            Ok(state) => {
                debug!("Got current shuffle state: {}", state.shuffle);
                state.shuffle
            },
            Err(_) => {
                warn!("Could not acquire immediate read lock for shuffle state, returning false");
                false
            }
        }
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
                            match crate::helpers::process_helper::systemd("librespot", crate::helpers::process_helper::SystemdAction::Restart) {
                                Ok(true) => {
                                    info!("Successfully restarted librespot systemd unit");
                                    return true;
                                }
                                Ok(false) => {
                                    error!("Failed to restart librespot systemd unit");
                                    return false;
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
                    
                    // Update PlayerProgress playing state
                    if let Ok(progress) = self.player_progress.write() {
                        let is_playing = state == PlaybackState::Playing;
                        progress.set_playing(is_playing);
                        log::info!("[API DEBUG] PlayerProgress playing state updated to: {}", is_playing);
                    }
                    
                    // Update internal state
                    if let Ok(mut current_state) = self.current_state.write() {
                        let state_changed = current_state.state != state;
                        current_state.state = state;
                        
                        if state_changed {
                            log::info!("[API DEBUG] State changed to: {:?}", state);
                            self.base.notify_state_changed(state);
                        }
                    }
                    
                    // Update position if provided
                    if let Some(position) = event_data.get("position").and_then(|p| p.as_f64()) {
                        if let Ok(mut current_state) = self.current_state.write() {
                            current_state.position = Some(position);
                            log::info!("[API DEBUG] Position updated to: {}", position);
                            self.base.notify_position_changed(position);
                        }
                        // Also update PlayerProgress position
                        if let Ok(progress) = self.player_progress.write() {
                            progress.set_position(position);
                            log::info!("[API DEBUG] PlayerProgress position updated to: {}", position);
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
                            *current_song = Some(song.clone());
                            
                            // Reset PlayerProgress position for new song
                            if let Ok(progress) = self.player_progress.write() {
                                progress.set_position(0.0);
                                log::info!("[API DEBUG] PlayerProgress position reset to 0.0 for new song");
                            }
                            
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
                        self.base.notify_position_changed(position);
                        
                        // Also update PlayerProgress position
                        if let Ok(progress) = self.player_progress.write() {
                            progress.set_position(position);
                            log::info!("[API DEBUG] PlayerProgress position updated to: {}", position);
                        }
                        
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
                // Handle both "mode" and "loop_mode" field names for compatibility
                let mode_str = event_data.get("mode")
                    .or_else(|| event_data.get("loop_mode"))
                    .and_then(|m| m.as_str());
                    
                if let Some(mode_str) = mode_str {
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
}

impl LibrespotPlayerController {
    /// Kill the librespot process
    fn kill_process(&self) -> bool {
        info!("Attempting to kill Librespot process: {}", self.process_name);
        
        // Use the process helper functions
        match crate::helpers::process_helper::pkill(&self.process_name, false) {
            Ok(true) => {
                info!("Successfully killed Librespot process");
                true
            }
            Ok(false) => {
                warn!("No Librespot process found to kill");
                false
            }
            Err(e) => {
                error!("Failed to kill Librespot process: {}", e);
                false
            }
        }
    }
}
