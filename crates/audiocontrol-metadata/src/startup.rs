//! The metadata side's *second* entry point: what can only start once the
//! player daemon's HTTP API is listening.
//!
//! [`crate::initialize_in_process`] is the first. It registers providers and
//! reads configuration, and needs nothing but the configuration document, so
//! `main` calls it early -- long before the API thread is spawned. The two
//! things started here are different in kind: the WebSocket subscriber and
//! the library puller are *clients* of the player daemon's API, and in Phase 1
//! that API is served by the very process they run in. Starting them beside
//! the providers would mean a subscriber connecting to a port nothing has
//! bound yet and a puller whose first sweep is guaranteed to fail.
//!
//! So they get their own function, called after the server is up. Phase 2's
//! metadata daemon calls the same one from its own `main`, against the same
//! `core` configuration key -- by then the wait below is a wait on another
//! process, which is exactly what it already is from this side's point of
//! view.
//!
//! ## Why `services.core` defaults rather than opting out
//!
//! `services.metadata` being absent means "make none of those calls": the
//! player side then keeps its offline fallbacks and nothing is lost but
//! enrichment. `services.core` is not symmetrical, and deliberately so. This
//! side exists to enrich a player daemon; there is always one to talk to, and
//! an absent section is a configuration file that never mentioned a seam that
//! did not exist when it was written, not an operator asking for a metadata
//! side that talks to nobody. So an absent section means the defaults below.

use std::sync::Arc;
use std::time::{Duration, Instant};

use acr_types::config::get_service_config;
use acr_types::now_playing::{LastfmWorkerConfig, PlaybackStateSource, SongInformationSink};
use log::{error, info, warn};

use crate::core_client::CoreClient;
use crate::{library_puller, now_playing, now_playing_ws};

/// The port [`core_settings`] assumes the player daemon is on when nothing in
/// the configuration says otherwise. The same number `src/api/server.rs`
/// falls back to for `services.webserver.port`, and the spec's value for
/// `services.core.url`.
pub const DEFAULT_CORE_PORT: u64 = 1080;

/// How long the library sweep waits between passes when `services.core` names
/// no `library_poll_seconds`. The spec's value, and the same number
/// [`library_puller::LIBRARY_POLL_INTERVAL`] carries.
pub const DEFAULT_LIBRARY_POLL_SECONDS: u64 = 30;

/// How long [`start_after_core_is_listening`] waits for the player daemon's
/// API before starting anyway.
///
/// Bounded rather than unbounded because nothing here *needs* the API to be
/// up: the subscriber reconnects with backoff and the puller retries at its
/// poll interval, so waiting only buys a quieter log on a normal start. A
/// wait without a bound would turn a misconfigured URL into a daemon that
/// never finishes starting.
const CORE_WAIT_LIMIT: Duration = Duration::from_secs(30);

/// How often the wait asks, between attempts that fail immediately. A refused
/// connection returns at once, so this is what sets the poll rate; an attempt
/// that times out takes its own timeout instead and this is added to it.
const CORE_WAIT_TICK: Duration = Duration::from_millis(250);

/// What `services.core` says, with the defaults filled in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreSettings {
    /// The player daemon's API root, e.g. `http://127.0.0.1:1080/api`.
    pub url: String,
    /// How often the library puller sweeps every player.
    pub poll: Duration,
}

/// Read `services.core`, or the defaults where it or a key is absent.
///
/// A `library_poll_seconds` of 0 is read as "the default", not as "sweep
/// continuously": a zero poll would busy-loop the puller against the player
/// daemon's library routes, and no operator writing 0 means that.
///
/// **The default URL follows `services.webserver.port`.** The spec writes it
/// out as `http://127.0.0.1:1080/api`, and that is what this produces for
/// every configuration that leaves the web server on its own default port --
/// including the one this repository ships. But 1080 is not a constant of the
/// system, it is `webserver.port`'s own fallback, and in this phase the
/// player daemon whose API this addresses *is this process*. Hardcoding 1080
/// would mean a daemon configured onto another port waits out
/// [`CORE_WAIT_LIMIT`] at every start and then subscribes to a socket nobody
/// is serving -- which is exactly what the integration suite, which runs the
/// daemon on 18080, would do.
///
/// This holds only while both halves share a process. Once the metadata
/// daemon has a `webserver.port` of *its own*, that port is emphatically not
/// the player daemon's, and its configuration must name `core.url` -- as the
/// spec's `metadata.json` does.
pub fn core_settings(config: &serde_json::Value) -> CoreSettings {
    let section = get_service_config(config, "core");

    let url = match section
        .and_then(|c| c.get("url"))
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
    {
        Some(url) => url.to_string(),
        None => format!(
            "http://127.0.0.1:{}/api",
            get_service_config(config, "webserver")
                .and_then(|ws| ws.get("port"))
                .and_then(|p| p.as_u64())
                .unwrap_or(DEFAULT_CORE_PORT)
        ),
    };

    let seconds = section
        .and_then(|c| c.get("library_poll_seconds"))
        .and_then(|v| v.as_u64())
        .filter(|s| *s > 0)
        .unwrap_or(DEFAULT_LIBRARY_POLL_SECONDS);

    CoreSettings {
        url,
        poll: Duration::from_secs(seconds),
    }
}

