//! The metadata daemon's composition root.
//!
//! The mirror of `src/main.rs` in the root package, for the other half. It
//! reads its own configuration file, brings up the stores and providers this
//! crate needs, serves this crate's routes on its own port, and then starts the
//! two parts of it that are *clients* of the player daemon.
//!
//! It lives in this crate rather than as a `[[bin]]` of the root package
//! because a binary there would link the player library, and with it ALSA,
//! D-Bus, MPD and evdev, into a daemon that needs none of them. The rule
//! `scripts/check-crate-deps.sh` enforces -- neither daemon crate depends on
//! the other -- would still hold, but the goal behind it, that either daemon
//! can be built without the other, would not.
//!
//! ## Where this differs from the player daemon's `main`
//!
//! **It refuses to start without an explicit `core.url`.** That is the one
//! decision in this file worth reading the reasoning for, and it is written out
//! at `startup::required_core_settings`. In one process the missing key falls
//! back to the port the same process is about to bind, which is right; here it
//! would derive this daemon's own port, and this daemon serves neither the
//! event stream nor the library it would then be reading. Nothing about that
//! fails loudly, so the gate is the only thing that catches it.
//!
//! **The seam is one-way, so nothing here waits on this daemon's own server.**
//! `startup::start_after_core_is_listening` waits for the *player* daemon's
//! API, in another process, which is why it can be called from the main thread
//! while Rocket serves on another. The order is still the player daemon's:
//! the server first, so `/api/metadata/capabilities` answers during the up-to
//! 30 s probe rather than after it.
//!
//! **There is no `AudioController` and no managed state.** Every route this
//! crate serves reads process-wide singletons, which is what lets the same
//! `api::routes()` be mounted by either daemon.
//!
//! ## What it does not do
//!
//! It does not read `--log-config`. The player daemon's `logging.json` reader
//! lives in the root package (`src/logging.rs`), which this crate must not
//! depend on, and sharing it means a seventh workspace crate -- a structural
//! change this file is not the place to make. Logging here is `env_logger`:
//! `RUST_LOG`, or `--debug` for everything at debug. A `--log-config` argument
//! is reported as ignored rather than silently accepted.

use std::path::{Path, PathBuf};
use std::process::exit;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;
use std::thread;

use acr_types::config::get_service_config;
use acr_types::API_PREFIX;
use log::{error, info, warn};
use parking_lot::Mutex;
use rocket::config::Config;
use rocket::data::{Limits, ToByteUnit};
use rocket::figment::Figment;

use audiocontrol_metadata::{api, coverart_providers, favourites, startup};

/// The port this daemon binds when `services.webserver.port` says nothing.
///
/// The spec's value, and the one `configs/metadata.json` writes out. It is
/// emphatically *not* the player daemon's 1080, and the gate on `core.url`
/// exists because a configuration that leaves the seam to be derived would
/// arrive back at this number.
const DEFAULT_PORT: u64 = 1084;

/// Where the running server publishes the means to stop it.
///
/// A process-wide slot for the same reason the player daemon has one: the
/// signal handler is registered before the server exists and cannot be handed
/// something that has not been built yet.
fn shutdown_slot() -> &'static Mutex<Option<rocket::Shutdown>> {
    static SLOT: OnceLock<Mutex<Option<rocket::Shutdown>>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(None))
}

/// Whether a stop has been asked for, for the window before the server has
/// published anything.
///
/// Rocket binds its port inside `launch()`, after the handle exists, so a
/// signal can arrive between `ignite()` and the first request. Without this the
/// server would go on to serve normally and the signal would be lost.
fn stop_requested() -> &'static AtomicBool {
    static REQUESTED: OnceLock<AtomicBool> = OnceLock::new();
    REQUESTED.get_or_init(|| AtomicBool::new(false))
}

