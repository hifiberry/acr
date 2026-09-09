use serde_json::Value;
use log::{debug, warn, info};
use acr_http::http_client;
use crate::coverart::{CoverartProvider, CoverartMethod};
use moka::sync::Cache;
use std::time::Duration;
use std::collections::HashSet;
use once_cell::sync::Lazy;
use std::sync::atomic::{AtomicBool, Ordering};
use parking_lot::Mutex;
use acr_types::config::get_service_config;
use acr_http::ratelimit;

/// Global flag to indicate if FanArt.tv lookups are enabled
static FANARTTV_ENABLED: AtomicBool = AtomicBool::new(false);

/// API key storage for FanArt.tv
#[derive(Default)]
struct FanarttvConfig {
    api_key: String,
}

// Default API key for FanArt.tv
pub fn default_fanarttv_api_key() -> String {
    "749a8fca4f2d3b0462b287820ad6ab06".to_string()
}

// Global singleton for FanArt.tv configuration
static FANARTTV_CONFIG: Lazy<Mutex<FanarttvConfig>> = Lazy::new(|| {
    Mutex::new(FanarttvConfig::default())
});

/// Initialize FanArt.tv module from configuration
pub fn initialize_from_config(config: &serde_json::Value) {    
    if let Some(fanarttv_config) = get_service_config(config, "fanarttv") {
        // Check if enabled flag exists and is set to true
        let enabled = fanarttv_config.get("enable")
            .and_then(|v| v.as_bool())
            .unwrap_or(true); // Default to enabled if not specified
        
        FANARTTV_ENABLED.store(enabled, Ordering::SeqCst);
        
        // Get API key if provided
        if let Some(api_key) = fanarttv_config.get("api_key").and_then(|v| v.as_str()) {
            {
                let mut config = FANARTTV_CONFIG.lock();
                debug!("Found FanArt.tv API key in config: {}",
                       if !api_key.is_empty() && api_key.len() > 4 {
                           format!("{}...", &api_key[0..4])
                       } else {
                           "Empty".to_string()
                       });

                config.api_key = api_key.to_string();
                if !api_key.is_empty() {
                    info!("FanArt.tv API key configured");
                } else {
                    // Use the default key
                    let default_key = default_fanarttv_api_key();
                    config.api_key = default_key;
                    info!("Using default FanArt.tv API key");
                }
            }
        } else {
            // Use default API key if none provided
            {
                let mut config = FANARTTV_CONFIG.lock();
                let default_key = default_fanarttv_api_key();
                config.api_key = default_key;
                debug!("No API key found for FanArt.tv in configuration, using default");
            }
        }
        
        // Register rate limit - default to 2 requests per second (500ms)
        let rate_limit_ms = fanarttv_config.get("rate_limit_ms")
            .and_then(|v| v.as_u64())
            .unwrap_or(500);
            
        ratelimit::register_service("fanarttv", rate_limit_ms);
        info!("FanArt.tv rate limit set to {} ms", rate_limit_ms);
        
        let status = if enabled { "enabled" } else { "disabled" };
        info!("FanArt.tv lookup {}", status);
    } else {
        // Default to enabled if not in config, with default API key
        FANARTTV_ENABLED.store(true, Ordering::SeqCst);
        {
            let mut config = FANARTTV_CONFIG.lock();
            let default_key = default_fanarttv_api_key();
            config.api_key = default_key;
        }
        debug!("FanArt.tv configuration not found, using defaults (enabled with default API key)");
        
        // Register default rate limit
        ratelimit::register_service("fanarttv", 500);
    }
}

/// Check if FanArt.tv lookups are enabled
pub fn is_enabled() -> bool {
    FANARTTV_ENABLED.load(Ordering::SeqCst)
}

/// Get the configured API key
pub fn get_api_key() -> Option<String> {
    let config = FANARTTV_CONFIG.lock();
    if !config.api_key.is_empty() {
        Some(config.api_key.clone())
    } else {
        None
    }
}

// Using once_cell for failed MBID cache with 24-hour expiry
static FAILED_MBID_CACHE: Lazy<Cache<String, bool>> = Lazy::new(|| {
    Cache::builder()
        // Set a 24-hour time-to-live (TTL)
        .time_to_live(Duration::from_secs(24 * 60 * 60))
        // Set a maximum capacity for the cache
        .max_capacity(1000)
        .build()
});



