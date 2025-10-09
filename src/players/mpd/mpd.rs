use crate::players::player_controller::{BasePlayerController, PlayerController};
use crate::data::{PlayerCapability, PlayerCapabilitySet, Song, LoopMode, PlaybackState, PlayerCommand, PlayerState, Track};
use crate::data::library::LibraryInterface;
use crate::constants::API_PREFIX;
use crate::helpers::retry::RetryHandler;
use crate::helpers::url_encoding;
use crate::helpers::songsplitmanager::SongSplitManager;
use crate::helpers::attributecache;
use crate::helpers::backgroundjobs::BackgroundJobs;
use delegate::delegate;
use std::sync::{Arc, Mutex};
use std::fs;
use std::io::{BufRead, BufReader};
use log::{debug, info, warn, error, trace};
use mpd::{Client, error::Error as MpdError, idle::Subsystem};
use mpd::Idle; // Add the Idle trait import
use std::net::TcpStream;
use std::thread;
use std::time::Duration;
use std::sync::atomic::{AtomicBool, Ordering};
use std::collections::HashMap;
use std::any::Any;
use lazy_static::lazy_static;

/// Constant for MPD image API URL prefix including API prefix
pub fn mpd_image_url() -> String {
    format!("{}/library/mpd/image", API_PREFIX)
}

/// MPD player controller implementation
pub struct MPDPlayerController {
    /// Base controller for managing state listeners
    base: BasePlayerController,
    
    /// MPD server hostname
    hostname: String,
    
    /// MPD server port
    port: u16,
    
    /// Current song information
    current_song: Arc<Mutex<Option<Song>>>,

    // current player state
    current_state: Arc<Mutex<PlayerState>>,
    
    /// Whether to load the MPD library into memory
    load_mpd_library: bool,
    
    /// Flag to control metadata enhancement
    enhance_metadata: bool,
    
    /// Flag to control cover art extraction from music files
    extract_coverart: bool,
    
    /// Custom artist separators for splitting artist names
    artist_separators: Option<Vec<String>>,
    
    /// MPD music directory path (if empty, will attempt to read from /etc/mpd.conf)
    music_directory: String,
    
    /// Cached effective music directory (to avoid parsing /etc/mpd.conf repeatedly)
    effective_music_directory: Arc<Mutex<Option<String>>>,
    
    /// MPD library instance wrapped in Arc and Mutex for thread-safe access
    library: Arc<Mutex<Option<crate::players::mpd::library::MPDLibrary>>>,
    
    /// Maximum number of reconnection attempts before giving up
    max_reconnect_attempts: u32,
    
    /// Current reconnection attempt counter
    reconnect_attempts: Arc<Mutex<u32>>,
    
    /// Flag indicating if connection has been permanently disabled due to max attempts
    connection_disabled: Arc<AtomicBool>,
    
    /// Song title splitter manager for radio stations that combine artist and song in title
    song_split_manager: SongSplitManager,
    
    /// Current MPD database update job ID (if any)
    current_update_job_id: Arc<Mutex<Option<String>>>,
}

// Manually implement Clone for MPDPlayerController
impl Clone for MPDPlayerController {
    fn clone(&self) -> Self {
        MPDPlayerController {
            // Share the BasePlayerController instance to maintain listener registrations
            base: self.base.clone(),
            hostname: self.hostname.clone(),
            port: self.port,
            current_song: Arc::clone(&self.current_song),
            current_state: Arc::clone(&self.current_state),
            load_mpd_library: self.load_mpd_library,
            enhance_metadata: self.enhance_metadata,
            extract_coverart: self.extract_coverart,
            artist_separators: self.artist_separators.clone(),
            music_directory: self.music_directory.clone(),
            effective_music_directory: Arc::clone(&self.effective_music_directory),
            library: Arc::clone(&self.library),
            max_reconnect_attempts: self.max_reconnect_attempts,
            reconnect_attempts: Arc::clone(&self.reconnect_attempts),
            connection_disabled: Arc::clone(&self.connection_disabled),
            song_split_manager: self.song_split_manager.clone(),
            current_update_job_id: Arc::clone(&self.current_update_job_id),
        }
    }
}

impl MPDPlayerController {
    /// Create a new MPD player controller with default settings
    pub fn new() -> Self {
        debug!("Creating new MPDPlayerController with default settings");
        let host = "localhost";
        let port = 6600;
        
        // Create a base controller with player name and ID
        let base = BasePlayerController::with_player_info("mpd", &format!("{}:{}", host, port));
        
        let player = Self {
            base,
            hostname: host.to_string(),
            port,
            current_song: Arc::new(Mutex::new(None)),
            current_state: Arc::new(Mutex::new(PlayerState::new())),
            load_mpd_library: true,
            enhance_metadata: true,
            extract_coverart: true,
            artist_separators: None,
            music_directory: String::new(),
            effective_music_directory: Arc::new(Mutex::new(None)),
            library: Arc::new(Mutex::new(None)),
            max_reconnect_attempts: 5, // Default value
            reconnect_attempts: Arc::new(Mutex::new(0)),
            connection_disabled: Arc::new(AtomicBool::new(false)),
            song_split_manager: SongSplitManager::new(),
            current_update_job_id: Arc::new(Mutex::new(None)),
        };
        
        // Set default capabilities
        player.set_default_capabilities();
        
        player
    }
    
    /// Create a new MPD player controller with custom settings
    pub fn with_connection(hostname: &str, port: u16) -> Self {
        debug!("Creating new MPDPlayerController with connection {}:{}", hostname, port);
        
        // Create a base controller with player name and ID
        let base = BasePlayerController::with_player_info("mpd", &format!("{}:{}", hostname, port));
        
        let player = Self {
            base,
            hostname: hostname.to_string(),
            port,
            current_song: Arc::new(Mutex::new(None)),
            current_state: Arc::new(Mutex::new(PlayerState::new())),
            load_mpd_library: true,
            enhance_metadata: true,
            extract_coverart: true,
            artist_separators: None,
            music_directory: String::new(),
            effective_music_directory: Arc::new(Mutex::new(None)),
            library: Arc::new(Mutex::new(None)),
            max_reconnect_attempts: 5, // Default value
            reconnect_attempts: Arc::new(Mutex::new(0)),
            connection_disabled: Arc::new(AtomicBool::new(false)),
            song_split_manager: SongSplitManager::new(),
            current_update_job_id: Arc::new(Mutex::new(None)),
        };
        
        // Set default capabilities
        player.set_default_capabilities();
        
        player
    }
    
    /// Set the default capabilities for this player
    fn set_default_capabilities(&self) {
        debug!("Setting default MPDPlayerController capabilities");
        self.base.set_capabilities(vec![
            PlayerCapability::Play,
            PlayerCapability::Pause,
            PlayerCapability::PlayPause,
            PlayerCapability::Stop,
            PlayerCapability::Next,
            PlayerCapability::Previous,
            PlayerCapability::Seek,
            PlayerCapability::Loop,
            PlayerCapability::Shuffle,
            PlayerCapability::Killable,
            PlayerCapability::Queue,
        ], false); // Don't notify on initialization
    }
    
    /// Attempt to reconnect to the MPD server
    pub fn reconnect(&self) -> Result<(), MpdError> {
        let addr = format!("{}:{}", self.hostname, self.port);
        debug!("Attempting to reconnect to MPD at {}", addr);
        
        match Client::connect(&addr) {
            Ok(_) => {
                info!("Successfully reconnected to MPD at {}", addr);
                self.reset_reconnect_attempts(); // Reset counter on successful connection
                Ok(())
            },
            Err(e) => {
                warn!("Failed to reconnect to MPD at {}: {}", addr, e);
                Err(e)
            }
        }
    }
    
    /// Check if connected to MPD server
    pub fn is_connected(&self) -> bool {
        // Create a fresh connection to check connectivity
        if let Some(mut client) = self.get_fresh_client() {
            // Try a simple ping to verify the connection
            match client.ping() {
                Ok(_) => {
                    debug!("MPD connection is active");
                    return true;
                },
                Err(e) => {
                    debug!("MPD connection lost: {}", e);
                    return false;
                }
            }
        }
        false
    }
    
    /// Get the current MPD server hostname
    pub fn hostname(&self) -> &str {
        &self.hostname
    }
    
    /// Get the current MPD server port
    pub fn port(&self) -> u16 {
        self.port
    }
    
