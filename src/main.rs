use audiocontrol::api::server::{self, ServerOutcome};
use audiocontrol::audiocontrol::eventbus::EventBus;
use audiocontrol::audiocontrol::now_playing_bridge;
use audiocontrol::config::{get_service_config, merge_player_includes};
use audiocontrol::helpers::imagecache::ImageCache;
use audiocontrol::helpers::lastfm;
use audiocontrol::helpers::musicbrainz;
use audiocontrol::helpers::security_store::SecurityStore;
use audiocontrol::helpers::settingsdb::SettingsDb;
use audiocontrol::helpers::spotify;
use audiocontrol::helpers::theaudiodb;
use audiocontrol::helpers::fanarttv;
use audiocontrol::logging;
use audiocontrol::players::PlayerController;
use audiocontrol::AudioController;
use audiocontrol_metadata::lastfm_worker::LastfmWorkerConfig;
use audiocontrol_metadata::secrets;
// Import LMS modules to ensure they're included in the build
#[allow(unused_imports)]
use audiocontrol::players::lms::lmsaudio::LMSAudioController;
use log::{debug, error, info, warn};
use std::env;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;
// Import global Tokio runtime functions from lib.rs
use audiocontrol::{get_tokio_runtime, initialize_tokio_runtime};

fn main() {
    // Initialize the Tokio runtime early
    initialize_tokio_runtime();

    // Parse command line arguments
    let args: Vec<String> = env::args().collect();

    // Check for --help option first
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        print_help();
        return;
    }

    // Check for --check-secrets option first (exit early if present)
    if args.iter().any(|arg| arg == "--check-secrets") {
        check_secrets_status();
        return;
    }

    // Look for config file path in command line arguments (-c option)
    let config_file_path = find_config_file_in_args(&args);

    // Look for logging config file path in command line arguments (--log-config option)
    let log_config_path = find_log_config_in_args(&args);

    // Initialize logging system
    if let Err(e) = logging::initialize_logging_with_args(&args, log_config_path.as_deref()) {
        // Exit with error instead of falling back to basic logging
        eprintln!("Error: Failed to initialize logging configuration: {}", e);
        eprintln!("AudioControl cannot start without a valid logging configuration.");
        std::process::exit(1);
    }

    info!("AudioControl Player Controller starting");

    // Use the config file path found earlier or default
    let config_path_str = config_file_path.unwrap_or_else(|| {
        info!("No configuration file specified, using default: audiocontrol.json");
        "audiocontrol.json".to_string()
    });

    // Check if the specified config file exists
    let config_path_obj = Path::new(&config_path_str);
    let mut controllers_config: serde_json::Value = if config_path_obj.exists() {
        // Read the configuration from the specified file
        info!("Found configuration file at {}, using it", config_path_str);
        match fs::read_to_string(&config_path_str) {
            Ok(config_str) => match serde_json::from_str(&config_str) {
                Ok(config) => {
                    info!("Successfully loaded configuration from {}", config_path_str);
                    config
                }
                Err(e) => {
                    error!("Failed to parse {}: {}", config_path_str, e);
                    eprintln!("Error: Failed to parse {}: {}", config_path_str, e);
                    eprintln!("Cannot continue without a valid configuration file.");
                    std::process::exit(1);
                }
            },
            Err(e) => {
                error!("Failed to read {}: {}", config_path_str, e);
                eprintln!("Error: Failed to read {}: {}", config_path_str, e);
                eprintln!("Cannot continue without a valid configuration file.");
                std::process::exit(1);
            }
        }
    } else {
        // No config file found
        error!("Configuration file not found at {}", config_path_str);
        eprintln!("Error: Configuration file not found at {}", config_path_str);
        eprintln!("Cannot continue without a valid configuration file.");
        std::process::exit(1);
    };

    // Merge player configurations from players.d/ include directory
    if let Some(config_dir) = config_path_obj.parent() {
        merge_player_includes(&mut controllers_config, config_dir);
    }

    // Initialize the Security Store
    let security_store_path_str = get_service_config(&controllers_config, "security_store")
        .and_then(|s| s.get("path"))
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
                info!(
                    "Created directory for security store: {}",
                    parent_dir.display()
                );
            }
        }
    }

    if let Err(e) = SecurityStore::initialize_with_defaults(Some(security_store_path.clone())) {
        error!("Failed to initialize security store at {}: {}. Please check permissions and configuration.", security_store_path.display(), e);
        eprintln!("Error: Security store initialization failed: {}", e);
        eprintln!("Check permissions and configuration at {}", security_store_path.display());
        std::process::exit(1);
    } else {
        info!(
            "Security store initialized successfully at {}",
            security_store_path.display()
        );
    } // Get the attribute cache configuration from datastore
    let (_attribute_cache_path, _preload_prefixes, _cache_size) = if let Some(datastore_config) =
        get_service_config(&controllers_config, "datastore")
    {
        let attribute_cache_config = datastore_config.get("attribute_cache");
        
        let cache_path = if let Some(cache_config) = attribute_cache_config {
            if let Some(cache_path) = cache_config
                .get("dbfile")
                .and_then(|p| p.as_str())
            {
                info!("Using attribute cache database file from config: {}", cache_path);
                cache_path.to_string()
            } else {
                let default_path = "/var/lib/audiocontrol/cache/attributes.db".to_string();
                info!(
                    "No dbfile specified in attribute_cache configuration, using default path: {}",
                    default_path
                );
                default_path
            }
        } else {
            let default_path = "/var/lib/audiocontrol/cache/attributes.db".to_string();
            info!(
                "No attribute_cache configuration found in datastore, using default path: {}",
                default_path
            );
            default_path
        };

        // Initialize using the new configuration method that supports both old and new formats
        if let Some(cache_config) = attribute_cache_config {
            match audiocontrol::helpers::attributecache::AttributeCache::initialize_from_config(cache_config) {
                Ok(_) => info!("Attribute cache initialized from configuration"),
                Err(e) => {
                    error!("Failed to initialize attribute cache from config: {}", e);
                    // Fall back to old method
                    if let Err(e) = audiocontrol::helpers::attributecache::AttributeCache::initialize_global(&cache_path) {
                        error!("Failed to initialize attribute cache with fallback method: {}", e);
                    }
                }
            }
        } else {
            // No configuration, use default
            if let Err(e) = audiocontrol::helpers::attributecache::AttributeCache::initialize_global(&cache_path) {
                error!("Failed to initialize attribute cache with defaults: {}", e);
            }
        }

        // Return simplified values since initialization is now handled above
        let prefixes: Vec<String> = Vec::new(); // Preloading is now handled in initialize_from_config
        (cache_path, prefixes, 20_000) // cache_size is no longer used but kept for compatibility
    } else {
        let default_path = "/var/lib/audiocontrol/cache/attributes.db".to_string();
        info!(
            "No datastore configuration found, using default attribute cache path: {}",
            default_path
        );
        
        // Initialize with defaults
        if let Err(e) = audiocontrol::helpers::attributecache::AttributeCache::initialize_global(&default_path) {
            error!("Failed to initialize attribute cache with defaults: {}", e);
        }
        
        (default_path, Vec::new(), 20_000)
    };

    // Get the image cache path from configuration
    let image_cache_path =
        if let Some(datastore_config) = get_service_config(&controllers_config, "datastore") {
            if let Some(cache_path) = datastore_config
                .get("image_cache_path")
                .and_then(|p| p.as_str())
            {
                info!("Using image cache path from config: {}", cache_path);
                cache_path.to_string()
            } else {
                let default_path = "/var/lib/audiocontrol/cache/images".to_string();
                info!(
                    "No image_cache_path specified in datastore configuration, using default path: {}",
                    default_path
                );
                default_path
            }
        } else {
            let default_path = "/var/lib/audiocontrol/cache/images".to_string();
            info!(
                "No datastore configuration found, using default image cache path: {}",
                default_path
            );
            default_path
        };

    // Resolve the image size ladder before anything can read it.
    {
        let images = get_service_config(&controllers_config, "images");
        let sizes = audiocontrol::helpers::imageresize::sizes_from_json("sizes", images);
        let prewarm = audiocontrol::helpers::imageresize::sizes_from_json("prewarm_sizes", images);
        audiocontrol::helpers::imageresize::configure(sizes, prewarm);
        info!(
            "Image sizes: {:?}, pre-warm sizes: {:?}",
            audiocontrol::helpers::imageresize::sizes(),
            audiocontrol::helpers::imageresize::prewarm_sizes()
        );
    }

    // Initialize the global image cache with the configured path from JSON
    initialize_image_cache(&image_cache_path);

    // Get the settings database path from configuration
    let settingsdb_path =
        if let Some(settingsdb_config) = get_service_config(&controllers_config, "settingsdb") {
            if let Some(db_path) = settingsdb_config
                .get("path")
                .and_then(|p| p.as_str())
            {
                info!("Using settings database path from config: {}", db_path);
                db_path.to_string()
            } else {
                let default_path = "/var/lib/audiocontrol/db".to_string();
                info!(
                    "No path specified in settingsdb configuration, using default path: {}",
                    default_path
                );
                default_path
            }
        } else {
            let default_path = "/var/lib/audiocontrol/db".to_string();
            info!(
                "No settingsdb configuration found, using default path: {}",
                default_path
            );
            default_path
        };

    // Initialize the global settings database with the configured path from JSON
    initialize_settingsdb(&settingsdb_path);
    // Initialize MusicBrainz with the configuration
    initialize_musicbrainz(&controllers_config);

    // Initialize TheAudioDB with the configuration
    initialize_theaudiodb(&controllers_config);
    
    // Initialize FanArt.tv with the configuration
    initialize_fanarttv(&controllers_config);

    // Initialize external cover art endpoints with the configuration
    initialize_external_coverart(&controllers_config);

    // Initialize configurator with the configuration
    initialize_configurator(&controllers_config);
    
    // Initialize Last.fm with the configuration
    initialize_lastfm(&controllers_config);
    // Initialize Spotify with the configuration
    if let Some(spotify_config) = get_service_config(&controllers_config, "spotify") {
        spotify::Spotify::set_global_config(spotify_config);
    }
    initialize_spotify(&controllers_config);

    // Initialize volume control with the configuration
    audiocontrol::helpers::global_volume::initialize_volume_control(&controllers_config);

    // Start volume change monitoring if supported
    if audiocontrol::helpers::global_volume::supports_volume_change_monitoring() {
        info!("Starting volume change monitoring");
        match audiocontrol::helpers::global_volume::start_volume_change_monitoring() {
            Ok(_) => {
                info!("Volume change monitoring started successfully");
            },
            Err(e) => {
                warn!("Failed to start volume change monitoring: {}", e);
            }
        }
    } else {
        info!("Volume change monitoring not supported by current volume control");
    }

    // Initialize favourite providers (Last.fm and SettingsDB)
    audiocontrol::helpers::favourites::initialize_favourite_providers();

    // Initialize genre cleanup with configuration
    if let Err(e) = audiocontrol::helpers::genre_cleanup::initialize_genre_cleanup_with_config(Some(&controllers_config)) {
        warn!("Failed to initialize genre cleanup: {}", e);
    } else {
        info!("Genre cleanup initialized successfully");
    }

    // Set up a shared flag for graceful shutdown
    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();

    // Where the API server publishes the means to stop it while it is running.
    let shutdown_handle = server::ShutdownHandle::new();
    let handle_in_signal = shutdown_handle.clone();

    // Set up the SIGINT/SIGTERM/SIGHUP handler.
    //
    // ctrlc only registers SIGTERM with its "termination" feature, which this
    // crate enables -- without it this handler covered SIGINT alone, and
    // SIGTERM, the signal systemd actually sends, had no handler at all.
    //
    // This is the only signal handler in the process: Rocket's own is switched
    // off in start_rocket_server. What a signal means depends on whether the
    // API server is running. While it is, the signal is passed to it so its
    // grace and mercy periods are honoured, and `running` is left alone -- the
    // API thread below clears it once the server reports it has finished, so
    // main cannot return part-way through a shutdown and cut in-flight
    // requests and WebSockets. With no server running -- before it starts,
    // after it stops, when a launch failed, or when the webserver is disabled
    // entirely -- this handler ends the process itself.
    if let Err(e) = ctrlc::set_handler(move || {
        info!("Shutdown signal received");

        // Ask first, arm second. This handler is the only signal consumer in
        // the process now that the webserver's own is switched off, and ctrlc
        // stops running it if it ever panics -- leaving the daemon deaf to
        // every later signal, with only SIGKILL left. So the request that
        // actually stops the daemon happens before anything that could fail,
        // and the watchdog is spawned in a way that reports failure instead of
        // unwinding.
        let asked_the_server = handle_in_signal.request_stop();

        // The force-exit watchdog, armed whichever way the shutdown goes: one
        // that overruns and one that never gets going both end here rather
        // than at systemd's SIGKILL. Eight seconds sits above the six the
        // webserver can take -- its grace and mercy, pinned at two and three,
        // plus one -- and below the ten systemd allows.
        let force_shutdown_delay = Duration::from_secs(8);
        if let Err(e) = thread::Builder::new()
            .name("force-shutdown".to_string())
            .spawn(move || {
                thread::sleep(force_shutdown_delay);
                info!(
                    "Graceful shutdown timed out after {} seconds, forcing exit...",
                    force_shutdown_delay.as_secs()
                );
                std::process::exit(0);
            })
        {
            warn!("Could not arm the force-shutdown watchdog: {}", e);
        }

        if asked_the_server {
            info!("Asked the API server to shut down");
            return;
        }

        info!("No API server running, shutting down directly");
        r.store(false, Ordering::SeqCst);
    }) {
        eprintln!("Error: Failed to set Ctrl+C handler: {}", e);
        std::process::exit(1);
    }

    // Create an AudioController from the JSON configuration and store it in the singleton
    let controller = match AudioController::from_json(&controllers_config) {
        Ok(controller) => {
            info!("Successfully created AudioController from JSON configuration");
            controller
        }
        Err(e) => {
            error!("Failed to create AudioController from JSON: {}", e);
            eprintln!("Error: Failed to create AudioController: {}", e);
            eprintln!("Check your player configuration in {}", config_path_str);
            std::process::exit(1);
        }
    };

    // Initialize the AudioController singleton
    match AudioController::initialize_instance(controller.clone()) {
        Ok(_) => info!("AudioController singleton initialized successfully"),
        Err(e) => warn!("AudioController singleton initialization: {}", e),
    }

    // Initialize cover art providers
    audiocontrol::helpers::coverart_providers::register_all_providers();

    // Metadata enrichment -- slow cover art endpoints, Last.fm -- runs on
    // workers that know nothing about players. This is the whole of the seam:
    // the bridge forwards song and state changes into a channel, and the same
    // object answers with what was found, through apply_song_information. When
    // nothing is configured to read them, the bridge unsubscribes itself rather
    // than filling a channel no one reads.
    let now_playing = now_playing_bridge::start(&EventBus::instance());
    let sink = Arc::new(now_playing_bridge::ControllerSink(controller.clone()));
    let lastfm = lastfm_worker_config(&controllers_config);
    if !audiocontrol_metadata::now_playing::start(now_playing, sink.clone(), sink, lastfm) {
        info!("No now-playing enrichment is configured");
    }

    // Get a reference to the AudioController singleton
    let controller = AudioController::instance();

    // Start input sources (USB HID remotes). Must come after the volume control
    // and the AudioController singleton exist, so the first keypress can act.
    audiocontrol::inputs::init_inputs(&controllers_config, Arc::downgrade(&controller));

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
        debug!(
            "Current song: {} by {}",
            song.title.unwrap_or_else(|| "Unknown".to_string()),
            song.artist.unwrap_or_else(|| "Unknown".to_string())
        );
    } else {
        debug!("No song currently playing");
    }

    // Read spotify.api_enabled config (default: false)
    //
    // Read here rather than in the server: the routes it selects between now
    // come from the metadata crate, which the server does not name.
    let spotify_api_enabled = get_service_config(&controllers_config, "spotify")
        .and_then(|s| s.get("api_enabled"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    // Start the API server using the global Tokio runtime
    let controllers_config_clone = controllers_config.clone();
    let api_running = running.clone();
    let shutdown_handle_for_outcome = shutdown_handle.clone();
    let _api_thread = thread::spawn(move || {
        let outcome = get_tokio_runtime().block_on(async {
            // Get a reference to the singleton AudioController for the server
            let controller = AudioController::instance();
            server::start_rocket_server(
                controller,
                &controllers_config_clone,
                shutdown_handle,
                audiocontrol_metadata::api::routes(spotify_api_enabled),
            )
            .await
        });

        match outcome {
            // Rocket reports several ways of stopping as an error rather than
            // as Ok: a shutdown that overran its grace and mercy windows
            // (outstanding background I/O, which long-lived WebSocket
            // connections routinely produce), the server still executing after
            // them, and a server that failed while serving without any
            // shutdown having been requested. In all of them launch() has
            // returned and Rocket is no longer serving.
            // Rocket has stopped serving either way, so this is the same cue to
            // exit as ShutDown.
            //
            // Treating it as a failure to start was the original bug wearing a
            // different hat -- `running` stayed true, the loop below spun, and
            // systemd killed the process, on exactly the paths a busy daemon is
            // most likely to take. Rocket's own forced-shutdown backstop does
            // not cover it either: that lives in rocket::async_main, which only
            // #[rocket::main] and #[launch] go through, and this process
            // launches on its own runtime.
            Err(e) if matches!(e.kind(), rocket::error::ErrorKind::Shutdown(..)) => {
                warn!("API server shutdown did not complete cleanly: {}", e);
                api_running.store(false, Ordering::SeqCst);
            }

            // A server that never came up -- Bind, Config, Collisions,
            // FailedFairings -- leaves the rest of the daemon running, as it
            // always has. The means to stop it has been withdrawn by this
            // point, so the handler above ends the process itself and the
            // daemon can still be stopped.
            //
            // Unless a stop was already asked for. The port is bound inside
            // launch(), after the handle is published, so a signal arriving
            // while an outgoing instance still holds port 1080 is passed to a
            // server that then fails to start -- and nothing would report a
            // shutdown that never began. Ending the loop here makes that stop
            // clean rather than leaving it to the watchdog.
            Err(e) => {
                error!("API server error: {}", e);
                if shutdown_handle_for_outcome.stop_requested() {
                    info!("A stop was asked for while the API server was starting");
                    api_running.store(false, Ordering::SeqCst);
                }
            }

            // The handler above passes a signal to the running server and
            // leaves `running` alone, so this is the only thing that clears it
            // on an ordinary stop. Without it the main loop below spun forever
            // after the server had shut down, and systemd SIGKILLed the
            // process on every stop, restart and package upgrade.
            //
            // ShutDown is returned exactly when Rocket has finished its
            // graceful shutdown, which is the event main is waiting for.
            Ok(ServerOutcome::ShutDown) => {
                info!("API server stopped, shutting down");
                api_running.store(false, Ordering::SeqCst);
            }

            // Nothing was started, so nothing has shut down and nothing was
            // ever published for a signal to be passed to. The handler above
            // ends the process itself -- for SIGTERM and SIGHUP as well as
            // SIGINT, given the "termination" feature -- and remains the way
            // this process stops.
            Ok(ServerOutcome::Disabled) => {}
        }
    });

    // Purge variants at retired ladder sizes on its own thread, off the startup
    // path. This used to run inside ImageCache::initialize, before the server
    // bound its port, and held the global cache lock for the whole walk - the
    // daemon answered nothing for about a minute on the 0.12.0 upgrade.
    audiocontrol::helpers::imagepurge::purge_retired_in_background();

    // Keep the main thread alive until the API server stops, or until a
    // signal arrives with no server running to pass it to.
    while running.load(Ordering::SeqCst) {
        thread::sleep(Duration::from_millis(100));
    }

    info!("Exiting application");
}

// Helper function to initialize the global image cache
fn initialize_image_cache(image_cache_path: &str) {
    match ImageCache::initialize(image_cache_path) {
        Ok(_) => info!("Image cache initialized with path: {}", image_cache_path),
        Err(e) => warn!("Failed to initialize image cache: {}", e),
    }
}

// Helper function to initialize the global settings database
fn initialize_settingsdb(settingsdb_path: &str) {
    match SettingsDb::initialize(settingsdb_path) {
        Ok(_) => info!("Settings database initialized with path: {}", settingsdb_path),
        Err(e) => warn!("Failed to initialize settings database: {}", e),
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

// Helper function to initialize external cover art endpoints
fn initialize_external_coverart(config: &serde_json::Value) {
    audiocontrol::helpers::external_coverart::initialize_from_config(config);
    info!("External cover art initialized successfully");
}

// Helper function to initialize FanArt.tv
fn initialize_fanarttv(config: &serde_json::Value) {
    fanarttv::initialize_from_config(config);
    info!("FanArt.tv initialized successfully");
}

// Helper function to initialize configurator
fn initialize_configurator(config: &serde_json::Value) {
    audiocontrol::helpers::configurator::initialize_from_config(config);
    info!("Configurator initialized successfully");
}

/// The `action_plugins` entry named `lastfm`, if the configuration has one.
///
/// That entry used to configure an action plugin; it now configures the Last.fm
/// worker in the metadata crate. Same array, same key, same fields, so an
/// existing configuration file needs no change -- and the entry is still
/// reported by `GET /api/plugins/actions`, which its own registration in
/// `plugin_factory` takes care of.
fn lastfm_worker_config(config: &serde_json::Value) -> Option<LastfmWorkerConfig> {
    let entries = config.get("action_plugins")?.as_array()?;

    for entry in entries {
        let Some(value) = entry.get("lastfm") else {
            continue;
        };

        match serde_json::from_value::<LastfmWorkerConfig>(value.clone()) {
            Ok(config) => return Some(config),
            Err(e) => {
                error!(
                    "Failed to parse the 'lastfm' action_plugins entry: {}. Last.fm will not run.",
                    e
                );
                return None;
            }
        }
    }

    None
}

// Helper function to initialize Last.fm
fn initialize_lastfm(config: &serde_json::Value) {
    if let Some(lastfm_config) = get_service_config(config, "lastfm") {
        // Check if enabled flag exists and is set to true
        let enabled = lastfm_config
            .get("enable")
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
                    warn!(
                        "Could not get Last.fm client instance to check status: {}",
                        e
                    );
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

    if let Some(spotify_config) = get_service_config(config, "spotify") {
        // Check if enabled flag exists and is set to true
        let enabled = spotify_config
            .get("enable")
            .and_then(|v| v.as_bool())
            .unwrap_or(false); // Default to disabled if not specified

        info!("Spotify enabled in config: {}", enabled);

        if enabled {
            // Get custom OAuth URL and proxy secret if specified in config
            let oauth_url = spotify_config
                .get("oauth_url")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());

            let proxy_secret = spotify_config
                .get("proxy_secret")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());

            info!(
                "Config values - OAuth URL present: {}, proxy secret present: {}",
                oauth_url.is_some(),
                proxy_secret.is_some()
            );

            // Initialize with values from config or fall back to defaults
            let init_result = match (oauth_url, proxy_secret) {
                (Some(url), Some(secret)) if !url.is_empty() && !secret.is_empty() => {
                    info!(
                        "Initializing Spotify with configuration from audiocontrol.json, URL: '{}'",
                        url
                    );
                    spotify::Spotify::initialize(url, secret)
                }
                _ => {
                    info!(
                        "No valid Spotify config in audiocontrol.json, falling back to secrets.txt"
                    );
                    spotify::Spotify::initialize_with_defaults()
                }
            };
            if let Err(e) = init_result {
                warn!("Failed to initialize Spotify client: {}", e);

                // Additional logging to help diagnose the issue
                info!(
                    "Checking default OAuth URL directly: '{}'",
                    spotify::default_spotify_oauth_url()
                );

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
                }
                Err(e) => {
                    warn!(
                        "Could not get Spotify client instance to check status: {}",
                        e
                    );
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

/// Find config file path from command line arguments (-c option)
fn find_config_file_in_args(args: &[String]) -> Option<String> {
    let mut i = 1;
    while i < args.len() {
        if args[i] == "-c" && i + 1 < args.len() {
            info!("Using configuration file specified by -c: {}", args[i + 1]);
            return Some(args[i + 1].clone());
        }
        i += 1;
    }
    None
}

/// Find logging config file path from command line arguments (--log-config option)
fn find_log_config_in_args(args: &[String]) -> Option<PathBuf> {
    let mut i = 1;
    while i < args.len() {
        if (args[i] == "--log-config" || args[i] == "--logging-config") && i + 1 < args.len() {
            let path = PathBuf::from(&args[i + 1]);
            info!("Using logging configuration file: {}", path.display());
            return Some(path);
        }
        i += 1;
    }

    // Check for default logging config files
    let default_paths = [
        "/etc/audiocontrol/logging.json",
        "logging.json",
        "config/logging.json",
    ];

    for path_str in &default_paths {
        let path = PathBuf::from(path_str);
        if path.exists() {
            info!(
                "Found default logging configuration file: {}",
                path.display()
            );
            return Some(path);
        }
    }

    None
}

/// Check and display the status of compiled secrets
fn check_secrets_status() {
    println!("AudioControl - Compiled Secrets Status");
    println!("=====================================");

    // Get all compiled secrets
    let secrets_map = secrets::get_all_secrets_obfuscated();

    if secrets_map.is_empty() {
        println!("❌ No secrets compiled into binary");
        println!("   This binary was compiled without any secrets configured.");
        println!("   External API integrations will not work unless configured at runtime.");
        return;
    }

    println!("✅ Secrets compiled into binary: {}", secrets_map.len());
    println!();

    // Check specific known secrets
    let known_secrets = vec![
        ("LASTFM_APIKEY", "Last.fm API integration"),
        ("LASTFM_API_KEY", "Last.fm API integration"),
        ("LASTFM_APISECRET", "Last.fm API secret"),
        ("LASTFM_API_SECRET", "Last.fm API secret"),
        ("ARTISTDB_APIKEY", "TheAudioDB API integration"),
        ("THEAUDIODB_APIKEY", "TheAudioDB API integration"),
        ("THEAUDIODB_API_KEY", "TheAudioDB API integration"),
        ("SECRETS_ENCRYPTION_KEY", "Security store encryption"),
        ("SECURITY_KEY", "Security store encryption"),
        ("SPOTIFY_OAUTH_URL", "Spotify OAuth integration"),
        ("SPOTIFY_PROXY_SECRET", "Spotify proxy authentication"),
    ];

    println!("Known Integration Status:");
    println!("------------------------");

    let mut found_any = false;
    for (key, description) in known_secrets {
        if secrets_map.contains_key(key) {
            println!("✅ {} - {}", key, description);
            found_any = true;
        }
    }

    if !found_any {
        println!("⚠️  No known integration secrets found");
        println!(
            "   Available keys: {}",
            secrets_map.keys().cloned().collect::<Vec<_>>().join(", ")
        );
    }

    println!();
    println!("API Service Status:");
    println!("------------------");

    // Test specific service functions
    let lastfm_key = secrets::lastfm_api_key();
    let audiodb_key = secrets::artistdb_api_key();
    let encryption_key = secrets::secrets_encryption_key();
    let spotify_oauth = secrets::spotify_oauth_url();
    let spotify_secret = secrets::spotify_proxy_secret();

    println!(
        "🔑 Last.fm API: {}",
        if lastfm_key != "unknown" {
            "✅ Available"
        } else {
            "❌ Not configured"
        }
    );
    println!(
        "🔑 TheAudioDB API: {}",
        if audiodb_key != "unknown" {
            "✅ Available"
        } else {
            "❌ Not configured"
        }
    );
    println!(
        "🔑 Security Store: {}",
        if encryption_key != "unknown" {
            "✅ Available"
        } else {
            "❌ Not configured"
        }
    );
    println!(
        "🔑 Spotify OAuth: {}",
        if spotify_oauth != "unknown" {
            "✅ Available"
        } else {
            "❌ Not configured"
        }
    );
    println!(
        "🔑 Spotify Proxy: {}",
        if spotify_secret != "unknown" {
            "✅ Available"
        } else {
            "❌ Not configured"
        }
    );

    println!();
    println!("Note: This shows compile-time secrets only. Runtime configuration");
    println!("      may override these values or provide additional secrets.");
}

/// Print help information for command line usage
fn print_help() {
    println!("AudioControl Player Controller");
    println!("==============================");
    println!();
    println!("USAGE:");
    println!("    audiocontrol [OPTIONS]");
    println!();
    println!("OPTIONS:");
    println!("    -c <FILE>                   Specify configuration file path");
    println!("                                (default: audiocontrol.json)");
    println!();
    println!("    --log-config <FILE>         Specify logging configuration file");
    println!("    --logging-config <FILE>     (alternative form)");
    println!("                                Defaults searched in order:");
    println!("                                - /etc/audiocontrol/logging.json");
    println!("                                - logging.json");
    println!("                                - config/logging.json");
    println!();
    println!("    -d, --debug                 Enable debug logging (if no log config)");
    println!();
    println!("    -h, --help                  Show this help message");
    println!();
    println!("EXAMPLES:");
    println!("    audiocontrol");
    println!("        Start with default configuration");
    println!();
    println!("    audiocontrol -c /etc/audiocontrol/config.json");
    println!("        Start with specific configuration file");
    println!();
    println!("    audiocontrol --log-config /etc/audiocontrol/logging.json");
    println!("        Start with specific logging configuration");
    println!();
    println!("    audiocontrol --debug");
    println!("        Start with debug logging enabled");
    println!();
    println!("For more information, see the documentation in the doc/ directory.");
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The entry that used to configure the action plugin now configures the
    /// worker, read from the same array under the same key. An existing
    /// configuration file has to keep working untouched, scrobbling included.
    #[test]
    fn the_lastfm_action_plugins_entry_configures_the_worker() {
        let config = serde_json::json!({
            "action_plugins": [
                { "active-monitor": { "enabled": true } },
                {
                    "lastfm": {
                        "enabled": true,
                        "api_key": "key",
                        "api_secret": "secret",
                        "scrobble": false
                    }
                }
            ]
        });

        let lastfm = lastfm_worker_config(&config).expect("the entry should be found");
        assert!(lastfm.enabled);
        assert_eq!(lastfm.api_key, "key");
        assert_eq!(lastfm.api_secret, "secret");
        assert!(!lastfm.scrobble);
    }

    /// `scrobble` has always defaulted to true when the key is absent, and the
    /// worker reads the same field, so the default has to survive the move.
    #[test]
    fn scrobble_still_defaults_to_true() {
        let config = serde_json::json!({
            "action_plugins": [
                { "lastfm": { "enabled": true, "api_key": "", "api_secret": "" } }
            ]
        });

        let lastfm = lastfm_worker_config(&config).expect("the entry should be found");
        assert!(lastfm.scrobble);
    }

    /// A disabled entry is still an entry: it is read, and the worker declines
    /// to start on it, which is what the plugin used to do with it.
    #[test]
    fn a_disabled_entry_is_read_rather_than_ignored() {
        let config = serde_json::json!({
            "action_plugins": [
                { "lastfm": { "enabled": false, "api_key": "", "api_secret": "" } }
            ]
        });

        let lastfm = lastfm_worker_config(&config).expect("the entry should be found");
        assert!(!lastfm.enabled);
    }

    #[test]
    fn no_action_plugins_and_no_lastfm_entry_both_mean_no_worker() {
        assert!(lastfm_worker_config(&serde_json::json!({})).is_none());
        assert!(lastfm_worker_config(&serde_json::json!({ "action_plugins": [] })).is_none());
        assert!(lastfm_worker_config(&serde_json::json!({
            "action_plugins": [{ "active-monitor": { "enabled": true } }]
        }))
        .is_none());
    }

    /// An entry missing the credentials the worker needs is a configuration
    /// error, and starting a worker on a guess would be worse than not starting
    /// one.
    #[test]
    fn an_unusable_entry_starts_no_worker() {
        let config = serde_json::json!({
            "action_plugins": [{ "lastfm": { "enabled": true } }]
        });

        assert!(lastfm_worker_config(&config).is_none());
    }
}
