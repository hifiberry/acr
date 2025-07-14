use crate::players::player_controller::{BasePlayerController, PlayerController};
use crate::data::{PlayerCapabilitySet, PlayerCapability, Song, LoopMode, PlaybackState, PlayerCommand, PlayerState, Track};
use crate::helpers::shairportsync_messages::{
    ShairportMessage, parse_shairport_message, 
    update_song_from_message, song_has_significant_metadata
};
use crate::helpers::process_helper::{systemd, SystemdAction};
use std::sync::{Arc, Mutex};
use log::{debug, info, warn, error, trace};
use std::net::UdpSocket;
use std::thread;
use std::time::Duration;
use std::sync::atomic::{AtomicBool, Ordering};
use std::any::Any;

/// ShairportSync player controller implementation
/// 
/// This controller listens to ShairportSync UDP metadata messages to track playback state
/// and current song information from AirPlay streams.
pub struct ShairportController {
    /// Base controller for managing state listeners
    base: BasePlayerController,
    
    /// UDP port to listen on for ShairportSync messages
    port: u16,
    
    /// Optional systemd unit name for controlling the ShairportSync service
    systemd_unit: Option<String>,
    
    /// Current song information (temporary storage until METADATA_END)
    current_song: Arc<Mutex<Option<Song>>>,
    
    /// Temporary song being built from metadata
    pending_song: Arc<Mutex<Option<Song>>>,
    
    /// Current player state
    current_state: Arc<Mutex<PlayerState>>,
    
    /// Flag to stop the UDP listener thread
    stop_listener: Arc<AtomicBool>,
    
    /// Thread handle for the UDP listener
    listener_thread: Arc<Mutex<Option<thread::JoinHandle<()>>>>,
}

impl Clone for ShairportController {
    fn clone(&self) -> Self {
        ShairportController {
            base: self.base.clone(),
            port: self.port,
            systemd_unit: self.systemd_unit.clone(),
            current_song: Arc::clone(&self.current_song),
            pending_song: Arc::clone(&self.pending_song),
            current_state: Arc::clone(&self.current_state),
            stop_listener: Arc::clone(&self.stop_listener),
            listener_thread: Arc::clone(&self.listener_thread),
        }
    }
}

impl ShairportController {
    /// Create a new ShairportSync controller with default port (5555)
    pub fn new() -> Self {
        Self::with_port(5555)
    }
    
    /// Create a new ShairportSync controller with custom port
    pub fn with_port(port: u16) -> Self {
        Self::with_config(port, None)
    }
    
    /// Create a new ShairportSync controller with custom port and systemd unit
    pub fn with_config(port: u16, systemd_unit: Option<String>) -> Self {
        debug!("Creating new ShairportController with port {} and systemd unit {:?}", port, systemd_unit);
        
        // Create a base controller with player name and ID
        let base = BasePlayerController::with_player_info("shairport", &format!("udp:{}", port));
        
        let controller = Self {
            base,
            port,
            systemd_unit,
            current_song: Arc::new(Mutex::new(None)),
            pending_song: Arc::new(Mutex::new(None)),
            current_state: Arc::new(Mutex::new(PlayerState::new())),
            stop_listener: Arc::new(AtomicBool::new(false)),
            listener_thread: Arc::new(Mutex::new(None)),
        };
        
        // Set default capabilities
        controller.set_default_capabilities();
        
        controller
    }
    
    /// Create a new ShairportSync controller from JSON configuration
    pub fn from_config(config: &serde_json::Value) -> Self {
        let port = config.get("port")
            .and_then(|p| p.as_u64())
            .unwrap_or(5555) as u16;
        
        let systemd_unit = config.get("systemd_unit")
            .and_then(|s| s.as_str())
            .map(|s| s.to_string());
        
        debug!("Creating ShairportController from config with port {} and systemd unit {:?}", port, systemd_unit);
        Self::with_config(port, systemd_unit)
    }
    
    /// Set the default capabilities for this player
    fn set_default_capabilities(&self) {
        debug!("Setting default ShairportController capabilities");
        // ShairportSync is a passive listener that can provide metadata
        let mut capabilities = vec![
            PlayerCapability::Metadata,
        ];
        
        // If systemd unit is configured, we can control playback
        if self.systemd_unit.is_some() {
            capabilities.extend_from_slice(&[
                PlayerCapability::Play,
                PlayerCapability::Pause,
                PlayerCapability::Stop,
            ]);
            debug!("Added playback control capabilities due to systemd unit configuration");
        }
        
        self.base.set_capabilities(capabilities, false); // Don't notify on initialization
    }
    