    /// Update the connection settings and reconnect
    pub fn set_connection(&mut self, hostname: &str, port: u16) {
        debug!("Updating MPD connection to {}:{}", hostname, port);
        self.hostname = hostname.to_string();
        self.port = port;
    }
    
    /// Get whether to load MPD library into memory
    pub fn load_mpd_library(&self) -> bool {
        self.load_mpd_library
    }
    
    /// Set whether to load MPD library into memory
    pub fn set_load_mpd_library(&mut self, load: bool) {
        self.load_mpd_library = load;
    }
    
    /// Get whether to enhance metadata
    pub fn get_enhance_metadata(&self) -> Option<bool> {
        Some(self.enhance_metadata)
    }

    /// Set whether to enhance metadata
    pub fn set_enhance_metadata(&mut self, enhance: bool) {
        self.enhance_metadata = enhance;
    }
    
    /// Get whether to extract cover art from music files
    pub fn get_extract_coverart(&self) -> Option<bool> {
        Some(self.extract_coverart)
    }

    /// Set whether to extract cover art from music files
    pub fn set_extract_coverart(&mut self, extract: bool) {
        self.extract_coverart = extract;
    }
    
    /// Get the configured music directory path
    pub fn get_music_directory(&self) -> &str {
        &self.music_directory
    }
    
    /// Set the music directory path
    pub fn set_music_directory(&mut self, directory: String) {
        debug!("Setting music directory to: {}", directory);
        self.music_directory = directory;
        // Clear the cached effective music directory so it will be recalculated
        if let Ok(mut cached) = self.effective_music_directory.lock() {
            *cached = None;
        }
    }
    
    /// Get the effective music directory path
    /// If configured music_directory is empty, attempts to parse it from /etc/mpd.conf
    pub fn get_effective_music_directory(&self) -> Option<String> {
        // First check if we have a cached result
        if let Ok(cached) = self.effective_music_directory.lock() {
            if let Some(ref cached_dir) = *cached {
                return Some(cached_dir.clone());
            }
        }
        
        // If music_directory is configured in the JSON, use it
        let effective_dir = if !self.music_directory.is_empty() {
            debug!("Using configured music directory: {}", self.music_directory);
            Some(self.music_directory.clone())
        } else {
            // Otherwise, try to parse it from /etc/mpd.conf
            debug!("Music directory not configured, attempting to parse from /etc/mpd.conf");
            self.parse_music_directory_from_config()
        };
        
        // Cache the result
        if let Ok(mut cached) = self.effective_music_directory.lock() {
            *cached = effective_dir.clone();
        }
        
        effective_dir
    }
    
    /// Parse the music directory from /etc/mpd.conf
    fn parse_music_directory_from_config(&self) -> Option<String> {
        let config_path = "/etc/mpd.conf";
        
        match fs::File::open(config_path) {
            Ok(file) => {
                let reader = BufReader::new(file);
                
                for line in reader.lines() {
                    if let Ok(line) = line {
                        let trimmed = line.trim();
                        
                        // Skip comments and empty lines
                        if trimmed.is_empty() || trimmed.starts_with('#') {
                            continue;
                        }
                        
                        // Look for music_directory line
                        if trimmed.starts_with("music_directory") {
                            // Parse the format: music_directory "/var/lib/mpd/music"
                            if let Some(start_quote) = trimmed.find('"') {
                                if let Some(end_quote) = trimmed.rfind('"') {
                                    if start_quote < end_quote {
                                        let directory = &trimmed[start_quote + 1..end_quote];
                                        info!("Auto-detected MPD music directory from {}: {}", config_path, directory);
                                        return Some(directory.to_string());
                                    }
                                }
                            }
                        }
                    }
                }
                
                warn!("No music_directory found in {}", config_path);
                None
            }
            Err(e) => {
                warn!("Failed to read {}: {}", config_path, e);
                None
            }
        }
    }
    
    /// Get the maximum number of reconnection attempts
    pub fn get_max_reconnect_attempts(&self) -> u32 {
        self.max_reconnect_attempts
    }
    
    /// Set the maximum number of reconnection attempts before giving up
    pub fn set_max_reconnect_attempts(&mut self, attempts: u32) {
        debug!("Setting maximum reconnection attempts to {}", attempts);
        self.max_reconnect_attempts = attempts;
    }
    
    /// Reset the reconnection attempt counter
    fn reset_reconnect_attempts(&self) {
        if let Ok(mut counter) = self.reconnect_attempts.lock() {
            *counter = 0;
        }
        // Re-enable connections when we successfully connect
        self.connection_disabled.store(false, Ordering::Relaxed);
    }
    
    /// Increment the reconnection attempt counter and return the new value
    fn increment_reconnect_attempts(&self) -> u32 {
        if let Ok(mut counter) = self.reconnect_attempts.lock() {
            *counter += 1;
            *counter
        } else {
            1
        }
    }
    
    /// Disable further connection attempts after max attempts reached
    fn disable_connections(&self) {
        self.connection_disabled.store(true, Ordering::Relaxed);
    }
    
    /// Check if connections are disabled
    fn are_connections_disabled(&self) -> bool {
        self.connection_disabled.load(Ordering::Relaxed)
    }
    
    /// Get a reference to the MPD library, if available
    pub fn get_library(&self) -> Option<crate::players::mpd::library::MPDLibrary> {
        // Lock the mutex and clone the library if it exists
        if let Ok(library_guard) = self.library.lock() {
            // Clone the library if it exists
            return library_guard.clone();
        }
        None
    }
    
    /// Force a refresh of the MPD library
    pub fn refresh_library(&self) -> Result<(), crate::data::library::LibraryError> {
        debug!("Requesting MPD library refresh");
        
        // Get the library instance if available
        if let Some(mut library) = self.get_library() {
            // Pass the artist separators to the library before refreshing
            if let Some(separators) = &self.artist_separators {
                library.set_artist_separators(separators.clone());
            }
            
            // Run the refresh in a separate thread
            let library_clone = library;
            thread::spawn(move || {
                match library_clone.refresh_library() {
                    Ok(_) => info!("MPD library refreshed successfully"),
                    Err(e) => warn!("Failed to refresh MPD library: {}", e),
                }
            });
            
            return Ok(());
        }
        
        Err(crate::data::library::LibraryError::InternalError("Library not initialized".to_string()))
    }
    
    /// Set the custom artist separators for splitting artist names
    pub fn set_artist_separators(&mut self, separators: Vec<String>) {
        debug!("Setting custom artist separators: {:?}", separators);
        self.artist_separators = Some(separators);
    }
    
    /// Get the current custom artist separators if set
    pub fn get_artist_separators(&self) -> Option<&[String]> {
        self.artist_separators.as_deref()
    }
    
    /// Clear all title splitters (useful for cleanup or configuration changes)
    pub fn clear_title_splitters(&self) {
        self.song_split_manager.clear_all_splitters();
    }
    
    /// Get the number of active title splitters
    pub fn get_title_splitter_count(&self) -> usize {
        self.song_split_manager.get_splitter_count()
    }
    
    /// Get statistics for a specific URL's title splitter
    pub fn get_title_splitter_stats(&self, url: &str) -> Option<(u32, u32, u32, u32, bool)> {
        self.song_split_manager.get_splitter_stats(url)
    }
    
    /// Get all splitter IDs currently being managed
    pub fn get_all_splitter_ids(&self) -> Vec<String> {
        self.song_split_manager.get_splitter_ids()
    }
    
    /// Get statistics for all active splitters
    pub fn get_all_splitter_stats(&self) -> HashMap<String, (u32, u32, u32, u32, bool)> {
        self.song_split_manager.get_all_splitter_stats()
    }
    
    /// Remove a specific splitter (useful for cleanup)
    pub fn remove_title_splitter(&self, url: &str) -> bool {
        self.song_split_manager.remove_splitter(url)
    }
      /// Notify all registered listeners that the database is being updated
    pub fn notify_database_update(&self, artist: Option<String>, album: Option<String>, 
                                 song: Option<String>, percentage: Option<f32>) {
        // The source parameter is redundant since BasePlayerController creates its own source
        // Just pass the remaining parameters to the base method
        self.base.notify_database_update(artist, album, song, percentage);
    }
    
