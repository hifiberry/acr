use std::collections::HashSet;
use std::sync::Arc;
use std::time::{Duration, Instant};
use std::sync::mpsc;
use parking_lot::Mutex;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use log::debug;
use crate::helpers::image_meta::{image_size, ImageMetadata};
use crate::helpers::image_grader::{ImageGrader, ImageInfo as GraderImageInfo};

/// Provider information structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderInfo {
    pub name: String,
    pub display_name: String,
}

/// Image information with URL and metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageInfo {
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub height: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grade: Option<i32>,
}

impl ImageInfo {
    /// Create a new ImageInfo with just a URL (no metadata)
    pub fn new(url: String) -> Self {
        Self {
            url,
            width: None,
            height: None,
            size_bytes: None,
            format: None,
            grade: None,
        }
    }

    /// Create a new ImageInfo with URL and metadata
    pub fn with_metadata(url: String, metadata: ImageMetadata) -> Self {
        Self {
            url,
            width: Some(metadata.width),
            height: Some(metadata.height),
            size_bytes: Some(metadata.size_bytes),
            format: Some(metadata.format),
            grade: None,
        }
    }

    /// Fetch and add metadata for this image
    pub fn fetch_metadata(&mut self) {
        if let Ok(metadata) = image_size(&self.url) {
            self.width = Some(metadata.width);
            self.height = Some(metadata.height);
            self.size_bytes = Some(metadata.size_bytes);
            self.format = Some(metadata.format);
        }
    }

    /// Set the grade for this image
    pub fn set_grade(&mut self, grade: i32) {
        self.grade = Some(grade);
    }
}

/// Cover art result from a specific provider
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoverartResult {
    pub provider: ProviderInfo,
    pub images: Vec<ImageInfo>,
}

impl CoverartResult {
    /// Create a new CoverartResult from a provider and list of URLs
    pub fn new(provider: ProviderInfo, urls: Vec<String>) -> Self {
        let mut images = Vec::new();
        
        for url in &urls {
            let mut image_info = ImageInfo::new(url.clone());
            // Try to fetch metadata for each image
            image_info.fetch_metadata();
            images.push(image_info);
        }
        
        Self::with_images(provider, images)
    }

    /// Create a new CoverartResult with pre-computed ImageInfo and apply grading
    pub fn with_images(provider: ProviderInfo, mut images: Vec<ImageInfo>) -> Self {
        // Apply grading to all images
        let grader = ImageGrader::new();
        
        for image in &mut images {
            // Convert to grader format
            let grader_info = GraderImageInfo {
                url: image.url.clone(),
                width: image.width,
                height: image.height,
                size_bytes: image.size_bytes,
                format: image.format.clone(),
                provider: provider.name.clone(),
            };
            
            // Grade the image
            let grade = grader.grade_image(&grader_info);
            image.set_grade(grade.score);
        }
        
        // Sort images by grade (highest first)
        images.sort_by(|a, b| {
            let grade_a = a.grade.unwrap_or(0);
            let grade_b = b.grade.unwrap_or(0);
            grade_b.cmp(&grade_a)
        });
        
        Self {
            provider,
            images,
        }
    }
}

/// Defines the types of cover art retrieval methods that a provider can support
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum CoverartMethod {
    /// Get cover art for an artist by name
    Artist,
    /// Get cover art for a song by title and artist
    Song,
    /// Get cover art for an album by title, artist, and optional year
    Album,
    /// Get cover art from a URL
    Url,
}

/// A cover art lookup, as one value.
///
/// The manager previously carried four near-identical query methods; making
/// the query a value collapses them to one fan-out and gives the provider
/// trait a single cache key to reason about.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum CoverartQuery {
    Artist(String),
    Song { title: String, artist: String },
    Album { title: String, artist: String, year: Option<i32> },
    Url(String),
}

