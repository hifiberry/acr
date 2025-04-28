use crate::players::base_controller::BasePlayerController;
use crate::players::player_controller::PlayerController;
use crate::data::{PlayerCapability, Song, LoopMode, PlayerState, PlayerCommand};
use std::sync::{Arc, Weak, Mutex};
use log::{debug, info, warn, error};
use mpd::{Client, error::Error as MpdError, idle::Subsystem};
use mpd::Idle; // Add the Idle trait import
use std::net::TcpStream;
use std::thread;
use std::time::Duration;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Once;
use std::collections::HashMap;

// Static instance for singleton pattern
static mut INSTANCE: Option<*mut MPDPlayer> = None;
static INIT: Once = Once::new();

/// MPD player controller implementation
pub struct MPDPlayer {
    /// Base controller for managing state listeners
    base: BasePlayerController,
    
    /// MPD server hostname
    hostname: String,
    
    /// MPD server port
    port: u16,

    /// MPD client connection
    connection: Mutex<Option<Client<TcpStream>>>,
    
    /// Current song information
    current_song: Mutex<Option<Song>>,
}

impl MPDPlayer {
    /// Create a new MPD player controller with default settings
    pub fn new() -> Self {
        debug!("Creating new MPDPlayer with default settings");
        let host = "localhost";
        let port = 6600;
        let connection = Self::establish_connection(host, port);
        
        Self {
            base: BasePlayerController::new(),
            hostname: host.to_string(),
            port,
            connection: Mutex::new(connection),
            current_song: Mutex::new(None),
        }
    }
    
    /// Create a new MPD player controller with custom settings
    pub fn with_connection(hostname: &str, port: u16) -> Self {
        debug!("Creating new MPDPlayer with connection {}:{}", hostname, port);
        let connection = Self::establish_connection(hostname, port);
        
        Self {
            base: BasePlayerController::new(),
            hostname: hostname.to_string(),
            port,
            connection: Mutex::new(connection),
            current_song: Mutex::new(None),
        }
    }
    
    /// Helper method to establish MPD connection
    fn establish_connection(hostname: &str, port: u16) -> Option<Client<TcpStream>> {
        debug!("Attempting to connect to MPD at {}:{}", hostname, port);
        let addr = format!("{}:{}", hostname, port);
        
        match Client::connect(&addr) {
            Ok(client) => {
                info!("Successfully connected to MPD at {}:{}", hostname, port);
                Some(client)
            },
            Err(e) => {
                warn!("Failed to connect to MPD at {}:{}: {}", hostname, port, e);
                None
            }
        }
    }
    