    /// Initialize the MPD library with retry logic
    /// 
    /// This method attempts to initialize the library and will retry with exponential backoff
    /// if the initial connection fails. The retry intervals are: 1s, 2s, 4s, 8s, 15s, 30s, 60s
    /// 
    /// # Arguments
    /// * `player_arc` - Arc reference to the player controller
    /// * `running` - Atomic boolean to check if the player is still running
    fn initialize_library_with_retry(player_arc: Arc<Self>, running: Arc<AtomicBool>) {
        info!("Starting MPD library initialization with retry logic");
        
        // Run in a separate thread to avoid blocking the main startup
        thread::spawn(move || {
            let mut retry_handler = RetryHandler::connection_retry();
              let library_initialized = retry_handler.execute_with_retry(
                || {
                    // Check if we should stop due to shutdown signal
                    if !running.load(Ordering::SeqCst) {
                        debug!("Library initialization interrupted by shutdown signal");
                        return None;
                    }
                    
                    debug!("Attempting to initialize MPD library");
                    
                    // Try to connect to MPD to test connectivity
                    if let Some(_client) = player_arc.get_fresh_client() {
                        info!("Successfully connected to MPD, initializing library");
                        
                        // Import MPDLibrary here to ensure it's available
                        use crate::players::mpd::library::MPDLibrary;
                        
                        // Create a library with the same connection parameters
                        let library = MPDLibrary::with_connection(
                            &player_arc.hostname, 
                            player_arc.port, 
                            player_arc.clone()
                        );
                        
                        // Store the library in the controller
                        if let Ok(mut library_guard) = player_arc.library.lock() {
                            *library_guard = Some(library.clone());
                            debug!("Library instance stored in controller");
                        } else {
                            warn!("Failed to store library in controller");
                            return None;
                        }
                        
                        // Start the library refresh in the current thread
                        info!("Starting MPD library refresh...");
                        match library.refresh_library() {
                            Ok(_) => {
                                info!("MPD library loaded successfully");
                                Some(()) // Success
                            },
                            Err(e) => {
                                warn!("Failed to refresh MPD library: {}", e);
                                None // Failed, will retry
                            }
                        }
                    } else {
                        debug!("Failed to connect to MPD for library initialization");
                        None // Failed, will retry
                    }
                },
                Some(&running),
                "MPD library initialization"
            );
            
            if library_initialized.is_none() {
                warn!("MPD library initialization failed after all retry attempts");
            }
        });
    }
    
    /// Starts a background thread that listens for MPD events
    /// The thread will run until the running flag is set to false
    fn start_event_listener(&self, running: Arc<AtomicBool>, self_arc: Arc<Self>) {
        let hostname = self.hostname.clone();
        let port = self.port;
        
        info!("Starting MPD event listener thread");
        
        // Spawn a new thread for event listening
        thread::spawn(move || {
            info!("MPD event listener thread started");
            Self::run_event_loop(&hostname, port, running, self_arc);
            info!("MPD event listener thread shutting down");
        });
    }

    /// Main event loop for listening to MPD events
    fn run_event_loop(hostname: &str, port: u16, running: Arc<AtomicBool>, player_arc: Arc<Self>) {
        while running.load(Ordering::SeqCst) {
            // Try to establish a connection for idle mode
            let idle_addr = format!("{}:{}", hostname, port);
            let idle_client = match Client::connect(&idle_addr) {
                Ok(client) => {
                    debug!("Connected to MPD for idle listening at {}", idle_addr);
                    player_arc.reset_reconnect_attempts(); // Reset counter on successful connection
                    client
                },
                Err(e) => {
                    warn!("Failed to connect to MPD for idle mode: {}", e);
                    
                    // Increment attempt counter and check if we should give up
                    let attempts = player_arc.increment_reconnect_attempts();
                    let max_attempts = player_arc.get_max_reconnect_attempts();
                    
                    if attempts >= max_attempts {
                        error!("Failed to connect to MPD after {} attempts, giving up", attempts);
                        player_arc.disable_connections(); // Mark connections as disabled
                        break; // Exit the loop and stop trying
                    }
                    
                    info!("Will attempt to reconnect in 5 seconds (attempt {}/{})", attempts, max_attempts);
                    Self::wait_for_reconnect(&running);
                    continue;
                }
            };
            
            // Process events until connection fails or shutdown requested
            Self::process_events(idle_client, &running, &player_arc);
            
            // If we get here, either there was a connection error or the connection was lost
            if running.load(Ordering::SeqCst) {
                // Only wait for reconnect if we haven't exceeded the limit yet
                let attempts = player_arc.increment_reconnect_attempts();
                let max_attempts = player_arc.get_max_reconnect_attempts();
                
                if attempts >= max_attempts {
                    error!("Connection lost and maximum reconnection attempts ({}) reached, giving up", max_attempts);
                    player_arc.disable_connections(); // Mark connections as disabled
                    break;
                }
                
                info!("Connection lost, will attempt to reconnect in 5 seconds (attempt {}/{})", attempts, max_attempts);
                Self::wait_for_reconnect(&running);
            }
        }
    }
    
    /// Process MPD events until connection fails or shutdown requested
    fn process_events(mut idle_client: Client<TcpStream>, 
                     running: &Arc<AtomicBool>, player: &Arc<Self>) {
        while running.load(Ordering::SeqCst) {
            let subsystems = match idle_client.idle(&[
                Subsystem::Player,
                Subsystem::Mixer,
                Subsystem::Options,
                Subsystem::Playlist,
                Subsystem::Database,
                Subsystem::Update,
            ]) {
                Ok(subs) => subs,
                Err(e) => {
                    warn!("MPD idle error: {}", e);
                    // Connection may have been lost, break out to try reconnecting
                    break;
                }
            };
            
            // Get the subsystems that changed
            let events = match subsystems.get() {
                Ok(events) => events,
                Err(e) => {
                    warn!("Error getting MPD events: {}", e);
                    continue;
                }
            };
            
            if events.is_empty() {
                continue;
            }
            
            // Convert to a format we can log
            let events_str: Vec<String> = events.iter()
                .map(|s| format!("{:?}", s))
                .collect();
            
            info!("Received MPD events: {}", events_str.join(", "));
            
            // Create a fresh command connection for handling events
            if let Some(mut cmd_client) = player.get_fresh_client() {
                // Process each subsystem event with our fresh connection
                for subsystem in events {
                    Self::handle_subsystem_event(subsystem, &mut cmd_client, player.clone());
                }
            } else {
                warn!("Failed to create command connection for event processing");
            }
        }
    }
    
    /// Handle a specific MPD subsystem event
    fn handle_subsystem_event(subsystem: Subsystem, client: &mut Client<TcpStream>, player: Arc<Self>) {
        // mark player as alive
        player.base.alive();

        match subsystem {
            Subsystem::Player => {
                debug!("Player state changed");
                // Pass the existing client connection to reuse it
                Self::handle_player_event(client, player);
            },
            Subsystem::Playlist => {
                warn!("Playlist changed");
                // Could notify about playlist/song changes
            },
            Subsystem::Options => {
                warn!("Options changed (repeat, random, etc.)");
                // Could query and notify about repeat/random state
            },
            Subsystem::Mixer => {
                debug!("Mixer changed (volume)");
            },
            Subsystem::Database => {
                debug!("Database changed, refreshing library");
                // Refresh the library if it's available
                if let Some(library) = player.get_library() {
                    // Run the refresh in a separate thread to avoid blocking the event handler
                    let library_clone = library.clone();
                    thread::spawn(move || {
                        match library_clone.refresh_library() {
                            Ok(_) => info!("MPD library refreshed successfully after database change"),
                            Err(e) => warn!("Failed to refresh MPD library after database change: {}", e),
                        }
                    });
                }
            },
            Subsystem::Update => {
                debug!("MPD database update status changed");
                // Get fresh status to check the current update state
                if let Some(mut status_client) = player.get_fresh_client() {
                    match status_client.status() {
                        Ok(status) => {
                            player.check_database_update_status(&status);
                        },
                        Err(e) => {
                            warn!("Failed to get MPD status for update event: {}", e);
                        }
                    }
                } else {
                    warn!("Failed to get client for update status check");
                }
            },
            _ => {
                debug!("Other subsystem changed: {:?}", subsystem);
            }
        }
    }
    