/// Create a new HTTP client with a timeout of 10 seconds
fn http_client() -> Box<dyn http_client::HttpClient> {
    http_client::new_http_client(10)
}

/// Fetch a fanart.tv URL under the service's rate limit.
///
/// Every request in this module goes through here, which is the point: it used
/// to register a rate limit for "fanarttv" -- and log that it had one --
/// without any call site ever applying it, so fanart.tv was the one provider
/// being hit with no spacing and no bound at all. Reaching the network any
/// other way should be harder than reaching it through this.
fn fanarttv_api_get(url: &str) -> Result<String, http_client::HttpClientError> {
    fanarttv_api_get_with(http_client().as_ref(), url)
}

/// The body of [`fanarttv_api_get`], with the transport supplied by the caller
/// so the request path can be exercised without a network.
///
/// The permit is held across the request, so concurrent callers queue instead
/// of opening parallel connections.
fn fanarttv_api_get_with(
    client: &dyn http_client::HttpClient,
    url: &str,
) -> Result<String, http_client::HttpClientError> {
    let _permit = ratelimit::rate_limit("fanarttv");
    client.get_text(url)
}

/// Get artist thumbnail URLs from FanArt.tv
/// 
/// # Arguments
/// * `artist_mbid` - MusicBrainz ID of the artist
/// * `max_images` - Maximum number of images to return (default: 10)
/// 
/// # Returns
/// * `Vec<String>` - URLs of all available thumbnails, empty if none found
pub fn get_artist_thumbnails(artist_mbid: &str, max_images: Option<usize>) -> Vec<String> {
    // Check if FanArt.tv is enabled
    if !is_enabled() {
        debug!("FanArt.tv lookups are disabled");
        return Vec::new();
    }

    // Get the configured API key
    let api_key = match get_api_key() {
        Some(key) => key,
        None => {
            warn!("No FanArt.tv API key configured");
            return Vec::new();
        }
    };

    // Check negative cache for failed lookups
    if FAILED_MBID_CACHE.get(artist_mbid).is_some() {
        debug!("MBID '{}' found in negative cache (previous FanArt.tv lookup failed)", artist_mbid);
        return Vec::new();
    }

    let max = max_images.unwrap_or(10);
    let url = format!(
        "http://webservice.fanart.tv/v3/music/{}?api_key={}", 
        artist_mbid,
        api_key
    );

    let mut thumbnail_urls = Vec::new();
    
    match fanarttv_api_get(&url) {
        Ok(response_text) => {
            // Parse the JSON response
            match serde_json::from_str::<Value>(&response_text) {
                Ok(data) => {
                    // Look for artist thumbnails
                    if let Some(artist_thumbs) = data.get("artistthumb").and_then(|t| t.as_array()) {
                        for thumb in artist_thumbs {
                            if let Some(url) = thumb.get("url").and_then(|u| u.as_str()) {
                                thumbnail_urls.push(url.to_string());
                                if thumbnail_urls.len() >= max {
                                    break;
                                }
                            }
                        }
                        
                        if !thumbnail_urls.is_empty() {
                            debug!("Found {} artist thumbnails on fanart.tv (limited to max {})", thumbnail_urls.len(), max);
                        } else {
                            debug!("Found no artist thumbnails on fanart.tv for MBID {}", artist_mbid);
                            // Add to negative cache if no thumbnails found
                            FAILED_MBID_CACHE.insert(artist_mbid.to_string(), true);
                        }
                    } else {
                        debug!("No artistthumb data found on fanart.tv for MBID {}", artist_mbid);
                        // Add to negative cache if no artistthumb section found  
                        FAILED_MBID_CACHE.insert(artist_mbid.to_string(), true);
                    }
                }
                Err(e) => {
                    warn!("Failed to parse JSON from fanart.tv for MBID {}: {}", artist_mbid, e);
                    // Add to negative cache on parse error
                    FAILED_MBID_CACHE.insert(artist_mbid.to_string(), true);
                }
            }
        }
        Err(e) => {
            debug!("GET request failed: {}: status code 404", e);
            // Add to negative cache on request failure (includes 404)
            FAILED_MBID_CACHE.insert(artist_mbid.to_string(), true);
        }
    }

    thumbnail_urls
}

