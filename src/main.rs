//! The composition root.
//!
//! This is the one file in the package that names both crates. The library
//! `audiocontrol` knows nothing about `audiocontrol-metadata`: it asks for
//! enrichment, title resolution and Spotify access tokens through traits in
//! `acr-types`, and `main` is what decides which implementation answers. In
//! Phase 0 that is the in-process metadata crate, linked behind the default
//! `metadata` feature; in Phase 1 it becomes a client for a separate daemon,
//! and only this file changes.

// The global allocator, on Linux, where this actually ships.
//
// **It has to be declared in the binary crate.** `#[global_allocator]` in
// `lib.rs` would compile and do nothing for the daemon -- the binary's choice
// is what the whole process uses -- which is exactly the kind of change that
// looks applied and is not. Anything linking the library (the test binaries)
// keeps the system allocator; that is fine, since this is about the daemon's
// resident set over hours, not about a test run.
//
// Measured before adding it: a Pi 5 holding a 202,393-song library sat at
// 370 MB RSS under glibc and 255 MB under jemalloc, with load time unchanged.
// That measurement used an LD_PRELOADed *unprefixed* jemalloc, which covers
// the whole process; this dependency's symbols are prefixed, so it takes
// Rust's allocations only. See Cargo.toml for the full figures, the caveat,
// and the MALLOC_ARENA_MAX result that did *not* help.
#[cfg(target_os = "linux")]
#[global_allocator]
static GLOBAL: tikv_jemallocator::Jemalloc = tikv_jemallocator::Jemalloc;

use audiocontrol::api::server::{self, ServerOutcome};
use audiocontrol::audiocontrol::metadata_client::MetadataClient;
use audiocontrol::config::{get_service_config, merge_player_includes};
use audiocontrol::helpers::imagecache::ImageCache;
use audiocontrol::helpers::settingsdb::SettingsDb;
use audiocontrol::logging;
use audiocontrol::players::PlayerController;
use audiocontrol::AudioController;
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

