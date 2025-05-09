use acr::data::PlayerCommand;
use acr::players::PlayerController;
use acr::AudioController;
use acr::api::server;
use acr::helpers::attributecache::AttributeCache;
use acr::helpers::imagecache::ImageCache;
use acr::helpers::musicbrainz;
use acr::helpers::theartistdb;
use std::thread;
use std::time::Duration;
use std::io::{self, Read};
use log::{debug, info, warn, error};
use env_logger::Env;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use ctrlc;
use std::fs;
use std::path::Path;

fn main() {
    // Initialize the logger with default configuration
    env_logger::Builder::from_env(Env::default().default_filter_or("info"))
        .format_timestamp_secs()
        .init();

    println!("AudioControl3 (ACR) Player Controller\n");
    info!("AudioControl3 (ACR) Player Controller starting");
    
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

    // Get the attribute cache path from configuration
    let attribute_cache_path = if let Some(cache_config) = controllers_config.get("cache") {
        if let Some(cache_path) = cache_config.get("attribute_cache_path").and_then(|p| p.as_str()) {
            info!("Using attribute cache path from config: {}", cache_path);
            cache_path.to_string()
        } else {
            let default_path = "cache/attributes".to_string();
            info!("No attribute_cache_path specified in cache configuration, using default path: {}", default_path);
            default_path
        }
    } else {
        let default_path = "cache/attributes".to_string();
        info!("No cache configuration found, using default attribute cache path: {}", default_path);
        default_path
    };

    // Get the image cache path from configuration
    let image_cache_path = if let Some(cache_config) = controllers_config.get("cache") {
        if let Some(cache_path) = cache_config.get("image_cache_path").and_then(|p| p.as_str()) {
            info!("Using image cache path from config: {}", cache_path);
            cache_path.to_string()
        } else {
            let default_path = "cache/images".to_string();
            info!("No image_cache_path specified in cache configuration, using default path: {}", default_path);
            default_path
        }
    } else {
        let default_path = "cache/images".to_string();
        info!("No cache configuration found, using default image cache path: {}", default_path);
        default_path
    };

    // Initialize the global attribute cache with the configured path from JSON
    initialize_attribute_cache(&attribute_cache_path);
    
    // Initialize the global image cache with the configured path from JSON
    initialize_image_cache(&image_cache_path);
    
    // Initialize MusicBrainz with the configuration
    initialize_musicbrainz(&controllers_config);

    // Initialize TheArtistDB with the configuration
    initialize_theartistdb(&controllers_config);
    
    // Set up a shared flag for graceful shutdown
    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();
    
    // Set up Ctrl+C handler
    ctrlc::set_handler(move || {
        println!("\nReceived Ctrl+C, shutting down...");
        r.store(false, Ordering::SeqCst);
    }).expect("Error setting Ctrl+C handler");
    
    // Create an AudioController from the JSON configuration and store it in the singleton
    let controller = match AudioController::from_json(&controllers_config) {
        Ok(controller) => {
            info!("Successfully created AudioController from JSON configuration");
            controller
        },
        Err(e) => {
            error!("Failed to create AudioController from JSON: {}", e);
            panic!("Cannot continue without a valid AudioController");
        }
    };
    
    // Initialize the AudioController singleton
    match AudioController::initialize_instance(controller.clone()) {
        Ok(_) => info!("AudioController singleton initialized successfully"),
        Err(e) => warn!("AudioController singleton initialization: {}", e),
    }
    
    // Get a reference to the AudioController singleton
    let controller = AudioController::instance();
    
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
    // No need to clone the controller, just get it from the singleton as needed
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
                // Get a fresh reference to the AudioController singleton for each command
                let controller = AudioController::instance();
                
                match buffer[0] {
                    // Space key (32 is ASCII for space)
                    32 => {
                        info!("Space key pressed: toggling play/pause");
                        controller.send_command(PlayerCommand::PlayPause);
                    },
                    // 'n' key
                    110 | 78 => {  // ASCII for 'n' or 'N'
                        info!("'n' key pressed: next track");
                        controller.send_command(PlayerCommand::Next);
                    },
                    // 'p' key
                    112 | 80 => {  // ASCII for 'p' or 'P'
                        info!("'p' key pressed: previous track");
                        controller.send_command(PlayerCommand::Previous);
                    },
                    // '?' key
                    63 => {  // ASCII for '?'
                        info!("'?' key pressed: displaying state of all players");
                        controller.display_all_player_states();
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
    
    // Start the API server in a Tokio runtime
    let controllers_config_clone = controllers_config.clone();
    // No need to pass the controller around, the API server can just access the singleton
    let _api_thread = thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().expect("Failed to create Tokio runtime");
        rt.block_on(async {
            // Get a reference to the singleton AudioController for the server
            let controller = AudioController::instance();
            if let Err(e) = server::start_rocket_server(controller, &controllers_config_clone).await {
                error!("API server error: {}", e);
            }
        });
    });
    
    info!("API server started on port {}", controllers_config.get("api_port")
        .and_then(|p| p.as_u64())
        .unwrap_or(1080));
    
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

// Helper function to initialize the global attribute cache
fn initialize_attribute_cache(attribute_cache_path: &str) {
    match AttributeCache::initialize(attribute_cache_path) {
        Ok(_) => info!("Attribute cache initialized with path: {}", attribute_cache_path),
        Err(e) => warn!("Failed to initialize attribute cache: {}", e)
    }
}

// Helper function to initialize the global image cache
fn initialize_image_cache(image_cache_path: &str) {
    match ImageCache::initialize(image_cache_path) {
        Ok(_) => info!("Image cache initialized with path: {}", image_cache_path),
        Err(e) => warn!("Failed to initialize image cache: {}", e)
    }
}

// Helper function to initialize MusicBrainz
fn initialize_musicbrainz(config: &serde_json::Value) {
    musicbrainz::initialize_from_config(config);
    info!("MusicBrainz initialized successfully");
}

// Helper function to initialize TheArtistDB
fn initialize_theartistdb(config: &serde_json::Value) {
    theartistdb::initialize_from_config(config);
    info!("TheArtistDB initialized successfully");
}