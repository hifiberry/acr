use crate::players::player_controller::{BasePlayerController, PlayerController};
use crate::data::{PlayerCapability, PlayerCapabilitySet, Song, LoopMode, PlaybackState, PlayerCommand, PlayerState, Track};
use crate::data::stream_details::StreamDetails;
use crate::helpers::mpris::{
    retrieve_mpris_metadata, extract_song_from_mpris_metadata, create_connection, 
    create_player_proxy, get_dbus_property, get_string_property, get_bool_property,
    get_i64_property, send_player_method, send_player_method_with_args, 
    set_player_property, bool_to_dbus_variant, BusType
};
use std::sync::{Arc, RwLock, atomic::{AtomicBool, Ordering}};
use std::thread;
use std::time::{Duration, Instant};
use log::{debug, info, warn, error};
use std::any::Any;
use dbus::blocking::Connection;

/// MPRIS player controller implementation
/// This controller interfaces with MPRIS-compatible media players via D-Bus
pub struct MprisPlayerController {
    /// Base controller
    base: BasePlayerController,
    
    /// MPRIS bus name to connect to
    bus_name: String,
    
    /// Bus type (session or system)
    bus_type: BusType,
    
    /// Current song information
    current_song: Arc<RwLock<Option<Song>>>,

    /// Current player state
    current_state: Arc<RwLock<PlayerState>>,
    
    /// Current stream details
    stream_details: Arc<RwLock<Option<StreamDetails>>>,
    
    /// Polling interval in seconds
    poll_interval: Duration,
    
    /// Flag to control the polling thread
    should_poll: Arc<AtomicBool>,
    
    /// Handle to the polling thread
    poll_thread_handle: Arc<RwLock<Option<thread::JoinHandle<()>>>>,
}

// Manually implement Clone for MprisPlayerController
impl Clone for MprisPlayerController {
    fn clone(&self) -> Self {
        MprisPlayerController {
            // Share the BasePlayerController instance to maintain listener registrations
            base: self.base.clone(),
            bus_name: self.bus_name.clone(),
            bus_type: self.bus_type.clone(),
            current_song: Arc::clone(&self.current_song),
            current_state: Arc::clone(&self.current_state),
            stream_details: Arc::clone(&self.stream_details),
            poll_interval: self.poll_interval,
            should_poll: Arc::clone(&self.should_poll),
            poll_thread_handle: Arc::new(RwLock::new(None)), // New instance gets new thread handle
        }
    }
}

impl MprisPlayerController {
    /// Create a new MPRIS player controller for the specified bus name
    pub fn new(bus_name: &str) -> Self {
        Self::new_with_poll_interval(bus_name, Duration::from_secs_f64(1.0))
    }
    
    /// Create a new MPRIS player controller with configurable polling interval
    pub fn new_with_poll_interval(bus_name: &str, poll_interval: Duration) -> Self {
        debug!("Creating new MprisPlayerController for bus: {} with poll interval: {:?}", bus_name, poll_interval);
        
        // Create a base controller with player name and ID derived from bus name
        let player_name = Self::extract_player_name(bus_name);
        let base = BasePlayerController::with_player_info(&player_name, bus_name);
        
        // Determine bus type - default to session, but check if it exists on system bus
        let bus_type = Self::determine_bus_type(bus_name);
        
        let controller = Self {
            base,
            bus_name: bus_name.to_string(),
            bus_type,
            current_song: Arc::new(RwLock::new(None)),
            current_state: Arc::new(RwLock::new(PlayerState::new())),
            stream_details: Arc::new(RwLock::new(None)),
            poll_interval,
            should_poll: Arc::new(AtomicBool::new(false)),
            poll_thread_handle: Arc::new(RwLock::new(None)),
        };
        
        // Set capabilities based on what MPRIS typically supports
        controller.set_default_capabilities();
        
        controller
    }
    
    /// Determine which bus type the player is on
    fn determine_bus_type(bus_name: &str) -> BusType {
        // Try session bus first
        if let Ok(conn) = create_connection(BusType::Session) {
            if crate::helpers::mpris::player_exists(&conn, bus_name) {
                debug!("Found MPRIS player {} on session bus", bus_name);
                return BusType::Session;
            }
        }
        
        // Try system bus
        if let Ok(conn) = create_connection(BusType::System) {
            if crate::helpers::mpris::player_exists(&conn, bus_name) {
                debug!("Found MPRIS player {} on system bus", bus_name);
                return BusType::System;
            }
        }
        
        // Default to session bus if we can't determine
        debug!("Could not determine bus type for {}, defaulting to session bus", bus_name);
        BusType::Session
    }
    