/// The player daemon's event socket, given its API root.
///
/// `http://host:port/api` becomes `ws://host:port/api/events`, which is where
/// `src/api/events.rs` serves the stream. A `https` root maps to `wss`, which
/// [`now_playing_ws::start`] will refuse with a message naming the URL --
/// better than silently subscribing to a plaintext socket the operator did
/// not ask for.
pub fn events_url(core_url: &str) -> String {
    let base = core_url.trim_end_matches('/');
    let scheme_swapped = match base.split_once("://") {
        Some(("http", rest)) => format!("ws://{}", rest),
        Some(("https", rest)) => format!("wss://{}", rest),
        // Anything else is passed through unchanged: it is not this
        // function's place to guess, and the subscriber reports what it was
        // given.
        _ => base.to_string(),
    };
    format!("{}/events", scheme_swapped)
}

/// The `action_plugins` entry named `lastfm`, if the configuration has one.
///
/// That entry used to configure an action plugin; it now configures the
/// Last.fm worker in this crate. Same array, same key, same fields, so an
/// existing configuration file needs no change -- and the entry is still
/// reported by `GET /api/plugins/actions`, which the player daemon's own
/// `plugin_factory` registration takes care of.
///
/// This lived in `src/main.rs` while `main` was what started the worker. It
/// moved here with that responsibility: the type it parses belongs to this
/// side, and Phase 2's metadata daemon has to read the same entry out of its
/// own configuration file with no `main.rs` of the player daemon's to ask.
pub fn lastfm_worker_config(config: &serde_json::Value) -> Option<LastfmWorkerConfig> {
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

/// Ask `GET /version` until the player daemon answers, or until `limit` has
/// passed.
///
/// Returns whether it answered. One attempt is always made, so a `limit` of
/// zero is "try once" rather than "do not try".
fn wait_for_core(core: &CoreClient, limit: Duration, tick: Duration) -> bool {
    let started = Instant::now();
    let mut reported = false;

    loop {
        match core.version() {
            Ok(version) => {
                info!("The player daemon's API answered; it is version {}", version);
                return true;
            }
            Err(e) => {
                // Once, not once per attempt: on a normal start this loop
                // spins a few times while Rocket binds, and a line per
                // attempt would be noise on every boot.
                if !reported {
                    info!("Waiting for the player daemon's API to answer ({})", e);
                    reported = true;
                }
            }
        }

        if started.elapsed() >= limit {
            return false;
        }
        std::thread::sleep(tick);
    }
}

/// Start the two parts of the metadata side that are clients of the player
/// daemon: the now-playing subscriber (with the enrichment workers it feeds)
/// and the library puller.
///
/// Called once, from the composition root, after the API server is listening.
/// Returns as soon as both are running on their own threads; the wait for the
/// player daemon is bounded by [`CORE_WAIT_LIMIT`] and never fails the start.
pub fn start_after_core_is_listening(config: &serde_json::Value) {
    let settings = core_settings(config);
    let core = Arc::new(CoreClient::new(&settings.url));

    if !wait_for_core(&core, CORE_WAIT_LIMIT, CORE_WAIT_TICK) {
        warn!(
            "The player daemon's API at {} did not answer within {:?}. Starting the \
             metadata side anyway: the event subscriber reconnects with backoff and the \
             library puller retries at its poll interval.",
            settings.url, CORE_WAIT_LIMIT
        );
    }

    // Interface 1, both directions, in one object: the subscriber reads the
    // event socket, and the same `CoreClient` is what the workers push
    // results back through and what the Last.fm worker asks for the playback
    // state. This is what `now_playing_bridge`'s `ControllerSink` used to be
    // on the player side, with an HTTP round trip where the method call was.
    let events = now_playing_ws::start(&events_url(&settings.url), Arc::clone(&core));
    // `core.clone()`, not `Arc::clone(&core)`: the expected type drives
    // inference through the associated function, so `Arc::clone` would be
    // asked for an `Arc<dyn ...>` it was not given. The method call unsizes
    // afterwards, which is what is wanted.
    let sink: Arc<dyn SongInformationSink> = core.clone();
    let state: Arc<dyn PlaybackStateSource> = core.clone();
    if !now_playing::start(events, sink, state, lastfm_worker_config(config)) {
        info!("No now-playing enrichment is configured");
    }

    library_puller::start(core, settings.poll);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::external_coverart::stub_server::StubServer;

    /// The asymmetry with `services.metadata`: an absent section is the
    /// defaults, not "make no calls". Checked for all three ways it can be
    /// absent, because the third -- no `services` key at all -- is what an
    /// existing installed configuration file looks like.
    #[test]
    fn an_absent_core_section_means_the_defaults() {
        for config in [
            serde_json::json!({ "services": { "core": {} } }),
            serde_json::json!({ "services": {} }),
            serde_json::json!({}),
        ] {
            let settings = core_settings(&config);
            assert_eq!(settings.url, "http://127.0.0.1:1080/api", "for {}", config);
            assert_eq!(
                settings.poll,
                Duration::from_secs(DEFAULT_LIBRARY_POLL_SECONDS),
                "for {}",
                config
            );
        }
    }

    /// With no `core.url`, the port the daemon is about to bind is the port
    /// to talk to. The integration suite runs on 18080; a hardcoded 1080
    /// would leave every one of its daemons waiting out the start-up probe
    /// and then subscribing to a socket nobody serves.
    #[test]
    fn the_default_url_follows_the_webservers_port() {
        let settings = core_settings(&serde_json::json!({
            "services": { "webserver": { "host": "0.0.0.0", "port": 18080 } }
        }));
        assert_eq!(settings.url, "http://127.0.0.1:18080/api");
    }

    /// An explicit `core.url` is never second-guessed by the web server's
    /// port -- which is the whole of what makes the fallback above safe once
    /// the two halves are separate processes with separate ports.
    #[test]
    fn an_explicit_core_url_wins_over_the_webservers_port() {
        let settings = core_settings(&serde_json::json!({
            "services": {
                "webserver": { "port": 1084 },
                "core": { "url": "http://127.0.0.1:1080/api" }
            }
        }));
        assert_eq!(settings.url, "http://127.0.0.1:1080/api");
    }

    #[test]
    fn a_core_section_overrides_both_defaults() {
        let settings = core_settings(&serde_json::json!({
            "services": { "core": { "url": "http://127.0.0.1:1080/api/", "library_poll_seconds": 5 } }
        }));
        // The trailing slash is `CoreClient`'s to trim, not this function's:
        // what is read is what was written.
        assert_eq!(settings.url, "http://127.0.0.1:1080/api/");
        assert_eq!(settings.poll, Duration::from_secs(5));
    }

    /// `get_service_config` falls back to the top level, which is the shape
    /// the spec gives the metadata daemon's own configuration file.
    #[test]
    fn a_top_level_core_section_is_read_too() {
        let settings = core_settings(&serde_json::json!({
            "core": { "url": "http://127.0.0.1:1080/api", "library_poll_seconds": 60 }
        }));
        assert_eq!(settings.url, "http://127.0.0.1:1080/api");
        assert_eq!(settings.poll, Duration::from_secs(60));
    }

    #[test]
    fn an_empty_url_or_a_zero_poll_falls_back_rather_than_being_taken_literally() {
        let settings = core_settings(&serde_json::json!({
            "services": { "core": { "url": "", "library_poll_seconds": 0 } }
        }));
        assert_eq!(settings.url, "http://127.0.0.1:1080/api");
        assert_eq!(
            settings.poll,
            Duration::from_secs(DEFAULT_LIBRARY_POLL_SECONDS)
        );
    }

    #[test]
    fn the_events_url_is_the_api_root_as_a_websocket() {
        assert_eq!(
            events_url("http://127.0.0.1:1080/api"),
            "ws://127.0.0.1:1080/api/events"
        );
        assert_eq!(
            events_url("http://127.0.0.1:1080/api/"),
            "ws://127.0.0.1:1080/api/events"
        );
        assert_eq!(
            events_url("https://example:443/api"),
            "wss://example:443/api/events"
        );
    }

    #[test]
    fn a_core_that_answers_ends_the_wait() {
        let server = StubServer::serving(200, r#"{"version":"0.13.0"}"#);
        let core = CoreClient::new(&server.base_url());

        assert!(wait_for_core(
            &core,
            Duration::from_secs(5),
            Duration::from_millis(10)
        ));

        let requests = server.requests();
        assert!(
            requests[0].starts_with("GET /version HTTP/1.1"),
            "the wait should probe GET /version, not {}",
            requests[0]
        );
    }

    /// Nothing listening on the port, and a limit that is already spent: the
    /// wait gives up and says so, rather than blocking start-up.
    #[test]
    fn a_core_that_never_answers_gives_up_and_reports_it() {
        // Port 1 needs no listener to be refused, and the refusal is
        // immediate, so this asserts on the answer rather than on a clock.
        let core = CoreClient::new("http://127.0.0.1:1/api");
        assert!(!wait_for_core(
            &core,
            Duration::ZERO,
            Duration::from_millis(10)
        ));
    }

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