    /// Start the UDP listener thread
    fn start_listener(&self) -> bool {
        if self.listener_thread.lock().unwrap().is_some() {
            warn!("ShairportSync listener already running");
            return false;
        }
        
        let port = self.port;
        let stop_flag = Arc::clone(&self.stop_listener);
        let current_song = Arc::clone(&self.current_song);
        let pending_song = Arc::clone(&self.pending_song);
        let current_state = Arc::clone(&self.current_state);
        let base = self.base.clone();
        
        info!("Starting ShairportSync UDP listener on port {}", port);
        
        let handle = thread::spawn(move || {
            Self::listener_loop(port, stop_flag, current_song, pending_song, current_state, base);
        });
        
        *self.listener_thread.lock().unwrap() = Some(handle);
        true
    }
    
    /// Stop the UDP listener thread
    fn stop_listener(&self) -> bool {
        info!("Stopping ShairportSync UDP listener");
        
        self.stop_listener.store(true, Ordering::SeqCst);
        
        if let Some(handle) = self.listener_thread.lock().unwrap().take() {
            match handle.join() {
                Ok(_) => {
                    debug!("ShairportSync listener thread stopped successfully");
                    true
                }
                Err(_) => {
                    error!("Failed to join ShairportSync listener thread");
                    false
                }
            }
        } else {
            debug!("No ShairportSync listener thread to stop");
            true
        }
    }
    
    /// Main UDP listener loop
    fn listener_loop(
        port: u16,
        stop_flag: Arc<AtomicBool>,
        current_song: Arc<Mutex<Option<Song>>>,
        pending_song: Arc<Mutex<Option<Song>>>,
        current_state: Arc<Mutex<PlayerState>>,
        base: BasePlayerController,
    ) {
        let bind_address = format!("0.0.0.0:{}", port);
        let socket = match UdpSocket::bind(&bind_address) {
            Ok(s) => {
                info!("ShairportSync listener bound to {}", bind_address);
                s
            }
            Err(e) => {
                error!("Failed to bind to {}: {}", bind_address, e);
                return;
            }
        };
        
        // Set socket timeout to allow checking the stop flag
        if let Err(e) = socket.set_read_timeout(Some(Duration::from_millis(1000))) {
            error!("Failed to set socket timeout: {}", e);
            return;
        }
        
        let mut buffer = [0; 4096];
        let mut packet_count = 0;
        
        while !stop_flag.load(Ordering::SeqCst) {
            match socket.recv_from(&mut buffer) {
                Ok((bytes_received, sender_addr)) => {
                    packet_count += 1;
                    trace!("Received packet #{} from {} ({} bytes)", 
                           packet_count, sender_addr, bytes_received);
                    
                    // Parse ShairportSync message
                    let message = parse_shairport_message(&buffer[..bytes_received]);
                    
                    // Process the message
                    Self::process_message(&message, &current_song, &pending_song, &current_state, &base);
                }
                Err(e) => {
                    match e.kind() {
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut => {
                            // Timeout occurred, continue loop to check stop flag
                            continue;
                        }
                        _ => {
                            error!("Error receiving packet: {}", e);
                            break;
                        }
                    }
                }
            }
        }
        
        info!("ShairportSync listener stopped. Total packets received: {}", packet_count);
    }
    