fn main() {
    let args: Vec<String> = std::env::args().collect();

    if args.iter().any(|a| a == "--help" || a == "-h") {
        print_help();
        return;
    }

    if args.iter().any(|a| a == "--check-secrets") {
        audiocontrol_metadata::check_secrets_status();
        return;
    }

    initialize_logging(&args);

    info!("AudioControl metadata service starting");

    let config_path = argument_after(&args, "-c").unwrap_or_else(|| {
        info!("No configuration file specified, using default: metadata.json");
        "metadata.json".to_string()
    });
    let config = load_config(Path::new(&config_path));

    // **The gate, before anything else touches the disk or a port.**
    //
    // First so that a configuration error is reported as one: a daemon that
    // opened its caches, bound 1084 and only then complained would make the
    // failure look like whichever of those went wrong, and a daemon that
    // checked nothing at all would run on talking to itself. Nothing below
    // this point is reached by a file without a `core.url`.
    //
    // The settings are read again by `start_after_core_is_listening` at the
    // bottom of this function, through `core_settings`. That is not a second
    // opinion: with `core.url` given the two readings are the same one, pinned
    // by `an_explicit_core_url_reads_the_same_either_way`. Reading it here is
    // what turns "the daemon would misbehave" into "the daemon does not start".
    let core = match startup::required_core_settings(&config) {
        Ok(core) => core,
        Err(e) => {
            error!("Cannot start: {}", e);
            eprintln!("Error: {}", e);
            eprintln!(
                "Add a \"core\" section to {} naming the player daemon's API root.",
                config_path
            );
            exit(1);
        }
    };
    info!(
        "The player daemon's API is at {}; sweeping its libraries every {:?}",
        core.url, core.poll
    );

    initialize_stores(&config);

    // MusicBrainz, TheAudioDB, FanArt.tv, the external cover art endpoints and
    // Last.fm, in the order the player daemon brings them up in-process.
    audiocontrol_metadata::initialize_in_process(&config);

    // After the settings database, which one of the two providers is.
    favourites::initialize_favourite_providers();

    // The cover art providers. In the player daemon these wait for the
    // `AudioController` to exist; there is none here, and none of them needs
    // one -- this crate cannot see that type at all.
    coverart_providers::register_all_providers();

    install_signal_handler();

    // Purge variants at retired ladder sizes on its own thread. The downloaded
    // image cache is this daemon's, so this walk is this daemon's; the player
    // daemon runs the same purge over its own tree.
    acr_store::imagepurge::purge_retired_in_background();

    let server = start_webserver(&config);

    // The now-playing subscriber and the library puller, both clients of the
    // *other* process. This blocks while it probes `GET /api/version` there,
    // for up to 30 s, and then starts them anyway so their own reconnect and
    // retry logic takes over. A signal arriving during the probe cuts it short
    // -- `startup::stop` shares the flag it watches, which is why the handler
    // above calls it.
    startup::start_after_core_is_listening(&config);

    match server {
        Some(server) => {
            // The server owns the lifetime of the process from here: it returns
            // when Rocket has finished its graceful shutdown, which is the
            // event to exit on.
            if server.join().is_err() {
                error!("The API server thread panicked");
                exit(1);
            }
        }
        // No server to wait on, so wait for the signal handler instead. The
        // subscriber and the puller are still running and are the whole of what
        // this daemon does in this configuration.
        None => {
            while !stop_requested().load(Ordering::SeqCst) {
                thread::sleep(std::time::Duration::from_millis(100));
            }
        }
    }

    info!("Exiting application");
}

/// `env_logger`, with `--debug` as a shortcut for everything at debug.
///
/// `--log-config` is named explicitly so that passing it is not silently
/// ignored: the shipped systemd unit passes it to the player daemon, and an
/// operator who copies that line here should be told the file is not read
/// rather than left wondering why it has no effect.
fn initialize_logging(args: &[String]) {
    let debug = args.iter().any(|a| a == "--debug" || a == "-d");

    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or(if debug { "debug" } else { "info" }),
    )
    .init();

    if args.iter().any(|a| a == "--log-config" || a == "--logging-config") {
        warn!(
            "--log-config is not read by the metadata daemon; set RUST_LOG, or pass \
             --debug, instead"
        );
    }
}

