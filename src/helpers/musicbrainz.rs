use crate::helpers::attributecache;
use crate::helpers::ratelimit;
use crate::helpers::sanitize;
use crate::helpers::artistsplitter;
use crate::config::get_service_config;
use log::{info, error, debug, warn};
use std::sync::atomic::{AtomicBool, Ordering};
use deunicode::deunicode;
use serde::Deserialize;
use urlencoding::encode;

/// Global flag to indicate if MusicBrainz lookups are enabled
pub static MUSICBRAINZ_ENABLED: AtomicBool = AtomicBool::new(false);

// Cache key prefixes
pub const ARTIST_MBID_CACHE_PREFIX: &str = "artist::mbid::";
pub const ARTIST_MBID_PARTIAL_CACHE_PREFIX: &str = "artist::mbid_partial::";
pub const ARTIST_NOT_FOUND_CACHE_PREFIX: &str = "artist::mbid_not_found::";

// Cache timeout for not found entries (48 hours in seconds)
const NOT_FOUND_CACHE_TIMEOUT_SECONDS: i64 = 48 * 60 * 60;

// MusicBrainz API Constants
const MUSICBRAINZ_API_BASE: &str = "https://musicbrainz.org/ws/2";
const MUSICBRAINZ_USER_AGENT: &str = "HifiBerry-ACR/1.0 (https://www.hifiberry.com/)";
const MUSICBRAINZ_SEARCH_LIMIT: u32 = 3; // Limit search results to save bandwidth

/// Structs for deserializing MusicBrainz API responses
#[derive(Debug, Deserialize)]
struct MusicBrainzArtistSearchResponse {
    #[serde(rename = "count")]
    #[allow(dead_code)]
    count: u32,
    #[serde(rename = "offset")]
    #[allow(dead_code)]
    offset: u32,
    #[serde(rename = "artists")]
    artists: Vec<MusicBrainzArtist>,
}