impl CoverartQuery {
    /// The method a provider must support to answer this query.
    pub fn method(&self) -> CoverartMethod {
        match self {
            CoverartQuery::Artist(_) => CoverartMethod::Artist,
            CoverartQuery::Song { .. } => CoverartMethod::Song,
            CoverartQuery::Album { .. } => CoverartMethod::Album,
            CoverartQuery::Url(_) => CoverartMethod::Url,
        }
    }
}

/// How long the fast path waits for the whole fan-out. Providers that
/// declare themselves slow are not on it; see `CoverartProvider::is_slow`.
pub const DEFAULT_FAST_DEADLINE: Duration = Duration::from_secs(5);

/// What a caller wants from a fan-out.
#[derive(Debug, Clone)]
pub struct QueryOptions {
    /// Wait for providers that declare themselves slow. Off by default: a
    /// slow provider may take tens of seconds, which no request path can sit
    /// through.
    pub include_slow: bool,

    /// The deadline for the fast path. When `include_slow` is set, the
    /// deadline instead comes from the slowest selected provider's own
    /// `timeout()`, since waiting is the whole point of asking.
    pub fast_deadline: Duration,
}

impl Default for QueryOptions {
    fn default() -> Self {
        Self {
            include_slow: false,
            fast_deadline: DEFAULT_FAST_DEADLINE,
        }
    }
}

/// Trait for cover art providers that can retrieve cover art from various sources
pub trait CoverartProvider {
    /// Returns the internal name identifier for this provider
    fn name(&self) -> &str;

    /// Returns the human-readable display name for this provider
    fn display_name(&self) -> &str;

    /// Returns the set of methods this provider supports
    fn supported_methods(&self) -> HashSet<CoverartMethod>;

    /// How long a call to this provider may take. Used to size the deadline
    /// when a caller opts into slow providers; ignored on the fast path,
    /// which has its own deadline.
    fn timeout(&self) -> Duration {
        DEFAULT_FAST_DEADLINE
    }

    /// Whether a call to this provider may take long enough that it must not
    /// sit on a request path. A slow provider is consulted for cached
    /// answers only, unless the caller opts in through
    /// [`QueryOptions::include_slow`].
    fn is_slow(&self) -> bool {
        false
    }

    /// Answer from this provider's own cache, without a network round trip.
    ///
    /// `None` means there is nothing to show: either nothing is cached, or
    /// what is cached is an absence of artwork, an error, or a set of images
    /// whose files have since gone. Callers do the same thing with all four,
    /// so they are not distinguished here. Only slow providers need to
    /// implement this: it is what keeps their answers available on the fast
    /// path once they have been found.
    fn cached_coverart(&self, _query: &CoverartQuery) -> Option<Vec<String>> {
        None
    }

    /// Get cover art for an artist by name
    /// 
    /// # Arguments
    /// * `artist` - The artist name
    /// 
    /// # Returns
    /// * `Vec<String>` - URLs or local file paths to cover art
    fn get_artist_coverart(&self, artist: &str) -> Vec<String> {
        if self.supported_methods().contains(&CoverartMethod::Artist) {
            self.get_artist_coverart_impl(artist)
        } else {
            Vec::new()
        }
    }

    /// Get cover art for a song by title and artist
    /// 
    /// # Arguments
    /// * `title` - The song title
    /// * `artist` - The artist name
    /// 
    /// # Returns
    /// * `Vec<String>` - URLs or local file paths to cover art
    fn get_song_coverart(&self, title: &str, artist: &str) -> Vec<String> {
        if self.supported_methods().contains(&CoverartMethod::Song) {
            self.get_song_coverart_impl(title, artist)
        } else {
            Vec::new()
        }
    }

    /// Get cover art for an album by title, artist, and optional year
    /// 
    /// # Arguments
    /// * `title` - The album title
    /// * `artist` - The artist name
    /// * `year` - Optional release year
    /// 
    /// # Returns
    /// * `Vec<String>` - URLs or local file paths to cover art
    fn get_album_coverart(&self, title: &str, artist: &str, year: Option<i32>) -> Vec<String> {
        if self.supported_methods().contains(&CoverartMethod::Album) {
            self.get_album_coverart_impl(title, artist, year)
        } else {
            Vec::new()
        }
    }