/// The value after `flag`, if the flag is present with one.
fn argument_after(args: &[String], flag: &str) -> Option<String> {
    args.windows(2)
        .find(|pair| pair[0] == flag)
        .map(|pair| pair[1].clone())
}

/// Read and parse the configuration, or exit.
///
/// A missing or unparseable file is fatal, as it is for the player daemon: a
/// metadata daemon running on defaults would open caches in the working
/// directory and reach for a player daemon it had not been told about.
fn load_config(path: &Path) -> serde_json::Value {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) => {
            error!("Failed to read {}: {}", path.display(), e);
            eprintln!("Error: Failed to read {}: {}", path.display(), e);
            eprintln!("Cannot continue without a valid configuration file.");
            exit(1);
        }
    };

    match serde_json::from_str(&text) {
        Ok(config) => {
            info!("Successfully loaded configuration from {}", path.display());
            config
        }
        Err(e) => {
            error!("Failed to parse {}: {}", path.display(), e);
            eprintln!("Error: Failed to parse {}: {}", path.display(), e);
            eprintln!("Cannot continue without a valid configuration file.");
            exit(1);
        }
    }
}

/// The process-wide singletons this crate's providers and routes read.
///
/// The order is the player daemon's, minus everything that belongs to it: the
/// credential store first, because the Last.fm client reads it as it comes up;
/// then the caches and the settings database, which the providers and the
/// artist store read; then the image size ladder, which has to be resolved
/// before anything can name a cached variant.
fn initialize_stores(config: &serde_json::Value) {
    initialize_security_store(config);

    let attribute_cache = get_service_config(config, "datastore")
        .and_then(|d| d.get("attribute_cache"))
        .cloned();
    match attribute_cache {
        Some(section) => {
            if let Err(e) =
                acr_store::attributecache::AttributeCache::initialize_from_config(&section)
            {
                error!(
                    "Failed to initialize the attribute cache from configuration: {}",
                    e
                );
            }
        }
        None => {
            let default_path = "/var/lib/audiocontrol/metadata/attributes.db";
            info!(
                "No datastore.attribute_cache configuration, using default path: {}",
                default_path
            );
            if let Err(e) =
                acr_store::attributecache::AttributeCache::initialize_global(default_path)
            {
                error!("Failed to initialize the attribute cache: {}", e);
            }
        }
    }

    let images = get_service_config(config, "images");
    acr_images::imageresize::configure(
        acr_images::imageresize::sizes_from_json("sizes", images),
        acr_images::imageresize::sizes_from_json("prewarm_sizes", images),
    );
    info!(
        "Image sizes: {:?}, pre-warm sizes: {:?}",
        acr_images::imageresize::sizes(),
        acr_images::imageresize::prewarm_sizes()
    );

    let image_cache_path = get_service_config(config, "datastore")
        .and_then(|d| d.get("image_cache_path"))
        .and_then(|p| p.as_str())
        .unwrap_or("/var/lib/audiocontrol/metadata/images")
        .to_string();
    match acr_store::imagecache::ImageCache::initialize(&image_cache_path) {
        Ok(_) => info!("Image cache initialized with path: {}", image_cache_path),
        Err(e) => warn!("Failed to initialize image cache: {}", e),
    }

    // A *directory*, not a file: `settings.db` is appended to it. A path
    // ending in `.db` produces a directory of that name with the database
    // inside, which is not what any operator writing it meant.
    let settingsdb_path = get_service_config(config, "settingsdb")
        .and_then(|s| s.get("path"))
        .and_then(|p| p.as_str())
        .unwrap_or("/var/lib/audiocontrol/metadata")
        .to_string();
    match acr_store::settingsdb::SettingsDb::initialize(&settingsdb_path) {
        Ok(_) => info!(
            "Settings database initialized with path: {}",
            settingsdb_path
        ),
        Err(e) => warn!("Failed to initialize settings database: {}", e),
    }

    // Album genres from TheAudioDB are cleaned through the same mapping the
    // player daemon applies to the library. Uninitialised this does not fail,
    // it silently stops mapping, so it is brought up rather than left out.
    if let Err(e) = acr_store::genre_cleanup::initialize_genre_cleanup_with_config(Some(config)) {
        warn!("Failed to initialize genre cleanup: {}", e);
    }
}

