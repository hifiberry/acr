use acr::data::{PlayerState, Song, LoopMode, PlayerCapability, PlayerCommand};
use acr::players::{PlayerController, PlayerStateListener, MPDPlayer};
use std::sync::{Arc, Weak};
use std::any::Any;
use std::thread;
use std::time::Duration;
use std::io::{self, Read, Write};
use log::{debug, info, warn, error};
use env_logger::Env;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc as StdArc;
use ctrlc;

/// Event Logger that implements the PlayerStateListener trait
struct EventLogger {
    name: String,
}

impl EventLogger {
    fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
        }
    }
}

impl PlayerStateListener for EventLogger {
    fn on_state_changed(&self, state: PlayerState) {
        info!("[{}] State changed: {}", self.name, state);
    }
    
    fn on_song_changed(&self, song: Option<Song>) {
        match song {
            Some(s) => info!("[{}] Song changed: {} by {}", self.name, 
                s.title.as_deref().unwrap_or("Unknown"), 
                s.artist.as_deref().unwrap_or("Unknown")),
            None => info!("[{}] Song cleared", self.name),
        }
    }
    
    fn on_loop_mode_changed(&self, mode: LoopMode) {
        info!("[{}] Loop mode changed: {}", self.name, mode);
    }
    
    fn on_capabilities_changed(&self, capabilities: Vec<PlayerCapability>) {
        info!("[{}] Capabilities changed:", self.name);
        for cap in capabilities {
            debug!("[{}]   - {}", self.name, cap);
        }
    }
    
    fn as_any(&self) -> &dyn Any {
        self
    }
}

fn main() {
    // Initialize the logger with default configuration
    env_logger::Builder::from_env(Env::default().default_filter_or("info"))
        .format_timestamp_secs()
        .init();

    info!("AudioControl3 (ACR) MPD Controller Demo starting");
    println!("AudioControl3 (ACR) MPD Controller Demo\n");
    
    // Create an MPD player controller
    let mut mpd_player = MPDPlayer::with_connection("localhost", 6600);
    println!("Created MPD controller with connection: {}:{}", 
        mpd_player.hostname(), mpd_player.port());
    
    // Initialize the singleton instance for event handling
    mpd_player.init_instance();
    
    // Create an event logger and subscribe to player events
    let event_logger = Arc::new(EventLogger::new("MPDLogger"));
    let weak_logger = Arc::downgrade(&event_logger) as Weak<dyn PlayerStateListener>;
    
    // Register the logger with the player
    if mpd_player.register_state_listener(weak_logger) {
        println!("Successfully registered event listener");
    } else {
        println!("Failed to register event listener");
    }
    
    // Get initial state information and log it
    info!("\nInitial player state:");
    info!("State: {}", mpd_player.get_player_state());
    
    let capabilities = mpd_player.get_capabilities();
    info!("Capabilities:");
    for cap in &capabilities {
        debug!("  - {}", cap);
    }
    
    info!("Loop mode: {}", mpd_player.get_loop_mode());
    
    match mpd_player.get_song() {
        Some(song) => info!("Current song: {} by {}", 
            song.title.unwrap_or_else(|| "Unknown".to_string()), 
            song.artist.unwrap_or_else(|| "Unknown".to_string())),
        None => info!("No song currently playing"),
    }
    
    // Set up a shared flag for graceful shutdown
    let running = StdArc::new(AtomicBool::new(true));
    let r = running.clone();
    
    // Set up Ctrl+C handler
    ctrlc::set_handler(move || {
        println!("\nReceived Ctrl+C, shutting down...");
        r.store(false, Ordering::SeqCst);
    }).expect("Error setting Ctrl+C handler");
    
    // Enter the event loop - listen for MPD events until Ctrl+C
    info!("\nEntering MPD event listening loop. Press Ctrl+C to exit.");
    println!("\nListening for MPD events. Press Ctrl+C to exit.");
    
    // Start the event listener thread in the MPD player
    mpd_player.start_event_listener(running.clone());
    
    // Create a shared reference to the MPD player for the keyboard handler
    let player_ref = Arc::new(mpd_player);
    let player_clone = player_ref.clone();
    
    // Start a thread to monitor keypresses
    let keyboard_running = running.clone();
    thread::spawn(move || {
        println!("Keyboard controls active:");
        println!("  Space: Play/Pause");
        println!("  n: Next track");
        println!("  p: Previous track");
        println!("  Ctrl+C: Exit");
        
        // Set up terminal for raw input mode if possible
        let mut stdin = io::stdin();
        
        // Buffer for reading single bytes
        let mut buffer = [0; 1];
        
        while keyboard_running.load(Ordering::SeqCst) {
            // Try to read a single keystroke
            if stdin.read_exact(&mut buffer).is_ok() {
                match buffer[0] {
                    // Space key (32 is ASCII for space)
                    32 => {
                        info!("Space key pressed: toggling play/pause");
                        player_clone.send_command(PlayerCommand::PlayPause);
                    },
                    // 'n' key
                    110 | 78 => {  // ASCII for 'n' or 'N'
                        info!("'n' key pressed: next track");
                        player_clone.send_command(PlayerCommand::Next);
                    },
                    // 'p' key
                    112 | 80 => {  // ASCII for 'p' or 'P'
                        info!("'p' key pressed: previous track");
                        player_clone.send_command(PlayerCommand::Previous);
                    },
                    _ => {
                        // Ignore other keys
                    }
                }
            } else {
                // If read failed, sleep a bit to avoid tight looping
                thread::sleep(Duration::from_millis(10));
            }
        }
        
        info!("Keyboard handler thread exiting");
    });
    
    // Keep the main thread alive until Ctrl+C is received
    while running.load(Ordering::SeqCst) {
        thread::sleep(Duration::from_millis(100));
    }
    
    info!("Exiting application");
}