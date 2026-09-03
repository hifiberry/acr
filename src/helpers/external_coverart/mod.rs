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

use std::collections::HashSet;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use log::{debug, info, warn};
use once_cell::sync::Lazy;
use parking_lot::Mutex;

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
/// 30 seconds twice for that would be the expensive kind of cache miss. The
/// fields are separated by a marker no field can contain after trimming, so a
/// title and an artist cannot collide by swapping places.
pub fn cache_key(endpoint_name: &str, query: &CoverartQuery) -> String {
    let clean = |value: &str| value.trim().to_lowercase();
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

/// A non-blocking counting semaphore.
///
/// A caller that cannot get a slot gives up rather than queueing. Queueing
/// would be the wrong answer here: a request that waits behind two 40-second
/// lookups has outlived whatever it was for, and letting threads accumulate
/// against a stuck endpoint is exactly the failure this whole change exists
/// to prevent.
struct Slots {
    in_use: AtomicUsize,
    limit: usize,
}

impl Slots {
    fn new(limit: usize) -> Self {
        Self {
            in_use: AtomicUsize::new(0),
            limit: limit.max(1),
        }
    }

    fn try_acquire(self: &Arc<Self>) -> Option<SlotGuard> {
        let mut current = self.in_use.load(Ordering::SeqCst);
        loop {
            if current >= self.limit {
                return None;
            }
            match self.in_use.compare_exchange(
                current,
                current + 1,
                Ordering::SeqCst,
                Ordering::SeqCst,
            ) {
                Ok(_) => return Some(SlotGuard { slots: self.clone() }),
                Err(actual) => current = actual,
            }
        }
    }
}

struct SlotGuard {
    slots: Arc<Slots>,
}

impl Drop for SlotGuard {
    fn drop(&mut self) {
        self.slots.in_use.fetch_sub(1, Ordering::SeqCst);
    }
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
    /// Returns the cached answer without a network call when there is one,
    /// including a cached error: an endpoint that just failed is not asked
    /// again for an hour.
    pub fn lookup(&self, query: &CoverartQuery) -> Lookup {
        let key = cache_key(&self.endpoint.name, query);

        match attributecache::get::<Lookup>(&key) {
            Ok(Some(cached)) => {
                debug!("External cover art '{}': cache hit for {}", self.endpoint.name, key);
                return cached;
            }
            Ok(None) => {}
            Err(e) => warn!("External cover art '{}': cache read failed: {}", self.endpoint.name, e),
        }

        let Some(_slot) = self.slots.try_acquire() else {
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
        self.lookup(&CoverartQuery::Artist(artist.to_string())).urls()
    }

    fn get_song_coverart_impl(&self, title: &str, artist: &str) -> Vec<String> {
        self.lookup(&CoverartQuery::Song {
            title: title.to_string(),
            artist: artist.to_string(),
        })
        .urls()
    }

    fn get_album_coverart_impl(&self, title: &str, artist: &str, year: Option<i32>) -> Vec<String> {
        self.lookup(&CoverartQuery::Album {
            title: title.to_string(),
            artist: artist.to_string(),
            year,
        })
        .urls()
    }

    fn get_url_coverart_impl(&self, url: &str) -> Vec<String> {
        self.lookup(&CoverartQuery::Url(url.to_string())).urls()
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
}