/// The credential store: this daemon's Last.fm session key and username.
///
/// Its own file, not the player daemon's. Both processes keep the whole store
/// in memory and rewrite it whole, so a shared file would lose whichever
/// daemon's write came first.
///
/// Fatal on failure, as in the player daemon: a metadata daemon that cannot
/// open its store cannot scrobble, cannot report Last.fm as connected, and
/// would answer the Last.fm routes with an error on every call.
fn initialize_security_store(config: &serde_json::Value) {
    let path = PathBuf::from(
        get_service_config(config, "security_store")
            .and_then(|s| s.get("path"))
            .and_then(|p| p.as_str())
            .unwrap_or_else(|| {
                info!(
                    "No security_store path specified in configuration, using default \
                     'secrets/security_store.json'"
                );
                "secrets/security_store.json"
            }),
    );

    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() && !parent.exists() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                error!(
                    "Failed to create directory for the security store at {}: {}",
                    parent.display(),
                    e
                );
            }
        }
    }

    if let Err(e) = acr_secrets::security_store::SecurityStore::initialize_with_defaults(
        &acr_secrets::secrets::secrets_encryption_key(),
        Some(path.clone()),
    ) {
        error!(
            "Failed to initialize the security store at {}: {}",
            path.display(),
            e
        );
        eprintln!("Error: Security store initialization failed: {}", e);
        eprintln!("Check permissions and configuration at {}", path.display());
        exit(1);
    }
    info!(
        "Security store initialized successfully at {}",
        path.display()
    );
}

/// SIGINT, SIGTERM and SIGHUP, for the whole life of the process.
///
/// Rocket's own signal handling is switched off in [`rocket_config`], so this
/// is the only handler. Two things happen, in this order and for the reasons
/// the player daemon's handler documents at length: the metadata side is asked
/// to stop first, because the flag it sets is what cuts short the up-to-30 s
/// wait for the player daemon's API on the main thread -- without it a
/// `systemctl stop` moments after a start waits the probe out and systemd
/// SIGKILLs the daemon. Then the server is asked to shut down, if one is
/// running; with none, this ends the process itself.
fn install_signal_handler() {
    if let Err(e) = ctrlc::set_handler(|| {
        info!("Shutdown signal received");
        stop_requested().store(true, Ordering::SeqCst);
        startup::stop();

        let shutdown = shutdown_slot().lock().clone();
        match shutdown {
            Some(shutdown) => {
                info!("Asked the API server to shut down");
                shutdown.notify();
            }
            None => info!("No API server running; the daemon will stop on its own"),
        }
    }) {
        eprintln!("Error: Failed to set the signal handler: {}", e);
        exit(1);
    }
}

/// Rocket's configuration.
///
/// The JSON body limit is the player daemon's, and has to be: this daemon is
/// the one that serves `POST /coverart/artist/<b64>/upload`, which takes a
/// base64-encoded image, and Rocket's 1 MiB default would reject an image over
/// roughly 768 KB with a 413 before the handler saw it. The shutdown windows
/// are pinned for the same reason they are there -- `Config::figment()` also
/// reads `Rocket.toml` and `ROCKET_SHUTDOWN_*` from the environment, and a
/// grace period set from outside would outlast systemd's patience.
fn rocket_config(host: &str, port: u64) -> Figment {
    Config::figment()
        .merge(("port", port))
        .merge(("address", host))
        .merge(("shutdown.ctrlc", false))
        .merge(("shutdown.signals", Vec::<String>::new()))
        .merge(("shutdown.grace", 2))
        .merge(("shutdown.mercy", 3))
        .merge(("limits", Limits::default().limit("json", 4.mebibytes())))
}

