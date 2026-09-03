use crate::AudioController;
use crate::api::{
    players, plugins, library, imagecache, coverart, events, lastfm, spotify,
    theaudiodb, favourites, volume, lyrics, m3u, settings, cache, backgroundjobs, genres,
    inputs, splitters, capabilities
};
use crate::api::events::WebSocketManager;
use crate::config::get_service_config;
use crate::constants::API_PREFIX;
use crate::players::{player_event_update};
 
use log::{info, warn};
use rocket::{routes, get};
use rocket::data::{Limits, ToByteUnit};
use rocket::figment::Figment;
use rocket::serde::json::Json;
use rocket::config::Config;
use rocket::fs::FileServer;
use std::sync::Arc;
use parking_lot::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

// Define the version response struct
#[derive(serde::Serialize)]
struct VersionResponse {
    version: String,
}

// API endpoint to get the version
#[get("/version")]
fn get_version() -> Json<VersionResponse> {
    Json(VersionResponse {
        version: env!("CARGO_PKG_VERSION").to_string(),
    })
}

/// The means to stop a running server: a closure that asks it to shut down.
type StopAction = Arc<dyn Fn() + Send + Sync>;

/// Where a running API server publishes the means to stop it gracefully.
///
/// The signal handler in main owns SIGINT, SIGTERM and SIGHUP for the whole
/// life of the process; Rocket's own signal handling is switched off, so there
/// is exactly one owner and no question of which handler runs. What a signal
/// *means* depends on whether a server is running, and this is how the handler
/// finds out: with a server running the signal is passed to it so its grace and
/// mercy periods are honoured, and with none the handler ends the process
/// itself.
///
/// The stored action is a closure rather than Rocket's own `Shutdown` so that
/// the decision this type encodes can be tested without launching a server.
#[derive(Clone, Default)]
pub struct ShutdownHandle {
    stop: Arc<Mutex<Option<StopAction>>>,
    requested: Arc<AtomicBool>,
}

impl ShutdownHandle {
    pub fn new() -> Self {
        Self::default()
    }

    /// Publish the means to stop the server that is now running.
    pub fn publish(&self, stop: impl Fn() + Send + Sync + 'static) {
        *self.stop.lock() = Some(Arc::new(stop));
    }

    /// Withdraw it once the server has stopped, or failed to start. A signal
    /// arriving afterwards has to end the process itself, so leaving a stale
    /// action published would make the daemon unstoppable.
    pub fn withdraw(&self) {
        *self.stop.lock() = None;
    }

    /// Whether a shutdown has been asked for at any point.
    ///
    /// A server that then fails to finish starting -- the port still held by
    /// an outgoing instance is the ordinary way -- leaves nobody to report
    /// that it has stopped, so the caller has to end the process on its
    /// behalf rather than wait for a shutdown that cannot arrive.
    pub fn stop_requested(&self) -> bool {
        self.requested.load(Ordering::SeqCst)
    }

    /// Ask a running server to shut down gracefully.
    ///
    /// Returns `false` when no server is running, in which case the caller is
    /// responsible for ending the process.
    pub fn request_stop(&self) -> bool {
        self.requested.store(true, Ordering::SeqCst);

        let stop = self.stop.lock().clone();
        match stop {
            Some(stop) => {
                stop();
                true
            }
            None => false,
        }
    }
}

/// What became of the API server.
///
/// `Ok(())` used to mean both "Rocket ran and has now shut down" and "the
/// webserver is disabled, nothing was started". The caller has to tell those
/// apart -- the first is the process's signal to exit, the second must not be
/// -- and reading the configuration a second time to do it produced two bugs:
/// a shutdown flag cleared at startup, and a log line announcing a port that
/// was never bound.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServerOutcome {
    /// Rocket launched, served, and has finished its graceful shutdown.
    ShutDown,
    /// The webserver is disabled in the configuration; nothing was started.
    Disabled,
}

