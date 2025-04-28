use acr::data::{PlayerState, Song, LoopMode, PlayerCapability, PlayerCommand};
use acr::players::{PlayerStateListener, PlayerController};
use acr::AudioController;
use std::sync::{Arc, Weak};
use std::any::Any;
use std::thread;
use std::time::Duration;
use std::io::{self, Read};
use log::{debug, info, warn, error};
use env_logger::Env;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc as StdArc;
use ctrlc;
use serde_json::json;

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
    fn on_state_changed(&self, player_name: String, player_id: String, state: PlayerState) {
        info!("[{}] Player {}:{} - State changed: {}", self.name, player_name, player_id, state);
    }
    
    fn on_song_changed(&self, player_name: String, player_id: String, song: Option<Song>) {
        match song {
            Some(s) => info!("[{}] Player {}:{} - Song changed: {} by {}", 
                self.name,
                player_name,
                player_id,
                s.title.as_deref().unwrap_or("Unknown"), 
                s.artist.as_deref().unwrap_or("Unknown")),
            None => info!("[{}] Player {}:{} - Song cleared", self.name, player_name, player_id),
        }
    }
    
    fn on_loop_mode_changed(&self, player_name: String, player_id: String, mode: LoopMode) {
        info!("[{}] Player {}:{} - Loop mode changed: {}", self.name, player_name, player_id, mode);
    }
    
    fn on_capabilities_changed(&self, player_name: String, player_id: String, capabilities: Vec<PlayerCapability>) {
        info!("[{}] Player {}:{} - Capabilities changed:", self.name, player_name, player_id);
        for cap in capabilities {
            debug!("[{}] Player {}:{} - Capability: {}", self.name, player_name, player_id, cap);
        }
    }
    
    fn as_any(&self) -> &dyn Any {
        self
    }
}

fn main() {
    // Initialize the logger with default configuration
    env_logger::Builder::from_env(Env::default().default_filter_or("debug"))
        .format_timestamp_secs()
        .init();

    info!("AudioControl3 (ACR) Player Controller Demo starting");
    println!("AudioControl3 (ACR) Player Controller Demo\n");
    
    // Create a JSON array with player configurations
    let controllers_config = json!([
        {
            "mpd": {
                "host": "localhost",
                "port": 6600
            }
        }
    ]);
    
    // Create an AudioController from the JSON configuration
    let audio_controller_result = AudioController::from_json(&controllers_config);
    
    // This will now contain an Arc<AudioController> with initialized self-reference
    let controller = match audio_controller_result {
        Ok(controller) => {
            info!("Successfully created AudioController from JSON configuration");
            controller
        },
        Err(e) => {
            error!("Failed to create AudioController from JSON: {}", e);
            error!("Cannot continue without a valid AudioController. Terminating.");
            std::process::exit(1);
        }
    };
    
    // Wrap the AudioController in a Box that implements PlayerController
    let mut player: Box<dyn PlayerController + Send + Sync> = Box::new(controller.as_ref().clone());
    
    // Let's determine what type of player we're using
    let player_type = if player.get_capabilities().contains(&PlayerCapability::Seek) {
        "Full-featured player"
    } else {
        "Basic player"
    };
    println!("Using {} with {} capabilities", player_type, player.get_capabilities().len());
    
    // Start the player directly through the trait interface
    // No need to downcast to specific implementation
    if player.start() {
        info!("Player initialized and started successfully");
    } else {
        warn!("Failed to start player");
    }
    
    // Create an event logger and subscribe to player events
    let event_logger = Arc::new(EventLogger::new("PlayerLogger"));
    let weak_logger = Arc::downgrade(&event_logger) as Weak<dyn PlayerStateListener>;
    
    // Register the logger with the player
    if player.register_state_listener(weak_logger) {
        println!("Successfully registered event listener");
    } else {
        println!("Failed to register event listener");
    }
    
    // Get initial state information and log it
    info!("\nInitial player state:");
    info!("State: {}", player.get_player_state());
    
    let capabilities = player.get_capabilities();
    info!("Capabilities:");
    for cap in &capabilities {
        debug!("  - {}", cap);
    }
    
    info!("Loop mode: {}", player.get_loop_mode());
    
    match player.get_song() {
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
    
    // Enter the event loop - listen for player events until Ctrl+C
    info!("\nEntering player event listening loop. Press Ctrl+C to exit.");
    println!("\nListening for player events. Press Ctrl+C to exit.");
    
    // Create a shared reference to the player for the keyboard handler
    let player_ref = Arc::new(player);
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