//! Cover art from HTTP endpoints named in the configuration.
//!
//! Unlike the providers in `coverart_providers`, these are not written
//! against a known service: an endpoint speaks a fixed JSON contract
//! documented in `doc/external-coverart.md`, and anything unusual about the
//! service behind it is that service's problem. They are assumed slow -- the
//! first is an LLM-backed lookup taking 20-40 seconds -- so they are never on
//! a request path unless a caller opts in.

pub mod config;
pub mod protocol;
pub mod template;
pub mod worker;

#[cfg(test)]
mod stub_server;

use std::collections::HashSet;
use std::sync::Arc;
use std::time::{Duration, Instant};

use log::{debug, info, warn};
use once_cell::sync::Lazy;
use parking_lot::{Condvar, Mutex};

use crate::helpers::attributecache;
use crate::helpers::coverart::{CoverartMethod, CoverartProvider, CoverartQuery};
use crate::helpers::http_client::new_http_client;

use config::EndpointConfig;
use protocol::{parse_response, ttl_seconds, Lookup};

/// The endpoints read from the configuration, in configuration order.
static ENDPOINTS: Lazy<Mutex<Vec<Arc<ExternalCoverartProvider>>>> =
    Lazy::new(|| Mutex::new(Vec::new()));

/// The cache key for one lookup against one endpoint.
///
/// Values are lower-cased and trimmed: the same track reaches the daemon with
/// different casing and stray whitespace from different players, and paying
/// 30 seconds twice for that would be the expensive kind of cache miss. Each
/// value is then escaped -- `\` becomes `\\` and `|` becomes `\|`, in that
/// order, so the escape character itself cannot be forged by input that
/// already contains one -- before the fields are joined with `|`. Artist and
/// title come from arbitrary player metadata and radio-stream text, not from
/// anything an administrator controls, so a literal `|` in one field must not
/// be able to make it read as the boundary between two different fields;
/// without the escaping a crafted `artist: "a|b", title: "c"` and
/// `artist: "a", title: "b|c"` would produce the same key.
pub fn cache_key(endpoint_name: &str, query: &CoverartQuery) -> String {
    let clean = |value: &str| {
        let value = value.trim().to_lowercase();
        value.replace('\\', "\\\\").replace('|', "\\|")
    };
    let body = match query {
        CoverartQuery::Artist(artist) => format!("artist|{}", clean(artist)),
        CoverartQuery::Song { title, artist } => {
            format!("song|{}|{}", clean(artist), clean(title))
        }
        CoverartQuery::Album { title, artist, year } => format!(
            "album|{}|{}|{}",
            clean(artist),
            clean(title),
            year.map(|y| y.to_string()).unwrap_or_default()
        ),
        CoverartQuery::Url(url) => format!("url|{}", url.trim()),
    };
    format!("coverart::external::{}::{}", endpoint_name, body)
}

/// A counting semaphore with two ways to ask for a slot.
///
/// A background caller gives up rather than queueing: a speculative lookup
/// that waits behind two 40-second lookups has outlived whatever it was for,
/// and letting threads accumulate against a stuck endpoint is exactly the
/// failure the non-blocking path exists to prevent. An interactive caller has
/// a person waiting who already consented to the wait, so it queues briefly
/// instead of returning nothing at the moment the feature is most active --
/// see `acquire_blocking`.
///
/// The count lives behind the same `Mutex` the `Condvar` waits on, rather
/// than as a bare atomic, so a release and a check-then-wait can never
/// interleave in a way that loses a wakeup.
struct Slots {
    in_use: Mutex<usize>,
    condvar: Condvar,
    limit: usize,
}

impl Slots {
    fn new(limit: usize) -> Self {
        Self {
            in_use: Mutex::new(0),
            condvar: Condvar::new(),
            limit: limit.max(1),
        }
    }

    /// Take a slot if one is free; give up instantly otherwise.
    fn try_acquire(self: &Arc<Self>) -> Option<SlotGuard> {
        let mut in_use = self.in_use.lock();
        if *in_use >= self.limit {
            return None;
        }
        *in_use += 1;
        Some(SlotGuard { slots: self.clone() })
    }

