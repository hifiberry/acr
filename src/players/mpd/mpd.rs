use crate::players::player_controller::{BasePlayerController, PlayerController, PlayerStateListener};
use crate::data::{PlayerCapability, PlayerCapabilitySet, Song, LoopMode, PlaybackState, PlayerCommand, PlayerState, Track};
use crate::data::library::LibraryInterface;
use crate::constants::API_PREFIX;
use delegate::delegate;
use std::sync::{Arc, Weak, Mutex};
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
use urlencoding;

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
    
    /// Custom artist separators for splitting artist names
    artist_separators: Option<Vec<String>>,
    
    /// MPD library instance wrapped in Arc and Mutex for thread-safe access
    library: Arc<Mutex<Option<crate::players::mpd::library::MPDLibrary>>>,
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
            artist_separators: self.artist_separators.clone(),
            library: Arc::clone(&self.library),
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
            artist_separators: None,
            library: Arc::new(Mutex::new(None)),
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
            artist_separators: None,
            library: Arc::new(Mutex::new(None)),
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
    
    /// Notify all registered listeners that the database is being updated
    pub fn notify_database_update(&self, artist: Option<String>, album: Option<String>, 
                                 song: Option<String>, percentage: Option<f32>) {
        // The source parameter is redundant since BasePlayerController creates its own source
        // Just pass the remaining parameters to the base method
        self.base.notify_database_update(artist, album, song, percentage);
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
                    client
                },
                Err(e) => {
                    warn!("Failed to connect to MPD for idle mode: {}", e);
                    Self::wait_for_reconnect(&running);
                    continue;
                }
            };
            
            // Process events until connection fails or shutdown requested
            Self::process_events(idle_client, &running, &player_arc);
            
            // If we get here, either there was a connection error or the connection was lost
            if running.load(Ordering::SeqCst) {
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
        info!("Will attempt to reconnect in 5 seconds");
        for _ in 0..50 {
            if !running.load(Ordering::SeqCst) {
                break;
            }
            thread::sleep(Duration::from_millis(100));
        }
    }
    
    /// Update the current song and notify listeners
    fn update_current_song(&self, song: Option<Song>) {
        // Store the new song
        let mut current_song = self.current_song.lock().unwrap();
        let song_changed = match (&*current_song, &song) {
            (Some(old), Some(new)) => old.stream_url != new.stream_url || old.title != new.title,
            (None, Some(_)) => true,
            (Some(_), None) => true,
            (None, None) => false,
        };
        
        if song_changed {
            debug!("Updating current song");
            // Update the stored song
            *current_song = song.clone();
            
            // Notify listeners of the song change
            drop(current_song); // Release the lock before notifying
            self.base.notify_song_changed(song.as_ref());
        }
    }

    /// Create a fresh MPD client connection for sending commands
    /// This creates a new connection each time, rather than reusing an existing one
    fn get_fresh_client(&self) -> Option<Client<TcpStream>> {
        debug!("Creating fresh MPD command connection");
        let addr = format!("{}:{}", self.hostname, self.port);
        
        match Client::connect(&addr) {
            Ok(client) => {
                debug!("Successfully created new MPD command connection");
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
    fn update_state_and_capabilities_from_mpd(client: &mut Client<TcpStream>, player: Arc<Self>, song: Option<Song>) {
        debug!("Updating player state and capabilities based on MPD status");
        
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
                    
                    // Store current song information in metadata if available
                    if let Some(sng) = &song {
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
                let is_seekable = match song {
                    Some(song) => {
                        // Check if the song has a duration
                        if let Some(duration) = song.duration {
                            // Check if the file is not a streaming URL
                            // Common streaming URLs start with http://, https://, or contain specific keywords
                            let file_path = song.stream_url.as_deref().unwrap_or("");
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
    }    /// Convert an MPD song to our Song format
    fn convert_mpd_song(mpd_song: mpd::Song) -> Song {
        // Generate cover art URL using the file path/URI from MPD song
        let cover_url = if !mpd_song.file.is_empty() {
            // Use the API endpoint for MPD images with the song URI
            Some(format!("{}/{}", mpd_image_url(), urlencoding::encode(&mpd_song.file)))
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
            
        Song {
            title: mpd_song.title,
            artist: mpd_song.artist,
            album,
            album_artist,
            track_number: mpd_song.place.as_ref().map(|p| p.pos as i32),
            total_tracks: None,
            duration: mpd_song.duration.map(|d| d.as_secs_f32() as f64),
            genre,
            year: None,
            cover_art_url: cover_url,
            stream_url: Some(mpd_song.file.clone()),
            source: Some("mpd".to_string()),
            liked: None,
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
                    let song = Self::convert_mpd_song(mpd_song);
                    
                    info!("Now playing: {} - {}", 
                        song.title.as_deref().unwrap_or("Unknown"),
                        song.artist.as_deref().unwrap_or("Unknown"));
                    
                    // Log additional song details if available
                    if let Some(duration) = song.duration {
                        debug!("Duration: {:.1} seconds", duration);
                    }
                    if let Some(track) = song.track_number {
                        debug!("Position: {} in queue", track);
                    }
                    
                    // Store the song for capability update
                    obtained_song = Some(song.clone());
                    
                    // Update stored song and notify listeners
                    player.update_current_song(Some(song));
                } else {
                    info!("No song currently playing");
                    
                    // Clear stored song and notify listeners
                    player.update_current_song(None);
                }
            },
            Err(e) => warn!("Failed to get current song information: {}", e),
        }
        
        // Update player capabilities based on the current playlist state and the song we just got
        Self::update_state_and_capabilities_from_mpd(client, player, obtained_song);
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

impl PlayerController for MPDPlayerController {
    delegate! {
        to self.base {
            fn register_state_listener(&mut self, listener: Weak<dyn PlayerStateListener>) -> bool;
            fn unregister_state_listener(&mut self, listener: &Arc<dyn PlayerStateListener>) -> bool;
            fn get_capabilities(&self) -> PlayerCapabilitySet;
            fn get_last_seen(&self) -> Option<std::time::SystemTime>;
        }
    }
    
    fn get_song(&self) -> Option<Song> {
        debug!("Getting current song from stored value");
        // Return a clone of the stored song
        self.current_song.lock().unwrap().clone()
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
                
                PlayerCommand::QueueTracks { uris, insert_at_beginning } => {
                    debug!("Adding {} tracks to MPD queue at {}", uris.len(), 
                          if insert_at_beginning { "beginning" } else { "end" });
                    
                    if uris.is_empty() {
                        debug!("No URIs provided to queue");
                        success = true; // Nothing to do, but not an error
                    } else {
                        let mut all_success = true;
                        
                        // Process each URI using our new queue_url function
                        for uri in &uris {
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
                info!("Loading MPD library data");
                // Import MPDLibrary here to ensure it's available
                use crate::players::mpd::library::MPDLibrary;
                
                // Create a library with the same connection parameters and pass self as controller
                let library = MPDLibrary::with_connection(&self.hostname, self.port, player_arc.clone());
                
                // Store the library in the controller first
                {
                    let mut library_guard = self.library.lock().unwrap();
                    *library_guard = Some(library.clone());
                }
                
                // Explicitly call refresh_library on the library instance
                // This ensures the library refresh is triggered immediately
                info!("Starting MPD library refresh...");
                
                // Get a clone of the library for the thread
                let library_clone = library.clone();
                
                // Run the refresh in a separate thread to avoid blocking startup
                thread::spawn(move || {
                    match library_clone.refresh_library() {
                        Ok(_) => info!("MPD library loaded successfully"),
                        Err(e) => warn!("Failed to load MPD library: {}", e),
                    }
                });
            } else {
                debug!("Skipping MPD library loading (disabled in config)");
            }
        } else {
            warn!("Could not connect to MPD to fetch initial song state");
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