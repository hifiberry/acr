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
        // Use the provided client connection instead of creating a new one
        match client.currentsong() {
            Ok(song_opt) => {
                if let Some(song) = song_opt {
                    info!("Now playing: {} - {}", 
                        song.title.unwrap_or_else(|| "Unknown".to_string()),
                        song.artist.unwrap_or_else(|| "Unknown".to_string()));
                    
                    // Log additional song details if available
                    if let Some(duration) = song.duration {
                        debug!("Duration: {:.1} seconds", duration.as_secs_f32());
                    }
                    if let Some(place) = song.place {
                        debug!("Position: {} in queue", place.pos);
                    }
                } else {
                    info!("No song currently playing");
                }
            },
            Err(e) => warn!("Failed to get current song information: {}", e),
        }
        
        // Also log the player state
        match client.status() {
            Ok(status) => {
                info!("Player status: {:?}, volume: {}%", 
                    status.state, status.volume);
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
        debug!("Getting current song (not implemented yet)");
        // Not implemented yet
        None
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
        // Not implemented yet
        warn!("MPD command implementation not yet available");
        false
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