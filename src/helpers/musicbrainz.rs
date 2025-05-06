use crate::helpers::attributecache;
use log::{info, warn, error, debug};
use reqwest::blocking::Client;
use serde_json::Value;
use std::time::Duration;
use std::thread;
use deunicode::deunicode;

/// Result type for MusicBrainz artist search
#[derive(Debug, Clone, PartialEq)]
pub enum MusicBrainzSearchResult {
    /// Artist(s) found with their MusicBrainz ID(s) from API search
    Found(Vec<String>),
    /// Artist(s) found with their MusicBrainz ID(s) from cache
    FoundCached(Vec<String>),
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
    let common_words = vec!["the", "and"];
    
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

/// Split an artist name that might contain multiple artists
/// 
/// # Arguments
/// * `artist_name` - The artist name to split
/// 
/// # Returns
/// * `Vec<String>` - Vector containing individual artist names
pub fn split_artist(artist_name: &str) -> Vec<String> {
    debug!("Splitting artist name: '{}'", artist_name);
    
    // Define separators that indicate multiple artists
    let separators = [',', '&'];
    
    // Check if the artist name contains any of the separators
    let mut contains_separator = false;
    for &sep in &separators {
        if artist_name.contains(sep) {
            contains_separator = true;
            break;
        }
    }
    
    // If no separators are found, return the artist name as is
    if !contains_separator {
        debug!("No separators found in artist name: '{}'", artist_name);
        return vec![artist_name.to_string()];
    }
    
    // Split the artist name by separators
    let mut artists = Vec::new();
    let mut current = String::new();
    
    for c in artist_name.chars() {
        if separators.contains(&c) {
            // Found a separator, add the current part if not empty
            let trimmed = current.trim();
            if !trimmed.is_empty() {
                artists.push(trimmed.to_string());
            }
            current.clear();
        } else {
            current.push(c);
        }
    }
    
    // Add the last part if not empty
    let trimmed = current.trim();
    if !trimmed.is_empty() {
        artists.push(trimmed.to_string());
    }
    
    // Filter out any empty strings and remove any "feat." prefixes
    let artists: Vec<String> = artists
        .into_iter()
        .map(|a| a.trim().to_string())
        .filter(|a| !a.is_empty() && !a.to_lowercase().starts_with("feat."))
        .collect();
    
    debug!("Split artist '{}' into: {:?}", artist_name, artists);
    artists
}

/// Search MusicBrainz API for an artist and return their MBID if found
/// 
/// # Arguments
/// * `artist_name` - The name of the artist to search for
/// * `search_multiple` - If true and artist name contains commas, split and search for each part
/// * `cache_only` - If true, only check the cache and don't make API calls
/// 
/// # Returns
/// * `MusicBrainzSearchResult` - Found with vector of MBIDs, or error/not found status
fn search_musicbrainz_for_artist(artist_name: &str, cache_only: bool) -> MusicBrainzSearchResult {
    debug!("Searching MusicBrainz for artist: '{}' (cache_only: {})", artist_name, cache_only);
    
    // Try to get MBID from cache first
    let cache_key = format!("artist::{}::mbid", artist_name);
    match attributecache::get::<String>(&cache_key) {
        Ok(Some(mbid)) => {
            debug!("Found MusicBrainz ID for '{}' in cache: {}", artist_name, mbid);
            return MusicBrainzSearchResult::FoundCached(vec![mbid]);
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
    
    // If cache_only is true, we shouldn't reach this point (should have returned earlier)
    if cache_only {
        debug!("Artist '{}' not found in cache and cache_only=true", artist_name);
        return MusicBrainzSearchResult::NotFound;
    }
    
    // Create a reqwest client with appropriate timeouts
    let client = match Client::builder()
        .timeout(Duration::from_secs(10))
        .user_agent("AudioControl3/1.0 (https://github.com/hifiberry/audiocontrol3)")
        .build() {
        Ok(client) => client,
        Err(e) => {
            error!("Failed to create HTTP client for MusicBrainz search: {}", e);
            return MusicBrainzSearchResult::Error(format!("HTTP client error: {}", e));
        }
    };
    
    // URL encode the artist name for the query
    let encoded_name = urlencoding::encode(artist_name).to_string();
    
    // Construct the MusicBrainz API query URL
    let url = format!("https://musicbrainz.org/ws/2/artist?query={}&fmt=json", encoded_name);
    
    // Add a 1-second delay between artists to limit API requests
    thread::sleep(Duration::from_secs(1));

    debug!("Sending request to MusicBrainz API: {}", url);
    let response = match client.get(&url).send() {
        Ok(response) => {
            if !response.status().is_success() {
                warn!("MusicBrainz API returned error status: {}", response.status());
                return MusicBrainzSearchResult::Error(format!("API error status: {}", response.status()));
            }
            response
        },
        Err(e) => {
            error!("Failed to execute MusicBrainz API request: {}", e);
            return MusicBrainzSearchResult::Error(format!("API request error: {}", e));
        }
    };
    
    // Parse the response JSON
    let json: Value = match response.json() {
        Ok(json) => json,
        Err(e) => {
            error!("Failed to parse MusicBrainz API response: {}", e);
            return MusicBrainzSearchResult::Error(format!("JSON parsing error: {}", e));
        }
    };
    
    // Extract the first artist's MBID and name if available
    if let Some(artists) = json.get("artists").and_then(|a| a.as_array()) {
        if !artists.is_empty() {
            let artist_obj = &artists[0];
            
            // Extract MBID and artist name from response
            let mbid = artist_obj.get("id").and_then(|id| id.as_str());
            let response_name = artist_obj.get("name").and_then(|name| name.as_str());
            
            // Process the response only if we have both MBID and name
            if let (Some(mbid), Some(response_name)) = (mbid, response_name) {
                let mbid_string = mbid.to_string();
                
                // Use our new normalized comparison that removes all special characters
                let normalized_query = normalize_artist_name_for_comparison(artist_name);
                let normalized_response = normalize_artist_name_for_comparison(response_name);
                
                debug!("Comparing normalized names: '{}' vs '{}'", normalized_query, normalized_response);
                
                // Check if the normalized names match
                if normalized_query == normalized_response {
                    debug!("Found exactly matching artist: '{}' with MBID: {}", response_name, mbid_string);
                    
                    // Store the MBID in the attribute cache
                    let cache_key = format!("artist::{}::mbid", artist_name);
                    debug!("Attempting to store MBID in cache with key: {}", cache_key);
                    
                    match attributecache::set(&cache_key, &mbid_string) {
                        Ok(_) => {
                            debug!("Successfully stored MusicBrainz ID for '{}' in cache", artist_name);
                            
                            // Verify the cache write by reading it back
                            match attributecache::get::<String>(&cache_key) {
                                Ok(Some(cached_mbid)) => {
                                    if cached_mbid == mbid_string {
                                        debug!("Verified MBID in cache matches: {}", cached_mbid);
                                    } else {
                                        warn!("Cache verification failed! Expected: {}, Got: {}", mbid_string, cached_mbid);
                                    }
                                },
                                Ok(None) => warn!("Failed to verify MBID in cache - not found after writing!"),
                                Err(e) => warn!("Failed to verify MBID in cache: {}", e)
                            }
                        },
                        Err(e) => {
                            error!("Failed to cache MusicBrainz ID for '{}': {}", artist_name, e);
                        }
                    }
                    
                    // Return the MBID
                    return MusicBrainzSearchResult::Found(vec![mbid_string]);
                } else {
                    // For cases where the names don't exactly match, implement a fuzzy comparison
                    // Check if the names are similar enough to be considered a match
                    let similarity_threshold = 0.9; // Adjust this threshold as needed
                    let similarity = strsim::jaro_winkler(normalized_query.as_str(), normalized_response.as_str());
                    if similarity >= similarity_threshold {
                        debug!("Found similar artist: '{}' with MBID: {}", response_name, mbid_string);
                        
                        // Store the MBID in the attribute cache
                        let cache_key = format!("artist::{}::mbid", artist_name);
                        debug!("Attempting to store MBID in cache with key: {}", cache_key);
                        
                        match attributecache::set(&cache_key, &mbid_string) {
                            Ok(_) => {
                                debug!("Successfully stored MusicBrainz ID for '{}' in cache", artist_name);
                            },
                            Err(e) => {
                                error!("Failed to cache MusicBrainz ID for '{}': {}", artist_name, e);
                            }
                        }
                        
                        // Return the MBID
                        return MusicBrainzSearchResult::Found(vec![mbid_string]);
                    
                    } else {
                        // Names don't match and aren't similar enough
                        debug!("Artist name mismatch! Searched for: '{}', but found: '{}'", 
                            artist_name, response_name);
                        debug!("Normalized names: '{}' vs '{}'", normalized_query, normalized_response);
                        debug!("Rejecting MBID due to name mismatch");
                        
                        // Fall through to continue searching or return None
                    }
                }
            }
        }
    }
    
    info!("No matching MusicBrainz ID found for artist '{}'", artist_name);
    MusicBrainzSearchResult::NotFound
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
/// 
/// # Returns
/// * `MusicBrainzSearchResult` - Found with vector of MBIDs, or error/not found status
pub fn search_mbids_for_artist(artist_name: &str, allow_multiple: bool, cache_only: bool) -> MusicBrainzSearchResult {
    debug!("Searching MBIDs for artist: '{}' (allow_multiple: {}, cache_only: {})", 
           artist_name, allow_multiple, cache_only);
    
    // First try to lookup the artist as a single entity
    let result = search_musicbrainz_for_artist(artist_name, cache_only);
    
    match result {
        MusicBrainzSearchResult::Found(ref _mbids) | MusicBrainzSearchResult::FoundCached(ref _mbids) => {
            // If we found results, return them
            return result;
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
                    
                    // Search for each artist individually
                    for artist in split_artists {
                        match search_musicbrainz_for_artist(&artist, cache_only) {
                            MusicBrainzSearchResult::Found(mbids) | MusicBrainzSearchResult::FoundCached(mbids) => {
                                debug!("Found MusicBrainz ID(s) for split artist '{}': {:?}", artist, mbids);
                                all_mbids.extend(mbids);
                                any_found = true;
                            },
                            _ => debug!("No MusicBrainz ID found for split artist: '{}'", artist)
                        }
                    }
                    
                    // If we found any MBIDs, return them
                    if any_found {
                        debug!("Found {} MusicBrainz ID(s) for split artists in '{}'", all_mbids.len(), artist_name);
                        return MusicBrainzSearchResult::Found(all_mbids);
                    }
                    
                    // Otherwise, fall through to return the original NotFound result
                }
            }
            
            // Return the original result if splitting didn't help or wasn't allowed
            return result;
        },
        _ => {
            // For errors, just return the original result
            return result;
        }
    }
}