    /// Process a ShairportSync message and update state
    fn process_message(
        message: &ShairportMessage,
        current_song: &Arc<Mutex<Option<Song>>>,
        pending_song: &Arc<Mutex<Option<Song>>>,
        current_state: &Arc<Mutex<PlayerState>>,
        base: &BasePlayerController,
    ) {
        match message {
            ShairportMessage::Control(action) => {
                // Always log control messages in debug mode
                debug!("ShairportSync handler: Processing control message: {}", action);
                
                // Handle playback control events
                match action.as_str() {
                    "PAUSE" => {
                        debug!("ShairportSync handler: Processing PAUSE command");
                        let mut state = current_state.lock().unwrap();
                        state.state = PlaybackState::Paused;
                        base.notify_state_changed(PlaybackState::Paused);
                    }
                    "RESUME" => {
                        debug!("ShairportSync handler: Processing RESUME command");
                        let mut state = current_state.lock().unwrap();
                        state.state = PlaybackState::Playing;
                        base.notify_state_changed(PlaybackState::Playing);
                    }
                    "SESSION_END" => {
                        debug!("ShairportSync handler: Processing SESSION_END command");
                        let mut state = current_state.lock().unwrap();
                        state.state = PlaybackState::Stopped;
                        base.notify_state_changed(PlaybackState::Stopped);
                        
                        // Clear current song on session end
                        *current_song.lock().unwrap() = None;
                        *pending_song.lock().unwrap() = None;
                        base.notify_song_changed(None);
                    }
                    "AUDIO_BEGIN" | "PLAYBACK_BEGIN" => {
                        debug!("ShairportSync handler: Processing {} command", action);
                        let mut state = current_state.lock().unwrap();
                        state.state = PlaybackState::Playing;
                        base.notify_state_changed(PlaybackState::Playing);
                    }
                    _ => {
                        // Check if this is a metadata message
                        if action.contains(": ") {
                            let parts: Vec<&str> = action.splitn(2, ": ").collect();
                            if parts.len() == 2 {
                                let key = parts[0];
                                let value = parts[1];
                                
                                // Handle special control messages
                                match key {
                                    "METADATA_START" => {
                                        debug!("ShairportSync handler: Starting metadata collection - {}", value);
                                        // Initialize pending song or preserve existing one
                                        let mut pending = pending_song.lock().unwrap();
                                        if pending.is_none() {
                                            *pending = Some(Song::default());
                                        }
                                        // Assume playing when metadata starts
                                        let mut state = current_state.lock().unwrap();
                                        state.state = PlaybackState::Playing;
                                        base.notify_state_changed(PlaybackState::Playing);
                                    }
                                    "METADATA_END" => {
                                        debug!("ShairportSync handler: Finalizing metadata collection - {}", value);
                                        // Move pending song to current and notify
                                        let mut pending = pending_song.lock().unwrap();
                                        if let Some(song) = pending.take() {
                                            if song_has_significant_metadata(&song) {
                                                debug!("ShairportSync handler: Publishing complete song metadata: {}", song);
                                                *current_song.lock().unwrap() = Some(song.clone());
                                                base.notify_song_changed(Some(&song));
                                            }
                                        }
                                    }
                                    "TRACK" | "ARTIST" | "ALBUM" | "GENRE" | "COMPOSER" | 
                                    "ALBUM_ARTIST" | "SONG_ALBUM_ARTIST" | "TRACK_NUMBER" | "TRACK_COUNT" => {
                                        debug!("ShairportSync handler: Processing metadata - {}: {}", key, value);
                                        // Update pending song metadata
                                        let mut pending = pending_song.lock().unwrap();
                                        let mut song = pending.take().unwrap_or_default();
                                        update_song_from_message(&mut song, message);
                                        *pending = Some(song);
                                    }
                                    _ => {
                                        debug!("ShairportSync handler: Processing other metadata - {}: {}", key, value);
                                        // Update pending song with other metadata
                                        let mut pending = pending_song.lock().unwrap();
                                        let mut song = pending.take().unwrap_or_default();
                                        update_song_from_message(&mut song, message);
                                        *pending = Some(song);
                                    }
                                }
                            }
                        }
                    }
                }
            }
            ShairportMessage::ChunkData { data_type, chunk_id, total_chunks, data } => {
                let clean_type = data_type.trim_end_matches('\0');
                
                // Ignore cover art data
                if clean_type == "ssncPICT" {
                    trace!("ShairportSync handler: Ignoring cover art chunk data - type: {}, chunk: {}/{}, size: {} bytes", 
                           clean_type, chunk_id, total_chunks, data.len());
                    return;
                }
                
                debug!("ShairportSync handler: Processing chunk data - type: {}, chunk: {}/{}, size: {} bytes", 
                       clean_type, chunk_id, total_chunks, data.len());
                
                // Handle chunk data for metadata updates (but don't notify yet)
                let mut pending = pending_song.lock().unwrap();
                let mut song = pending.take().unwrap_or_default();
                update_song_from_message(&mut song, message);
                *pending = Some(song);
            }
            ShairportMessage::CompletePicture { data: _, format: _ } => {
                // Ignore cover art completely
                trace!("ShairportSync handler: Ignoring complete cover art message");
            }
            ShairportMessage::SessionStart(session_id) => {
                debug!("Session started: {}", session_id);
                // Clear previous song data on new session
                *current_song.lock().unwrap() = None;
                *pending_song.lock().unwrap() = None;
            }
            ShairportMessage::SessionEnd(session_id) => {
                debug!("Session ended: {}", session_id);
                let mut state = current_state.lock().unwrap();
                state.state = PlaybackState::Stopped;
                base.notify_state_changed(PlaybackState::Stopped);
                
                *current_song.lock().unwrap() = None;
                *pending_song.lock().unwrap() = None;
                base.notify_song_changed(None);
            }
            ShairportMessage::Unknown(data) => {
                trace!("Unknown message: {} bytes", data.len());
            }
        }
    }
    