    /// Attempt to reconnect to the MPD server
    pub fn reconnect(&self) -> Result<(), MpdError> {
        let addr = format!("{}:{}", self.hostname, self.port);
        debug!("Attempting to reconnect to MPD at {}", addr);
        
        match Client::connect(&addr) {
            Ok(client) => {
                let mut conn = self.connection.lock().unwrap();
                *conn = Some(client);
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
        if let Ok(mut conn) = self.connection.lock() {
            if let Some(ref mut client) = *conn {
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
        
        // Try to establish a new connection with updated settings
        let connection = Self::establish_connection(hostname, port);
        if let Ok(mut conn) = self.connection.lock() {
            *conn = connection;
        }
    }
    
    /// Helper method for simulating state changes (for demo purposes)
    pub fn notify_state_changed(&self, state: PlayerState) {
        debug!("MPDPlayer forwarding state change notification: {}", state);
        self.base.notify_state_changed(state);
    }
    
    /// Helper method for simulating song changes (for demo purposes)
    pub fn notify_song_changed(&self, song: Option<&Song>) {
        let song_title = song.map_or("None".to_string(), |s| s.title.as_deref().unwrap_or("Unknown").to_string());
        debug!("MPDPlayer forwarding song change notification: {}", song_title);
        self.base.notify_song_changed(song);
    }
    
    /// Helper method for simulating loop mode changes (for demo purposes)
    pub fn notify_loop_mode_changed(&self, mode: LoopMode) {
        debug!("MPDPlayer forwarding loop mode change notification: {}", mode);
        self.base.notify_loop_mode_changed(mode);
    }
    
    /// Helper method for simulating capability changes (for demo purposes)
    pub fn notify_capabilities_changed(&self, capabilities: &[PlayerCapability]) {
        debug!("MPDPlayer forwarding capabilities change notification with {} capabilities", capabilities.len());
        self.base.notify_capabilities_changed(capabilities);
    }

    /// Starts a background thread that listens for MPD events
    /// The thread will run until the running flag is set to false
    pub fn start_event_listener(&self, running: Arc<AtomicBool>) {
        let hostname = self.hostname.clone();
        let port = self.port;
        
        info!("Starting MPD event listener thread");
        
        // Spawn a new thread for event listening
        thread::spawn(move || {
            info!("MPD event listener thread started");
            Self::run_event_loop(&hostname, port, running);
            info!("MPD event listener thread shutting down");
        });
    }

    /// Main event loop for listening to MPD events
    fn run_event_loop(hostname: &str, port: u16, running: Arc<AtomicBool>) {
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
            Self::process_events(idle_client, hostname, port, &running);
            
            // If we get here, either there was a connection error or the connection was lost
            if running.load(Ordering::SeqCst) {
                Self::wait_for_reconnect(&running);
            }
        }
    }
    
    /// Process MPD events until connection fails or shutdown requested
    fn process_events(mut idle_client: Client<TcpStream>, hostname: &str, port: u16, running: &Arc<AtomicBool>) {
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
            
            // We need to establish a new command connection since the idle connection
            // is blocked waiting for events. MPD protocol doesn't allow commands during idle state.
            match Client::connect(&format!("{}:{}", hostname, port)) {
                Ok(mut cmd_client) => {
                    // Process each subsystem event with our command connection
                    for subsystem in events {
                        Self::handle_subsystem_event(subsystem, &mut cmd_client);
                    }
                },
                Err(e) => {
                    warn!("Failed to connect for command processing: {}", e);
                    // Don't break the event loop if we can't get a command connection,
                    // just skip processing this batch of events
                }
            }
        }
    }
    
    /// Handle a specific MPD subsystem event
    fn handle_subsystem_event(subsystem: Subsystem, client: &mut Client<TcpStream>) {
        match subsystem {
            Subsystem::Player => {
                debug!("Player state changed");
                // Pass the existing client connection to reuse it
                Self::handle_player_event(client);
            },
            Subsystem::Playlist => {
                debug!("Playlist changed");
                // Could notify about playlist/song changes
            },
            Subsystem::Options => {
                debug!("Options changed (repeat, random, etc.)");
                // Could query and notify about repeat/random state
            },
            Subsystem::Mixer => {
                debug!("Mixer changed (volume)");
            },
            Subsystem::Database => {
                debug!("Database changed");
            },
            _ => {
                debug!("Other subsystem changed: {:?}", subsystem);
            }
        }
    }
    
    /// Handle player events and log song information
    fn handle_player_event(client: &mut Client<TcpStream>) {
        // Get player instance to store the song update
        let player = MPDPlayer::get_instance();
        
        // Use the provided client connection instead of creating a new one
        match client.currentsong() {
            Ok(song_opt) => {
                if let Some(mpd_song) = song_opt {
                    // Convert MPD song to our Song format
                    let song = Song {
                        title: mpd_song.title,
                        artist: mpd_song.artist,
                        album: None,
                        album_artist: None,
                        track_number: mpd_song.place.as_ref().map(|p| p.pos as i32),
                        total_tracks: None,
                        duration: mpd_song.duration.map(|d| d.as_secs_f32() as f64),
                        genre: None,
                        year: None,
                        cover_art_url: None,
                        stream_url: Some(mpd_song.file),
                        source: Some("mpd".to_string()),
                        metadata: HashMap::new(),
                    };
                    
                    info!("Now playing: {} - {}", 
                        song.title.as_deref().unwrap_or("Unknown"),
                        song.artist.as_deref().unwrap_or("Unknown"));
                    
                    // Log additional song details if available
                    if let Some(duration) = mpd_song.duration {
                        debug!("Duration: {:.1} seconds", duration.as_secs_f32());
                    }
                    if let Some(place) = mpd_song.place {
                        debug!("Position: {} in queue", place.pos);
                    }
                    
                    // Update stored song and notify listeners
                    if let Some(player) = player {
                        player.update_current_song(Some(song));
                    }
                } else {
                    info!("No song currently playing");
                    
                    // Clear stored song and notify listeners
                    if let Some(player) = player {
                        player.update_current_song(None);
                    }
                }
            },
            Err(e) => warn!("Failed to get current song information: {}", e),
        }
        
        // Also log the player state
        match client.status() {
            Ok(status) => {
                info!("Player status: {:?}, volume: {}%", 
                    status.state, status.volume);
                
                // Could update player state here as well
            },
            Err(e) => warn!("Failed to get player status: {}", e),
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
    
    /// Initialize the singleton instance
    pub fn init_instance(&mut self) {
        INIT.call_once(|| {
            // Safety: We ensure this is only called once from a single thread
            // during initialization
            unsafe {
                INSTANCE = Some(self as *mut MPDPlayer);
            }
        });
        debug!("MPDPlayer singleton instance initialized");
    }

    /// Get access to the player instance for updating from events
    fn get_instance() -> Option<&'static MPDPlayer> {
        unsafe {
            if let Some(instance) = INSTANCE {
                // Safety: We ensure that the instance will not be deallocated
                // while the program is running
                Some(&*instance)
            } else {
                None
            }
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
}

impl PlayerController for MPDPlayer {
    fn get_capabilities(&self) -> Vec<PlayerCapability> {
        debug!("Getting MPDPlayer capabilities");
        // Return basic capabilities for now
        vec![
            PlayerCapability::Play,
            PlayerCapability::Pause,
            PlayerCapability::PlayPause,
            PlayerCapability::Stop,
        ]
    }
    
    fn get_song(&self) -> Option<Song> {
        debug!("Getting current song from stored value");
        // Return a clone of the stored song
        self.current_song.lock().unwrap().clone()
    }
    
    fn get_loop_mode(&self) -> LoopMode {
        debug!("Getting current loop mode (not implemented yet)");
        // Not implemented yet
        LoopMode::None
    }
    
    fn get_player_state(&self) -> PlayerState {
        debug!("Getting current player state (not implemented yet)");
        // Not implemented yet
        PlayerState::Stopped
    }
    
    fn send_command(&self, command: PlayerCommand) -> bool {
        info!("Sending command to MPD: {}", command);
        
        let mut success = false;
        
        // Try to get a connection
        if let Ok(mut conn_guard) = self.connection.lock() {
            if let Some(ref mut client) = *conn_guard {
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
                }
                
                // If the command was successful, we may want to update our stored state
                if success {
                    // We'll update our state asynchronously via the MPD idle events
                    // but we could trigger an immediate update here if needed
                }
            } else {
                // No active connection
                warn!("Cannot send command to MPD: no active connection");
                // Try to reconnect
                if let Ok(new_client) = Client::connect(&format!("{}:{}", self.hostname, self.port)) {
                    debug!("Reconnected to MPD");
                    *conn_guard = Some(new_client);
                    // Retry the command with the new connection
                    drop(conn_guard); // Release the lock before recursion
                    return self.send_command(command);
                }
            }
        } else {
            error!("Failed to acquire connection lock when sending command to MPD");
        }
        
        success
    }
    
    fn register_state_listener(&mut self, listener: Weak<dyn crate::players::player_controller::PlayerStateListener>) -> bool {
        debug!("Registering new state listener with MPDPlayer");
        self.base.register_listener(listener)
    }
    
    fn unregister_state_listener(&mut self, listener: &Arc<dyn crate::players::player_controller::PlayerStateListener>) -> bool {
        debug!("Unregistering state listener from MPDPlayer");
        self.base.unregister_listener(listener)
    }
}