/// What a build without the `metadata` feature says for itself, once, where
/// the metadata providers would have been brought up.
///
/// `metadata` is in `default`, so the shipped daemon never reaches any of
/// these branches. They exist so that dropping the feature produces a daemon
/// that says what it cannot do rather than one that silently serves an
/// unenriched library.
#[cfg(not(feature = "metadata"))]
const WITHOUT_METADATA: &str =
    "built without the metadata crate; no enrichment, no resolver, no Spotify transport";

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
    //
    // The secrets it reports on are compiled into the metadata crate, so the
    // report lives there too and this only decides whether to ask for it.
    if args.iter().any(|arg| arg == "--check-secrets") {
        #[cfg(feature = "metadata")]
        audiocontrol_metadata::check_secrets_status();
        #[cfg(not(feature = "metadata"))]
        println!("{}", WITHOUT_METADATA);
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

    // Stays here rather than joining `initialize_in_process` below: it has to
    // run before the attribute cache and the settings database, both of which
    // are set up between this point and there.
    #[cfg(feature = "metadata")]
    if let Err(e) = audiocontrol_metadata::security_store::SecurityStore::initialize_with_defaults(
        Some(security_store_path.clone()),
    ) {
        error!("Failed to initialize security store at {}: {}. Please check permissions and configuration.", security_store_path.display(), e);
        eprintln!("Error: Security store initialization failed: {}", e);
        eprintln!("Check permissions and configuration at {}", security_store_path.display());
        std::process::exit(1);
    } else {
        info!(
            "Security store initialized successfully at {}",
            security_store_path.display()
        );
    }
    #[cfg(not(feature = "metadata"))]
    let _ = &security_store_path;
    // Get the attribute cache configuration from datastore
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

    // Initialize configurator with the configuration
    //
    // This used to sit between the cover art and Last.fm initialisations
    // below, which are now one call into the metadata crate. It reads only the
    // `configurator` section and writes only its own URL, and nothing in the
    // metadata crate can see that URL -- it does not link this package -- so
    // where it sits among them cannot matter.
    initialize_configurator(&controllers_config);

    // MusicBrainz, TheAudioDB, FanArt.tv, the external cover art endpoints,
    // Last.fm and Spotify, in the order they have always been brought up.
    #[cfg(feature = "metadata")]
    audiocontrol_metadata::initialize_in_process(&controllers_config);
    #[cfg(not(feature = "metadata"))]
    info!("{}", WITHOUT_METADATA);

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
    //
    // Stays here rather than joining `initialize_in_process`: one of the two
    // providers is the settings database, and the volume control between them
    // is set up after it.
    #[cfg(feature = "metadata")]
    audiocontrol_metadata::favourites::initialize_favourite_providers();

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

        // The metadata side's subscriber holds a WebSocket open against this
        // process's own API, and Rocket waits out its whole grace period for
        // open I/O. What brings `systemctl stop` back from 5 s to 2 s is the
        // server closing the connection from its end -- see `run_client_loop`.
        // This call is not that. It stops the subscriber reconnecting into a
        // daemon that is going away, and it shares the flag `wait_for_core`
        // watches, which is what turns a signal during start-up from the full
        // 8 s force-exit into 0.14 s. Second, not first: `request_stop` above
        // is what actually stops the daemon, and nothing that could fail may
        // come before it.
        #[cfg(feature = "metadata")]
        audiocontrol_metadata::startup::stop();

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
    //
    // Stays here rather than joining `initialize_in_process`: the providers are
    // registered only once the AudioController exists.
    #[cfg(feature = "metadata")]
    audiocontrol_metadata::coverart_providers::register_all_providers();

    // Library enrichment, resolvers and Spotify access tokens -- the three
    // player-side seams the metadata side now answers over HTTP.
    // `MetadataClient` lives in this package and names nothing from
    // `audiocontrol-metadata`, so building and installing it does not need
    // the `metadata` feature: even a `--no-default-features` daemon reaches
    // the metadata side over loopback once `services.metadata` is
    // configured, which is the phase working as intended. With no
    // `services.metadata` section, nothing is installed and every caller
    // keeps the offline fallback it already has (see `resolver`, `token` and
    // `enrichment` in this crate).
    //
    // Installed before any player starts, so no library, title splitter or
    // librespot backend finds any of the three missing -- the same lifetime
    // rule the old in-process setters kept.
    match MetadataClient::from_config(&controllers_config) {
        Some(client) => {
            let client = Arc::new(client);
            audiocontrol::audiocontrol::enrichment::set_enricher(client.clone());
            audiocontrol::audiocontrol::resolver::set_resolver(client.clone());
            audiocontrol::audiocontrol::token::set_token_source(client);
        }
        None => {
            info!("services.metadata is not configured: no resolver, Spotify token source or library enricher installed");
        }
    }

    // Metadata enrichment -- slow cover art endpoints, Last.fm -- runs on
    // workers that know nothing about players, and no longer starts here.
    // Both halves of that seam are HTTP now: the events arrive over the
    // daemon's own WebSocket and results go back through
    // `POST /api/player/<name>/song-information`, so the subscriber cannot
    // start until the server has bound its port. See
    // `audiocontrol_metadata::startup::start_after_core_is_listening`, called
    // after the API thread below. `now_playing_bridge` stays in the library,
    // and is exercised by its own tests, but nothing in the daemon uses it.

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

    // The route groups the server does not own. Every one of them is a
    // metadata route, so a build without the metadata crate mounts none.
    #[cfg(feature = "metadata")]
    let extra_routes = metadata_route_groups(spotify_api_enabled);
    #[cfg(not(feature = "metadata"))]
    let extra_routes: Vec<(String, Vec<rocket::Route>)> = {
        let _ = spotify_api_enabled;
        Vec::new()
    };

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
                extra_routes,
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

    // The rest of the metadata side: the now-playing subscriber and the
    // library puller, both of which are HTTP clients of the server the thread
    // above just started. `initialize_in_process` (near the top of this
    // function) cannot host them -- it runs before anything has bound a port,
    // so the subscriber's first connection would be refused and the puller's
    // first sweep would fail.
    //
    // This blocks while it waits for the server to answer `GET /api/version`.
    // It ends three ways: the server answers, the 30 s bound expires and the
    // subscriber starts anyway to let its own reconnect logic take over, or a
    // signal arrives and it gives up at once. The third matters as much as the
    // other two -- this wait runs on the thread that ends the process, so
    // before it could be interrupted a SIGTERM during start-up cost the full
    // 8 s force-exit below rather than the 0.14 s it costs now.
    //
    // Placed after the purge above so a slow start here cannot delay it, and
    // before the keep-alive loop because there is nothing left to do first.
    #[cfg(feature = "metadata")]
    audiocontrol_metadata::startup::start_after_core_is_listening(&controllers_config);

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

// Helper function to initialize configurator
fn initialize_configurator(config: &serde_json::Value) {
    audiocontrol::helpers::configurator::initialize_from_config(config);
    info!("Configurator initialized successfully");
}

/// Every route group the metadata crate contributes, and where each mounts
/// relative to the daemon's `/api` prefix.
///
/// There are two mounts, and they are not interchangeable.
///
/// The **bare** mounts -- `""`, `/lastfm`, `/spotify`, `/favourites`,
/// `/coverart` -- are where these routes have always been served, and where
/// `services.metadata.url` points: `http://127.0.0.1:1080/api`. The player
/// side's own `MetadataClient` calls them over loopback, so moving or
/// renaming them would break this process's conversation with itself as well
/// as every shipped client.
///
/// The **`/metadata`** mounts are the client-facing address the spec gives
/// this side, the one nginx routes to the separate daemon in Phase 2. It
/// carries the same set *plus* `api::standalone_routes()`, which is
/// `GET /capabilities`. That route cannot join the bare group: the daemon
/// serves `GET /api/capabilities` itself, and two routes at one method, path
/// and rank make Rocket refuse to ignite -- the daemon would not start at
/// all. Under `/api/metadata` there is nothing to collide with, and
/// `/api/metadata/capabilities` is where the spec puts it.
///
/// `imagecache` rides along under `/metadata` for the same reason: it lives
/// in `acr-web` because both sides serve it, and the spec's nginx snippet
/// routes `/imagecache/external/` to the metadata daemon.
///
/// Every `routes()` call here is a *second call*, never a clone: Rocket
/// refuses to mount one `Route` value at two mount points, so the two sets
/// have to be built independently.
#[cfg(feature = "metadata")]
fn metadata_route_groups(spotify_api_enabled: bool) -> Vec<(String, Vec<rocket::Route>)> {
    let mut groups = audiocontrol_metadata::api::routes(spotify_api_enabled);

    for (mount, routes) in audiocontrol_metadata::api::routes(spotify_api_enabled) {
        groups.push((format!("/metadata{}", mount), routes));
    }
    for (mount, routes) in audiocontrol_metadata::api::standalone_routes() {
        groups.push((format!("/metadata{}", mount), routes));
    }
    groups.push((
        "/metadata/imagecache".to_string(),
        audiocontrol::api::imagecache::routes(),
    ));

    groups
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

    /// The class of bug this guards against already happened once: the
    /// metadata crate's plan for `GET /capabilities` put it in
    /// `audiocontrol_metadata::api::routes(..)`'s `""` group, which mounts at
    /// the same `API_PREFIX` this daemon's own `api_routes()` mounts at, and
    /// the daemon already serves `GET /capabilities` there itself. Two
    /// routes at the same method, path and rank make Rocket refuse to
    /// ignite rather than pick one -- see `src/api/server.rs`'s comment on
    /// `extra_routes`, and the `capabilities.rs` module doc in the metadata
    /// crate that this test exists precisely so nobody has to remember that
    /// history to avoid repeating it.
    ///
    /// Two things are checked, because there are now two mounts.
    ///
    /// First, no group collides with the daemon's own routes -- the original
    /// check, now over [`metadata_route_groups`] rather than
    /// `api::routes(..)`, so the `/metadata` mounts are covered by it too.
    ///
    /// Second, the groups do not collide with *each other*. That is what
    /// catches `standalone_routes()` being appended to the shared set instead
    /// of the `/metadata` one, or the same group being pushed twice at one
    /// mount: either is a daemon that will not start, and neither shows up in
    /// the first check.
    ///
    /// A Rocket built from these lists is still not stood up here -- doing
    /// that needs the managed state the whole of `main` assembles -- so what
    /// is compared is (method, full path), which is what a Rocket collision
    /// actually turns on.
    #[cfg(feature = "metadata")]
    #[test]
    fn the_metadata_crates_routes_do_not_collide_with_the_daemons_own() {
        use audiocontrol::api::server;
        use audiocontrol::constants::API_PREFIX;
        use rocket::http::Method;
        use std::collections::HashSet;

        fn full_path(prefix: &str, route: &rocket::Route) -> (Method, String) {
            (route.method, format!("{}{}", prefix, route.uri.path()))
        }

        let daemon_routes: HashSet<(Method, String)> = server::api_routes()
            .iter()
            .map(|route| full_path(API_PREFIX, route))
            .collect();

        // If this is empty the loop below would pass vacuously; make sure
        // there is actually something in it to collide with.
        assert!(daemon_routes.contains(&(Method::Get, format!("{}/capabilities", API_PREFIX))));

        // Both branches. `spotify.api_enabled` adds four routes to the
        // `/spotify` group, and therefore to both of its mounts; a daemon
        // that ignites with the flag off and refuses to start with it on
        // would be a configuration option that breaks the daemon, found by
        // whoever set it.
        for spotify_api_enabled in [false, true] {
            let mut mounted: HashSet<(Method, String)> = HashSet::new();
            for (mount, routes) in metadata_route_groups(spotify_api_enabled) {
                let prefix = format!("{}{}", API_PREFIX, mount);
                for route in &routes {
                    let key = full_path(&prefix, route);
                    assert!(
                        !daemon_routes.contains(&key),
                        "metadata route {:?} {} collides with a route the daemon already mounts there (spotify.api_enabled = {})",
                        key.0,
                        key.1,
                        spotify_api_enabled
                    );
                    assert!(
                        mounted.insert(key.clone()),
                        "metadata route {:?} {} is mounted twice by metadata_route_groups (spotify.api_enabled = {})",
                        key.0,
                        key.1,
                        spotify_api_enabled
                    );
                }
            }

            // The client-facing mount actually carries the capabilities route,
            // which is the whole reason `standalone_routes()` is in the set: a
            // check that only looks for collisions passes just as happily when
            // nothing is mounted at all.
            assert!(
                mounted.contains(&(Method::Get, format!("{}/metadata/capabilities", API_PREFIX))),
                "the /metadata mount should serve the capabilities route"
            );
        }
    }
}