    /// Control systemd service for playback control
    fn control_systemd_service(&self, action: &str) -> bool {
        if let Some(ref unit_name) = self.systemd_unit {
            debug!("Controlling systemd unit '{}' with action '{}'", unit_name, action);
            
            let systemd_action = match action {
                "restart" => SystemdAction::Restart,
                "stop" => SystemdAction::Stop,
                "start" => SystemdAction::Start,
                _ => {
                    error!("Unknown systemd action: {}", action);
                    return false;
                }
            };
            
            info!("Executing {} on systemd unit '{}'", systemd_action, unit_name);
            
            match systemd(unit_name, systemd_action) {
                Ok(success) => {
                    if success {
                        info!("Successfully executed {} on systemd unit '{}'", action, unit_name);
                        true
                    } else {
                        warn!("Systemd command completed but may not have been successful for unit '{}'", unit_name);
                        false
                    }
                }
                Err(e) => {
                    error!("Failed to {} systemd unit '{}': {}", action, unit_name, e);
                    false
                }
            }
        } else {
            debug!("No systemd unit configured, cannot control service");
            false
        }
    }
}

impl PlayerController for ShairportController {
    fn get_capabilities(&self) -> PlayerCapabilitySet {
        self.base.get_capabilities()
    }
    
    fn get_song(&self) -> Option<Song> {
        self.current_song.lock().unwrap().clone()
    }
    
    fn get_queue(&self) -> Vec<Track> {
        // ShairportSync doesn't provide queue information
        Vec::new()
    }
    
    fn get_loop_mode(&self) -> LoopMode {
        // ShairportSync doesn't provide loop mode information
        LoopMode::None
    }
    
    fn get_playback_state(&self) -> PlaybackState {
        self.current_state.lock().unwrap().state
    }
    
    fn get_position(&self) -> Option<f64> {
        // ShairportSync doesn't provide reliable position information
        None
    }
    
    fn get_shuffle(&self) -> bool {
        // ShairportSync doesn't provide shuffle information
        false
    }
    
    fn get_player_name(&self) -> String {
        "shairport".to_string()
    }
    
    fn get_player_id(&self) -> String {
        format!("shairport-udp:{}", self.port)
    }
    
    fn get_last_seen(&self) -> Option<std::time::SystemTime> {
        self.base.get_last_seen()
    }
    
    fn send_command(&self, command: PlayerCommand) -> bool {
        // If systemd unit is configured, we can control playback via systemd
        if self.systemd_unit.is_some() {
            match command {
                PlayerCommand::Play => {
                    info!("ShairportSync received Play command, restarting systemd service");
                    self.control_systemd_service("restart")
                }
                PlayerCommand::Pause => {
                    info!("ShairportSync received Pause command, stopping systemd service");
                    self.control_systemd_service("stop")
                }
                PlayerCommand::Stop => {
                    info!("ShairportSync received Stop command, stopping systemd service");
                    self.control_systemd_service("stop")
                }
                _ => {
                    debug!("ShairportSync received unsupported command {:?}", command);
                    false
                }
            }
        } else {
            // ShairportSync is a passive listener, it can't control playback without systemd
            debug!("ShairportSync received command {:?} but no systemd unit configured", command);
            false
        }
    }
    
    fn as_any(&self) -> &dyn Any {
        self
    }
    
    fn start(&self) -> bool {
        info!("Starting ShairportSync controller on port {}", self.port);
        self.start_listener()
    }
    
    fn stop(&self) -> bool {
        info!("Stopping ShairportSync controller");
        self.stop_listener()
    }
    
    fn get_metadata_value(&self, _key: &str) -> Option<String> {
        // ShairportSync doesn't provide general metadata access
        None
    }
    
    fn get_meta_keys(&self) -> Vec<String> {
        // ShairportSync doesn't provide metadata keys
        vec![]
    }
}

impl Default for ShairportController {
    fn default() -> Self {
        Self::new()
    }
}