    /// Take a slot, waiting up to `timeout` for one to free up.
    ///
    /// Bounded by the caller's own deadline rather than waiting forever: a
    /// person is waiting, but not indefinitely.
    fn acquire_blocking(self: &Arc<Self>, timeout: Duration) -> Option<SlotGuard> {
        let mut in_use = self.in_use.lock();
        let deadline = Instant::now() + timeout;
        while *in_use >= self.limit {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return None;
            }
            // `wait_for` re-checks the predicate itself on return, so a
            // notification that arrives just as the timeout expires is not
            // lost -- the loop condition above is what decides, not the
            // timed-out flag.
            self.condvar.wait_for(&mut in_use, remaining);
        }
        *in_use += 1;
        Some(SlotGuard { slots: self.clone() })
    }
}

struct SlotGuard {
    slots: Arc<Slots>,
}

impl Drop for SlotGuard {
    fn drop(&mut self) {
        let mut in_use = self.slots.in_use.lock();
        *in_use = in_use.saturating_sub(1);
        drop(in_use);
        // At most one waiter can make use of the freed slot; a released slot
        // waking every waiter to have all but one immediately re-block would
        // just be wasted work.
        self.slots.condvar.notify_one();
    }
}

/// Who is asking for a lookup, because the two have different tolerance for
/// waiting behind a busy endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LookupMode {
    /// The now-playing worker: speculative, so a busy endpoint is skipped
    /// rather than queued behind.
    Background,
    /// A REST caller that set `?include_slow=true`: a person is waiting who
    /// has already consented to the delay, so a brief queue is preferable to
    /// silently returning nothing.
    Interactive,
}

/// One configured endpoint, as a cover art provider.
pub struct ExternalCoverartProvider {
    endpoint: EndpointConfig,
    methods: HashSet<CoverartMethod>,
    slots: Arc<Slots>,
}

impl ExternalCoverartProvider {
    pub fn new(endpoint: EndpointConfig) -> Self {
        let methods = endpoint.methods.iter().cloned().collect();
        let slots = Arc::new(Slots::new(endpoint.max_concurrent));
        Self { endpoint, methods, slots }
    }

    pub fn endpoint(&self) -> &EndpointConfig {
        &self.endpoint
    }

    /// Ask the endpoint, and cache whatever it says.
    ///
    /// The background-worker wrapper around [`Self::lookup_with`]; every
    /// other caller reaches this provider through the `CoverartProvider`
    /// trait impls below, which pass [`LookupMode::Interactive`].
    pub fn lookup(&self, query: &CoverartQuery) -> Lookup {
        self.lookup_with(query, LookupMode::Background)
    }

    /// Ask the endpoint, and cache whatever it says.
    ///
    /// Returns the cached answer without a network call when there is one,
    /// including a cached error: an endpoint that just failed is not asked
    /// again for an hour.
    pub fn lookup_with(&self, query: &CoverartQuery, mode: LookupMode) -> Lookup {
        let key = cache_key(&self.endpoint.name, query);

        match attributecache::get::<Lookup>(&key) {
            Ok(Some(cached)) => {
                debug!("External cover art '{}': cache hit for {}", self.endpoint.name, key);
                return cached;
            }
            Ok(None) => {}
            Err(e) => warn!("External cover art '{}': cache read failed: {}", self.endpoint.name, e),
        }

        let slot = match mode {
            LookupMode::Background => self.slots.try_acquire(),
            LookupMode::Interactive => self
                .slots
                .acquire_blocking(Duration::from_secs(self.endpoint.timeout_seconds)),
        };

        let Some(_slot) = slot else {
            // Not cached as anything: nothing was learned about the track.
            debug!(
                "External cover art '{}': already at {} concurrent lookups; skipping",
                self.endpoint.name, self.endpoint.max_concurrent
            );
            return Lookup::Error;
        };

        let url = template::expand(&self.endpoint.url, query);
        let headers: Vec<(&str, &str)> = self
            .endpoint
            .headers
            .iter()
            .map(|(name, value)| (name.as_str(), value.as_str()))
            .collect();

        let client = new_http_client(self.endpoint.timeout_seconds);
        let lookup = match client.get_json_with_headers(&url, &headers) {
            Ok(body) => parse_response(&body),
            Err(e) => {
                warn!("External cover art '{}': lookup failed: {}", self.endpoint.name, e);
                Lookup::Error
            }
        };

        if let Err(e) = attributecache::set_with_ttl(&key, &lookup, ttl_seconds(&lookup, &self.endpoint)) {
            warn!("External cover art '{}': cache write failed: {}", self.endpoint.name, e);
        }

        lookup
    }
}

