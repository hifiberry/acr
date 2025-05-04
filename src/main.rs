use acr::data::PlayerCommand;
use acr::players::PlayerController;
use acr::AudioController;
use acr::webserver::{ApiServer, ApiConfig}; // Import the API server components
use std::thread;
use std::time::Duration;
use std::io::{self, Read};
use log::{debug, info, warn, error};
use env_logger::Env;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc as StdArc;
use ctrlc;
use std::fs;
use std::path::Path;
use std::sync::mpsc;

#[actix_web::main] // Use actix_web main attribute for async support
async fn main() {
    // Initialize the logger with default configuration
    env_logger::Builder::from_env(Env::default().default_filter_or("info"))
        .format_timestamp_secs()
        .init();

    println!("AudioControl3 (ACR) Player Controller Demo\n");
    info!("AudioControl3 (ACR) Player Controller Demo starting");

    // Check if acr.json exists in the current directory
    let config_path = Path::new("acr.json");
    let controllers_config: serde_json::Value = if config_path.exists() {
        // Read the configuration from acr.json
        info!("Found acr.json configuration file, using it");
        match fs::read_to_string(config_path) {
            Ok(config_str) => {
                match serde_json::from_str(&config_str) {
                    Ok(config) => {
                        info!("Successfully loaded configuration from acr.json");
                        config
                    },
                    Err(e) => {
                        error!("Failed to parse acr.json: {}", e);
                        info!("Falling back to sample configuration");
                        // Fall back to sample config if parsing fails
                        parse_sample_config()
                    }
                }
            },
            Err(e) => {
                error!("Failed to read acr.json: {}", e);
                info!("Falling back to sample configuration");
                // Fall back to sample config if reading fails
                parse_sample_config()
            }
        }
    } else {
        // Use sample configuration
        info!("No acr.json found, using sample configuration");
        parse_sample_config()
    };

    // Set up a shared flag for graceful shutdown
    let running = StdArc::new(AtomicBool::new(true));
    let r = running.clone();
    
    // Set up Ctrl+C handler
    ctrlc::set_handler(move || {
        println!("\nReceived Ctrl+C, shutting down...");
        r.store(false, Ordering::SeqCst);
    }).expect("Error setting Ctrl+C handler");
    
    // Initialize and start API server first
    let api_config = match controllers_config.get("api") {
        Some(api_json) => {
            // Extract configuration options with defaults
            let enable = api_json.get("enable").and_then(|v| v.as_bool()).unwrap_or(true);
            let host = api_json.get("host").and_then(|v| v.as_str()).unwrap_or("0.0.0.0").to_string();
            let port = api_json.get("port").and_then(|v| v.as_u64()).unwrap_or(1080) as u16;
            
            ApiConfig {
                enable,
                host,
                port,
            }
        },
        None => {
            info!("No API configuration found, using defaults");
            ApiConfig::default()
        }
    };
    
    // Create API server with configuration
    let api_server = ApiServer::with_config(api_config.clone());
    
    // Clone running flag for API server thread
    let api_running = running.clone();
    
    // Start API server in a separate thread
    let (api_tx, api_rx) = mpsc::channel();
    
    thread::spawn(move || {
        let runtime = tokio::runtime::Runtime::new().expect("Failed to create Tokio runtime");
        
        runtime.block_on(async {
            // Start the API server
            if api_config.enable {
                info!("Starting API server on {}:{}", api_config.host, api_config.port);
                
                // Send back signal that API server is starting
                let _ = api_tx.send(true);
                
                // Create a future that completes when the running flag is false
                let shutdown_signal = async {
                    while api_running.load(Ordering::SeqCst) {
                        tokio::time::sleep(Duration::from_millis(100)).await;
                    }
                };
                
                // Start the server and handle shutdown
                tokio::select! {
                    result = api_server.start() => {
                        if let Err(e) = result {
                            error!("API server error: {}", e);
                        }
                    },
                    _ = shutdown_signal => {
                        info!("API server shutting down");
                    }
                }
            } else {
                info!("API server is disabled in configuration");
                let _ = api_tx.send(false);
            }
        });
        
        info!("API server thread exited");
    });
    
    // Wait for confirmation that API server has started (or been disabled)
    match api_rx.recv() {
        Ok(true) => info!("API server started successfully"),
        Ok(false) => info!("API server disabled, continuing with player initialization"),
        Err(_) => error!("Failed to receive confirmation from API server thread"),
    }
    
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
            panic!("Cannot continue without a valid AudioController");
        }
    };
    
    // Wrap the AudioController in a Box that implements PlayerController
    let player: Box<dyn PlayerController + Send + Sync> = Box::new(controller.as_ref().clone());
       
    // Start the player directly through the trait interface
    // No need to downcast to specific implementation
    if player.start() {
        info!("Player initialized and started successfully");
    } else {
        warn!("Failed to start player");
    }
    
    // Get initial state information and log it
    info!("\nInitial player state:");
    info!("State: {}", player.get_playback_state());
    
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
    
    // Start a thread to monitor keypresses
    let keyboard_running = running.clone();
    // Create a clone of the controller to access all players
    let controller_clone = controller.clone();
    thread::spawn(move || {
        println!("Keyboard controls active:");
        println!("  Space: Play/Pause");
        println!("  n: Next track");
        println!("  p: Previous track");
        println!("  ?: Display state of all players");
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
                        controller_clone.send_command(PlayerCommand::PlayPause);
                    },
                    // 'n' key
                    110 | 78 => {  // ASCII for 'n' or 'N'
                        info!("'n' key pressed: next track");
                        controller_clone.send_command(PlayerCommand::Next);
                    },
                    // 'p' key
                    112 | 80 => {  // ASCII for 'p' or 'P'
                        info!("'p' key pressed: previous track");
                        controller_clone.send_command(PlayerCommand::Previous);
                    },
                    // '?' key
                    63 => {  // ASCII for '?'
                        info!("'?' key pressed: displaying state of all players");
                        controller_clone.display_all_player_states();
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

// Helper function to parse the sample configuration
fn parse_sample_config() -> serde_json::Value {
    // Use the sample JSON configuration from AudioController
    let sample_config = AudioController::sample_json_config();
    info!("Using sample configuration: {}", sample_config);
    
    // Parse the sample configuration string into a JSON Value
    match serde_json::from_str(&sample_config) {
        Ok(config) => {
            info!("Successfully parsed sample JSON configuration");
            config
        },
        Err(e) => {
            error!("Failed to parse sample JSON configuration: {}", e);
            panic!("Cannot continue with invalid sample configuration");
        }
    }
}