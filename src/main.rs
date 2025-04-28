use acr::data::{PlayerState, Song, LoopMode, PlayerCapability, PlayerCommand};
use acr::players::{PlayerController, PlayerStateListener, MPDPlayer};
use std::sync::{Arc, Weak};
use std::any::Any;
use std::thread;
use std::time::Duration;
use std::io::{self, Write};
use log::{debug, info, warn, error};
use env_logger::Env;

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
    
    fn on_song_changed(&self, song: Option<&Song>) {
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
    
    fn on_capabilities_changed(&self, capabilities: &[PlayerCapability]) {
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
    let mut mpd_player = MPDPlayer::with_connection("localhost", 8000);
    println!("Created MPD controller with connection: {}:{}", 
        mpd_player.hostname(), mpd_player.port());
    
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
    
    // Enter a simulation loop - in a real application this would be event-driven
    info!("\nEntering event simulation loop. Press Ctrl+C to exit.");
    info!("Simulating player events every few seconds...");
    println!("\nEntering event simulation loop. Press Ctrl+C to exit.");
    println!("Simulating player events every few seconds...");
    
    // Create a list of simulated commands to rotate through
    let commands = [
        PlayerCommand::Play,
        PlayerCommand::Pause,
        PlayerCommand::Next,
        PlayerCommand::Previous,
        PlayerCommand::SetLoopMode(LoopMode::Track),
        PlayerCommand::SetLoopMode(LoopMode::Playlist),
        PlayerCommand::SetLoopMode(LoopMode::None),
        PlayerCommand::SetRandom(true),
        PlayerCommand::SetRandom(false),
    ];
    
    let mut command_index = 0;
    
    // In a real application, we'd be listening for MPD events
    // Here we're just simulating them in a loop
    loop {
        // Simulate sending a command
        let command = &commands[command_index];
        debug!("Sending command: {}", command);
        print!("Sending command: {} ... ", command);
        io::stdout().flush().unwrap();
        
        // Send the command to the player
        mpd_player.send_command(commands[command_index].clone());
        println!("done");
        
        // In a real implementation, the send_command would trigger state changes
        // and those would fire notifications. Here we'll simulate those by directly
        // triggering notifications for demonstration purposes.
        
        // Simulate a state change based on the command
        match command {
            PlayerCommand::Play => {
                info!("Simulating state change to Playing");
                println!("Simulating state change to Playing");
                mpd_player.notify_state_changed(PlayerState::Playing);
            },
            PlayerCommand::Pause => {
                info!("Simulating state change to Paused");
                println!("Simulating state change to Paused");
                mpd_player.notify_state_changed(PlayerState::Paused);
            },
            PlayerCommand::SetLoopMode(mode) => {
                info!("Simulating loop mode change to {}", mode);
                println!("Simulating loop mode change to {}", mode);
                mpd_player.notify_loop_mode_changed(*mode);
            },
            _ => {
                debug!("Command sent (no state change simulated)");
                println!("Command sent (no state change simulated)");
            }
        }
        
        // Rotate to the next command for the next iteration
        command_index = (command_index + 1) % commands.len();
        
        // Wait between simulated events
        debug!("Waiting for {} seconds before next command", 3);
        thread::sleep(Duration::from_secs(3));
    }
}