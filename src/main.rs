use acr::players::PlayerController;
use acr::AudioController;
use acr::api::server;
use acr::helpers::attributecache::AttributeCache;
use acr::helpers::imagecache::ImageCache;
use acr::helpers::musicbrainz;
use acr::helpers::theaudiodb;
use acr::helpers::lastfm;
use acr::helpers::spotify;
use acr::helpers::security_store::SecurityStore;
// Import LMS modules to ensure they're included in the build
#[allow(unused_imports)]
use acr::players::lms::lmsaudio::LMSAudioController;
use std::thread;
use std::time::Duration;
use log::{debug, info, warn, error};
use env_logger::Env;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use ctrlc;
use std::fs;
use std::path::Path;
use std::env;
use std::path::PathBuf;
// Import global Tokio runtime functions from lib.rs
use acr::{initialize_tokio_runtime, get_tokio_runtime};

fn main() {
    // Initialize the Tokio runtime early
    initialize_tokio_runtime();
      // Parse command line arguments
    let args: Vec<String> = env::args().collect();
    let debug_mode = args.iter().any(|arg| arg == "--debug");
    
    // Initialize the logger with the appropriate level based on debug flag
    if debug_mode {
        env_logger::Builder::from_env(Env::default().default_filter_or("debug"))
            .format_timestamp_secs()
            .init();
        info!("Debug mode enabled");
    } else {
        env_logger::Builder::from_env(Env::default().default_filter_or("info"))
            .format_timestamp_secs()
            .init();
    }

    info!("AudioControl3 (ACR) Player Controller starting");
    
    // Check for config file path in command line arguments (-c option)
    let mut config_path_str = String::from("acr.json");
    let mut i = 1;
    while i < args.len() {
        if args[i] == "-c" && i + 1 < args.len() {
            config_path_str = args[i + 1].clone();
            info!("Using configuration file specified by -c: {}", config_path_str);
            break;
        }
        i += 1;
    }
    
    // Check if the specified config file exists
    let config_path_obj = Path::new(&config_path_str);
    let controllers_config: serde_json::Value = if config_path_obj.exists() {
        // Read the configuration from the specified file
        info!("Found configuration file at {}, using it", config_path_str);
        match fs::read_to_string(&config_path_str) {
            Ok(config_str) => {
                match serde_json::from_str(&config_str) {
                    Ok(config) => {
                        info!("Successfully loaded configuration from {}", config_path_str);
                        config
                    },
                    Err(e) => {
                        error!("Failed to parse {}: {}", config_path_str, e);
                        panic!("Cannot continue without a valid configuration file");
                    }
                }
            },
            Err(e) => {
                error!("Failed to read {}: {}", config_path_str, e);
                panic!("Cannot continue without a valid configuration file");
            }
        }
    } else {
        // No config file found
        error!("Configuration file not found at {}", config_path_str);
        panic!("Cannot continue without a valid configuration file");
    };

    // Initialize the Security Store (Moved Up)
    let security_store_path_str = controllers_config
        .get("general")
        .and_then(|g| g.get("security_store"))
        .and_then(|s| s.as_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| {
            info!("No security_store path specified in configuration, using default 'secrets/security_store.json'");
            "secrets/security_store.json".to_string() // Ensure this default path is appropriate
        });

    let security_store_path = PathBuf::from(&security_store_path_str);
    // Ensure the directory for the security store exists, especially if it's not in the root
    if let Some(parent_dir) = security_store_path.parent() {
        if !parent_dir.exists() {
            if let Err(e) = fs::create_dir_all(parent_dir) {
                error!("Failed to create directory for security store at {}: {}. Please check permissions.", parent_dir.display(), e);
                // Depending on how critical this is, you might panic or try a default fallback.
                // For now, we'll log an error and proceed, initialize_with_defaults might handle it or fail.
            } else {
                info!("Created directory for security store: {}", parent_dir.display());
            }
        }
    }
    
    if let Err(e) = SecurityStore::initialize_with_defaults(Some(security_store_path.clone())) {
        error!("Failed to initialize security store at {}: {}. Please check permissions and configuration.", security_store_path.display(), e);
        panic!("Critical component: Security store initialization failed. Application cannot continue. Error: {}", e);
    } else {
        info!("Security store initialized successfully at {}", security_store_path.display());
    }

    // Get the attribute cache path from configuration
    let attribute_cache_path = if let Some(cache_config) = controllers_config.get("cache") {
        if let Some(cache_path) = cache_config.get("attribute_cache_path").and_then(|p| p.as_str()) {
            info!("Using attribute cache path from config: {}", cache_path);
            cache_path.to_string()
        } else {            let default_path = "cache/attributes".to_string();
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
        } else {            let default_path = "cache/images".to_string();
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

    // Initialize TheAudioDB with the configuration
    initialize_theaudiodb(&controllers_config);
      // Initialize Last.fm with the configuration
    initialize_lastfm(&controllers_config);
    
    // Initialize Spotify with the configuration
    initialize_spotify(&controllers_config);
    
    // Set up a shared flag for graceful shutdown
    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();
    
    // Set up Ctrl+C handler
    ctrlc::set_handler(move || {
        info!("Received Ctrl+C, shutting down...");
        r.store(false, Ordering::SeqCst);
        
        // Set up a force shutdown after a timeout
        let force_shutdown_delay = Duration::from_secs(5); // 5 seconds timeout
        let r_clone = r.clone();  // Clone the Arc for the new thread
        let _force_shutdown_thread = thread::spawn(move || {
            thread::sleep(force_shutdown_delay);
            // If we're still running after the timeout, force exit
            if !r_clone.load(Ordering::SeqCst) {
                info!("Graceful shutdown timed out after {} seconds, forcing exit...", force_shutdown_delay.as_secs());
                std::process::exit(0);
            }
        });
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
    if player.start() {
        info!("Player initialized and started successfully");
    } else {
        warn!("Failed to start player");
    }
    
    // Log initial state information
    debug!("Initial player state:");
    debug!("State: {}", player.get_playback_state());
    
    let capabilities = player.get_capabilities();
    debug!("Capabilities:");
    for cap in &capabilities {
        debug!("  - {}", cap);
    }
    
    debug!("Loop mode: {}", player.get_loop_mode());
    
    if let Some(song) = player.get_song() {
        debug!("Current song: {} by {}", 
            song.title.unwrap_or_else(|| "Unknown".to_string()), 
            song.artist.unwrap_or_else(|| "Unknown".to_string()));
    } else {
        debug!("No song currently playing");
    }
    
    // Start the API server using the global Tokio runtime
    let controllers_config_clone = controllers_config.clone();
    let _api_thread = thread::spawn(move || {
        get_tokio_runtime().block_on(async {
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

// Helper function to initialize TheAudioDB
fn initialize_theaudiodb(config: &serde_json::Value) {
    theaudiodb::initialize_from_config(config);
    info!("TheAudioDB initialized successfully");
}

// Helper function to initialize Last.fm
fn initialize_lastfm(config: &serde_json::Value) {
    if let Some(lastfm_config) = config.get("lastfm") {
        // Check if enabled flag exists and is set to true
        let enabled = lastfm_config.get("enable")
            .and_then(|v| v.as_bool())
            .unwrap_or(false); // Default to disabled if not specified
        
        if enabled {
            // Initialize with default API credentials
            if let Err(e) = lastfm::LastfmClient::initialize_with_defaults() {
                warn!("Failed to initialize Last.fm client: {}", e);
                return;
            }
            
            // Log Last.fm connection status
            match lastfm::LastfmClient::get_instance() {
                Ok(client) => {
                    if client.is_authenticated() {
                        if let Some(username) = client.get_username() {
                            info!("Last.fm connected as user: {}", username);
                        } else {
                            // This case should ideally not happen if is_authenticated is true
                            warn!("Last.fm is authenticated but username is not available.");
                        }
                    } else {
                        info!("Last.fm is not connected. User needs to authenticate.");
                    }
                }
                Err(e) => {
                    // This might happen if initialization failed silently or was never called
                    warn!("Could not get Last.fm client instance to check status: {}", e);
                }
            }
            info!("Last.fm initialized successfully"); // This message might be redundant now or could be rephrased
        } else {
            info!("Last.fm integration is disabled");
        }
    } else {
        debug!("No Last.fm configuration found, Last.fm features will be unavailable.");
    }
}

// Helper function to initialize Spotify
fn initialize_spotify(config: &serde_json::Value) {
    info!("Starting Spotify initialization");
    
    if let Some(spotify_config) = config.get("spotify") {
        // Check if enabled flag exists and is set to true
        let enabled = spotify_config.get("enable")
            .and_then(|v| v.as_bool())
            .unwrap_or(false); // Default to disabled if not specified
        
        info!("Spotify enabled in config: {}", enabled);
        
        if enabled {
            // Get custom OAuth URL and proxy secret if specified in config
            let oauth_url = spotify_config.get("oauth_url")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            
            let proxy_secret = spotify_config.get("proxy_secret")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            
            info!("Config values - OAuth URL present: {}, proxy secret present: {}", 
                 oauth_url.is_some(), proxy_secret.is_some());
            
            // Initialize with values from config or fall back to defaults
            let init_result = match (oauth_url, proxy_secret) {
                (Some(url), Some(secret)) if !url.is_empty() && !secret.is_empty() => {
                    info!("Initializing Spotify with configuration from acr.json, URL: '{}'", url);
                    spotify::Spotify::initialize(url, secret)
                },
                _ => {
                    info!("No valid Spotify config in acr.json, falling back to secrets.txt");
                    spotify::Spotify::initialize_with_defaults()
                }
            };
              if let Err(e) = init_result {
                warn!("Failed to initialize Spotify client: {}", e);
                
                // Additional logging to help diagnose the issue
                info!("Checking default OAuth URL directly: '{}'", 
                      spotify::default_spotify_oauth_url());
                      
                return;
            }
            
            // Log Spotify connection status
            match spotify::Spotify::get_instance() {
                Ok(client) => {
                    if client.has_valid_tokens() {
                        info!("Spotify is connected with valid tokens");
                    } else {
                        info!("Spotify is not connected. User needs to authenticate.");
                    }
                },
                Err(e) => {
                    warn!("Could not get Spotify client instance to check status: {}", e);
                }
            }
            info!("Spotify initialized successfully");
        } else {
            info!("Spotify integration is disabled");
        }
    } else {
        debug!("No Spotify configuration found, Spotify features will be unavailable.");
    }
}