    /// Handle player events and log song information
    fn handle_player_event(client: &mut Client<TcpStream>, player: Arc<Self>) {

        // Update the song information and capabilities
        Self::update_song_from_mpd(client, player.clone());
        
        // Get and update the player state
        match client.status() {
            Ok(status) => {
                info!("Player status: {:?}, volume: {}%", 
                    status.state, status.volume);
                
                // Convert MPD state to our PlaybackState
                let player_state = match status.state {
                    mpd::State::Play => PlaybackState::Playing,
                    mpd::State::Pause => PlaybackState::Paused,
                    mpd::State::Stop => PlaybackState::Stopped,
                };
                
                // Notify listeners about the state change
                debug!("MPDPlayerController forwarding state change notification: {}", player_state);
                player.base.notify_state_changed(player_state);
            },
            Err(e) => {
                warn!("Failed to get player status: {}", e);
                // In case of error, assume stopped state
                player.base.notify_state_changed(PlaybackState::Stopped);
            }
        }
    }
    
    /// Wait for a short period before attempting to reconnect
    fn wait_for_reconnect(running: &Arc<AtomicBool>) {
        for _ in 0..50 {
            if !running.load(Ordering::SeqCst) {
                break;
            }
            thread::sleep(Duration::from_millis(100));
        }
    }
    
    /// Enhance a song with cached metadata if available
    fn enhance_song_with_cache(&self, mut song: Song) -> Song {
        // Check if the song has a stream URL that might be in our cache
        if let Some(ref stream_url) = song.stream_url {
            let cache_key = format!("mpd.urlmeta.{}", stream_url);
            
            match attributecache::get::<HashMap<String, serde_json::Value>>(&cache_key) {
                Ok(Some(cached_metadata)) => {
                    debug!("Found cached metadata for URL: {}", stream_url);
                    
                    // Add all cached metadata to the song's metadata
                    for (key, value) in &cached_metadata {
                        song.metadata.insert(key.clone(), value.clone());
                        debug!("Added cached metadata: {} = {:?}", key, value);
                    }
                },
                Ok(None) => {
                    debug!("No cached metadata found for URL: {}", stream_url);
                },
                Err(e) => {
                    debug!("Failed to retrieve cached metadata for URL {}: {}", stream_url, e);
                }
            }
        }
        
        song
    }
    
    /// Update the current song and notify listeners
    fn update_current_song(&self, song: Option<Song>) {
        // Enhance the song with cached metadata if available
        let enhanced_song = song.map(|s| self.enhance_song_with_cache(s));
        
        // Store the new song
        let mut current_song = self.current_song.lock().unwrap();
        let song_changed = match (&*current_song, &enhanced_song) {
            (Some(old), Some(new)) => old.stream_url != new.stream_url || old.title != new.title,
            (None, Some(_)) => true,
            (Some(_), None) => true,
            (None, None) => false,
        };
        
        if song_changed {
            debug!("Updating current song");
            // Update the stored song
            *current_song = enhanced_song.clone();
            
            // Notify listeners of the song change
            drop(current_song); // Release the lock before notifying
            self.base.notify_song_changed(enhanced_song.as_ref());
        }
    }

    /// Create a fresh MPD client connection for sending commands
    /// This creates a new connection each time, rather than reusing an existing one
    pub fn get_fresh_client(&self) -> Option<Client<TcpStream>> {
        // Check if connections have been disabled due to max reconnection attempts
        if self.are_connections_disabled() {
            debug!("MPD connections are disabled due to max reconnection attempts reached");
            return None;
        }
        
        debug!("Creating fresh MPD command connection");
        let addr = format!("{}:{}", self.hostname, self.port);
        
        match Client::connect(&addr) {
            Ok(client) => {
                debug!("Successfully created new MPD command connection");
                // Reset connection attempts on successful connection
                self.reset_reconnect_attempts();
                Some(client)
            },
            Err(e) => {
                warn!("Failed to create MPD command connection: {}", e);
                None
            }
        }
    }
    
    /// Update player state and capabilities based on the current MPD status
    /// 
    /// Updates the PlayerState object with current information from MPD including:
    /// - Playback state (playing/paused/stopped)
    /// - Volume
    /// - Loop mode
    /// - Shuffle status
    /// - Current position
    /// - Available capabilities (Next/Previous/Seek)
    /// 
    /// Returns an updated song with lyrics metadata if applicable
    fn update_state_and_capabilities_from_mpd(client: &mut Client<TcpStream>, player: Arc<Self>, song: Option<Song>) -> Option<Song> {
        debug!("Updating player state and capabilities based on MPD status");
        
        let updated_song = song;
        
        // Try to get current status to determine playlist position and other state info
        match client.status() {
            Ok(status) => {
                // Get a lock on the current_state to update it
                if let Ok(mut current_state) = player.current_state.lock() {
                    // Update playback state
                    current_state.state = match status.state {
                        mpd::State::Play => PlaybackState::Playing,
                        mpd::State::Pause => PlaybackState::Paused,
                        mpd::State::Stop => PlaybackState::Stopped,
                    };
                    debug!("Updated player state: {:?}", current_state.state);
                    
                    // Update volume if available (MPD returns -1 for no volume control)
                    if status.volume >= 0 {
                        current_state.volume = Some(status.volume as i32);
                        debug!("Updated volume: {}%", status.volume);
                    }
                    
                    // Update loop mode based on MPD repeat and single flags
                    current_state.loop_mode = if status.repeat {
                        if status.single {
                            LoopMode::Track
                        } else {
                            LoopMode::Playlist
                        }
                    } else {
                        LoopMode::None
                    };
                    debug!("Updated loop mode: {:?}", current_state.loop_mode);
                    
                    // Update shuffle status
                    current_state.shuffle = status.random;
                    debug!("Updated shuffle: {}", status.random);
                    
                    // Update playback position if available
                    if let Some(elapsed) = status.elapsed {
                        current_state.position = Some(elapsed.as_secs_f64());
                        debug!("Updated position: {:.1}s", elapsed.as_secs_f64());
                    }
                    
                    // Store current song information in player metadata if available
                    if let Some(sng) = &updated_song {
                        let mut metadata = HashMap::new();
                        
                        if let Some(duration) = sng.duration {
                            if let Some(num) = serde_json::Number::from_f64(duration) {
                                metadata.insert("duration".to_string(), serde_json::Value::Number(num));
                            }
                        }
                        
                        if let Some(track) = sng.track_number {
                            metadata.insert("track".to_string(), serde_json::Value::Number(serde_json::Number::from(track)));
                        }
                        
                        // Queue status info
                        metadata.insert("queue_length".to_string(), serde_json::Value::Number(serde_json::Number::from(status.queue_len)));
                        
                        if let Some(song_id) = status.song.map(|s| s.id) {
                            // Convert the mpd::Id to a number that can be stored in metadata
                            let id_value = song_id.0; // Access the inner numeric value directly
                            metadata.insert("song_id".to_string(), serde_json::Value::Number(serde_json::Number::from(id_value)));
                        }
                        
                        if let Some(song_pos) = status.song.map(|s| s.pos) {
                            metadata.insert("queue_position".to_string(), serde_json::Value::Number(serde_json::Number::from(song_pos)));
                        }
                        
                        // Update metadata in state
                        current_state.metadata = metadata;
                    }
                } else {
                    warn!("Failed to acquire lock on player state for updating");
                }
                
                // Total songs in playlist
                let queue_len = status.queue_len;
                
                // Current song position (0-indexed)
                let current_pos = status.song.map(|s| s.pos).unwrap_or(0);
                
                // Check if we have a next song
                let has_next = current_pos + 1 < queue_len;
                
                // Check if we have a previous song
                let has_previous = current_pos > 0;
                
                // Check if player is stopped - if so, disable stop/next/previous buttons
                let is_stopped = status.state == mpd::State::Stop;
                
                debug!("Playlist status: position {}/{}, has_next={}, has_previous={}, is_stopped={}", 
                       current_pos, queue_len, has_next, has_previous, is_stopped);
                
                // Update capabilities without sending notifications yet
                let mut capabilities_changed = false;
                
                // Update Next capability if needed - disable when stopped
                capabilities_changed |= player.base.set_capability(
                    PlayerCapability::Next, 
                    has_next && !is_stopped, 
                    false // Don't notify yet
                );
                
                // Update Previous capability if needed - disable when stopped
                capabilities_changed |= player.base.set_capability(
                    PlayerCapability::Previous, 
                    has_previous && !is_stopped, 
                    false // Don't notify yet
                );
                
                // Update Stop capability - disable when already stopped
                capabilities_changed |= player.base.set_capability(
                    PlayerCapability::Stop,
                    !is_stopped,
                    false // Don't notify yet
                );

                // Check if the current song is seekable
                let is_seekable = match &updated_song {
                    Some(current_song) => {
                        // Check if the song has a duration
                        if let Some(duration) = current_song.duration {
                            // Check if the file is not a streaming URL
                            // Common streaming URLs start with http://, https://, or contain specific keywords
                            let file_path = current_song.stream_url.as_deref().unwrap_or("");
                            let is_stream = file_path.starts_with("http://") ||
                                           file_path.starts_with("https://") ||
                                           file_path.contains("://") ;
                            
                            // Seekable if it has duration and is not a stream
                            let seekable = duration > 0.0 && !is_stream;
                            debug!("Song seekability check: duration={:?}s, is_stream={}, seekable={}", 
                                  duration, is_stream, seekable);
                            seekable
                        } else {
                            debug!("Song has no duration, not seekable");
                            false
                        }
                    },
                    None => {
                        debug!("No current song, marking as not seekable");
                        false
                    }
                };
                
                // Update Seek capability based on our assessment
                capabilities_changed |= player.base.set_capability(
                    PlayerCapability::Seek,
                    is_seekable,
                    false // Don't notify yet
                );
                
                // Update capabilities with a single notification
                if capabilities_changed {
                    let current_caps = player.base.get_capabilities();
                    player.base.notify_capabilities_changed(&current_caps);
                    debug!("Player capabilities updated: Next={}, Previous={}, Stop={}, Seek={}", 
                          has_next && !is_stopped, has_previous && !is_stopped, !is_stopped, is_seekable);
                }
            },
            Err(e) => {
                warn!("Failed to get MPD status for player state and capability update: {}", e);
                
                // If we can't get status, disable navigation capabilities
                let mut capabilities_changed = false;
                
                capabilities_changed |= player.base.set_capability(
                    PlayerCapability::Next, 
                    false, 
                    false // Don't notify yet
                );
                
                capabilities_changed |= player.base.set_capability(
                    PlayerCapability::Previous, 
                    false, 
                    false // Don't notify yet
                );
                
                capabilities_changed |= player.base.set_capability(
                    PlayerCapability::Stop,
                    false,
                    false // Don't notify yet
                );

                // Also disable seek capability when there's an error
                capabilities_changed |= player.base.set_capability(
                    PlayerCapability::Seek,
                    false,
                    false // Don't notify yet
                );
                
                if capabilities_changed {
                    let current_caps = player.base.get_capabilities();
                    player.base.notify_capabilities_changed(&current_caps);
                    debug!("Player capabilities updated: disabled Next/Previous/Stop/Seek due to error");
                }
                
                // Update state to reflect error condition
                if let Ok(mut current_state) = player.current_state.lock() {
                    current_state.state = PlaybackState::Stopped;
                }
            }
        }
        
        updated_song
    }
    