/// Every route group this daemon mounts, and where each goes below `/api`.
///
/// Three sets, and the duplication is deliberate.
///
/// The **bare** mounts -- `""`, `/lastfm`, `/favourites`, `/coverart` -- are
/// the paths nginx forwards the compatibility prefixes to
/// (`/api/audiocontrol/coverart/` to `/api/coverart/`, and so on), and the
/// paths this daemon writes into its own responses: `thumb_url` and the
/// localized external image URLs are `/api/coverart/...` and
/// `/api/imagecache/external/...`, rewritten with the forwarded prefix on the
/// way out.
///
/// The **`/metadata`** mounts are the address the spec gives this daemon for
/// newer clients, and the only place `standalone_routes()` may go: that set is
/// `GET /capabilities`, which the player daemon serves at `/api/capabilities`
/// itself, so putting it at the bare mount here would give two daemons two
/// different answers at one path.
///
/// **`imagecache` is mounted twice**, at both. nginx sends
/// `/api/audiocontrol/imagecache/external/` to the bare one and
/// `/api/metadata/imagecache/` to the other; the player daemon keeps serving
/// its own `/api/imagecache/` over its own tree for the images it produces.
///
/// Every `routes()` call is a fresh call rather than a clone: Rocket refuses to
/// mount one `Route` value at two mount points.
fn route_groups() -> Vec<(String, Vec<rocket::Route>)> {
    let mut groups = api::routes();
    groups.push(("/imagecache".to_string(), acr_web::imagecache::routes()));

    for (mount, routes) in api::routes() {
        groups.push((format!("/metadata{}", mount), routes));
    }
    for (mount, routes) in api::standalone_routes() {
        groups.push((format!("/metadata{}", mount), routes));
    }
    groups.push((
        "/metadata/imagecache".to_string(),
        acr_web::imagecache::routes(),
    ));

    groups
}

/// Bring up the API server on its own thread, or nothing if it is disabled.
///
/// Its own thread, and not the main one, because the main thread has to go on
/// to wait for the *player* daemon's API: doing that first would delay this
/// daemon's own routes by the whole probe, and doing it after `launch()`
/// returned would mean never.
fn start_webserver(config: &serde_json::Value) -> Option<thread::JoinHandle<()>> {
    let webserver = get_service_config(config, "webserver");

    let enabled = webserver
        .and_then(|ws| ws.get("enable"))
        .and_then(|e| e.as_bool())
        .unwrap_or(true);
    if !enabled {
        warn!(
            "The webserver is disabled in configuration. Enrichment and scrobbling still \
             run, but nothing serves cover art, the artist store, favourites, the Last.fm \
             routes or /api/metadata/capabilities."
        );
        return None;
    }

    let host = webserver
        .and_then(|ws| ws.get("host"))
        .and_then(|h| h.as_str())
        .unwrap_or("127.0.0.1")
        .to_string();
    let port = webserver
        .and_then(|ws| ws.get("port"))
        .and_then(|p| p.as_u64())
        .unwrap_or(DEFAULT_PORT);

    info!("Starting webserver on {}:{}", host, port);

    let handle = thread::Builder::new()
        .name("api-server".to_string())
        .spawn(move || {
            let runtime = match rocket::tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime,
                Err(e) => {
                    error!("Could not build the async runtime for the API server: {}", e);
                    return;
                }
            };

            runtime.block_on(async move {
                let mut builder = rocket::custom(rocket_config(&host, port));
                for (mount, routes) in route_groups() {
                    builder = builder.mount(format!("{}{}", API_PREFIX, mount), routes);
                }

                // Ignite before launching, so the means to stop the server
                // exists before anything can ask for it. Ignite is also where
                // a route collision or a bad configuration surfaces, and those
                // return with nothing published -- which is what leaves a
                // daemon whose API never came up still stoppable.
                let ignited = match builder.ignite().await {
                    Ok(ignited) => ignited,
                    Err(e) => {
                        error!("The API server failed to start: {}", e);
                        return;
                    }
                };

                *shutdown_slot().lock() = Some(ignited.shutdown());

                // A stop asked for while this thread was igniting. The handler
                // runs concurrently and saw no handle to notify, so it is
                // honoured here rather than by a server that goes on to serve.
                if stop_requested().load(Ordering::SeqCst) {
                    info!("A stop was asked for before the API server started; stopping it now");
                    ignited.shutdown().notify();
                }

                let outcome = ignited.launch().await;

                // Withdrawn however that turned out: a signal from here on has
                // to end the process itself.
                *shutdown_slot().lock() = None;

                match outcome {
                    Ok(_) => info!("API server stopped"),
                    Err(e) => warn!("API server shutdown did not complete cleanly: {}", e),
                }
            });
        });

    match handle {
        Ok(handle) => Some(handle),
        Err(e) => {
            error!("Could not start the API server thread: {}", e);
            None
        }
    }
}