    /// Extract a friendly player name from the bus name
    fn extract_player_name(bus_name: &str) -> String {
        // Extract the last part of the bus name as the player name
        // e.g., "org.mpris.MediaPlayer2.vlc" -> "vlc"
        if let Some(last_part) = bus_name.split('.').last() {
            last_part.to_string()
        } else {
            bus_name.to_string()
        }
    }
    
    /// Set the default capabilities for MPRIS players
    fn set_default_capabilities(&self) {
        debug!("Setting default MprisPlayerController capabilities");
        
        // MPRIS players typically support most playback controls
        self.base.set_capabilities(vec![
            PlayerCapability::Play,
            PlayerCapability::Pause,
            PlayerCapability::Stop,
            PlayerCapability::Previous,
            PlayerCapability::Next,
            PlayerCapability::Seek,
            PlayerCapability::Position,
            PlayerCapability::Volume,
            PlayerCapability::Shuffle,
            PlayerCapability::Loop,
            PlayerCapability::Killable, // Can always try to kill via D-Bus
        ], false); // Don't notify on initialization
    }
    
    /// Get or create an MPRIS player connection
    fn get_mpris_connection(&self) -> Result<(Connection, dbus::blocking::Proxy<'_, &Connection>), String> {
        // Create new connection each time (no caching to avoid threading issues)
        debug!("Creating new MPRIS connection to {} on {} bus", self.bus_name, self.bus_type);
        
        let conn = create_connection(self.bus_type.clone())
            .map_err(|e| format!("Failed to create D-Bus connection: {}", e))?;
        
        // Check if player exists
        if !crate::helpers::mpris::player_exists(&conn, &self.bus_name) {
            return Err(format!("MPRIS player '{}' not found on {} bus", self.bus_name, self.bus_type));
        }
        
        let proxy = create_player_proxy(&conn, &self.bus_name);
        
        info!("Connected to MPRIS player: {} on {} bus", self.bus_name, self.bus_type);
        Ok((conn, proxy))
    }
    
    /// Update internal state from MPRIS player
    fn update_state_from_mpris(&self) {
        let Ok((_conn, proxy)) = self.get_mpris_connection() else {
            debug!("Failed to connect to MPRIS player for state update");
            return;
        };
        
        // Update playback state
        if let Some(status) = get_string_property(&proxy, "org.mpris.MediaPlayer2.Player", "PlaybackStatus") {
            let state = match status.as_str() {
                "Playing" => PlaybackState::Playing,
                "Paused" => PlaybackState::Paused,
                "Stopped" => PlaybackState::Stopped,
                _ => PlaybackState::Unknown,
            };
            
            if let Ok(mut current_state) = self.current_state.write() {
                current_state.state = state;
                
                // Update shuffle
                current_state.shuffle = get_bool_property(&proxy, "org.mpris.MediaPlayer2.Player", "Shuffle")
                    .unwrap_or(false);
                
                // Update loop mode
                if let Some(loop_status) = get_string_property(&proxy, "org.mpris.MediaPlayer2.Player", "LoopStatus") {
                    current_state.loop_mode = match loop_status.as_str() {
                        "None" => LoopMode::None,
                        "Track" => LoopMode::Track,
                        "Playlist" => LoopMode::Playlist,
                        _ => LoopMode::None,
                    };
                }
                
                // Update position (convert from microseconds to seconds)
                if let Some(position_us) = get_i64_property(&proxy, "org.mpris.MediaPlayer2.Player", "Position") {
                    current_state.position = Some(position_us as f64 / 1_000_000.0);
                }
            }
        }
        
        // Update song metadata using helper functions
        if let Some(metadata_variant) = retrieve_mpris_metadata(&proxy) {
            let song = extract_song_from_mpris_metadata(&metadata_variant);
            if let Ok(mut current_song) = self.current_song.write() {
                *current_song = song;
            }
        }
        
        // Mark player as alive
        self.base.alive();
    }
    