    /// Convert an MPD song to our Song format
    fn convert_mpd_song(mpd_song: mpd::Song, player_arc: Option<Arc<Self>>) -> Song {
        // Generate cover art URL using the file path/URI from MPD song
        let cover_url = if !mpd_song.file.is_empty() {
            // Try to use encoded URL if library is available
            if let Some(player) = &player_arc {
                if let Ok(library_guard) = player.library.lock() {
                    if let Some(library) = library_guard.as_ref() {
                        // Use the library's create_encoded_image_url method
                        Some(library.create_encoded_image_url(&mpd_song.file))
                    } else {
                        // Fallback to base64 encoded URL if library not available
                        Some(format!("{}/{}", mpd_image_url(), url_encoding::encode_url_safe(&mpd_song.file)))
                    }
                } else {
                    // Fallback to base64 encoded URL if can't access library
                    Some(format!("{}/{}", mpd_image_url(), url_encoding::encode_url_safe(&mpd_song.file)))
                }
            } else {
                // Fallback to base64 encoded URL if no player provided
                Some(format!("{}/{}", mpd_image_url(), url_encoding::encode_url_safe(&mpd_song.file)))
            }
        } else {
            None
        };
        
        // Extract album from tags
        let album = mpd_song.tags.iter()
            .find(|(tag, _)| tag == "Album")
            .map(|(_, value)| value.clone());
            
        // Extract album artist from tags
        let album_artist = mpd_song.tags.iter()
            .find(|(tag, _)| tag == "AlbumArtist")
            .map(|(_, value)| value.clone());
            
        // Extract genre from tags
        let genre = mpd_song.tags.iter()
            .find(|(tag, _)| tag == "Genre")
            .map(|(_, value)| value.clone());
        
        // Handle title splitting for radio stations
        let (final_title, final_artist) = if mpd_song.artist.is_none() && mpd_song.title.is_some() {
            // No artist but has title - try to split it (common for web radio)
            let title_str = mpd_song.title.as_ref().unwrap();
            
            if let Some(player) = &player_arc {
                // Use the song URL as the splitter ID for radio stations
                let splitter_id = &mpd_song.file;
                
                // Try to split the title using the manager
                if let Some((artist, song)) = player.song_split_manager.split_song(splitter_id, title_str) {
                    debug!("Split title '{}' into artist='{}', song='{}'", title_str, artist, song);
                    
                    // Save the splitter state after successful split
                    if let Err(e) = player.song_split_manager.save(splitter_id) {
                        debug!("Failed to save splitter state for '{}': {}", splitter_id, e);
                    }
                    
                    (Some(song), Some(artist))
                } else {
                    debug!("Could not split title '{}', keeping as-is", title_str);
                    (mpd_song.title.clone(), mpd_song.artist.clone())
                }
            } else {
                // No player reference, can't split
                (mpd_song.title.clone(), mpd_song.artist.clone())
            }
        } else {
            // Artist exists or no title, use as-is
            (mpd_song.title.clone(), mpd_song.artist.clone())
        };
            
        Song {
            title: final_title,
            artist: final_artist,
            album,
            album_artist,
            track_number: mpd_song.place.as_ref().map(|p| p.pos as i32),
            total_tracks: None,
            duration: mpd_song.duration.map(|d| d.as_secs_f32() as f64),
            genre: genre.clone(),
            genres: genre.map(|g| vec![g]).unwrap_or_default(),
            year: None,
            cover_art_url: cover_url,
            stream_url: Some(mpd_song.file.clone()),
            source: Some("mpd".to_string()),
            liked: None,
            composer: None,
            metadata: HashMap::new(),
        }
    }
    