fn print_help() {
    println!("AudioControl metadata service");
    println!("=============================");
    println!();
    println!("USAGE:");
    println!("    audiocontrol-metadata [OPTIONS]");
    println!();
    println!("OPTIONS:");
    println!("    -c <FILE>          Configuration file path (default: metadata.json)");
    println!("    -d, --debug        Log everything at debug level");
    println!("    --check-secrets    Report the secrets compiled into this binary");
    println!("    -h, --help         Show this message");
    println!();
    println!("Logging is configured with RUST_LOG; --log-config is not read by this");
    println!("daemon.");
    println!();
    println!("The configuration file must name the player daemon's API root:");
    println!();
    println!("    \"core\": {{ \"url\": \"http://127.0.0.1:1080/api\" }}");
    println!();
    println!("It is not derived from webserver.port, which is this daemon's own.");
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every mount is distinct, and no two groups put the same method and path
    /// on the wire.
    ///
    /// A collision here is not a wrong answer, it is a daemon that refuses to
    /// ignite -- Rocket rejects an exact duplicate rather than resolving it --
    /// so the failure mode is a service that will not start at all. The
    /// player daemon has the same guard over the same route lists
    /// (`the_metadata_crates_routes_do_not_collide_with_the_daemons_own`);
    /// this one covers the second mounting of `imagecache` and the bare group,
    /// which that test does not see.
    #[test]
    fn no_two_route_groups_collide() {
        use rocket::http::Method;
        use std::collections::HashSet;

        let mut seen: HashSet<(Method, String)> = HashSet::new();
        for (mount, routes) in route_groups() {
            for route in &routes {
                let key = (
                    route.method,
                    format!("{}{}{}", API_PREFIX, mount, route.uri.path()),
                );
                assert!(
                    seen.insert(key.clone()),
                    "{:?} {} is mounted twice",
                    key.0,
                    key.1
                );
            }
        }

        // The two paths that must exist, so a `route_groups` that returned
        // nothing could not pass this vacuously.
        assert!(
            seen.contains(&(Method::Get, format!("{}/metadata/capabilities", API_PREFIX))),
            "the /metadata mount must serve the capabilities route"
        );
        assert!(
            seen.contains(&(Method::Get, format!("{}/coverart/methods", API_PREFIX))),
            "the bare mount must serve the cover art routes nginx forwards to"
        );
    }

    /// `-c` takes the path after it, and nothing else does.
    #[test]
    fn the_config_path_comes_from_minus_c() {
        let args = as_args(&["audiocontrol-metadata", "-d", "-c", "/etc/x.json"]);
        assert_eq!(argument_after(&args, "-c"), Some("/etc/x.json".to_string()));

        let args = as_args(&["audiocontrol-metadata", "-c"]);
        assert_eq!(
            argument_after(&args, "-c"),
            None,
            "a trailing -c has no value to take"
        );
    }

    fn as_args(args: &[&str]) -> Vec<String> {
        args.iter().map(|a| a.to_string()).collect()
    }
}