/// Get artist banner URLs from FanArt.tv
/// 
/// # Arguments
/// * `artist_mbid` - MusicBrainz ID of the artist
/// 
/// # Returns
/// * `Vec<String>` - URLs of all available banners, empty if none found
pub fn get_artist_banners(artist_mbid: &str) -> Vec<String> {
    // Check if FanArt.tv is enabled
    if !is_enabled() {
        debug!("FanArt.tv lookups are disabled");
        return Vec::new();
    }

    // Get the configured API key
    let api_key = match get_api_key() {
        Some(key) => key,
        None => {
            warn!("No FanArt.tv API key configured");
            return Vec::new();
        }
    };

    // Check negative cache for failed lookups
    if FAILED_MBID_CACHE.get(artist_mbid).is_some() {
        debug!("MBID '{}' found in negative cache (previous FanArt.tv lookup failed)", artist_mbid);
        return Vec::new();
    }

    let url = format!(
        "http://webservice.fanart.tv/v3/music/{}?api_key={}", 
        artist_mbid,
        api_key
    );

    let mut banner_urls = Vec::new();
    
    match fanarttv_api_get(&url) {
        Ok(response_text) => {
            // Parse the JSON response
            match serde_json::from_str::<Value>(&response_text) {
                Ok(data) => {
                    // Look for artist banners
                    if let Some(artist_banners) = data.get("musicbanner").and_then(|b| b.as_array()) {
                        for banner in artist_banners {
                            if let Some(url) = banner.get("url").and_then(|u| u.as_str()) {
                                banner_urls.push(url.to_string());
                            }
                        }
                        
                        if !banner_urls.is_empty() {
                            debug!("Found {} artist banners on fanart.tv", banner_urls.len());
                        } else {
                            debug!("Found no artist banners on fanart.tv for MBID {}", artist_mbid);
                            // Add to negative cache if no banners found
                            FAILED_MBID_CACHE.insert(artist_mbid.to_string(), true);
                        }
                    } else {
                        debug!("No musicbanner data found on fanart.tv for MBID {}", artist_mbid);
                        // Add to negative cache if no musicbanner section found
                        FAILED_MBID_CACHE.insert(artist_mbid.to_string(), true);
                    }
                }
                Err(e) => {
                    warn!("Failed to parse JSON from fanart.tv for MBID {}: {}", artist_mbid, e);
                    // Add to negative cache on parse error
                    FAILED_MBID_CACHE.insert(artist_mbid.to_string(), true);
                }
            }
        }
        Err(e) => {
            debug!("GET request failed: {}: status code 404", e);
            // Add to negative cache on request failure (includes 404)
            FAILED_MBID_CACHE.insert(artist_mbid.to_string(), true);
        }
    }

    banner_urls
}









/// A dedicated CoverArt provider for FanArt.tv that includes MusicBrainz integration
pub struct FanarttvCoverartProvider;

impl Default for FanarttvCoverartProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl FanarttvCoverartProvider {
    pub fn new() -> Self {
        FanarttvCoverartProvider
    }
    
    /// Helper function to get artist MusicBrainz ID by name
    /// This integrates with the MusicBrainz lookup service
    fn get_artist_mbid(&self, artist_name: &str) -> Option<String> {
        debug!("FanArt.tv: Looking up MusicBrainz ID for artist '{}'", artist_name);
        
        // Use the MusicBrainz integration to find the MBID
        match crate::musicbrainz::search_mbids_for_artist(artist_name, false, false, true) {
            crate::musicbrainz::MusicBrainzSearchResult::Found(mbids, cached) => {
                if let Some(mbid) = mbids.first() {
                    debug!("FanArt.tv: Found MusicBrainz ID '{}' for artist '{}' (cached: {})", 
                           mbid, artist_name, cached);
                    Some(mbid.clone())
                } else {
                    debug!("FanArt.tv: Empty MBID list returned for artist '{}'", artist_name);
                    None
                }
            },
            crate::musicbrainz::MusicBrainzSearchResult::FoundPartial(mbids, cached) => {
                if let Some(mbid) = mbids.first() {
                    debug!("FanArt.tv: Found partial MusicBrainz ID '{}' for artist '{}' (cached: {})", 
                           mbid, artist_name, cached);
                    Some(mbid.clone())
                } else {
                    debug!("FanArt.tv: Empty partial MBID list returned for artist '{}'", artist_name);
                    None
                }
            },
            crate::musicbrainz::MusicBrainzSearchResult::NotFound => {
                debug!("FanArt.tv: No MusicBrainz ID found for artist '{}'", artist_name);
                None
            },
            crate::musicbrainz::MusicBrainzSearchResult::Error(err) => {
                warn!("FanArt.tv: Error looking up MusicBrainz ID for artist '{}': {}", artist_name, err);
                None
            }
        }
    }
}