    /// Update the player's current song from MPD
    fn update_song_from_mpd(client: &mut Client<TcpStream>, player: Arc<Self>) {
        // Variable to store the obtained song for later use in updating capabilities
        let mut obtained_song: Option<Song> = None;
        
        // Use the provided client connection
        match client.currentsong() {
            Ok(song_opt) => {
                if let Some(mpd_song) = song_opt {
                    // Convert MPD song to our Song format
                    let mut song = Self::convert_mpd_song(mpd_song, Some(player.clone()));
                    
                    // Check for lyrics and add to song metadata
                    if let Some(library) = player.get_library() {
                        if let Some(music_dir) = library.get_music_directory() {
                            use crate::helpers::lyrics::{MPDLyricsProvider, LyricsProvider};
                            let lyrics_provider = MPDLyricsProvider::new(music_dir.clone());
                            
                            // Try to find lyrics by URL (file path) from stream_url
                            let has_lyrics = if let Some(file_path) = &song.stream_url {
                                debug!("Checking for lyrics for file: {}", file_path);
                                match lyrics_provider.get_lyrics_by_url(file_path) {
                                    Ok(_) => {
                                        debug!("Found lyrics for: {}", file_path);
                                        true
                                    },
                                    Err(e) => {
                                        debug!("No lyrics found for {}: {:?}", file_path, e);
                                        false
                                    }
                                }
                            } else {
                                debug!("No stream_url available for lyrics check");
                                false
                            };
                            
                            if has_lyrics {
                                song.metadata.insert("lyrics_available".to_string(), serde_json::Value::Bool(true));
                                debug!("Added lyrics_available=true to song metadata");
                                
                                // Add API endpoint for lyrics by song ID
                                if let (Some(artist), Some(title), Some(file_path)) = (&song.artist, &song.title, &song.stream_url) {
                                    // Use the encoded file path as the song ID for the lyrics API
                                    let encoded_file_path = url_encoding::encode_url_safe(file_path);
                                    let lyrics_url = format!("{}/lyrics/mpd/{}", crate::constants::API_PREFIX, encoded_file_path);
                                    song.metadata.insert("lyrics_url".to_string(), serde_json::Value::String(lyrics_url));
                                    debug!("Added lyrics_url with song ID to metadata: {}", encoded_file_path);
                                    
                                    // Also add the metadata that can be used for the POST request
                                    let mut lyrics_metadata = serde_json::Map::new();
                                    lyrics_metadata.insert("artist".to_string(), serde_json::Value::String(artist.clone()));
                                    lyrics_metadata.insert("title".to_string(), serde_json::Value::String(title.clone()));
                                    
                                    if let Some(duration) = song.duration {
                                        lyrics_metadata.insert("duration".to_string(), serde_json::Value::Number(serde_json::Number::from_f64(duration).unwrap_or(serde_json::Number::from(0))));
                                    }
                                    
                                    if let Some(album) = &song.album {
                                        lyrics_metadata.insert("album".to_string(), serde_json::Value::String(album.clone()));
                                    }
                                    
                                    song.metadata.insert("lyrics_metadata".to_string(), serde_json::Value::Object(lyrics_metadata));
                                    debug!("Added lyrics_metadata object to song metadata");
                                }
                            } else {
                                song.metadata.insert("lyrics_available".to_string(), serde_json::Value::Bool(false));
                                debug!("Added lyrics_available=false to song metadata");
                            }
                        }
                    }
                    
                    info!("Now playing: {} - {}", 
                        song.title.as_deref().unwrap_or("Unknown"),
                        song.artist.as_deref().unwrap_or("Unknown"));
                    
                    debug!("Song metadata contains {} entries: {:?}", song.metadata.len(), song.metadata.keys().collect::<Vec<_>>());
                    
                    // Log additional song details if available
                    if let Some(duration) = song.duration {
                        debug!("Duration: {:.1} seconds", duration);
                    }
                    if let Some(track) = song.track_number {
                        debug!("Position: {} in queue", track);
                    }
                    
                    // Store the song for capability update (with lyrics metadata already added)
                    obtained_song = Some(song.clone());
                } else {
                    info!("No song currently playing");
                }
            },
            Err(e) => warn!("Failed to get current song information: {}", e),
        }
        
        // Update player capabilities based on the current playlist state and the song we just got
        let final_song = Self::update_state_and_capabilities_from_mpd(client, player.clone(), obtained_song);
        
        // Update stored song and notify listeners with the final version
        player.update_current_song(final_song);
    }

    /// Add a song URL to the MPD queue
    /// 
    /// # Arguments
    /// * `url` - The URL/path of the song to add
    /// * `at_beginning` - If Some(true), insert at the beginning of the queue, otherwise append to the end
    /// 
    /// # Returns
    /// * `bool` - true if the operation was successful, false otherwise
    pub fn queue_url(&self, url: &str, at_beginning: Option<bool>) -> bool {
        debug!("Adding URL to queue: {}, at_beginning: {:?}", url, at_beginning);
        
        if let Some(mut client) = self.get_fresh_client() {
            // Use the appropriate method based on whether to add at beginning or end
            let result = if at_beginning.unwrap_or(false) {
                // Insert at position 0 (beginning of queue)
                debug!("Inserting track at position 0: {}", url);
                // Create a song path that mpd library can use
                let song_path = mpd::Song {
                    file: url.to_string(),
                    ..Default::default()
                };
                client.insert(&song_path, 0)
            } else {
                // Push to the end of the queue
                debug!("Pushing track to end of queue: {}", url);
                // Create a song path that mpd library can use
                let song_path = mpd::Song {
                    file: url.to_string(),
                    ..Default::default()
                };
                client.push(&song_path).map(|_id| 0) // Convert Result<Id, Error> to Result<usize, Error>
            };
            
            match result {
                Ok(_) => {
                    debug!("Successfully added URL to queue: {}", url);
                    return true;
                },
                Err(e) => {
                    warn!("Failed to add URL to queue: {} - Error: {}", url, e);
                    return false;
                }
            }
        } else {
            warn!("Failed to get MPD client connection for queue_url");
            return false;
        }
    }
}

/// Structure to store player state for each instance
struct PlayerInstanceData {
    running_flag: Arc<AtomicBool>
}

/// A map to store running state for each player instance
type PlayerStateMap = HashMap<usize, PlayerInstanceData>;
lazy_static! {
    static ref PLAYER_STATE: Mutex<PlayerStateMap> = Mutex::new(HashMap::new());
}

impl MPDPlayerController {
    /// Check MPD database update status and manage background job
    fn check_database_update_status(&self, status: &mpd::Status) {
        let job_id = "mpd_database_update";
        
        // Check if MPD is currently updating the database
        let is_updating = status.updating_db.is_some();
        
        if let Ok(mut current_job_guard) = self.current_update_job_id.lock() {
            let has_active_job = current_job_guard.is_some();
            
            if is_updating && !has_active_job {
                // MPD started updating and we don't have an active job - start one
                match BackgroundJobs::instance().register_job(
                    job_id.to_string(), 
                    "MPD Database Update".to_string()
                ) {
                    Ok(_) => {
                        info!("Started background job for MPD database update");
                        *current_job_guard = Some(job_id.to_string());
                    },
                    Err(e) => {
                        warn!("Failed to register MPD database update job: {}", e);
                    }
                }
            } else if !is_updating && has_active_job {
                // MPD finished updating and we have an active job - finish it
                if let Some(active_job_id) = current_job_guard.take() {
                    if let Err(e) = BackgroundJobs::instance().complete_job(&active_job_id) {
                        warn!("Failed to complete MPD database update job: {}", e);
                    } else {
                        info!("Finished background job for MPD database update");
                    }
                }
            } else if is_updating && has_active_job {
                // MPD is still updating and we have an active job - update progress if available
                if let Some(update_id) = status.updating_db {
                    let progress_msg = format!("Updating database (job {})", update_id);
                    if let Err(e) = BackgroundJobs::instance().update_job(job_id, Some(progress_msg), None, None) {
                        debug!("Failed to update MPD database update job progress: {}", e);
                    }
                }
            }
        } else {
            warn!("Failed to acquire lock on current update job ID");
        }
    }
}

impl PlayerController for MPDPlayerController {
    delegate! {
        to self.base {
            fn get_capabilities(&self) -> PlayerCapabilitySet;
            fn get_last_seen(&self) -> Option<std::time::SystemTime>;
        }
    }
    
    fn get_song(&self) -> Option<Song> {
        debug!("Getting current song from stored value");
        // Return a clone of the stored song with any fresh cache enhancements
        let song_clone = self.current_song.lock().unwrap().clone();
        if let Some(song) = song_clone {
            // Apply fresh cache enhancements in case cache was updated after the song was stored
            let enhanced_song = self.enhance_song_with_cache(song);
            debug!("Returning song with {} metadata entries: {:?}", 
                   enhanced_song.metadata.len(), 
                   enhanced_song.metadata.keys().collect::<Vec<_>>());
            Some(enhanced_song)
        } else {
            debug!("No current song available");
            None
        }
    }
    
    fn get_loop_mode(&self) -> LoopMode {
        trace!("MPDController: get_loop_mode called");
        if let Some(mut mpd_client) = self.get_fresh_client() {
            if let Ok(status) = mpd_client.status() {
                return match (status.repeat, status.single) {
                    (true, true) => LoopMode::Track,
                    (true, false) => LoopMode::Playlist,
                    _ => LoopMode::None,
                };
            }
        }
        debug!("Failed to get loop mode from MPD");
        LoopMode::None
    }
    
    fn get_playback_state(&self) -> PlaybackState {
        trace!("MPDController: get_playback_state called");
        if let Some(mut mpd_client) = self.get_fresh_client() {
            if let Ok(status) = mpd_client.status() {
                match status.state {
                    mpd::State::Play => return PlaybackState::Playing,
                    mpd::State::Pause => return PlaybackState::Paused,
                    mpd::State::Stop => return PlaybackState::Stopped,
                }
            }
        }
        debug!("Failed to get state from MPD");
        PlaybackState::Unknown
    }
    