impl CoverartProvider for ExternalCoverartProvider {
    fn name(&self) -> &str {
        &self.endpoint.name
    }

    fn display_name(&self) -> &str {
        &self.endpoint.display_name
    }

    fn supported_methods(&self) -> HashSet<CoverartMethod> {
        self.methods.clone()
    }

    fn is_slow(&self) -> bool {
        true
    }

    fn timeout(&self) -> Duration {
        Duration::from_secs(self.endpoint.timeout_seconds)
    }

    fn cached_coverart(&self, query: &CoverartQuery) -> Option<Vec<String>> {
        let key = cache_key(&self.endpoint.name, query);
        match attributecache::get::<Lookup>(&key) {
            Ok(Some(cached)) => Some(cached.urls()),
            Ok(None) => None,
            Err(e) => {
                warn!("External cover art '{}': cache read failed: {}", self.endpoint.name, e);
                None
            }
        }
    }

    fn get_artist_coverart_impl(&self, artist: &str) -> Vec<String> {
        self.lookup_with(&CoverartQuery::Artist(artist.to_string()), LookupMode::Interactive)
            .urls()
    }

    fn get_song_coverart_impl(&self, title: &str, artist: &str) -> Vec<String> {
        self.lookup_with(
            &CoverartQuery::Song {
                title: title.to_string(),
                artist: artist.to_string(),
            },
            LookupMode::Interactive,
        )
        .urls()
    }

    fn get_album_coverart_impl(&self, title: &str, artist: &str, year: Option<i32>) -> Vec<String> {
        self.lookup_with(
            &CoverartQuery::Album {
                title: title.to_string(),
                artist: artist.to_string(),
                year,
            },
            LookupMode::Interactive,
        )
        .urls()
    }

    fn get_url_coverart_impl(&self, url: &str) -> Vec<String> {
        self.lookup_with(&CoverartQuery::Url(url.to_string()), LookupMode::Interactive)
            .urls()
    }
}

/// Read `services.external_coverart` and build a provider per endpoint.
///
/// Must run before `coverart_providers::register_all_providers`, which is
/// what puts them in the registry.
pub fn initialize_from_config(config: &serde_json::Value) {
    let endpoints = config::parse_endpoints(config);
    let providers: Vec<Arc<ExternalCoverartProvider>> = endpoints
        .into_iter()
        .map(|endpoint| Arc::new(ExternalCoverartProvider::new(endpoint)))
        .collect();

    info!("External cover art: {} endpoint(s) configured", providers.len());
    *ENDPOINTS.lock() = providers;
}