impl CoverartProvider for FanarttvCoverartProvider {
    /// Returns the internal name identifier for this provider
    fn name(&self) -> &str {
        "fanarttv"
    }
    
    /// Returns the human-readable display name for this provider
    fn display_name(&self) -> &str {
        "FanArt.tv"
    }
    
    /// Returns the set of cover art methods this provider supports
    fn supported_methods(&self) -> HashSet<CoverartMethod> {
        let mut methods = HashSet::new();
        methods.insert(CoverartMethod::Artist);
        methods
    }
    
    /// Implementation for artist cover art retrieval
    /// Returns thumbnail URLs for the given artist by looking up their MusicBrainz ID
    fn get_artist_coverart_impl(&self, artist: &str) -> Vec<String> {
        debug!("FanArt.tv CoverArt: Getting cover art for artist '{}'", artist);
        
        // First, attempt to get the MusicBrainz ID for the artist
        if let Some(mbid) = self.get_artist_mbid(artist) {
            debug!("FanArt.tv CoverArt: Found MBID '{}' for artist '{}'", mbid, artist);
            
            // Get artist thumbnails using the MBID
            let thumbnails = get_artist_thumbnails(&mbid, Some(5));
            if !thumbnails.is_empty() {
                debug!("FanArt.tv CoverArt: Found {} thumbnails for artist '{}'", thumbnails.len(), artist);
                return thumbnails;
            } else {
                debug!("FanArt.tv CoverArt: No thumbnails found for artist '{}'", artist);
            }
        } else {
            debug!("FanArt.tv CoverArt: No MusicBrainz ID found for artist '{}'", artist);
        }
        
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coverart::CoverartProvider;
    use serde_json::Value as JsonValue;
    use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};
    use std::sync::Arc;
    use std::thread;
    use std::time::Duration;

    /// A transport that answers instantly but records how many requests were
    /// in flight at the same time, so the test observes the request path
    /// rather than the rate limiter it is built on.
    #[derive(Debug, Default)]
    struct ConcurrencyWitnessClient {
        current: AtomicUsize,
        peak: AtomicUsize,
    }

    impl ConcurrencyWitnessClient {
        fn peak(&self) -> usize {
            self.peak.load(AtomicOrdering::SeqCst)
        }
    }

    impl http_client::HttpClient for ConcurrencyWitnessClient {
        fn get_text(&self, _url: &str) -> Result<String, http_client::HttpClientError> {
            let now = self.current.fetch_add(1, AtomicOrdering::SeqCst) + 1;
            self.peak.fetch_max(now, AtomicOrdering::SeqCst);
            // Stand in for a slow fanart.tv response.
            thread::sleep(Duration::from_millis(100));
            self.current.fetch_sub(1, AtomicOrdering::SeqCst);
            Ok("{}".to_string())
        }