    /// Start the polling thread
    fn start_polling(&self) {
        if self.should_poll.load(Ordering::Relaxed) {
            debug!("Polling already started for MPRIS player {}", self.bus_name);
            return;
        }
        
        info!("Starting polling thread for MPRIS player {} with interval {:?}", self.bus_name, self.poll_interval);
        self.should_poll.store(true, Ordering::Relaxed);
        
        let bus_name = self.bus_name.clone();
        let bus_type = self.bus_type.clone();
        let poll_interval = self.poll_interval;
        let should_poll = Arc::clone(&self.should_poll);
        let current_song = Arc::clone(&self.current_song);
        let current_state = Arc::clone(&self.current_state);
        let base = self.base.clone();
        
        let handle = thread::spawn(move || {
            debug!("MPRIS polling thread started for {}", bus_name);
            let mut last_update = Instant::now();
            
            while should_poll.load(Ordering::Relaxed) {
                let now = Instant::now();
                if now.duration_since(last_update) >= poll_interval {
                    // Update state
                    if let Ok(conn) = create_connection(bus_type.clone()) {
                        if crate::helpers::mpris::player_exists(&conn, &bus_name) {
                            let proxy = create_player_proxy(&conn, &bus_name);
                            
                            // Update playback state
                            if let Some(status) = get_string_property(&proxy, "org.mpris.MediaPlayer2.Player", "PlaybackStatus") {
                                let state = match status.as_str() {
                                    "Playing" => PlaybackState::Playing,
                                    "Paused" => PlaybackState::Paused,
                                    "Stopped" => PlaybackState::Stopped,
                                    _ => PlaybackState::Unknown,
                                };
                                
                                if let Ok(mut current_state) = current_state.write() {
                                    current_state.state = state;
                                    
                                    // Update shuffle
                                    current_state.shuffle = get_bool_property(&proxy, "org.mpris.MediaPlayer2.Player", "Shuffle")
                                        .unwrap_or(false);
                                    
                                    // Update loop mode
                                    if let Some(loop_status) = get_string_property(&proxy, "org.mpris.MediaPlayer2.Player", "LoopStatus") {
                                        current_state.loop_mode = match loop_status.as_str() {
                                            "None" => LoopMode::None,
                                            "Track" => LoopMode::Track,
                                            "Playlist" => LoopMode::Playlist,
                                            _ => LoopMode::None,
                                        };
                                    }
                                    
                                    // Update position (convert from microseconds to seconds)
                                    if let Some(position_us) = get_i64_property(&proxy, "org.mpris.MediaPlayer2.Player", "Position") {
                                        current_state.position = Some(position_us as f64 / 1_000_000.0);
                                    }
                                }
                            }
                            
                            // Update song metadata
                            if let Some(metadata_variant) = retrieve_mpris_metadata(&proxy) {
                                let song = extract_song_from_mpris_metadata(&metadata_variant);
                                if let Ok(mut current_song) = current_song.write() {
                                    *current_song = song;
                                }
                            }
                            
                            // Mark player as alive
                            base.alive();
                        } else {
                            debug!("MPRIS player {} no longer exists", bus_name);
                        }
                    }
                    
                    last_update = now;
                }
                
                // Sleep for a short time to avoid busy waiting
                thread::sleep(Duration::from_millis(100));
            }
            
            debug!("MPRIS polling thread stopped for {}", bus_name);
        });
        
        if let Ok(mut thread_handle) = self.poll_thread_handle.write() {
            *thread_handle = Some(handle);
        }
    }
    
    /// Stop the polling thread
    fn stop_polling(&self) {
        if !self.should_poll.load(Ordering::Relaxed) {
            debug!("Polling already stopped for MPRIS player {}", self.bus_name);
            return;
        }
        
        info!("Stopping polling thread for MPRIS player {}", self.bus_name);
        self.should_poll.store(false, Ordering::Relaxed);
        
        if let Ok(mut thread_handle) = self.poll_thread_handle.write() {
            if let Some(handle) = thread_handle.take() {
                if let Err(e) = handle.join() {
                    warn!("Error joining polling thread for {}: {:?}", self.bus_name, e);
                }
            }
        }
    }
}

impl PlayerController for MprisPlayerController {
    fn get_capabilities(&self) -> PlayerCapabilitySet {
        self.base.get_capabilities()
    }
    
    fn get_player_name(&self) -> String {
        self.base.get_player_name()
    }
    
    fn get_player_id(&self) -> String {
        self.base.get_player_id()
    }
    
    fn has_library(&self) -> bool {
        false // MPRIS players typically don't expose library functionality
    }
    
    fn supports_api_events(&self) -> bool {
        false // MPRIS doesn't support API events
    }
    
    fn get_last_seen(&self) -> Option<std::time::SystemTime> {
        self.base.get_last_seen()
    }
    