/// The configured providers, for the registry and for the worker.
pub fn configured_providers() -> Vec<Arc<ExternalCoverartProvider>> {
    ENDPOINTS.lock().clone()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::helpers::coverart::{CoverartMethod, CoverartProvider, CoverartQuery};

    fn endpoint() -> config::EndpointConfig {
        config::EndpointConfig {
            name: "llm".to_string(),
            display_name: "AI Lookup".to_string(),
            url: "https://x.example/c?artist={artist}&title={title}".to_string(),
            methods: vec![CoverartMethod::Song, CoverartMethod::Album],
            headers: Default::default(),
            timeout_seconds: 45,
            trigger: config::Trigger::Fallback,
            cache_ttl_days: 30,
            negative_cache_ttl_days: 7,
            max_concurrent: 1,
        }
    }

    fn song() -> CoverartQuery {
        CoverartQuery::Song {
            title: "Uni Acronym".to_string(),
            artist: "Alva Noto".to_string(),
        }
    }

    /// The whole reason for the latency class: this provider must never be on
    /// a request path by default.
    #[test]
    fn the_provider_is_slow_and_reports_its_timeout() {
        let provider = ExternalCoverartProvider::new(endpoint());
        assert!(provider.is_slow());
        assert_eq!(provider.timeout(), Duration::from_secs(45));
    }

    #[test]
    fn supported_methods_come_from_the_configuration() {
        let provider = ExternalCoverartProvider::new(endpoint());
        let methods = provider.supported_methods();
        assert!(methods.contains(&CoverartMethod::Song));
        assert!(methods.contains(&CoverartMethod::Album));
        assert!(!methods.contains(&CoverartMethod::Artist));
    }

    #[test]
    fn the_name_and_display_name_come_from_the_configuration() {
        let provider = ExternalCoverartProvider::new(endpoint());
        assert_eq!(provider.name(), "llm");
        assert_eq!(provider.display_name(), "AI Lookup");
    }

    /// Two endpoints must not share a cache entry, and two different songs
    /// must not either.
    #[test]
    fn cache_keys_separate_endpoints_and_queries() {
        let a = cache_key("llm", &song());
        let b = cache_key("other", &song());
        let c = cache_key(
            "llm",
            &CoverartQuery::Song {
                title: "Xerrox Monophaser 1".to_string(),
                artist: "Alva Noto".to_string(),
            },
        );
        assert_ne!(a, b);
        assert_ne!(a, c);
        assert!(a.starts_with("coverart::external::llm::"));
    }

    /// Metadata casing and stray whitespace vary between players for the same
    /// track; paying 30 seconds twice for that would be the expensive kind of
    /// cache miss.
    #[test]
    fn cache_keys_ignore_case_and_surrounding_whitespace() {
        let padded = CoverartQuery::Song {
            title: "  uni acronym ".to_string(),
            artist: "ALVA NOTO".to_string(),
        };
        assert_eq!(cache_key("llm", &song()), cache_key("llm", &padded));
    }

    /// A song title and an artist name must not be able to collide by
    /// swapping which field they are in.
    #[test]
    fn cache_keys_separate_the_fields() {
        let swapped = CoverartQuery::Song {
            title: "Alva Noto".to_string(),
            artist: "Uni Acronym".to_string(),
        };
        assert_ne!(cache_key("llm", &song()), cache_key("llm", &swapped));
    }

    /// Artist and title come from arbitrary player metadata and radio-stream
    /// text, not from anything an administrator controls. A literal `|` in
    /// one field must not be readable as the boundary between two different
    /// fields: without escaping, `artist: "a|b", title: "c"` and
    /// `artist: "a", title: "b|c"` would produce the identical key.
    #[test]
    fn cache_keys_escape_a_literal_separator_inside_a_field() {
        let a = cache_key(
            "llm",
            &CoverartQuery::Song {
                artist: "a|b".to_string(),
                title: "c".to_string(),
            },
        );
        let b = cache_key(
            "llm",
            &CoverartQuery::Song {
                artist: "a".to_string(),
                title: "b|c".to_string(),
            },
        );
        assert_ne!(a, b);
    }

    /// An album's year is part of its identity for this lookup.
    #[test]
    fn cache_keys_include_the_album_year() {
        let with_year = CoverartQuery::Album {
            title: "Xerrox Vol. 2".to_string(),
            artist: "Alva Noto".to_string(),
            year: Some(2009),
        };
        let without_year = CoverartQuery::Album {
            title: "Xerrox Vol. 2".to_string(),
            artist: "Alva Noto".to_string(),
            year: None,
        };
        assert_ne!(cache_key("llm", &with_year), cache_key("llm", &without_year));
    }

    /// A miss must be distinguishable from a cached "there is nothing":
    /// `None` sends the caller to the network, `Some(vec![])` does not.
    #[test]
    fn an_uncached_query_returns_none() {
        let provider = ExternalCoverartProvider::new(endpoint());
        let query = CoverartQuery::Song {
            title: "A Track Nothing Has Ever Looked Up".to_string(),
            artist: "No Such Artist At All".to_string(),
        };
        assert_eq!(provider.cached_coverart(&query), None);
    }

    use super::stub_server::StubServer;
    use protocol::Lookup;
    use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};

    /// A title no earlier run can have cached.
    ///
    /// The attribute cache is a real SQLite file whose location depends on the
    /// machine, and a cached answer would make the provider skip the HTTP call
    /// these tests exist to observe. A unique title sidesteps that without the
    /// tests having to reach into the cache.
    fn unique_title() -> String {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock is after the epoch")
            .as_nanos();
        format!(
            "stub-{}-{}-{}",
            std::process::id(),
            nanos,
            COUNTER.fetch_add(1, AtomicOrdering::SeqCst)
        )
    }

    fn stub_query() -> CoverartQuery {
        CoverartQuery::Song {
            title: unique_title(),
            artist: "Alva Noto".to_string(),
        }
    }

    fn provider_for(server: &StubServer, timeout_seconds: u64) -> ExternalCoverartProvider {
        let mut endpoint = endpoint();
        endpoint.name = format!("stub-{}", unique_title());
        endpoint.url = format!("{}?artist={{artist}}&title={{title}}", server.url());
        endpoint.timeout_seconds = timeout_seconds;
        endpoint.headers.insert(
            "Authorization".to_string(),
            "Bearer test-token".to_string(),
        );
        ExternalCoverartProvider::new(endpoint)
    }

    #[test]
    fn a_populated_response_becomes_found() {
        let server = StubServer::serving(
            200,
            r#"{"images":[{"url":"https://img.example/a.jpg"}]}"#,
        );
        let provider = provider_for(&server, 5);

        assert_eq!(
            provider.lookup(&stub_query()),
            Lookup::Found(vec!["https://img.example/a.jpg".to_string()])
        );
    }

    /// The template and the configured headers have to survive the trip to
    /// the wire; this is the only test that sees what was actually sent.
    #[test]
    fn the_expanded_url_and_configured_headers_reach_the_server() {
        let server = StubServer::serving(200, r#"{"images":[]}"#);
        let provider = provider_for(&server, 5);

        let query = CoverartQuery::Song {
            title: "Uni Acronym".to_string(),
            artist: "Alva Noto".to_string(),
        };
        provider.lookup(&query);

        let request = server.last_request().expect("the server was called");
        assert!(
            request.contains("artist=Alva%20Noto&title=Uni%20Acronym"),
            "request line did not carry the expanded template: {}",
            request
        );
        assert!(
            request.contains("Authorization: Bearer test-token"),
            "configured headers were not sent: {}",
            request
        );
    }

    #[test]
    fn an_empty_response_becomes_no_artwork() {
        let server = StubServer::serving(200, r#"{"images":[]}"#);
        let provider = provider_for(&server, 5);

        assert_eq!(provider.lookup(&stub_query()), Lookup::NoArtwork);
    }

    /// A server error is a fault, not a statement that the track has no
    /// artwork -- the distinction the TTLs rest on.
    #[test]
    fn a_server_error_becomes_error() {
        let server = StubServer::serving(500, "upstream exploded");
        let provider = provider_for(&server, 5);

        assert_eq!(provider.lookup(&stub_query()), Lookup::Error);
    }

    #[test]
    fn a_body_that_is_not_json_becomes_error() {
        let server = StubServer::serving(200, "<html>not json</html>");
        let provider = provider_for(&server, 5);

        assert_eq!(provider.lookup(&stub_query()), Lookup::Error);
    }

    /// The timeout is the promise that a wedged endpoint cannot hold a thread
    /// open indefinitely.
    #[test]
    fn a_server_that_never_answers_times_out_as_error() {
        let server = StubServer::silent();
        let provider = provider_for(&server, 1);

        let start = std::time::Instant::now();
        assert_eq!(provider.lookup(&stub_query()), Lookup::Error);
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "the configured 1s timeout was not honoured"
        );
    }

    /// A second lookup while the first still holds the only slot is abandoned
    /// rather than queued, so threads cannot pile up against a stuck endpoint.
    #[test]
    fn a_lookup_that_cannot_get_a_slot_gives_up() {
        let server = StubServer::silent();
        let provider = Arc::new(provider_for(&server, 5));

        let busy = provider.clone();
        let holder = std::thread::spawn(move || busy.lookup(&stub_query()));

        // Let the first lookup take the slot and block on the silent server.
        std::thread::sleep(Duration::from_millis(200));

        let start = std::time::Instant::now();
        assert_eq!(provider.lookup(&stub_query()), Lookup::Error);
        assert!(
            start.elapsed() < Duration::from_secs(1),
            "the second lookup waited instead of giving up"
        );

        let _ = holder.join();
    }

    // --- LookupMode: Background gives up, Interactive queues briefly ---
    //
    // These exercise the `Slots` primitive directly, with the test itself
    // (or a channel handshake between threads) establishing "a slot is
    // held" rather than a sleep guessing at timing.

    /// `try_acquire` (what `Background` uses) never blocks, even though a
    /// slot is provably held for the whole assertion.
    #[test]
    fn slots_try_acquire_does_not_wait_for_a_held_slot() {
        let slots = Arc::new(Slots::new(1));
        let held = slots.try_acquire().expect("the only slot");

        let start = Instant::now();
        assert!(slots.try_acquire().is_none(), "a held slot must not be handed out");
        assert!(start.elapsed() < Duration::from_millis(50), "try_acquire must not block");

        drop(held);
    }

    /// `acquire_blocking` (what `Interactive` uses) queues rather than
    /// failing immediately, and succeeds once the slot is released.
    ///
    /// The proof that it actually waited -- rather than racing ahead before
    /// the release -- is `acquired_rx.recv_timeout` returning nothing while
    /// the slot is still held: a bounded, reactive wait, not a fixed sleep
    /// standing in for "long enough".
    #[test]
    fn slots_acquire_blocking_waits_for_a_held_slot_then_succeeds() {
        use std::sync::mpsc;

        let slots = Arc::new(Slots::new(1));
        let held = slots.try_acquire().expect("the only slot");

        let (acquired_tx, acquired_rx) = mpsc::channel::<()>();
        let waiter_slots = slots.clone();
        let waiter = std::thread::spawn(move || {
            let guard = waiter_slots.acquire_blocking(Duration::from_secs(5));
            let _ = acquired_tx.send(());
            guard
        });

        assert!(
            acquired_rx.recv_timeout(Duration::from_millis(200)).is_err(),
            "the waiter acquired a slot that was still held"
        );

        drop(held);

        acquired_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("the waiter acquired the slot after it was released");
        let result = waiter.join().expect("waiter thread completed");
        assert!(result.is_some(), "acquire_blocking must succeed once the slot frees up");
    }

    /// `acquire_blocking` gives up once its own deadline passes, rather than
    /// waiting forever for a slot that never frees.
    #[test]
    fn slots_acquire_blocking_gives_up_after_its_timeout() {
        let slots = Arc::new(Slots::new(1));
        let held = slots.try_acquire().expect("the only slot");

        let start = Instant::now();
        assert!(slots.acquire_blocking(Duration::from_millis(100)).is_none());
        assert!(
            start.elapsed() < Duration::from_secs(1),
            "acquire_blocking did not honour its own timeout"
        );

        drop(held);
    }

    /// Wiring: `lookup_with(.., Background)` still gives up immediately when
    /// a slot is held, exactly like the pre-`LookupMode` `lookup()` did.
    #[test]
    fn background_mode_gives_up_immediately_when_a_slot_is_held() {
        let server = StubServer::silent();
        let provider = Arc::new(provider_for(&server, 5));
        let held = provider.slots.try_acquire().expect("the only slot");

        let start = Instant::now();
        assert_eq!(
            provider.lookup_with(&stub_query(), LookupMode::Background),
            Lookup::Error
        );
        assert!(
            start.elapsed() < Duration::from_millis(200),
            "Background must not wait for a held slot"
        );

        drop(held);
    }

    /// Wiring: `lookup_with(.., Interactive)` queues behind a held slot and
    /// then completes a real lookup once it frees up -- the fix for
    /// `?include_slow=true` returning nothing while the worker holds the
    /// endpoint's only slot.
    #[test]
    fn interactive_mode_waits_for_a_held_slot_and_then_completes_the_lookup() {
        let server = StubServer::serving(
            200,
            r#"{"images":[{"url":"https://img.example/interactive.jpg"}]}"#,
        );
        let provider = Arc::new(provider_for(&server, 5));
        let held = provider.slots.try_acquire().expect("the only slot");

        let waiter = provider.clone();
        let query = stub_query();
        let waiter_query = query.clone();
        let handle = std::thread::spawn(move || waiter.lookup_with(&waiter_query, LookupMode::Interactive));

        drop(held);

        let result = handle.join().expect("interactive lookup thread completed");
        assert_eq!(
            result,
            Lookup::Found(vec!["https://img.example/interactive.jpg".to_string()])
        );
    }

    /// A denied slot must never be cached as anything: the next request,
    /// whichever mode it arrives in, must still be free to try the network.
    #[test]
    fn a_denied_slot_is_not_cached() {
        let server = StubServer::serving(200, r#"{"images":[]}"#);
        let provider = Arc::new(provider_for(&server, 5));
        let query = stub_query();

        let held = provider.slots.try_acquire().expect("the only slot");
        assert_eq!(provider.lookup_with(&query, LookupMode::Background), Lookup::Error);
        assert_eq!(provider.cached_coverart(&query), None, "a denied slot must not be cached");
        drop(held);

        // With the slot free, the same query now actually reaches the
        // network and is cached as a real answer.
        assert_eq!(provider.lookup_with(&query, LookupMode::Interactive), Lookup::NoArtwork);
    }
}