        fn post_json_value(&self, _url: &str, _payload: JsonValue) -> Result<JsonValue, http_client::HttpClientError> {
            unimplemented!("not used by fanart.tv")
        }
        fn post_json_status(&self, _url: &str, _payload: JsonValue) -> Result<(u16, JsonValue), http_client::HttpClientError> {
            unimplemented!("not used by fanart.tv")
        }
        fn get_binary(&self, _url: &str) -> Result<(Vec<u8>, String), http_client::HttpClientError> {
            unimplemented!("not used by fanart.tv")
        }
        fn get_binary_with_headers(&self, _url: &str, _headers: &[(&str, &str)], _max_bytes: u64) -> Result<(Vec<u8>, String), http_client::HttpClientError> {
            unimplemented!("not used by fanart.tv")
        }
        fn get_json_with_headers(&self, _url: &str, _headers: &[(&str, &str)]) -> Result<JsonValue, http_client::HttpClientError> {
            unimplemented!("not used by fanart.tv")
        }
        fn post_json_value_with_headers(&self, _url: &str, _payload: JsonValue, _headers: &[(&str, &str)]) -> Result<JsonValue, http_client::HttpClientError> {
            unimplemented!("not used by fanart.tv")
        }
        fn put_json_value_with_headers(&self, _url: &str, _payload: JsonValue, _headers: &[(&str, &str)]) -> Result<JsonValue, http_client::HttpClientError> {
            unimplemented!("not used by fanart.tv")
        }
        fn clone_box(&self) -> Box<dyn http_client::HttpClient> {
            unimplemented!("not used by fanart.tv")
        }
    }

    /// fanart.tv registered a rate limit and logged that it had one, but no
    /// call site ever applied it, so its requests went out unthrottled and
    /// unbounded while the other providers were being serialised.
    #[test]
    fn fanarttv_requests_go_out_one_at_a_time() {
        // This registers the production service name in the global rate
        // limiter, which outlives the test. Spacing is set to 1 ms so it does
        // not slow anything else down; the assertion below is about the
        // concurrency bound, which holds whatever the spacing is.
        ratelimit::register_service("fanarttv", 1);

        let client = Arc::new(ConcurrencyWitnessClient::default());
        let handles: Vec<_> = (0..6)
            .map(|_| {
                let client = Arc::clone(&client);
                thread::spawn(move || {
                    let _ = fanarttv_api_get_with(client.as_ref(), "http://example.invalid/");
                })
            })
            .collect();
        for handle in handles {
            handle.join().expect("worker thread panicked");
        }

        assert_eq!(
            client.peak(),
            1,
            "expected one fanart.tv request in flight at a time, saw {}",
            client.peak()
        );
    }

    
    #[test]
    fn test_fanarttv_coverart_provider_name() {
        let provider = FanarttvCoverartProvider::new();
        assert_eq!(provider.name(), "fanarttv");
    }
    
    #[test]
    fn test_fanarttv_coverart_provider_display_name() {
        let provider = FanarttvCoverartProvider::new();
        assert_eq!(provider.display_name(), "FanArt.tv");
    }
    
    #[test]
    fn test_fanarttv_coverart_provider_supported_methods() {
        let provider = FanarttvCoverartProvider::new();
        let methods = provider.supported_methods();
        assert_eq!(methods.len(), 1);
        assert!(methods.contains(&CoverartMethod::Artist));
        assert!(!methods.contains(&CoverartMethod::Song));
        assert!(!methods.contains(&CoverartMethod::Album));
        assert!(!methods.contains(&CoverartMethod::Url));
    }
    
    #[test]
    fn test_fanarttv_coverart_provider_get_artist_coverart_impl() {
        let provider = FanarttvCoverartProvider::new();
        let result = provider.get_artist_coverart_impl("Test Artist");
        // Should return empty since get_artist_mbid returns None (placeholder implementation)
        assert!(result.is_empty());
    }
    
    #[test]
    fn test_fanarttv_coverart_provider_get_artist_mbid() {
        let provider = FanarttvCoverartProvider::new();
        let result = provider.get_artist_mbid("Test Artist");
        // Should return None since it's a placeholder implementation
        assert!(result.is_none());
    }
    
    #[test]
    fn test_coverart_manager_integration() {
        use crate::coverart::{fan_out, CoverartManager, CoverartQuery, QueryOptions};
        use std::sync::Arc;

        let mut manager = CoverartManager::new();

        // Register FanArt.tv coverart provider
        let fanarttv_coverart = Arc::new(FanarttvCoverartProvider::new());

        manager.register_provider(fanarttv_coverart);

        // Test artist coverart retrieval (should return empty since no MusicBrainz lookup)
        let results = fan_out(
            manager.get_providers().clone(),
            &CoverartQuery::Artist("Test Artist".to_string()),
            &QueryOptions::default(),
        );

        // Provider should be called but return no results
        assert_eq!(results.len(), 0);
    }
}