/// The Rocket configuration for the API server.
///
/// The JSON body limit is pinned above Rocket's 1 MiB default:
/// `POST /coverart/artists/upload` takes a batch of base64-encoded images,
/// and base64 adds a third on top, so the default rejects a batch of more
/// than roughly 768 KB of image bytes with a 413 before the handler could
/// report which entries failed. 10 MiB holds several artist JPEGs while
/// bounding what one request may hold in memory on a 1 GB device; every other
/// JSON endpoint takes bodies far below it, so raising the limit changes
/// nothing for them. The other limits are carried over from the defaults
/// unchanged: a merged `limits` value replaces the whole set, so starting from
/// `Limits::default()` keeps form, file, string and friends at what they were.
fn rocket_config(host: &str, port: u64) -> Figment {
    Config::figment()
        .merge(("port", port))
        .merge(("address", host))
        .merge(("shutdown.ctrlc", false))
        .merge(("shutdown.signals", Vec::<String>::new()))
        // Pinned, not left to the defaults. Config::figment() also reads
        // Rocket.toml from the working directory and ROCKET_SHUTDOWN_GRACE /
        // ROCKET_SHUTDOWN_MERCY from the environment, and the force-exit
        // watchdog in main is sized against these two: a larger grace set
        // from outside would have the watchdog fire in the middle of a
        // shutdown that was proceeding normally.
        .merge(("shutdown.grace", 2))
        .merge(("shutdown.mercy", 3))
        .merge(("limits", Limits::default().limit("json", 10.mebibytes())))
}