    /// Get cover art from a URL
    /// 
    /// # Arguments
    /// * `url` - The URL to retrieve cover art from
    /// 
    /// # Returns
    /// * `Vec<String>` - URLs or local file paths to cover art
    fn get_url_coverart(&self, url: &str) -> Vec<String> {
        if self.supported_methods().contains(&CoverartMethod::Url) {
            self.get_url_coverart_impl(url)
        } else {
            Vec::new()
        }
    }

    // Implementation methods that providers must implement for supported methods
    // These are called only if the method is marked as supported

    /// Implementation for artist cover art retrieval
    /// Only called if CoverartMethod::Artist is in supported_methods()
    fn get_artist_coverart_impl(&self, _artist: &str) -> Vec<String> {
        Vec::new()
    }

    /// Implementation for song cover art retrieval
    /// Only called if CoverartMethod::Song is in supported_methods()
    fn get_song_coverart_impl(&self, _title: &str, _artist: &str) -> Vec<String> {
        Vec::new()
    }

    /// Implementation for album cover art retrieval
    /// Only called if CoverartMethod::Album is in supported_methods()
    fn get_album_coverart_impl(&self, _title: &str, _artist: &str, _year: Option<i32>) -> Vec<String> {
        Vec::new()
    }

    /// Implementation for URL cover art retrieval
    /// Only called if CoverartMethod::Url is in supported_methods()
    fn get_url_coverart_impl(&self, _url: &str) -> Vec<String> {
        Vec::new()
    }
}

/// Global coverart manager that maintains a registry of coverart providers
pub struct CoverartManager {
    providers: Vec<Arc<dyn CoverartProvider + Send + Sync>>,
}

impl CoverartManager {
    /// Create a new empty coverart manager
    pub fn new() -> Self {
        Self {
            providers: Vec::new(),
        }
    }

    /// Register a new coverart provider
    pub fn register_provider(&mut self, provider: Arc<dyn CoverartProvider + Send + Sync>) {
        debug!("Registering coverart provider: {} ({})", provider.name(), provider.display_name());
        self.providers.push(provider);
        debug!("Total registered providers: {}", self.providers.len());
    }

    /// Get all registered providers (for debugging/inspection)
    pub fn get_providers(&self) -> &Vec<Arc<dyn CoverartProvider + Send + Sync>> {
        &self.providers
    }
    
    /// Get the number of registered providers
    pub fn provider_count(&self) -> usize {
        self.providers.len()
    }
}

impl Default for CoverartManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Ask one provider the query it was selected for.
fn run_query(
    provider: &Arc<dyn CoverartProvider + Send + Sync>,
    query: &CoverartQuery,
) -> Vec<String> {
    match query {
        CoverartQuery::Artist(artist) => provider.get_artist_coverart(artist),
        CoverartQuery::Song { title, artist } => provider.get_song_coverart(title, artist),
        CoverartQuery::Album { title, artist, year } => {
            provider.get_album_coverart(title, artist, *year)
        }
        CoverartQuery::Url(url) => provider.get_url_coverart(url),
    }
}