#[derive(Debug, Deserialize)]
struct MusicBrainzArtist {
    id: String,
    name: String,
    #[serde(default)]
    aliases: Vec<MusicBrainzAlias>,
    #[serde(rename = "type")]
    #[allow(dead_code)]
    artist_type: Option<String>,
    #[allow(dead_code)]
    score: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct MusicBrainzAlias {
    name: String,
    #[serde(rename = "sort-name")]
    #[allow(dead_code)]
    sort_name: Option<String>,
    #[serde(rename = "type")]
    #[allow(dead_code)]
    alias_type: Option<String>,
    #[allow(dead_code)]
    locale: Option<String>,
}

/// Structs for recording (song) search responses
#[derive(Debug, Deserialize)]
pub struct MusicBrainzRecordingSearchResponse {
    #[serde(rename = "count")]
    pub count: u32,
    #[serde(rename = "offset")]
    #[allow(dead_code)]
    pub offset: u32,
    #[serde(rename = "recordings")]
    pub recordings: Vec<MusicBrainzRecording>,
}

#[derive(Debug, Deserialize)]
pub struct MusicBrainzRecording {
    #[allow(dead_code)]
    id: String,
    #[allow(dead_code)]
    title: String,
    #[serde(rename = "artist-credit")]
    #[allow(dead_code)]
    artist_credit: Option<Vec<MusicBrainzArtistCredit>>,
    #[allow(dead_code)]
    score: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct MusicBrainzArtistCredit {
    #[allow(dead_code)]
    name: String,
    #[allow(dead_code)]
    artist: MusicBrainzArtistRef,
}

#[derive(Debug, Deserialize)]
struct MusicBrainzArtistRef {
    #[allow(dead_code)]
    id: String,
    #[allow(dead_code)]
    name: String,
}

/// Initialize the MusicBrainz module from configuration
pub fn initialize_from_config(config: &serde_json::Value) {    
    if let Some(mb_config) = get_service_config(config, "musicbrainz") {
        if let Some(enabled) = mb_config.get("enable").and_then(|v| v.as_bool()) {
            MUSICBRAINZ_ENABLED.store(enabled, Ordering::SeqCst);
            info!("MusicBrainz lookup {}", if enabled { "enabled" } else { "disabled" });
        }
        
        // Register rate limit - default to 1000ms (2 requests per second)
        let rate_limit_ms = mb_config.get("rate_limit_ms")
            .and_then(|v| v.as_u64())
            .unwrap_or(500);
            
        ratelimit::register_service("musicbrainz", rate_limit_ms);
        info!("MusicBrainz rate limit set to {} ms", rate_limit_ms);
    } else {
        // Default to disabled if not in config
        MUSICBRAINZ_ENABLED.store(false, Ordering::SeqCst);
        debug!("MusicBrainz configuration not found, lookups disabled");
        
        // Register default rate limit even if disabled
        ratelimit::register_service("musicbrainz", 500);
    }
}

/// Check if MusicBrainz lookups are enabled
pub fn is_enabled() -> bool {
    MUSICBRAINZ_ENABLED.load(Ordering::SeqCst)
}

/// Result type for MusicBrainz artist search
#[derive(Debug, Clone, PartialEq)]
pub enum MusicBrainzSearchResult {
    /// Artist(s) found with their MusicBrainz ID(s)
    /// First parameter is the list of MusicBrainz IDs
    /// Second parameter indicates whether result was cached (true) or from API (false)
    Found(Vec<String>, bool),
    /// Partial match - some artists in a multi-artist name were found, but not all
    /// First parameter is the list of found MusicBrainz IDs
    /// Second parameter indicates whether result was cached (true) or from API (false)
    FoundPartial(Vec<String>, bool),
    /// Artist couldn't be found in MusicBrainz
    NotFound,
    /// Error occurred during the search
    Error(String),
}

/// Normalize an artist name for comparison by removing all special characters
/// and common words like "the", "and", etc.
/// 
/// This function:
/// - Converts to ASCII (removing accents, etc.)
/// - Removes ALL special characters (keeping only letters, numbers, and spaces)
/// - Converts to lowercase
/// - Removes common words like "the", "and" (only complete words, not substrings)
/// - Removes ALL spaces in the final result
/// - Trims whitespace and collapses multiple spaces to single space
/// 
/// # Arguments
/// * `artist_name` - The artist name to normalize
/// 
/// # Returns
/// A normalized string suitable for comparison
fn normalize_artist_name_for_comparison(artist_name: &str) -> String {
    // Step 1: Convert to ASCII
    let ascii_name = deunicode(artist_name);
    
    // Step 2: Remove all special characters and convert to lowercase
    let mut normalized = String::new();
    for c in ascii_name.chars() {
        if c.is_alphanumeric() || c.is_whitespace() {
            normalized.push(c.to_ascii_lowercase());
        }
    }
    
    // Step 3: Collapse multiple spaces to single space and trim
    let mut result = String::new();
    let mut last_was_space = true; // Start with true to trim leading spaces
    
    for c in normalized.chars() {
        if c.is_whitespace() {
            if !last_was_space {
                result.push(' ');
                last_was_space = true;
            }
        } else {
            result.push(c);
            last_was_space = false;
        }
    }
    
    // Remove trailing space if it exists
    if result.ends_with(' ') {
        result.pop();
    }
    
    // Step 4: Remove common words (as complete words only, not substrings)
    let common_words = ["the", "and"];
    
    // Split into words, filter out common words, and rejoin
    let filtered_words: Vec<&str> = result
        .split(' ')
        .filter(|word| !common_words.contains(word))
        .collect();
    
    // If all words were filtered out, return the original normalized result
    if filtered_words.is_empty() {
        return result;
    }
    
    // Join the filtered words back together
    let result = filtered_words.join(" ");
    
    // Step 5: Remove ALL spaces in the final result
    result.replace(" ", "")
}

/// Sanitize an artist name for MusicBrainz API queries by replacing problematic characters
/// 
/// # Arguments
/// * `artist_name` - The artist name to sanitize
/// 
/// # Returns
/// * `String` - Sanitized artist name
fn sanitize_artist_name_for_search(artist_name: &str) -> String {
    // Replace ampersands with "and" as they can cause search issues
    let sanitized = artist_name.replace("&", "and");
    
    debug!("Sanitized artist name '{}' to '{}' for MusicBrainz search", artist_name, sanitized);
    
    sanitized
}

/// Split an artist name using custom separators
/// 
/// This function has been moved to artistsplitter module for better organization.
/// Use `crate::helpers::artistsplitter::split_artist_with_separators` instead.
/// 
/// # Arguments
/// * `artist_name` - The artist name to split
/// * `separators` - The separators to use for splitting
/// Split an artist name that might contain multiple artists
/// 
/// This function has been moved to artistsplitter module for better organization.
/// Use `crate::helpers::artistsplitter::split_artist` instead.
/// 
/// # Arguments
/// * `artist_name` - The artist name to split
/// 
/// # Returns
/// * `Vec<String>` - Vector containing individual artist names
pub fn split_artist(artist_name: &str) -> Vec<String> {
    artistsplitter::split_artist(artist_name)
}

/// Compare two artist names to see if they match, using both exact normalized comparison
/// and fuzzy matching when needed
/// 
/// # Arguments
/// * `query_name` - The artist name we're searching for
/// * `response_name` - The artist name returned from MusicBrainz
/// * `response_aliases` - Optional vector of artist aliases/alternative names
/// 
/// # Returns
/// * `bool` - True if names are considered a match, false otherwise
fn artist_names_match(query_name: &str, response_name: &str, response_aliases: Option<&Vec<String>>) -> bool {
    // Use normalized comparison that removes all special characters
    let normalized_query = normalize_artist_name_for_comparison(query_name);
    let normalized_response = normalize_artist_name_for_comparison(response_name);
    
    debug!("Comparing normalized names: '{}' vs '{}'", normalized_query, normalized_response);
    
    // Check for exact match first
    if normalized_query == normalized_response {
        debug!("Found exactly matching artist: '{}' vs '{}'", query_name, response_name);
        return true;
    }
    
    // For cases where the names don't exactly match, implement a fuzzy comparison
    // Check if the names are similar enough to be considered a match
    let similarity_threshold = 0.9; // Adjust this threshold as needed
    let similarity = strsim::jaro_winkler(normalized_query.as_str(), normalized_response.as_str());
    
    if similarity >= similarity_threshold {
        debug!("Found similar artist: '{}' vs '{}' (similarity: {})", 
              query_name, response_name, similarity);
        return true;
    }
    
    // Check aliases if provided and main name didn't match
    if let Some(aliases) = response_aliases {
        debug!("Checking {} aliases for artist '{}'", aliases.len(), response_name);
        
        for alias in aliases {
            let normalized_alias = normalize_artist_name_for_comparison(alias);
            
            // Try exact match with alias
            if normalized_query == normalized_alias {
                debug!("Found exactly matching alias: '{}' vs '{}'", query_name, alias);
                return true;
            }
            
            // Try fuzzy match with alias
            let alias_similarity = strsim::jaro_winkler(normalized_query.as_str(), normalized_alias.as_str());
            if alias_similarity >= similarity_threshold {
                debug!("Found similar alias: '{}' vs '{}' (similarity: {})",
                      query_name, alias, alias_similarity);
                return true;
            }
        }
        
        debug!("No matching aliases found for '{}'", query_name);
    }
    
    // Names don't match and aren't similar enough
    debug!("Artist name mismatch! Searched for: '{}', but found: '{}'", 
          query_name, response_name);
    debug!("Normalized names: '{}' vs '{}'", normalized_query, normalized_response);
    debug!("Rejecting due to name mismatch");
    
    false
}

/// Make a GET request to the MusicBrainz API with proper headers and rate limiting
/// 
/// # Arguments
/// * `url` - The URL to request
/// 
/// # Returns
/// * `Result<String, String>` - API response or error message
fn musicbrainz_api_get(url: &str) -> Result<String, String> {
    debug!("Making MusicBrainz API request: {}", url);
    
    // Add proper User-Agent header and timeout using ureq's raw API
    // Use a longer timeout (10s) for MusicBrainz API as it can be slow
    let response = match ureq::get(url)
        .timeout(std::time::Duration::from_secs(10))
        .set("User-Agent", MUSICBRAINZ_USER_AGENT)
        .set("Accept", "application/json")
        .call() {
        Ok(resp) => resp,
        Err(e) => {
            error!("MusicBrainz API request failed: {}", e);
            return Err(format!("Request error: {}", e));
        }
    };
    
    // Log response status and content-length if available
    debug!("MusicBrainz API response status: {}", response.status());
    if let Some(content_length) = response.header("Content-Length") {
        debug!("MusicBrainz API response content length: {}", content_length);
    }
    
    // Get response body
    match response.into_string() {
        Ok(body) => {
            if body.is_empty() {
                error!("Empty response from MusicBrainz API");
                Err("Empty response from MusicBrainz API".to_string())
            } else {
                debug!("Successfully received MusicBrainz API response ({} bytes)", body.len());
                Ok(body)
            }
        },
        Err(e) => {
            error!("Failed to read MusicBrainz API response: {}", e);
            Err(format!("Response error: {}", e))
        }
    }
}

/// Search MusicBrainz API for an artist and return their MBID if found
/// 
/// # Arguments
/// * `artist_name` - The name of the artist to search for
/// * `cache_only` - If true, only check the cache and don't make API calls
/// 
/// # Returns
/// * `MusicBrainzSearchResult` - Found with vector of MBIDs, or error/not found status
fn search_musicbrainz_for_artist(artist_name: &str, cache_only: bool) -> MusicBrainzSearchResult {
    debug!("Searching MusicBrainz for artist: '{}' (cache_only: {})", artist_name, cache_only);
    
    // Check if MusicBrainz lookups are enabled
    if !is_enabled() {
        debug!("MusicBrainz lookups are disabled, skipping search for '{}'", artist_name);
        return MusicBrainzSearchResult::NotFound;
    }
    
    // Try to get MBID from cache first
    let cache_key = format!("{}{}", ARTIST_MBID_CACHE_PREFIX, artist_name);
    match attributecache::get::<String>(&cache_key) {
        Ok(Some(mbid)) => {
            debug!("Found MusicBrainz ID for '{}' in cache: {}", artist_name, mbid);
            return MusicBrainzSearchResult::Found(vec![mbid], true);
        },
        _ => {
            // If cache_only is true and we didn't find it in cache, return NotFound
            if cache_only {
                debug!("Artist '{}' not found in cache and cache_only=true", artist_name);
                return MusicBrainzSearchResult::NotFound;
            }
            // Otherwise continue with API search if not found in cache
        }
    }
    
    // Check negative cache for failed lookups using attributecache
    let not_found_cache_key = format!("{}{}", ARTIST_NOT_FOUND_CACHE_PREFIX, artist_name);
    match attributecache::get::<bool>(&not_found_cache_key) {
        Ok(Some(true)) => {
            debug!("Artist '{}' found in negative cache (previous lookup failed)", artist_name);
            return MusicBrainzSearchResult::NotFound;
        },
        _ => {
            // Continue with search if not in negative cache
        }
    }
    
    // If cache_only is true, we shouldn't reach this point (should have returned earlier)
    if cache_only {
        debug!("Artist '{}' not found in cache and cache_only=true", artist_name);
        return MusicBrainzSearchResult::NotFound;
    }
      // Apply rate limiting before making the API request
    ratelimit::rate_limit("musicbrainz");
    
    // Sanitize artist name for the API query
    let sanitized_artist_name = sanitize_artist_name_for_search(artist_name);
    debug!("Searching MusicBrainz for artist: '{}' (sanitized from '{}')", sanitized_artist_name, artist_name);
    
    // Construct the API URL
    let encoded_name = encode(&sanitized_artist_name);
    let url = format!(
        "{}/artist?query=artist:{}&fmt=json&limit={}",
        MUSICBRAINZ_API_BASE, 
        encoded_name,
        MUSICBRAINZ_SEARCH_LIMIT
    );
    debug!("MusicBrainz API request URL: {}", url);
    // Execute the HTTP GET request with proper headers
    let response = match musicbrainz_api_get(&url) {
        Ok(response_text) => response_text,
        Err(e) => {
            error!("Failed to execute MusicBrainz API request: {}", e);
            // Add to negative cache with 48-hour expiry before returning
            let not_found_cache_key = format!("{}{}", ARTIST_NOT_FOUND_CACHE_PREFIX, artist_name);
            if let Err(cache_err) = attributecache::set_with_expiry(&not_found_cache_key, &true, Some(NOT_FOUND_CACHE_TIMEOUT_SECONDS)) {
                debug!("Failed to cache API failure for '{}': {}", artist_name, cache_err);
            } else {
                debug!("Cached API failure for '{}' with 48-hour expiry", artist_name);
            }
            return MusicBrainzSearchResult::Error(format!("API request error: {}", e));
        }
    };
      // Parse the JSON response
    debug!("Received MusicBrainz API response: {} chars", response.len());
    // Print the first 200 chars of the response for debugging (UTF-8 safe)
    debug!("Response starts with: {}", sanitize::safe_truncate(&response, 200));
    
    let search_result: Result<MusicBrainzArtistSearchResponse, _> = serde_json::from_str(&response);
    match search_result {
        Ok(results) => {
            // Check if we have any results
            if !results.artists.is_empty() {
                // Get the first artist from results
                let artist = &results.artists[0];
                let mbid = artist.id.clone();
                let response_name = &artist.name;
                
                // Extract aliases if available
                let aliases: Vec<String> = artist.aliases
                    .iter()
                    .map(|alias| alias.name.clone())
                    .collect();
                
                // Use our dedicated function to compare artist names
                if artist_names_match(artist_name, response_name, if aliases.is_empty() { None } else { Some(&aliases) }) {
                    debug!("Found matching artist: '{}' with MBID: {}", response_name, mbid);
                    
                    // Store the MBID in the attribute cache
                    let cache_key = format!("{}{}", ARTIST_MBID_CACHE_PREFIX, artist_name);
                    debug!("Attempting to store MBID in cache with key: {}", cache_key);
                    
                    match attributecache::set(&cache_key, &mbid) {
                        Ok(_) => {
                            debug!("Successfully stored MusicBrainz ID for '{}' in cache", artist_name);
                        },
                        Err(e) => {
                            error!("Failed to cache MusicBrainz ID for '{}': {}", artist_name, e);
                        }
                    }
                    
                    // Return the MBID
                    return MusicBrainzSearchResult::Found(vec![mbid], false);
                } else {
                    // No matching artist found, add to negative cache with 48-hour expiry
                    debug!("Found artist but names don't match: '{}' vs '{}'", artist_name, response_name);
                    let not_found_cache_key = format!("{}{}", ARTIST_NOT_FOUND_CACHE_PREFIX, artist_name);
                    if let Err(cache_err) = attributecache::set_with_expiry(&not_found_cache_key, &true, Some(NOT_FOUND_CACHE_TIMEOUT_SECONDS)) {
                        debug!("Failed to cache name mismatch for '{}': {}", artist_name, cache_err);
                    } else {
                        debug!("Cached name mismatch for '{}' with 48-hour expiry", artist_name);
                    }
                }
            } else {
                // No results found, add to negative cache with 48-hour expiry
                debug!("No results found for artist '{}'", artist_name);
                let not_found_cache_key = format!("{}{}", ARTIST_NOT_FOUND_CACHE_PREFIX, artist_name);
                if let Err(cache_err) = attributecache::set_with_expiry(&not_found_cache_key, &true, Some(NOT_FOUND_CACHE_TIMEOUT_SECONDS)) {
                    debug!("Failed to cache no results for '{}': {}", artist_name, cache_err);
                } else {
                    debug!("Cached no results for '{}' with 48-hour expiry", artist_name);
                }
            }
            
            debug!("No matching MusicBrainz ID found for artist '{}'", artist_name);
            MusicBrainzSearchResult::NotFound
        },        Err(e) => {
            error!("Failed to parse MusicBrainz API response: {}", e);
            // Print error response safely (UTF-8 safe truncation)
            error!("Response text: {}", sanitize::safe_truncate(&response, 500));
            debug!("Full response JSON: {}", response);
            // Add to negative cache with 48-hour expiry before returning
            let not_found_cache_key = format!("{}{}", ARTIST_NOT_FOUND_CACHE_PREFIX, artist_name);
            if let Err(cache_err) = attributecache::set_with_expiry(&not_found_cache_key, &true, Some(NOT_FOUND_CACHE_TIMEOUT_SECONDS)) {
                debug!("Failed to cache parse error for '{}': {}", artist_name, cache_err);
            } else {
                debug!("Cached parse error for '{}' with 48-hour expiry", artist_name);
            }
            MusicBrainzSearchResult::Error(format!("Response parse error: {} ({})", e, 
                                                 if response.len() < 50 { "possibly truncated response" } else { "check response format" }))
        }
    }
}

/// Search for MusicBrainz IDs for an artist, handling multiple artists if needed
/// 
/// This function first tries to lookup the artist using search_musicbrainz_for_artist.
/// If that fails and allow_multiple is true, it checks if the artist name might contain 
/// multiple artists (separated by commas or &) and looks up each of them individually.
/// 
/// # Arguments
/// * `artist_name` - The name of the artist to search for
/// * `allow_multiple` - If true, handle potential multiple artists in the name
/// * `cache_only` - If true, only check the cache and don't make API calls
/// * `cache_failures` - If true, cache artists that are not found to avoid repeated lookups
/// 
/// # Returns
/// * `MusicBrainzSearchResult` - Found with vector of MBIDs, or error/not found status
pub fn search_mbids_for_artist(artist_name: &str, allow_multiple: bool, 
                               cache_only: bool, cache_failures: bool) -> MusicBrainzSearchResult {
    debug!("Searching MBIDs for artist: '{}' (allow_multiple: {}, cache_only: {}, cache_failures: {})", 
           artist_name, allow_multiple, cache_only, cache_failures);
    
    // Check if MusicBrainz lookups are enabled
    if !is_enabled() {
        debug!("MusicBrainz lookups are disabled, skipping search for '{}'", artist_name);
        return MusicBrainzSearchResult::NotFound;
    }
    
    // Try to get MBID from cache first for the full combined name
    let cache_key = format!("{}{}", ARTIST_MBID_CACHE_PREFIX, artist_name);
    let cache_partial_key = format!("{}{}", ARTIST_MBID_PARTIAL_CACHE_PREFIX, artist_name);
    
    // Check if we have already determined this artist doesn't exist
    if cache_failures {
        let not_found_cache_key = format!("{}{}", ARTIST_NOT_FOUND_CACHE_PREFIX, artist_name);
        match attributecache::get::<bool>(&not_found_cache_key) {
            Ok(Some(true)) => {
                debug!("Artist '{}' previously marked as not found in cache", artist_name);
                return MusicBrainzSearchResult::NotFound;
            },
            _ => {
                // Continue with search if not marked as not found or error reading cache
            }
        }
    }
    
    // Try to get MBIDs and partial status from cache first
    match attributecache::get::<Vec<String>>(&cache_key) {
        Ok(Some(mbids)) => {
            debug!("Found MusicBrainz IDs for '{}' in cache: {:?}", artist_name, mbids);
            
            // Check if this was a partial result
            match attributecache::get::<bool>(&cache_partial_key) {
                Ok(Some(true)) => {
                    debug!("Cached result for '{}' was marked as partial", artist_name);
                    return MusicBrainzSearchResult::FoundPartial(mbids, true);
                },
                _ => {
                    // Default to standard Found if not marked as partial or error reading cache
                    return MusicBrainzSearchResult::Found(mbids, true);
                }
            }
        },
        _ => {
            // Continue with search if not found in cache
            debug!("No cached MusicBrainz IDs found for '{}'", artist_name);
        }
    }
    
    // First try to lookup the artist as a single entity
    let result = search_musicbrainz_for_artist(artist_name, cache_only);
    
    match result {
        MusicBrainzSearchResult::Found(_, _) => {
            // If we found results, return them
            result
        },
        MusicBrainzSearchResult::NotFound => {
            // If no results and allow_multiple is true, try splitting
            if allow_multiple {
                let split_artists = split_artist(artist_name);
                
                // If we have multiple artists, try to look up each one
                if split_artists.len() > 1 {
                    debug!("No result for '{}' as a single artist, trying split artists: {:?}", 
                           artist_name, split_artists);
                    
                    let mut all_mbids = Vec::new();
                    let mut any_found = false;
                    let mut all_found = true;  // New flag to track if all split artists were found
                    
                    // Search for each artist individually
                    for artist in &split_artists {
                        match search_musicbrainz_for_artist(artist, cache_only) {
                            MusicBrainzSearchResult::Found(mbids, _) => {
                                debug!("Found MusicBrainz ID(s) for split artist '{}': {:?}", artist, mbids);
                                all_mbids.extend(mbids);
                                any_found = true;
                            },
                            _ => {
                                debug!("No MusicBrainz ID found for split artist: '{}'", artist);
                                all_found = false;  // Mark that at least one artist wasn't found
                            }
                        }
                    }
                    
                    // If we found any MBIDs, return them and store in cache
                    if any_found {
                        debug!("Found {} MusicBrainz ID(s) for split artists in '{}'", all_mbids.len(), artist_name);
                        
                        // Store the combined result in the cache with the full artist name
                        match attributecache::set(&cache_key, &all_mbids) {
                            Ok(_) => {
                                debug!("Successfully stored multiple MusicBrainz IDs for '{}' in cache", artist_name);
                                
                                // Only store partial status if we didn't find all the artists
                                if !all_found {
                                    debug!("Not all artists in '{}' were found, marking as partial result", artist_name);
                                    match attributecache::set(&cache_partial_key, &true) {
                                        Ok(_) => {
                                            debug!("Successfully marked '{}' as a partial match in cache", artist_name);
                                        },
                                        Err(e) => {
                                            error!("Failed to cache partial status for '{}': {}", artist_name, e);
                                        }
                                    }
                                    
                                    return MusicBrainzSearchResult::FoundPartial(all_mbids, false);
                                } else {
                                    debug!("All split artists in '{}' were found, returning as full match", artist_name);
                                    return MusicBrainzSearchResult::Found(all_mbids, false);
                                }
                            },
                            Err(e) => {
                                error!("Failed to cache multiple MusicBrainz IDs for '{}': {}", artist_name, e);
                                
                                // Even if caching failed, return the appropriate result type
                                if !all_found {
                                    return MusicBrainzSearchResult::FoundPartial(all_mbids, false);
                                } else {
                                    return MusicBrainzSearchResult::Found(all_mbids, false);
                                }
                            }
                        }
                    }
                    
                    // Otherwise, fall through to return the original NotFound result
                }
            }
            
            // If we reached here, the artist was not found. Cache this result if requested.
            if cache_failures {
                let not_found_cache_key = format!("{}{}", ARTIST_NOT_FOUND_CACHE_PREFIX, artist_name);
                match attributecache::set_with_expiry(&not_found_cache_key, &true, Some(NOT_FOUND_CACHE_TIMEOUT_SECONDS)) {
                    Ok(_) => {
                        debug!("Cached '{}' as not found with 48-hour expiry to prevent future lookups", artist_name);
                    },
                    Err(e) => error!("Failed to cache not_found status for '{}': {}", artist_name, e)
                }
            }
            
            // Return the original result
            result
        },
        _ => {
            // For errors, just return the original result
            result
        }
    }
}

/// Check if an artist name contains multiple artists by looking up MBIDs
/// and splitting the name if multiple MBIDs are found
///
/// This function has been moved to artistsplitter module for better organization.
/// Use `crate::helpers::artistsplitter::split_artist_names_with_mbid_lookup` instead.
///
/// # Arguments
/// * `artist_name` - The name of the artist to check
/// * `cache_only` - If true, only check the cache and don't make API calls (default: true)
/// * `custom_separators` - Optional list of custom separators to use instead of the default
///
/// # Returns
/// * `Option<Vec<String>>` - None if single artist, or Some(Vec<String>) with split artist names if multiple
pub fn split_artist_names(artist_name: &str, cache_only: bool, custom_separators: Option<&[String]>) -> Option<Vec<String>> {
    artistsplitter::split_artist_names_with_mbid_lookup(artist_name, cache_only, custom_separators)
}

/// Search for recordings (songs) by artist and title
/// 
/// Performs an exact match search for recordings in MusicBrainz.
/// 
/// # Arguments
/// * `artist` - The artist name to search for
/// * `title` - The recording title to search for
/// 
/// # Returns
/// A result containing the search response or an error
pub fn search_recording(artist: &str, title: &str) -> Result<MusicBrainzRecordingSearchResponse, String> {
    debug!("Searching MusicBrainz for recording: artist='{}', title='{}'", artist, title);
    
    // Check if MusicBrainz lookups are enabled
    if !is_enabled() {
        debug!("MusicBrainz lookups are disabled, skipping recording search");
        return Ok(MusicBrainzRecordingSearchResponse { 
            recordings: Vec::new(), 
            count: 0,
            offset: 0,
        });
    }
    
    // Apply rate limiting before making the API request
    ratelimit::rate_limit("musicbrainz");
    
    // Build query for exact match
    let query = format!("artist:\"{}\" AND recording:\"{}\"", artist, title);
    let url = format!("{}/recording/?query={}&fmt=json&limit=5", MUSICBRAINZ_API_BASE, urlencoding::encode(&query));
    
    // Execute the HTTP GET request
    let response_text = match musicbrainz_api_get(&url) {
        Ok(response_text) => response_text,
        Err(e) => {
            warn!("Failed to search MusicBrainz for recording: {}", e);
            return Ok(MusicBrainzRecordingSearchResponse { 
                recordings: Vec::new(), 
                count: 0,
                offset: 0,
            });
        }
    };
    
    // Parse the JSON response
    match serde_json::from_str::<MusicBrainzRecordingSearchResponse>(&response_text) {
        Ok(data) => {
            debug!("Found {} recordings for artist='{}', title='{}'", data.count, artist, title);
            Ok(data)
        },
        Err(e) => {
            warn!("Failed to parse MusicBrainz recording search response: {}", e);
            Ok(MusicBrainzRecordingSearchResponse { 
                recordings: Vec::new(), 
                count: 0,
                offset: 0,
            })
        }
    }
}

/// Check if a string appears to be a valid MusicBrainz ID (MBID)
/// 
/// MusicBrainz IDs are formatted as UUIDs: xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx
/// 
/// # Arguments
/// * `input` - The string to check
/// 
/// # Returns
/// true if the string looks like a valid MBID, false otherwise
pub fn is_mbid(input: &str) -> bool {
    // MusicBrainz IDs are in UUID format:
    // 8 chars, dash, 4 chars, dash, 4 chars, dash, 4 chars, dash, 12 chars
    input.len() == 36 
        && input.chars().all(|c| c.is_ascii_hexdigit() || c == '-')
        && input.matches('-').count() == 4
}