    fn get_position(&self) -> Option<f64> {
        trace!("MPDController: get_position called");
        if let Some(mut mpd_client) = self.get_fresh_client() {
            if let Ok(status) = mpd_client.status() {
                if let Some(elapsed) = status.elapsed {
                    // Convert Duration to f64 seconds
                    return Some(elapsed.as_secs_f64());
                }
            }
        }
        debug!("Failed to get position from MPD");
        None
    }
    
    fn get_shuffle(&self) -> bool {
        trace!("MPDController: get_shuffle called");
        if let Some(mut mpd_client) = self.get_fresh_client() {
            if let Ok(status) = mpd_client.status() {
                return status.random;
            }
        }
        debug!("Failed to get shuffle status from MPD");
        false
    }
    
    fn get_player_name(&self) -> String {
        "mpd".to_string()
    }
    
    fn get_aliases(&self) -> Vec<String> {
        vec!["mpd".to_string()]
    }
    
    fn get_player_id(&self) -> String {
        format!("{}:{}", self.hostname, self.port)
    }
    
    fn send_command(&self, command: PlayerCommand) -> bool {
        info!("Sending command to MPD: {}", command);
        
        let mut success = false;
        
        // Create a fresh connection for each command
        if let Some(mut client) = self.get_fresh_client() {
            // Process the command based on its type
            match command {
                PlayerCommand::Play => {
                    // Start playback
                    success = client.play().is_ok();
                    if success {
                        debug!("MPD playback started");
                    }
                },
                
                PlayerCommand::Pause => {
                    // Pause playback
                    success = client.pause(true).is_ok();
                    if success {
                        debug!("MPD playback paused");
                    }
                },
                
                PlayerCommand::PlayPause => {
                    // Toggle between play and pause
                    match client.status() {
                        Ok(status) => {
                            match status.state {
                                mpd::State::Play => {
                                    success = client.pause(true).is_ok();
                                    if success {
                                        debug!("MPD playback paused (toggle)");
                                    }
                                },
                                _ => {
                                    success = client.play().is_ok();
                                    if success {
                                        debug!("MPD playback started (toggle)");
                                    }
                                }
                            }
                        },
                        Err(e) => {
                            warn!("Failed to get MPD status for play/pause toggle: {}", e);
                        }
                    }
                },
                
                PlayerCommand::Stop => {
                    // Stop playback
                    success = client.stop().is_ok();
                    if success {
                        debug!("MPD playback stopped");
                    } else {
                        warn!("Failed to stop MPD playback");
                    }
                },
                
                PlayerCommand::Next => {
                    // Skip to next track
                    success = client.next().is_ok();
                    if success {
                        debug!("Skipped to next track in MPD");
                    }
                },
                
                PlayerCommand::Previous => {
                    // Go back to previous track
                    success = client.prev().is_ok();
                    if success {
                        debug!("Went back to previous track in MPD");
                    }
                },
                
                PlayerCommand::SetLoopMode(mode) => {
                    // Map our loop mode to MPD repeat/single settings
                    match mode {
                        LoopMode::None => {
                            // Turn off both repeat and single
                            let repeat_ok = client.repeat(false).is_ok();
                            let single_ok = client.single(false).is_ok();
                            success = repeat_ok && single_ok;
                            if success {
                                debug!("MPD loop mode set to None");
                            }
                        },
                        LoopMode::Track => {
                            // Single track repeat (single=true)
                            let repeat_ok = client.repeat(true).is_ok();
                            let single_ok = client.single(true).is_ok();
                            success = repeat_ok && single_ok;
                            if success {
                                debug!("MPD loop mode set to Track (single repeat)");
                            }
                        },
                        LoopMode::Playlist => {
                            // Whole playlist repeat (repeat=true, single=false)
                            let repeat_ok = client.repeat(true).is_ok();
                            let single_ok = client.single(false).is_ok();
                            success = repeat_ok && single_ok;
                            if success {
                                debug!("MPD loop mode set to Playlist (whole playlist repeat)");
                            }
                        },
                    }
                },
                
                PlayerCommand::Seek(position) => {
                    // Seek to a position in seconds
                    match client.currentsong() {
                        Ok(song_opt) => {
                            if let Some(song) = song_opt {
                                if let Some(place) = song.place {
                                    // Use the song's position in the queue
                                    // Position needs to be f64 to satisfy ToSeconds trait
                                    let position_seconds: f64 = position; 
                                    success = client.seek(place.pos, position_seconds).is_ok();
                                    if success {
                                        debug!("Sought to position {}s in current track", position);
                                    }
                                } else {
                                    warn!("Current song has no position, cannot seek");
                                }
                            } else {
                                warn!("No current song to seek in");
                            }
                        },
                        Err(e) => {
                            warn!("Failed to get current song for seeking: {}", e);
                        }
                    }
                },
                
                PlayerCommand::SetRandom(enabled) => {
                    // Set shuffle/random mode
                    success = client.random(enabled).is_ok();
                    if success {
                        debug!("MPD random mode set to: {}", enabled);
                    }
                },
                
                PlayerCommand::Kill => {
                    // Kill the MPD process via the kill command
                    // Note: this requires the MPD server to have proper permissions configured
                    success = client.kill().is_ok();
                    if success {
                        debug!("MPD kill command sent successfully");
                        
                        // Stop the player controller since MPD process is now killed
                        self.stop();
                    } else {
                        warn!("Failed to kill MPD process, might not have permission");
                    }
                },
                
                PlayerCommand::QueueTracks { uris, insert_at_beginning, metadata } => {
                    debug!("Adding {} tracks to MPD queue at {}", uris.len(), 
                          if insert_at_beginning { "beginning" } else { "end" });
                    
                    if uris.is_empty() {
                        debug!("No URIs provided to queue");
                        success = true; // Nothing to do, but not an error
                    } else {
                        let mut all_success = true;
                        
                        // Process each URI with its metadata using our new queue_url function
                        for (i, uri) in uris.iter().enumerate() {
                            // Get metadata for this URI if available
                            let track_metadata = metadata.get(i).and_then(|m| m.as_ref());
                            
                            // Store metadata in cache if provided
                            if let Some(meta) = track_metadata {
                                if !meta.metadata.is_empty() {
                                    debug!("Caching metadata for URI {}: {:?}", 
                                           uri, meta.metadata);
                                    let cache_key = format!("mpd.urlmeta.{}", uri);
                                    
                                    match attributecache::set(&cache_key, &meta.metadata) {
                                        Ok(_) => {
                                            debug!("Successfully cached metadata for URI: {}", uri);
                                        },
                                        Err(e) => {
                                            warn!("Failed to cache metadata for URI {}: {}", uri, e);
                                        }
                                    }
                                }
                            }
                            
                            let result = self.queue_url(uri, Some(insert_at_beginning));
                            if !result {
                                all_success = false;
                            }
                        }
                        
                        success = all_success;
                    }
                    
                    if success {
                        debug!("Successfully added all tracks to MPD queue");
                    } else {
                        warn!("Failed to add some or all tracks to MPD queue");
                    }
                },
                    
                PlayerCommand::RemoveTrack(position) => {
                    debug!("Removing track at position {} from MPD queue", position);
                    
                    // Remove the track at the specified position
                    let result = client.delete(position as u32);
                    
                    if let Err(e) = result {
                        warn!("Failed to remove track at position {}: {}", position, e);
                        success = false;
                    } else {
                        debug!("Successfully removed track at position {}", position);
                        success = true;
                        
                        // Notify listeners that the queue has been modified
                        self.base.notify_queue_changed();
                    }
                },
                  PlayerCommand::ClearQueue => {
                    debug!("Clearing MPD queue");
                    
                    success = client.clear().is_ok();
                    if success {
                        debug!("Successfully cleared MPD queue");
                        
                        // Notify listeners that the queue has been cleared
                        self.base.notify_queue_changed();
                    } else {
                        warn!("Failed to clear MPD queue");
                    }
                },                  PlayerCommand::PlayQueueIndex(index) => {
                    debug!("Playing track at index {} in MPD queue", index);
                    
                    // Use MPD's switch function to start playback from a specific position
                    // This plays the song at the specified position in the playlist (0-based)
                    success = client.switch(index as u32).is_ok();
                    if success {
                        debug!("Started playback of track at position {} in MPD queue", index);
                    } else {
                        warn!("Failed to play track at position {} in MPD queue", index);
                    }
                },
            }
            
            // If the command was successful, we may want to update our stored state
            if success {
                // We'll update our state asynchronously via the MPD idle events
            }
        } else {
            warn!("Cannot send command to MPD: failed to create a fresh connection");
        }
        
        success
    }
    
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn start(&self) -> bool {
        info!("Starting MPD player controller");
        
        // Create a new Arc<Self> for thread-safe sharing of player instance
        let player_arc = Arc::new(self.clone());
        
        // Create a new running flag
        let running = Arc::new(AtomicBool::new(true));
          // Try to get the current song from MPD first
        if let Some(mut client) = self.get_fresh_client() {
            // Initialize song state and capabilities
            info!("Fetching initial song state from MPD");
            Self::update_song_from_mpd(&mut client, player_arc.clone());
            
            // Load MPD library if configured to do so
            if self.load_mpd_library {
                info!("Starting MPD library initialization with retry mechanism");
                
                // Use the new retry mechanism for library initialization
                Self::initialize_library_with_retry(player_arc.clone(), running.clone());
            } else {
                debug!("Skipping MPD library loading (disabled in config)");
            }
        } else {
            warn!("Could not connect to MPD to fetch initial song state");
            
            // Even if we can't connect initially, still try to initialize the library with retry
            // if library loading is enabled
            if self.load_mpd_library {
                info!("Initial MPD connection failed, but starting library initialization with retry mechanism");
                Self::initialize_library_with_retry(player_arc.clone(), running.clone());
            }
        }
        
        // Store the running flag in the MPD player instance
        if let Ok(mut state) = PLAYER_STATE.lock() {
            let instance_id = self as *const _ as usize;
            
            if let Some(data) = state.get(&instance_id) {
                // Stop any existing thread
                data.running_flag.store(false, Ordering::SeqCst);
            }
            
            // Start a new listener thread
            self.start_event_listener(running.clone(), player_arc.clone());
            
            // Store the running flag
            state.insert(instance_id, PlayerInstanceData { running_flag: running });
            true
        } else {
            error!("Failed to acquire lock for player state");
            false
        }
    }
    