/// Query a set of providers in parallel, under a deadline.
///
/// Each provider runs on its own thread, so the metadata fetch and grading
/// that `CoverartResult::new` performs per URL are parallel too -- they used
/// to be serial across every URL from every provider, and under the registry
/// lock. A provider that misses the deadline is not waited for; its thread
/// finishes and whatever caching it does is not wasted.
pub(crate) fn fan_out(
    providers: Vec<Arc<dyn CoverartProvider + Send + Sync>>,
    query: &CoverartQuery,
    opts: &QueryOptions,
) -> Vec<CoverartResult> {
    let method = query.method();
    let selected: Vec<_> = providers
        .into_iter()
        .filter(|p| p.supported_methods().contains(&method))
        .collect();

    if selected.is_empty() {
        return Vec::new();
    }

    let deadline = if opts.include_slow {
        selected
            .iter()
            .map(|p| p.timeout())
            .max()
            .unwrap_or(opts.fast_deadline)
            .max(opts.fast_deadline)
    } else {
        opts.fast_deadline
    };

    let (tx, rx) = mpsc::channel::<(usize, Option<CoverartResult>)>();
    let expected = selected.len();

    for (index, provider) in selected.into_iter().enumerate() {
        let tx = tx.clone();
        let query = query.clone();
        let include_slow = opts.include_slow;
        std::thread::spawn(move || {
            // A slow provider is not called on the fast path, but its cache
            // is: a cache hit is not slow, so an answer it found earlier
            // still reaches the caller.
            let urls = if provider.is_slow() && !include_slow {
                provider.cached_coverart(&query).unwrap_or_default()
            } else {
                run_query(&provider, &query)
            };

            let message = if urls.is_empty() {
                None
            } else {
                Some(CoverartResult::new(
                    ProviderInfo {
                        name: provider.name().to_string(),
                        display_name: provider.display_name().to_string(),
                    },
                    urls,
                ))
            };
            let _ = tx.send((index, message));
        });
    }
    drop(tx);

    let end = Instant::now() + deadline;
    let mut collected: Vec<(usize, CoverartResult)> = Vec::new();
    for _ in 0..expected {
        let remaining = end.saturating_duration_since(Instant::now());
        match rx.recv_timeout(remaining) {
            Ok((index, Some(result))) => collected.push((index, result)),
            Ok((_, None)) => {}
            Err(_) => break,
        }
    }

    // Registration order, whatever order the threads finished in.
    collected.sort_by_key(|(index, _)| *index);
    collected.into_iter().map(|(_, result)| result).collect()
}

/// Run a cover art query against every registered provider.
///
/// The registry lock is taken only to snapshot the providers, never across
/// the network calls that follow.
pub fn query_coverart(query: &CoverartQuery, opts: &QueryOptions) -> Vec<CoverartResult> {
    let providers = {
        let manager = get_coverart_manager();
        let guard = manager.lock();
        guard.get_providers().clone()
    };
    fan_out(providers, query, opts)
}

/// Global singleton instance of the coverart manager
static COVERART_MANAGER: Lazy<Arc<Mutex<CoverartManager>>> = Lazy::new(|| {
    Arc::new(Mutex::new(CoverartManager::new()))
});

