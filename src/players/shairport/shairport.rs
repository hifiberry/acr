use crate::players::player_controller::{BasePlayerController, PlayerController};
use crate::data::{PlayerCapabilitySet, PlayerCapability, Song, LoopMode, PlaybackState, PlayerCommand, PlayerState, Track};
use crate::helpers::shairportsync_messages::{
    ShairportMessage, parse_shairport_message, 
    update_song_from_message, song_has_significant_metadata
};
use crate::helpers::process_helper::{systemd, SystemdAction};
use crate::helpers::imagecache;
use std::sync::{Arc, Mutex};
use log::{debug, info, warn, error, trace};
use std::net::UdpSocket;
use std::thread;
use std::time::{Duration, SystemTime};
use std::sync::atomic::{AtomicBool, Ordering};
use std::any::Any;
use std::path::Path;
use notify::{Watcher, RecursiveMode, Event, EventKind, recommended_watcher, event::CreateKind, event::ModifyKind};
use std::sync::mpsc;
use md5;

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
    
    /// Cover art directory to monitor for new images
    coverart_dir: String,
    
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
    
    /// Thread handle for the directory watcher
    watcher_thread: Arc<Mutex<Option<thread::JoinHandle<()>>>>,
}

impl Clone for ShairportController {
    fn clone(&self) -> Self {
        ShairportController {
            base: self.base.clone(),
            port: self.port,
            systemd_unit: self.systemd_unit.clone(),
            coverart_dir: self.coverart_dir.clone(),
            current_song: Arc::clone(&self.current_song),
            pending_song: Arc::clone(&self.pending_song),
            current_state: Arc::clone(&self.current_state),
            stop_listener: Arc::clone(&self.stop_listener),
            listener_thread: Arc::clone(&self.listener_thread),
            watcher_thread: Arc::clone(&self.watcher_thread),
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
        Self::with_full_config(port, systemd_unit, "/tmp/shairport-sync/.cache/coverart".to_string())
    }
    
    /// Create a new ShairportSync controller with full configuration
    pub fn with_full_config(port: u16, systemd_unit: Option<String>, coverart_dir: String) -> Self {
        debug!("Creating new ShairportController with port {}, systemd unit {:?}, and coverart dir {}", port, systemd_unit, coverart_dir);
        
        // Create a base controller with player name and ID
        let base = BasePlayerController::with_player_info("shairport", "shairport");
        
        let controller = Self {
            base,
            port,
            systemd_unit,
            coverart_dir,
            current_song: Arc::new(Mutex::new(None)),
            pending_song: Arc::new(Mutex::new(None)),
            current_state: Arc::new(Mutex::new(PlayerState::new())),
            stop_listener: Arc::new(AtomicBool::new(false)),
            listener_thread: Arc::new(Mutex::new(None)),
            watcher_thread: Arc::new(Mutex::new(None)),
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
        
        let coverart_dir = config.get("coverart_dir")
            .and_then(|s| s.as_str())
            .unwrap_or("/tmp/shairport-sync/.cache/coverart")
            .to_string();
        
        Self::with_full_config(port, systemd_unit, coverart_dir)
    }
    
    /// Set the default capabilities for this player
    fn set_default_capabilities(&self) {
        debug!("Setting default ShairportController capabilities");
        // ShairportSync is a passive listener that can provide metadata and album art
        let mut capabilities = vec![
            PlayerCapability::Metadata,
            PlayerCapability::AlbumArt,
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
        
        debug!("Starting ShairportSync UDP listener on port {}", port);
        
        let handle = thread::spawn(move || {
            Self::listener_loop(port, stop_flag, current_song, pending_song, current_state, base);
        });
        
        *self.listener_thread.lock().unwrap() = Some(handle);
        true
    }
    
    /// Start the directory watcher thread
    fn start_watcher(&self) -> bool {
        if self.watcher_thread.lock().unwrap().is_some() {
            warn!("ShairportSync directory watcher already running");
            return false;
        }
        
        let coverart_dir = self.coverart_dir.clone();
        let stop_flag = Arc::clone(&self.stop_listener);
        let current_song = Arc::clone(&self.current_song);
        let pending_song = Arc::clone(&self.pending_song);
        let base = self.base.clone();
        
        debug!("Starting ShairportSync directory watcher for {}", coverart_dir);
        
        let handle = thread::spawn(move || {
            Self::watcher_loop(coverart_dir, stop_flag, current_song, pending_song, base);
        });
        
        *self.watcher_thread.lock().unwrap() = Some(handle);
        true
    }
    
    /// Stop the UDP listener thread
    fn stop_listener(&self) -> bool {
        debug!("Stopping ShairportSync UDP listener");
        
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
    
    /// Stop the directory watcher thread
    fn stop_watcher(&self) -> bool {
        debug!("Stopping ShairportSync directory watcher");
        
        if let Some(handle) = self.watcher_thread.lock().unwrap().take() {
            match handle.join() {
                Ok(_) => {
                    debug!("ShairportSync watcher thread stopped successfully");
                    true
                }
                Err(_) => {
                    error!("Failed to join ShairportSync watcher thread");
                    false
                }
            }
        } else {
            debug!("No ShairportSync watcher thread to stop");
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
                debug!("ShairportSync listener bound to {}", bind_address);
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
        
        debug!("ShairportSync listener stopped. Total packets received: {}", packet_count);
    }
    
    /// Main directory watcher loop
    fn watcher_loop(
        coverart_dir: String,
        stop_flag: Arc<AtomicBool>,
        current_song: Arc<Mutex<Option<Song>>>,
        pending_song: Arc<Mutex<Option<Song>>>,
        base: BasePlayerController,
    ) {
        let path = Path::new(&coverart_dir);
        
        // Create directory if it doesn't exist
        if !path.exists() {
            if let Err(e) = std::fs::create_dir_all(path) {
                error!("Failed to create coverart directory {}: {}", coverart_dir, e);
                return;
            }
            debug!("Created coverart directory: {}", coverart_dir);
        }
        
        // Set up file system watcher with simplified event handling
        let (tx, rx) = mpsc::channel();
        
        let mut watcher = match recommended_watcher(move |res: Result<Event, notify::Error>| {
            if let Ok(event) = res {
                let _ = tx.send(event);
            }
        }) {
            Ok(w) => w,
            Err(e) => {
                error!("Failed to create file system watcher: {}", e);
                return;
            }
        };
        
        // Watch the directory for file changes
        if let Err(e) = watcher.watch(path, RecursiveMode::NonRecursive) {
            error!("Failed to watch directory {}: {}", coverart_dir, e);
            return;
        }
        
        debug!("Watching directory for cover art files: {}", coverart_dir);
        
        // Initial scan for existing cover art files
        Self::scan_existing_coverart(&coverart_dir, &current_song, &pending_song, &base);
        
        while !stop_flag.load(Ordering::SeqCst) {
            match rx.recv_timeout(Duration::from_millis(1000)) {
                Ok(event) => {
                    debug!("Received filesystem event in watcher loop");
                    Self::handle_filesystem_event(
                        &event, 
                        &current_song, 
                        &pending_song, 
                        &base
                    );
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    // Timeout is normal, continue loop to check stop flag
                    continue;
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    error!("File system watcher channel disconnected");
                    break;
                }
            }
        }
        
        debug!("ShairportSync directory watcher stopped");
    }
    
    /// Handle filesystem events for cover art
    fn handle_filesystem_event(
        event: &Event,
        current_song: &Arc<Mutex<Option<Song>>>,
        pending_song: &Arc<Mutex<Option<Song>>>,
        base: &BasePlayerController,
    ) {
        debug!("Filesystem event received: {:?}", event);
        
        // Only process file creation and modification events
        match &event.kind {
            EventKind::Create(CreateKind::File) => {
                debug!("File creation event detected");
                for path in &event.paths {
                    debug!("Processing created file: {}", path.display());
                    Self::process_new_coverart_file(path, current_song, pending_song, base);
                }
            }
            EventKind::Modify(ModifyKind::Data(_)) => {
                debug!("File modification event detected");
                for path in &event.paths {
                    debug!("Processing modified file: {}", path.display());
                    Self::process_new_coverart_file(path, current_song, pending_song, base);
                }
            }
            _ => {
                debug!("Ignoring filesystem event type: {:?}", event.kind);
            }
        }
    }
    
    /// Process a new cover art file
    fn process_new_coverart_file(
        path: &Path,
        current_song: &Arc<Mutex<Option<Song>>>,
        pending_song: &Arc<Mutex<Option<Song>>>,
        base: &BasePlayerController,
    ) {
        debug!("Evaluating file for cover art processing: {}", path.display());
        
        // Get filename
        let Some(filename) = path.file_name().and_then(|f| f.to_str()) else {
            debug!("Could not extract filename from path: {}", path.display());
            return;
        };
        
        debug!("Extracted filename: {}", filename);
        
        // Skip temporary files, hidden files, and non-image files
        if filename.starts_with('.') {
            debug!("Skipping hidden file: {}", filename);
            return;
        }
        
        if filename.starts_with("tmp") {
            debug!("Skipping temporary file: {}", filename);
            return;
        }
        
        if !Self::is_image_file(filename) {
            debug!("Skipping non-image file: {}", filename);
            return;
        }
        
        debug!("New cover art file detected: {}", path.display());
        debug!("File passes all filters, processing as cover art");
        
        // Process the new cover art file
        if let Some(artwork_url) = Self::process_cover_art_file(path) {
            debug!("Successfully processed cover art, updating song with URL: {}", artwork_url);
            Self::update_song_cover_art(artwork_url, current_song, pending_song, base);
        } else {
            warn!("Failed to process cover art file: {}", path.display());
        }
    }
    
    /// Check if a filename represents an image file
    fn is_image_file(filename: &str) -> bool {
        let lower = filename.to_lowercase();
        let is_image = lower.ends_with(".jpg") || 
                      lower.ends_with(".jpeg") || 
                      lower.ends_with(".png") || 
                      lower.ends_with(".gif") || 
                      lower.ends_with(".bmp") || 
                      lower.ends_with(".webp") ||
                      lower.ends_with(".heic");
        
        debug!("Image file check for '{}': {}", filename, is_image);
        is_image
    }
    
    /// Process a cover art file and store it in the image cache
    fn process_cover_art_file(file_path: &Path) -> Option<String> {
        // Read the file
        let artwork_data = match std::fs::read(file_path) {
            Ok(data) => data,
            Err(e) => {
                error!("Failed to read cover art file {}: {}", file_path.display(), e);
                return None;
            }
        };
        
        if artwork_data.is_empty() {
            debug!("Empty cover art file: {}", file_path.display());
            return None;
        }
        
        // Generate MD5 hash for unique filename
        let digest = md5::compute(&artwork_data);
        let hash_string = format!("{:x}", digest);
        
        // Get extension from file
        let extension = file_path
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("jpg");
        
        // Create cache path
        let filename = format!("{}.{}", hash_string, extension);
        let cache_path = format!("shairportsync/{}", filename);
        
        // Set expiry to 1 week from now
        let expiry_time = SystemTime::now() + Duration::from_secs(7 * 24 * 60 * 60); // 7 days
        
        // Store in image cache with expiry
        match imagecache::store_image_with_expiry(&cache_path, &artwork_data, Some(expiry_time)) {
            Ok(_) => {
                debug!("Stored cover art in cache: {} ({} bytes, expires in 1 week)", 
                      cache_path, artwork_data.len());
                
                // Return URL path for accessing the image
                Some(format!("/api/imagecache/{}", cache_path))
            }
            Err(e) => {
                error!("Failed to store cover art in cache: {}", e);
                None
            }
        }
    }
    
    /// Update song cover art and notify listeners
    fn update_song_cover_art(
        artwork_url: String,
        current_song: &Arc<Mutex<Option<Song>>>,
        pending_song: &Arc<Mutex<Option<Song>>>,
        base: &BasePlayerController,
    ) {
        // Update current song if it exists
        {
            let mut current = current_song.lock().unwrap();
            if let Some(ref mut song) = *current {
                song.cover_art_url = Some(artwork_url.clone());
                base.notify_song_changed(Some(song));
                return;
            }
        }
        
        // Update pending song if it exists
        {
            let mut pending = pending_song.lock().unwrap();
            if let Some(ref mut song) = *pending {
                song.cover_art_url = Some(artwork_url.clone());
                // Don't notify yet for pending songs
                return;
            }
        }
        
        // If no current or pending song, create a minimal song with just cover art
        {
            let mut current = current_song.lock().unwrap();
            let mut song = Song::default();
            song.cover_art_url = Some(artwork_url.clone());
            *current = Some(song.clone());
            base.notify_song_changed(Some(&song));
        }
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
                debug!("Processing control message: {}", action);
                
                // Handle playback control events
                match action.as_str() {
                    "PAUSE" => {
                        debug!("Processing PAUSE command");
                        let mut state = current_state.lock().unwrap();
                        state.state = PlaybackState::Paused;
                        base.notify_state_changed(PlaybackState::Paused);
                    }
                    "RESUME" => {
                        debug!("Processing RESUME command");
                        let mut state = current_state.lock().unwrap();
                        state.state = PlaybackState::Playing;
                        base.notify_state_changed(PlaybackState::Playing);
                    }
                    "SESSION_END" => {
                        debug!("Processing SESSION_END command");
                        let mut state = current_state.lock().unwrap();
                        state.state = PlaybackState::Stopped;
                        base.notify_state_changed(PlaybackState::Stopped);
                        
                        // Clear current song on session end
                        *current_song.lock().unwrap() = None;
                        *pending_song.lock().unwrap() = None;
                        base.notify_song_changed(None);
                    }
                    "AUDIO_BEGIN" | "PLAYBACK_BEGIN" => {
                        debug!("Processing {} command", action);
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
                                        debug!("Starting metadata collection - {}", value);
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
                                        debug!("Finalizing metadata collection - {}", value);
                                        // Move pending song to current and notify
                                        let mut pending = pending_song.lock().unwrap();
                                        if let Some(song) = pending.take() {
                                            if song_has_significant_metadata(&song) {
                                                debug!("Publishing complete song metadata: {}", song);
                                                *current_song.lock().unwrap() = Some(song.clone());
                                                base.notify_song_changed(Some(&song));
                                            }
                                        }
                                    }
                                    "TRACK" | "ARTIST" | "ALBUM" | "GENRE" | "COMPOSER" | 
                                    "ALBUM_ARTIST" | "SONG_ALBUM_ARTIST" | "TRACK_NUMBER" | "TRACK_COUNT" => {
                                        debug!("Processing metadata - {}: {}", key, value);
                                        // Update pending song metadata
                                        let mut pending = pending_song.lock().unwrap();
                                        let mut song = pending.take().unwrap_or_default();
                                        update_song_from_message(&mut song, message);
                                        *pending = Some(song);
                                    }
                                    _ => {
                                        debug!("Processing other metadata - {}: {}", key, value);
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
                    return;
                }
                
                debug!("Processing chunk data - type: {}, chunk: {}/{}, size: {} bytes", 
                       clean_type, chunk_id, total_chunks, data.len());
                
                // Handle chunk data for metadata updates (but don't notify yet)
                let mut pending = pending_song.lock().unwrap();
                let mut song = pending.take().unwrap_or_default();
                update_song_from_message(&mut song, message);
                *pending = Some(song);
            }
            ShairportMessage::CompletePicture { data: _, format: _ } => {
                // Ignore cover art completely
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
    
    /// Scan the coverart directory for existing image files and set initial cover art
    fn scan_existing_coverart(
        coverart_dir: &str,
        current_song: &Arc<Mutex<Option<Song>>>,
        pending_song: &Arc<Mutex<Option<Song>>>,
        base: &BasePlayerController,
    ) {
        let path = std::path::Path::new(coverart_dir);
        let Ok(entries) = std::fs::read_dir(path) else {
            debug!("scan_existing_coverart: Could not read directory: {}", coverart_dir);
            return;
        };
        
        debug!("scan_existing_coverart: Scanning directory {} for existing cover art", coverart_dir);
        
        for entry in entries.flatten() {
            let file_path = entry.path();
            if let Some(filename) = file_path.file_name().and_then(|f| f.to_str()) {
                // Skip temporary files, hidden files, and non-image files
                if filename.starts_with('.') || filename.starts_with("tmp") {
                    continue;
                }
                
                if Self::is_image_file(filename) {
                    debug!("scan_existing_coverart: Found existing image file: {}", file_path.display());
                    if let Some(artwork_url) = Self::process_cover_art_file(&file_path) {
                        debug!("scan_existing_coverart: Setting initial cover art: {}", artwork_url);
                        Self::update_song_cover_art(artwork_url, current_song, pending_song, base);
                        // Only set the first valid image found
                        break;
                    }
                }
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
            
            debug!("Executing {} on systemd unit '{}'", systemd_action, unit_name);
            
            match systemd(unit_name, systemd_action) {
                Ok(success) => {
                    if success {
                        debug!("Successfully executed {} on systemd unit '{}'", action, unit_name);
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
    
    fn get_aliases(&self) -> Vec<String> {
        vec!["airplay".to_string(), "shairport".to_string(), "shairport-sync".to_string()]
    }
    
    fn get_player_id(&self) -> String {
        "shairport".to_string()
    }
    
    fn get_last_seen(&self) -> Option<std::time::SystemTime> {
        self.base.get_last_seen()
    }
    
    fn send_command(&self, command: PlayerCommand) -> bool {
        // If systemd unit is configured, we can control playback via systemd
        if self.systemd_unit.is_some() {
            match command {
                PlayerCommand::Play => {
                    debug!("ShairportSync received Play command, restarting systemd service");
                    self.control_systemd_service("restart")
                }
                PlayerCommand::Pause => {
                    debug!("ShairportSync received Pause command, stopping systemd service");
                    self.control_systemd_service("stop")
                }
                PlayerCommand::Stop => {
                    debug!("ShairportSync received Stop command, stopping systemd service");
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
        info!("Starting ShairportSync player on port {}", self.port);
        
        let listener_started = self.start_listener();
        let watcher_started = self.start_watcher();
        
        let success = listener_started && watcher_started;
        if success {
            info!("ShairportSync player started successfully");
        } else {
            error!("ShairportSync player failed to start (listener: {}, watcher: {})", listener_started, watcher_started);
        }
        
        success
    }
    
    fn stop(&self) -> bool {
        info!("Stopping ShairportSync player");
        let listener_stopped = self.stop_listener();
        let watcher_stopped = self.stop_watcher();
        let success = listener_stopped && watcher_stopped;
        
        if success {
            info!("ShairportSync player stopped successfully");
        } else {
            error!("ShairportSync player failed to stop (listener: {}, watcher: {})", listener_stopped, watcher_stopped);
        }
        
        success
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