    fn stop(&self) -> bool {
        info!("Stopping MPD player controller");
        
        // Signal the event listener thread to stop
        if let Ok(mut state) = PLAYER_STATE.lock() {
            let instance_id = self as *const _ as usize;
            
            if let Some(data) = state.remove(&instance_id) {
                data.running_flag.store(false, Ordering::SeqCst);
                debug!("Signaled event listener thread to stop");
                return true;
            }
        }
        
        debug!("No active event listener thread found");
        false
    }
    
    // Implement the get_library method for MPDPlayerController
    fn get_library(&self) -> Option<Box<dyn LibraryInterface>> {
        if let Some(library) = self.get_library() {
            Some(Box::new(library))
        } else {
            None
        }
    }

    fn get_queue(&self) -> Vec<Track> {
        debug!("MPDController: get_queue called - fetching playlist");
        
        // Get a fresh client connection
        if let Some(mut client) = self.get_fresh_client() {
            // Use the queue method to get all songs in the current queue
            match client.queue() {
                Ok(songs) => {
                    debug!("Retrieved {} songs from MPD queue", songs.len());
                    
                    // Convert MPD songs to our Track format
                    let tracks: Vec<Track> = songs.into_iter()
                        .map(|mpd_song| {
                            // Extract useful information from the song
                            let title = mpd_song.title.unwrap_or_else(|| "Unknown Title".to_string());
                            let artist = mpd_song.artist;
                            
                            // Create a Track with just the name
                            let mut track = Track::with_name(title);
                            
                            // Set artist if available
                            if let Some(artist_name) = artist {
                                track.artist = Some(artist_name);
                            }
                            
                            // Set URI if available
                            if !mpd_song.file.is_empty() {
                                track.uri = Some(mpd_song.file);
                            }
                            
                            track
                        })
                        .collect();
                    
                    return tracks;
                },
                Err(e) => {
                    warn!("Failed to retrieve queue from MPD: {}", e);
                }
            }
        } else {
            warn!("Failed to create MPD client connection for get_queue");
        }
        
        // Return empty vector if anything fails
        Vec::new()
    }

    fn get_meta_keys(&self) -> Vec<String> {
        vec![
            "hostname".to_string(),
            "port".to_string(),
            "connection_status".to_string(),
            "queue_length".to_string(),
            "volume".to_string(),
            "playback_state".to_string(),
            "last_seen".to_string(),
            "stats".to_string(),
            "library_loaded".to_string(),
            "library_loading_progress".to_string(),
            "mpd_version".to_string(),
        ]
    }

    fn get_metadata_value(&self, key: &str) -> Option<String> {
        match key {
            "hostname" => Some(self.hostname.clone()),
            "port" => Some(self.port.to_string()),
            "connection_status" => {
                let connected = self.is_connected();
                Some(if connected { "connected".to_string() } else { "disconnected".to_string() })
            },
            "queue_length" => {
                if let Some(mut client) = self.get_fresh_client() {
                    match client.status() {
                        Ok(status) => Some(status.queue_len.to_string()),
                        Err(_) => Some("0".to_string())
                    }
                } else {
                    Some("0".to_string())
                }
            },
            "mpd_version" => {
                if let Some(client) = self.get_fresh_client() {
                    // Get MPD version from the client and format it as major.minor.patch
                    Some(format!("{}.{}.{}", client.version.0, client.version.1, client.version.2))
                } else {
                    Some("unknown".to_string())
                }
            },
            "volume" => {
                if let Some(mut client) = self.get_fresh_client() {
                    match client.status() {
                        Ok(status) => {
                            if status.volume >= 0 {
                                Some(status.volume.to_string())
                            } else {
                                Some("unknown".to_string())
                            }
                        },
                        Err(_) => Some("unknown".to_string())
                    }
                } else {
                    Some("unknown".to_string())
                }
            },
            "playback_state" => Some(self.get_playback_state().to_string()),
            "last_seen" => {
                if let Some(timestamp) = self.get_last_seen() {
                    let duration = std::time::SystemTime::now()
                        .duration_since(timestamp)
                        .unwrap_or_else(|_| std::time::Duration::from_secs(0));
                    Some(format!("{} seconds ago", duration.as_secs()))
                } else {
                    Some("never".to_string())
                }
            },
            "stats" => {
                if let Some(mut client) = self.get_fresh_client() {
                    match client.stats() {
                        Ok(stats) => {
                            // Format MPD stats as JSON
                            // Note: db_update is not a duration but rather a timestamp
                            Some(serde_json::json!({
                                "artists": stats.artists,
                                "albums": stats.albums,
                                "songs": stats.songs,
                                "uptime": stats.uptime.as_secs(),
                                "db_playtime": stats.db_playtime.as_secs(),
                                "db_update": stats.db_update,
                                "playtime": stats.playtime.as_secs()
                            }).to_string())
                        },
                        Err(_) => Some("{}".to_string())
                    }
                } else {
                    Some("{}".to_string())
                }
            },
            "library_loaded" => {
                // Check if library is loaded
                if let Some(library) = self.get_library() {
                    Some(library.is_loaded().to_string())
                } else {
                    Some("false".to_string())
                }
            },
            "library_loading_progress" => {
                // Get library loading progress
                if let Some(library) = self.get_library() {
                    Some(format!("{:.1}%", library.get_loading_progress() * 100.0))
                } else {
                    Some("0.0%".to_string())
                }
            },
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use tempfile::TempDir;

    /// Test that songs without cached metadata are not affected
    #[test]
    fn test_mpd_no_cached_metadata() {
        // Create a temporary directory for the test cache
        let temp_dir = TempDir::new().expect("Failed to create temp directory");
        
        // Initialize AttributeCache with the temporary directory
        attributecache::AttributeCache::initialize_global(temp_dir.path()).expect("Failed to configure cache");
        
        let mut song = Song::default();
        song.stream_url = Some("http://example.com/not-cached.mp3".to_string());
        song.metadata.insert("existing".to_string(), Value::String("data".to_string()));
        
        // Create an MPD player controller
        let player = MPDPlayerController::with_connection("localhost", 6600);
        
        // Enhance the song (should not find any cached metadata)
        let enhanced_song = player.enhance_song_with_cache(song);
        
        // Verify that no cached metadata was added, but existing metadata is preserved
        assert_eq!(enhanced_song.metadata.len(), 1);
        assert_eq!(enhanced_song.metadata.get("existing"), Some(&Value::String("data".to_string())));
    }
}