/// Get a reference to the global coverart manager
pub fn get_coverart_manager() -> Arc<Mutex<CoverartManager>> {
    COVERART_MANAGER.clone()
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};

    /// A provider that blocks for `delay`, so a test can tell a sequential
    /// fan-out from a parallel one and see whether a deadline is honoured.
    struct SlowStub {
        name: String,
        delay: Duration,
        url: String,
        called: Arc<AtomicBool>,
    }

    impl CoverartProvider for SlowStub {
        fn name(&self) -> &str { &self.name }
        fn display_name(&self) -> &str { &self.name }
        fn supported_methods(&self) -> HashSet<CoverartMethod> {
            let mut m = HashSet::new();
            m.insert(CoverartMethod::Artist);
            m
        }
        fn get_artist_coverart_impl(&self, _artist: &str) -> Vec<String> {
            self.called.store(true, Ordering::SeqCst);
            std::thread::sleep(self.delay);
            vec![self.url.clone()]
        }
    }

    fn stub(name: &str, delay_ms: u64) -> (Arc<SlowStub>, Arc<AtomicBool>) {
        let called = Arc::new(AtomicBool::new(false));
        let stub = Arc::new(SlowStub {
            name: name.to_string(),
            delay: Duration::from_millis(delay_ms),
            // A URL no metadata fetch can resolve: the point of these tests is
            // the fan-out, and `CoverartResult::new` tolerates a failed fetch.
            url: format!("https://coverart.invalid/{}.jpg", name),
            called: called.clone(),
        });
        (stub, called)
    }

    fn providers(list: Vec<Arc<SlowStub>>) -> Vec<Arc<dyn CoverartProvider + Send + Sync>> {
        list.into_iter()
            .map(|p| p as Arc<dyn CoverartProvider + Send + Sync>)
            .collect()
    }

    /// Three providers that each block for 300ms must finish together, not
    /// one after another. Sequentially this is 900ms; the assertion leaves
    /// generous room for a loaded CI machine while still failing a serial
    /// implementation.
    #[test]
    fn providers_are_queried_in_parallel() {
        let (a, _) = stub("a", 300);
        let (b, _) = stub("b", 300);
        let (c, _) = stub("c", 300);

        let start = Instant::now();
        let results = fan_out(
            providers(vec![a, b, c]),
            &CoverartQuery::Artist("Alva Noto".to_string()),
            &QueryOptions::default(),
        );
        let elapsed = start.elapsed();

        assert_eq!(results.len(), 3, "every provider answered");
        assert!(
            elapsed < Duration::from_millis(700),
            "fan-out took {:?}; a sequential one would take ~900ms",
            elapsed
        );
    }

    /// Results keep registration order however the threads finish, because
    /// the API is a list clients render in order.
    #[test]
    fn results_keep_registration_order() {
        let (slow, _) = stub("slow", 200);
        let (fast, _) = stub("fast", 0);

        let results = fan_out(
            providers(vec![slow, fast]),
            &CoverartQuery::Artist("Alva Noto".to_string()),
            &QueryOptions::default(),
        );

        let names: Vec<_> = results.iter().map(|r| r.provider.name.as_str()).collect();
        assert_eq!(names, vec!["slow", "fast"]);
    }

    /// A provider that overruns the deadline does not hold up the ones that
    /// answered. Its own thread runs on; the caller simply stops waiting.
    #[test]
    fn a_provider_past_the_deadline_is_not_waited_for() {
        let (quick, _) = stub("quick", 0);
        let (glacial, _) = stub("glacial", 5_000);

        let opts = QueryOptions {
            fast_deadline: Duration::from_millis(300),
            ..QueryOptions::default()
        };

        let start = Instant::now();
        let results = fan_out(
            providers(vec![quick, glacial]),
            &CoverartQuery::Artist("Alva Noto".to_string()),
            &opts,
        );
        let elapsed = start.elapsed();

        assert_eq!(results.len(), 1, "only the provider that answered in time");
        assert_eq!(results[0].provider.name, "quick");
        assert!(elapsed < Duration::from_secs(2), "returned at the deadline, not at 5s");
    }

    /// A provider that does not support the query's method is never called --
    /// the old code asked every provider and relied on each to return empty.
    #[test]
    fn providers_that_do_not_support_the_method_are_not_called() {
        let (artist_only, called) = stub("artist-only", 0);

        let results = fan_out(
            providers(vec![artist_only]),
            &CoverartQuery::Song {
                title: "Uni Acronym".to_string(),
                artist: "Alva Noto".to_string(),
            },
            &QueryOptions::default(),
        );

        assert!(results.is_empty());
        assert!(!called.load(Ordering::SeqCst), "the provider was not asked");
    }

    /// The global registry lock guards the registry, not the network. If the
    /// fan-out still held it, this second lock attempt would deadlock.
    #[test]
    fn the_registry_lock_is_not_held_across_provider_calls() {
        let (slow, _) = stub("holds-the-lock", 200);
        {
            let manager = get_coverart_manager();
            manager.lock().register_provider(slow);
        }

        let done = Arc::new(AtomicBool::new(false));
        let flag = done.clone();
        std::thread::spawn(move || {
            query_coverart(
                &CoverartQuery::Artist("Alva Noto".to_string()),
                &QueryOptions::default(),
            );
            flag.store(true, Ordering::SeqCst);
        });

        // Give the query time to reach the provider call, then prove the
        // registry is still lockable while it is in flight.
        std::thread::sleep(Duration::from_millis(50));
        let manager = get_coverart_manager();
        let guard = manager.try_lock();
        assert!(
            guard.is_some(),
            "the registry lock is held across a provider call"
        );
        drop(guard);

        std::thread::sleep(Duration::from_millis(400));
        assert!(done.load(Ordering::SeqCst), "the query finished");
    }

    /// A provider that is slow, answers only from its cache, and records
    /// whether the network path was taken.
    struct SlowCachingStub {
        cached: Option<Vec<String>>,
        network_called: Arc<AtomicBool>,
    }

    impl CoverartProvider for SlowCachingStub {
        fn name(&self) -> &str { "slow-caching" }
        fn display_name(&self) -> &str { "Slow Caching" }
        fn supported_methods(&self) -> HashSet<CoverartMethod> {
            let mut m = HashSet::new();
            m.insert(CoverartMethod::Artist);
            m
        }
        fn is_slow(&self) -> bool { true }
        fn timeout(&self) -> Duration { Duration::from_secs(45) }
        fn cached_coverart(&self, _query: &CoverartQuery) -> Option<Vec<String>> {
            self.cached.clone()
        }
        fn get_artist_coverart_impl(&self, _artist: &str) -> Vec<String> {
            self.network_called.store(true, Ordering::SeqCst);
            vec!["https://coverart.invalid/from-network.jpg".to_string()]
        }
    }

    fn slow_caching(cached: Option<Vec<String>>) -> (Arc<SlowCachingStub>, Arc<AtomicBool>) {
        let network_called = Arc::new(AtomicBool::new(false));
        let stub = Arc::new(SlowCachingStub {
            cached,
            network_called: network_called.clone(),
        });
        (stub, network_called)
    }

    /// The whole point of the latency class: a 20-40s provider is never on a
    /// request path unless the caller asked for it.
    #[test]
    fn the_fast_path_does_not_call_a_slow_provider() {
        let (stub, network_called) = slow_caching(None);

        let results = fan_out(
            vec![stub as Arc<dyn CoverartProvider + Send + Sync>],
            &CoverartQuery::Artist("Alva Noto".to_string()),
            &QueryOptions::default(),
        );

        assert!(results.is_empty());
        assert!(!network_called.load(Ordering::SeqCst), "no network call was made");
    }

    /// A cache hit is not slow, so an answer the provider found earlier still
    /// reaches the fast path. This is what lets the REST endpoint improve over
    /// time without ever waiting.
    #[test]
    fn the_fast_path_still_serves_a_slow_providers_cached_answer() {
        let cached = vec!["https://coverart.invalid/cached.jpg".to_string()];
        let (stub, network_called) = slow_caching(Some(cached.clone()));

        let results = fan_out(
            vec![stub as Arc<dyn CoverartProvider + Send + Sync>],
            &CoverartQuery::Artist("Alva Noto".to_string()),
            &QueryOptions::default(),
        );

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].images[0].url, cached[0]);
        assert!(!network_called.load(Ordering::SeqCst), "served from cache, not the network");
    }

    /// Opting in reaches the network, and the deadline comes from the
    /// provider's own timeout rather than the 5s fast deadline.
    #[test]
    fn opting_in_calls_a_slow_provider() {
        let (stub, network_called) = slow_caching(None);

        let results = fan_out(
            vec![stub as Arc<dyn CoverartProvider + Send + Sync>],
            &CoverartQuery::Artist("Alva Noto".to_string()),
            &QueryOptions { include_slow: true, ..QueryOptions::default() },
        );

        assert_eq!(results.len(), 1);
        assert!(network_called.load(Ordering::SeqCst));
    }

    /// Every provider shipped today is fast and answers only over the
    /// network; the defaults must not change their behaviour.
    #[test]
    fn providers_are_fast_and_uncached_by_default() {
        let (plain, _) = stub("plain", 0);
        assert!(!plain.is_slow());
        assert_eq!(
            plain.cached_coverart(&CoverartQuery::Artist("Alva Noto".to_string())),
            None
        );
    }
}