// Start the Rocket server
pub async fn start_rocket_server(
    controller: Arc<AudioController>,
    config_json: &serde_json::Value,
    // Where this server publishes the means to stop it, for the signal
    // handler in main to use while it is running.
    shutdown_handle: ShutdownHandle,
) -> Result<ServerOutcome, rocket::Error> {
    // Check if webserver is enabled (default to true if not specified)
    let webserver_enabled = get_service_config(config_json, "webserver")
        .and_then(|ws| ws.get("enable"))
        .and_then(|e| e.as_bool())
        .unwrap_or(true);
        
    if !webserver_enabled {
        info!("Webserver is disabled in configuration");
        return Ok(ServerOutcome::Disabled);
    }
    
    // Get webserver config or use defaults
    let host = get_service_config(config_json, "webserver")
        .and_then(|ws| ws.get("host"))
        .and_then(|h| h.as_str())
        .unwrap_or("0.0.0.0");
        
    let port = get_service_config(config_json, "webserver")
        .and_then(|ws| ws.get("port"))
        .and_then(|p| p.as_u64())
        .unwrap_or(1080);
    
    info!("Starting webserver on {}:{}", host, port);
    
    // Rocket's own signal handling is switched off. The handler in main owns
    // SIGINT, SIGTERM and SIGHUP for the whole life of the process and asks
    // this server to stop through the handle published below, so there is one
    // owner rather than two chained handlers whose order, survival and
    // registration timing all have to be reasoned about.
    let config = rocket_config(host, port);
    
    // Create WebSocket manager and start the background pruning task
    let ws_manager = Arc::new(WebSocketManager::new());
    events::start_prune_task(ws_manager.clone());
    
    let api_routes = routes![
        get_version,
        capabilities::get_capabilities,

        // Player routes
        players::get_current_player,
        players::list_players,
        players::send_command_to_player_by_name,
        players::get_now_playing,
        players::get_player_queue,
        players::get_player_metadata,      
        players::get_player_metadata_key,
        players::pause_all_players,
        players::stop_all_players,        
        // Plugin routes
        plugins::list_action_plugins,
        
        // Stream title splitter routes
        splitters::list_splitters,
        splitters::get_splitter,
        splitters::set_splitter,
        splitters::delete_splitter,

        // Library routes
        library::list_libraries,
        library::get_library_info,
        library::get_player_albums,
        library::get_player_artists,
        library::get_album_by_id,
        library::get_albums_by_artist,
        library::get_albums_by_artist_id,
        library::refresh_player_library,
        library::update_player_library,
        library::get_artist_by_name,
        library::get_artist_by_id,
        library::get_artist_by_mbid,
        library::get_image,
        library::get_library_metadata,
        library::get_library_metadata_key,
        library::get_library_genres,
        library::get_albums_by_genre,
        library::get_artists_by_genre,
        library::get_library_categories,
        library::get_albums_by_category,
        library::get_artists_by_category,
        library::delete_library_album,
        library::delete_library_track,

        // TheAudioDB routes
        theaudiodb::lookup_artist_by_mbid,
        
        // WebSocket routes
        events::event_messages,
        events::player_event_messages,
        
        // Generic player API endpoints
        player_event_update,
    ];

    // Define volume routes
    let volume_routes = routes![
        volume::get_volume_info,
        volume::get_volume_state,
        volume::set_volume,
        volume::increase_volume,
        volume::decrease_volume,
        volume::toggle_mute,
    ];

    // Define inputs routes
    let inputs_routes = routes![
        inputs::get_inputs_status,
    ];

    // Define coverart routes
    let coverart_routes = routes![
        coverart::get_artist_coverart,
        coverart::get_song_coverart,
        coverart::get_album_coverart,
        coverart::get_album_coverart_with_year,
        coverart::get_url_coverart,
        coverart::get_coverart_methods,
        coverart::upload_artists_images,
        coverart::update_artist_image,
        coverart::get_artist_image,
        coverart::get_artist_images,
        coverart::get_artist_image_by_id,
    ];

    // Define Last.fm specific routes
    let lastfm_routes = routes![
        lastfm::get_status,
        lastfm::get_auth_url_handler,
        lastfm::prepare_complete_auth,
        lastfm::complete_auth,
        lastfm::disconnect_handler,
    ];

    // Read spotify.api_enabled config (default: false)
    let spotify_api_enabled = get_service_config(config_json, "spotify")
        .and_then(|s| s.get("api_enabled"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    // Define Spotify authentication-only routes
    let spotify_auth_routes = routes![
        spotify::store_tokens,
        spotify::token_status,
        spotify::logout,
        spotify::get_oauth_config,
        spotify::create_session,
        spotify::login,
        spotify::poll_session,
        spotify::check_server,
        spotify::get_access_token
    ];
    // Define full Spotify API routes
    let spotify_full_routes = routes![
        spotify::store_tokens,
        spotify::token_status,
        spotify::logout,
        spotify::get_oauth_config,
        spotify::create_session,
        spotify::login,
        spotify::poll_session,
        spotify::check_server,
        spotify::spotify_command,
        spotify::get_playback,
        spotify::spotify_currently_playing,
        spotify::spotify_search,
        spotify::get_access_token
    ];
    
    // ImageCache routes
    let imagecache_routes = routes![
        imagecache::get_image_from_cache,
        imagecache::purge_variants
    ];
    
    // Favourites routes
    let favourites_routes = favourites::routes();
    
    // Lyrics routes
    let lyrics_routes = routes![
        lyrics::get_lyrics_by_id,
        lyrics::get_lyrics_by_metadata,
    ];
    
    // M3U routes
    let m3u_routes = routes![
        m3u::parse_m3u_playlist,
    ];
    
    // Settings routes
    let settings_routes = routes![
        settings::get_setting,
        settings::set_setting,
    ];
    
    // Cache routes
    let cache_routes = routes![
        cache::get_cache_statistics,
    ];
    
    // Background jobs routes
    let backgroundjobs_routes = routes![
        backgroundjobs::get_background_jobs,
        backgroundjobs::get_background_job,
    ];

    // Genre config routes
    let genres_routes = routes![
        genres::get_config,
        genres::get_user_config_endpoint,
        genres::put_user_config,
        genres::post_mapping,
        genres::delete_mapping,
        genres::post_ignore,
        genres::delete_ignore,
    ];
      let mut rocket_builder = rocket::custom(config)
        .mount(API_PREFIX, api_routes) // Use API_PREFIX here when mounting general api routes
        .mount(format!("{}/lastfm", API_PREFIX), lastfm_routes) // Mount Last.fm routes under /api/lastfm (or similar)
        .mount(
            format!("{}/spotify", API_PREFIX),
            if spotify_api_enabled { spotify_full_routes } else { spotify_auth_routes }
        )
        .mount(format!("{}/imagecache", API_PREFIX), imagecache_routes) // Mount imagecache routes
        .mount(format!("{}/favourites", API_PREFIX), favourites_routes) // Mount favourites routes
        .mount(format!("{}/lyrics", API_PREFIX), lyrics_routes) // Mount lyrics routes
        .mount(format!("{}/m3u", API_PREFIX), m3u_routes) // Mount M3U routes
        .mount(format!("{}/settings", API_PREFIX), settings_routes) // Mount settings routes
        .mount(format!("{}/cache", API_PREFIX), cache_routes) // Mount cache routes
        .mount(format!("{}/background", API_PREFIX), backgroundjobs_routes) // Mount background jobs routes
        .mount(format!("{}/genres", API_PREFIX), genres_routes) // Mount genre config routes
        .mount(format!("{}/volume", API_PREFIX), volume_routes) // Mount volume routes
        .mount(format!("{}/inputs", API_PREFIX), inputs_routes) // Mount inputs status routes
        .mount(format!("{}/coverart", API_PREFIX), coverart_routes) // Mount coverart routes
        .manage(controller)
        .manage(ws_manager); // Add WebSocket manager as managed state
      // Check for static file routes in the configuration
    if let Some(static_routes) = get_service_config(config_json, "webserver")
        .and_then(|ws| ws.get("static_routes"))
        .and_then(|sr| sr.as_array()) {
        for (index, route_config) in static_routes.iter().enumerate() {
            if let (Some(url_path), Some(directory)) = (
                route_config.get("url_path").and_then(|p| p.as_str()),
                route_config.get("directory").and_then(|d| d.as_str())
            ) {
                info!("Mounting static files from '{}' at URL path '{}'", directory, url_path);
                rocket_builder = rocket_builder.mount(url_path, FileServer::from(directory));
            } else {
                warn!("Invalid static file route configuration at index {}: missing url_path or directory", index);
            }
        }
    }
    
    // Ignite before launching, so the means to stop the server exists before
    // anything can ask for it. Rocket creates the shutdown handle during
    // ignite, and only begins watching for signals later still, inside
    // http_server -- so a handle published before launch() would not exist
    // yet, and a signal arriving while Rocket built its router or bound its
    // port would reach a handler that had stood aside for a server not yet
    // listening, and be lost. Igniting here leaves no such window: from the
    // moment the handle exists it is published, and tripping it is remembered
    // whether or not the server has started serving.
    //
    // Ignite is also where most startup failures surface -- FailedFairings,
    // route Collisions, config extraction, sentinels -- and those return with
    // nothing published, so the signals stay with main, as they must for a
    // daemon whose API never came up to still be stoppable.
    let ignited = rocket_builder.ignite().await?;

    shutdown_handle.publish({
        let shutdown = ignited.shutdown();
        move || shutdown.clone().notify()
    });

    // A stop asked for before the server got this far is honoured now rather
    // than after it has bound its port and started serving. The handler runs on
    // ctrlc's own thread, concurrently with this one, and it is registered long
    // before main reaches its wait loop -- config loading and controller setup
    // sit in between, which on a Pi is seconds, not instructions. Without this,
    // a signal arriving in that stretch would leave main on its way out while
    // this thread went on to launch a fully serving webserver, which process
    // exit would then tear down with no grace period and no shutdown fairings.
    // Tripping the wire here means launch() returns through the ordinary path
    // instead, and the outcome is reported as a clean shutdown.
    if shutdown_handle.stop_requested() {
        info!("A stop was asked for before the API server started; stopping it now");
        ignited.shutdown().notify();
    }

    let launched = ignited.launch().await;

    // Handed back however that turned out: the server is gone either way, and
    // a signal from here on has to end the process itself.
    shutdown_handle.withdraw();

    let _rocket = launched?;

    Ok(ServerOutcome::ShutDown)
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Before the server is up, and after it has gone, a signal has to end the
    /// process itself -- there is nothing to hand it to.
    #[test]
    fn a_signal_with_no_server_running_is_not_absorbed() {
        let handle = ShutdownHandle::new();

        assert!(!handle.request_stop());
    }

    /// While a server is running the signal is passed to it rather than ending
    /// the process, so its graceful shutdown is allowed to finish.
    #[test]
    fn a_signal_reaches_a_running_server() {
        let handle = ShutdownHandle::new();
        let stops = Arc::new(AtomicUsize::new(0));
        let counter = stops.clone();
        handle.publish(move || {
            counter.fetch_add(1, Ordering::SeqCst);
        });

        assert!(handle.request_stop());
        assert_eq!(stops.load(Ordering::SeqCst), 1, "the server should be asked once");
    }

    /// A stop asked for while the server was still starting must not be
    /// forgotten. Nothing will report that server as stopped -- it never
    /// finished starting -- so the caller has to end the process itself, and
    /// this is how it learns that it should.
    #[test]
    fn a_stop_asked_for_is_remembered() {
        let handle = ShutdownHandle::new();
        assert!(!handle.stop_requested(), "nothing has been asked for yet");

        handle.request_stop();

        assert!(handle.stop_requested());
    }

    /// A launch that failed, or a server that has finished, must hand the
    /// signals back. Holding a stale action would leave the daemon with a
    /// handler that defers to something no longer there -- unstoppable until
    /// systemd's SIGKILL, which is the failure this whole path exists to avoid.
    #[test]
    fn a_server_that_has_stopped_no_longer_absorbs_signals() {
        let handle = ShutdownHandle::new();
        let stops = Arc::new(AtomicUsize::new(0));
        let counter = stops.clone();
        handle.publish(move || {
            counter.fetch_add(1, Ordering::SeqCst);
        });

        handle.withdraw();

        assert!(!handle.request_stop());
        assert_eq!(stops.load(Ordering::SeqCst), 0, "a withdrawn server must not be asked");
    }

    /// The batch of base64-encoded artist images in
    /// `POST /coverart/artists/upload` must fit in one request body, so the
    /// JSON limit may not sit at Rocket's 1 MiB default (rough 768 KB of
    /// image bytes once base64 is subtracted).
    #[test]
    fn the_json_body_limit_covers_an_image_upload_batch() {
        let config = Config::try_from(rocket_config("127.0.0.1", 1080))
            .expect("a valid configuration");
        assert_eq!(
            config.limits.get("json"),
            Some(10.mebibytes()),
            "the image-upload batch must fit in one request"
        );
    }

    /// Raising the JSON limit must not disturb the other limits: a merged
    /// `limits` value replaces the whole set, so the defaults have to be
    /// carried over explicitly.
    #[test]
    fn the_other_body_limits_keep_their_defaults() {
        let config = Config::try_from(rocket_config("127.0.0.1", 1080))
            .expect("a valid configuration");
        assert_eq!(config.limits.get("form"), Some(32.kibibytes()));
        assert_eq!(config.limits.get("file"), Some(1.mebibytes()));
        assert_eq!(config.limits.get("string"), Some(8.kibibytes()));
    }
}
