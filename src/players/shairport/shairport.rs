use crate::players::player_controller::{BasePlayerController, PlayerController};
use crate::data::{PlayerCapabilitySet, Song, LoopMode, PlaybackState, PlayerCommand, PlayerState, Track};
use crate::helpers::shairportsync_messages::{
    ShairportMessage, ChunkCollector, parse_shairport_message, 
    detect_image_format,
    update_song_from_message, song_has_significant_metadata
};
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
    
    /// Current song information
    current_song: Arc<Mutex<Option<Song>>>,
    
    /// Current player state
    current_state: Arc<Mutex<PlayerState>>,
    
    /// Flag to stop the UDP listener thread
    stop_listener: Arc<AtomicBool>,
    
    /// Thread handle for the UDP listener
    listener_thread: Arc<Mutex<Option<thread::JoinHandle<()>>>>,
    
    /// Chunk collector for assembling multi-part artwork
    picture_collector: Arc<Mutex<Option<ChunkCollector>>>,
}

impl Clone for ShairportController {
    fn clone(&self) -> Self {
        ShairportController {
            base: self.base.clone(),
            port: self.port,
            current_song: Arc::clone(&self.current_song),
            current_state: Arc::clone(&self.current_state),
            stop_listener: Arc::clone(&self.stop_listener),
            listener_thread: Arc::clone(&self.listener_thread),
            picture_collector: Arc::clone(&self.picture_collector),
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
        debug!("Creating new ShairportController with port {}", port);
        
        // Create a base controller with player name and ID
        let base = BasePlayerController::with_player_info("shairport", &format!("udp:{}", port));
        
        let controller = Self {
            base,
            port,
            current_song: Arc::new(Mutex::new(None)),
            current_state: Arc::new(Mutex::new(PlayerState::new())),
            stop_listener: Arc::new(AtomicBool::new(false)),
            listener_thread: Arc::new(Mutex::new(None)),
            picture_collector: Arc::new(Mutex::new(None)),
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
        
        debug!("Creating ShairportController from config with port {}", port);
        Self::with_port(port)
    }
    
    /// Set the default capabilities for this player
    fn set_default_capabilities(&self) {
        debug!("Setting default ShairportController capabilities");
        // ShairportSync is a passive listener, so it has limited control capabilities
        self.base.set_capabilities(vec![
            // We can't actually control playback, but we can track state
            // PlayerCapability::Play,
            // PlayerCapability::Pause,
            // PlayerCapability::Stop,
        ], false); // Don't notify on initialization
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
        let current_state = Arc::clone(&self.current_state);
        let picture_collector = Arc::clone(&self.picture_collector);
        let base = self.base.clone();
        
        info!("Starting ShairportSync UDP listener on port {}", port);
        
        let handle = thread::spawn(move || {
            Self::listener_loop(port, stop_flag, current_song, current_state, picture_collector, base);
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
        current_state: Arc<Mutex<PlayerState>>,
        picture_collector: Arc<Mutex<Option<ChunkCollector>>>,
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
                    let mut message = parse_shairport_message(&buffer[..bytes_received]);
                    
                    // Handle chunk collection for pictures (though we'll ignore artwork for now)
                    if let ShairportMessage::ChunkData { chunk_id, total_chunks, data_type, data } = &message {
                        let clean_type = data_type.trim_end_matches('\0');
                        
                        if clean_type == "ssncPICT" && *total_chunks > 1 {
                            let mut collector_lock = picture_collector.lock().unwrap();
                            
                            // Initialize collector if needed
                            if collector_lock.is_none() || 
                               collector_lock.as_ref().unwrap().total_chunks != *total_chunks {
                                *collector_lock = Some(ChunkCollector::new(*total_chunks, clean_type.to_string()));
                            }
                            
                            // Add chunk to collector
                            if let Some(ref mut collector) = *collector_lock {
                                if let Some(complete_data) = collector.add_chunk(*chunk_id, data.clone()) {
                                    // We have a complete picture, but we'll ignore it for now
                                    let format = detect_image_format(&complete_data);
                                    debug!("Assembled complete artwork: {} ({} bytes)", format, complete_data.len());
                                    
                                    message = ShairportMessage::CompletePicture {
                                        data: complete_data,
                                        format,
                                    };
                                    *collector_lock = None; // Reset for next picture
                                }
                            }
                        }
                    }
                    
                    // Process the message
                    Self::process_message(&message, &current_song, &current_state, &base);
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
        current_state: &Arc<Mutex<PlayerState>>,
        base: &BasePlayerController,
    ) {
        match message {
            ShairportMessage::Control(action) => {
                debug!("Received control message: {}", action);
                
                // Handle playback control events
                match action.as_str() {
                    "PAUSE" => {
                        let mut state = current_state.lock().unwrap();
                        state.state = PlaybackState::Paused;
                        base.notify_state_changed(PlaybackState::Paused);
                    }
                    "RESUME" => {
                        let mut state = current_state.lock().unwrap();
                        state.state = PlaybackState::Playing;
                        base.notify_state_changed(PlaybackState::Playing);
                    }
                    "SESSION_END" => {
                        let mut state = current_state.lock().unwrap();
                        state.state = PlaybackState::Stopped;
                        base.notify_state_changed(PlaybackState::Stopped);
                        
                        // Clear current song on session end
                        *current_song.lock().unwrap() = None;
                        base.notify_song_changed(None);
                    }
                    "AUDIO_BEGIN" | "PLAYBACK_BEGIN" => {
                        let mut state = current_state.lock().unwrap();
                        state.state = PlaybackState::Playing;
                        base.notify_state_changed(PlaybackState::Playing);
                    }
                    _ => {
                        // Check if this is a metadata message and update song
                        let mut song_lock = current_song.lock().unwrap();
                        let mut song = song_lock.take().unwrap_or_default();
                        
                        if update_song_from_message(&mut song, message) {
                            if song_has_significant_metadata(&song) {
                                debug!("Updated song metadata: {}", song);
                                base.notify_song_changed(Some(&song));
                            }
                            *song_lock = Some(song);
                        } else if song_lock.is_some() {
                            // Put the song back if no update occurred
                            *song_lock = Some(song);
                        }
                    }
                }
            }
            ShairportMessage::ChunkData { .. } => {
                // Handle chunk data for metadata updates
                let mut song_lock = current_song.lock().unwrap();
                let mut song = song_lock.take().unwrap_or_default();
                
                if update_song_from_message(&mut song, message) {
                    if song_has_significant_metadata(&song) {
                        debug!("Updated song metadata from chunk: {}", song);
                        base.notify_song_changed(Some(&song));
                    }
                    *song_lock = Some(song);
                } else if song_lock.is_some() {
                    // Put the song back if no update occurred
                    *song_lock = Some(song);
                }
            }
            ShairportMessage::CompletePicture { .. } => {
                // Artwork completed but we're ignoring it for now
                debug!("Artwork assembled but ignoring for now");
            }
            ShairportMessage::SessionStart(session_id) => {
                debug!("Session started: {}", session_id);
                // Clear previous song data on new session
                *current_song.lock().unwrap() = None;
            }
            ShairportMessage::SessionEnd(session_id) => {
                debug!("Session ended: {}", session_id);
                let mut state = current_state.lock().unwrap();
                state.state = PlaybackState::Stopped;
                base.notify_state_changed(PlaybackState::Stopped);
                
                *current_song.lock().unwrap() = None;
                base.notify_song_changed(None);
            }
            ShairportMessage::Unknown(data) => {
                trace!("Unknown message: {} bytes", data.len());
            }
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
        // ShairportSync is a passive listener, it can't control playback
        debug!("ShairportSync received command {:?} but cannot control playback", command);
        false
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