    fn receive_update(&self, _update: crate::data::PlayerUpdate) -> bool {
        false // MPRIS doesn't support receiving updates
    }
    
    fn get_metadata(&self) -> Option<std::collections::HashMap<String, serde_json::Value>> {
        // MPRIS doesn't provide generic metadata access, return None
        None
    }
    
    fn get_playback_state(&self) -> PlaybackState {
        self.update_state_from_mpris();
        if let Ok(state) = self.current_state.read() {
            state.state
        } else {
            PlaybackState::Unknown
        }
    }
    
    fn get_song(&self) -> Option<Song> {
        self.update_state_from_mpris();
        if let Ok(song) = self.current_song.read() {
            song.clone()
        } else {
            None
        }
    }
    
    fn get_queue(&self) -> Vec<Track> {
        // MPRIS doesn't typically expose queue information
        Vec::new()
    }
    
    fn get_shuffle(&self) -> bool {
        self.update_state_from_mpris();
        if let Ok(state) = self.current_state.read() {
            state.shuffle
        } else {
            false
        }
    }
    
    fn get_loop_mode(&self) -> LoopMode {
        self.update_state_from_mpris();
        if let Ok(state) = self.current_state.read() {
            state.loop_mode
        } else {
            LoopMode::None
        }
    }
    
    fn get_position(&self) -> Option<f64> {
        if let Ok((_conn, proxy)) = self.get_mpris_connection() {
            if let Some(position_us) = get_i64_property(&proxy, "org.mpris.MediaPlayer2.Player", "Position") {
                return Some(position_us as f64 / 1_000_000.0);
            }
        }
        None
    }
    
    fn send_command(&self, command: PlayerCommand) -> bool {
        info!("Sending command to MPRIS player: {}", command);
        
        let (_conn, proxy) = match self.get_mpris_connection() {
            Ok(conn_proxy) => conn_proxy,
            Err(e) => {
                error!("Failed to get MPRIS player connection: {}", e);
                return false;
            }
        };
        
        let result = match command {
            PlayerCommand::Play => send_player_method(&proxy, "Play"),
            PlayerCommand::Pause => send_player_method(&proxy, "Pause"),
            PlayerCommand::PlayPause => send_player_method(&proxy, "PlayPause"),
            PlayerCommand::Stop => send_player_method(&proxy, "Stop"),
            PlayerCommand::Next => send_player_method(&proxy, "Next"),
            PlayerCommand::Previous => send_player_method(&proxy, "Previous"),
            PlayerCommand::Seek(offset) => {
                // MPRIS seek expects microseconds as i64
                let microseconds = (offset * 1_000_000.0) as i64;
                send_player_method_with_args(&proxy, "Seek", (microseconds,))
            },
            PlayerCommand::SetRandom(enabled) => {
                set_player_property(&proxy, "Shuffle", bool_to_dbus_variant(enabled).0)
            },
            PlayerCommand::SetLoopMode(mode) => {
                let loop_status_str = match mode {
                    LoopMode::None => "None",
                    LoopMode::Track => "Track", 
                    LoopMode::Playlist => "Playlist",
                };
                set_player_property(&proxy, "LoopStatus", loop_status_str)
            },
            PlayerCommand::Kill => {
                // For MPRIS, we can't really "kill" the player, but we can try to quit
                warn!("Kill command not supported for MPRIS players, ignoring");
                return false;
            }
            _ => {
                warn!("Command not supported by MPRIS: {}", command);
                return false;
            }
        };
        
        match result {
            Ok(()) => {
                info!("Successfully sent command {} to MPRIS player", command);
                // Trigger an immediate state update
                self.update_state_from_mpris();
                true
            }
            Err(e) => {
                error!("Failed to send command {} to MPRIS player: {}", command, e);
                false
            }
        }
    }
    
    fn as_any(&self) -> &dyn Any {
        self
    }
    
    fn start(&self) -> bool {
        info!("Starting MPRIS player controller for {}", self.bus_name);
        
        // Test connection
        match self.get_mpris_connection() {
            Ok(_) => {
                info!("Successfully connected to MPRIS player: {}", self.bus_name);
                self.base.alive();
                
                // Start polling thread
                self.start_polling();
                
                true
            }
            Err(e) => {
                error!("Failed to connect to MPRIS player {}: {}", self.bus_name, e);
                false
            }
        }
    }
    
    fn stop(&self) -> bool {
        info!("Stopping MPRIS player controller for {}", self.bus_name);
        
        // Stop polling thread
        self.stop_polling();
        
        true
    }
}
