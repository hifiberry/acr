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
    /// Artist(s) found with their MusicBrainz ID(s)
    Found(Vec<String>),
    /// Artist was intentionally ignored (e.g., contains multiple artists)
    Ignored,
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

/// Search MusicBrainz API for an artist and return their MBID if found
/// 
/// # Arguments
/// * `artist_name` - The name of the artist to search for
/// * `search_multiple` - If true and artist name contains commas, split and search for each part
/// 
/// # Returns
/// * `MusicBrainzSearchResult` - Found with vector of MBIDs, or error/not found status
pub fn search_musicbrainz_for_artist(artist_name: &str, search_multiple: bool) -> MusicBrainzSearchResult {
    debug!("Searching MusicBrainz for artist: '{}'", artist_name);
    
    // If search_multiple is true and artist name contains commas, split and search for each part
    if search_multiple && artist_name.contains(',') {
        debug!("Artist name contains multiple artists, splitting and searching individually");
        let artist_names: Vec<&str> = artist_name.split(',')
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .collect();
        
        let mut all_mbids = Vec::new();
        
        for name in artist_names {
            debug!("Searching for individual artist: '{}'", name);
            match search_musicbrainz_for_artist(name, false) {
                MusicBrainzSearchResult::Found(mut mbids) => {
                    debug!("Found MBID(s) for '{}': {:?}", name, mbids);
                    all_mbids.append(&mut mbids);
                },
                MusicBrainzSearchResult::NotFound => {
                    debug!("No MBID found for '{}'", name);
                },
                MusicBrainzSearchResult::Error(e) => {
                    warn!("Error searching for '{}': {}", name, e);
                },
                MusicBrainzSearchResult::Ignored => {
                    debug!("Artist '{}' was ignored", name);
                }
            }
        }
        
        if !all_mbids.is_empty() {
            debug!("Found combined {} MBID(s) for split search of '{}'", all_mbids.len(), artist_name);
            return MusicBrainzSearchResult::Found(all_mbids);
        } else {
            debug!("No MBIDs found for any part of '{}'", artist_name);
            return MusicBrainzSearchResult::NotFound;
        }
    }
    
    // First check if this artist was previously flagged as having multiple artists
    let ignored_flag_key = format!("artist::{}::ignored_multiple_artists", artist_name);
    match attributecache::get::<bool>(&ignored_flag_key) {
        Ok(Some(true)) => {
            debug!("Skipping search for '{}' as it was previously flagged as containing multiple artists", artist_name);
            return MusicBrainzSearchResult::Ignored;
        },
        _ => {} // Continue with the search if not found or there was an error
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
                    // Check if one name is fully contained within the other
                    if normalized_query.contains(&normalized_response) || normalized_response.contains(&normalized_query) {
                        // ignore if the artist name contains "," or "feat." and we're not in a recursive search
                        if search_multiple && (artist_name.contains(",") || artist_name.contains("feat.")) {
                            debug!("Ignoring similar artist match due to multiple artists in name: '{}'", artist_name);
                            
                            // Store a flag in the attribute cache to avoid looking up this artist again
                            let ignored_flag_key = format!("artist::{}::ignored_multiple_artists", artist_name);
                            if let Err(e) = attributecache::set(&ignored_flag_key, &true) {
                                warn!("Failed to store ignored flag for artist with multiple names '{}': {}", artist_name, e);
                            } else {
                                debug!("Stored ignored flag for artist with multiple names: '{}'", artist_name);
                            }
                            
                            return MusicBrainzSearchResult::Ignored;
                        }

                        info!("Found similar artist: '{}' (searched for: '{}') with MBID: {}", 
                            response_name, artist_name, mbid_string);
                        
                        // Store the MBID in the cache but mark it as a partial match
                        let cache_key = format!("artist::{}::mbid", artist_name);
                        debug!("Storing MBID for similar artist match in cache with key: {}", cache_key);
                        
                        match attributecache::set(&cache_key, &mbid_string) {
                            Ok(_) => debug!("Stored MBID for similar artist: '{}' -> '{}'", artist_name, response_name),
                            Err(e) => error!("Failed to cache MBID for similar artist: {}", e)
                        }
                        
                        return MusicBrainzSearchResult::Found(vec![mbid_string]);
                    } else {
                        // Names don't match and aren't similar enough
                        warn!("Artist name mismatch! Searched for: '{}', but found: '{}'", 
                            artist_name, response_name);
                        warn!("Normalized names: '{}' vs '{}'", normalized_query, normalized_response);
                        warn!("Rejecting MBID due to name mismatch");
                        
                        // Fall through to continue searching or return None
                    }
                }
            }
        }
    }
    
    info!("No matching MusicBrainz ID found for artist '{}'", artist_name);
    MusicBrainzSearchResult::NotFound
}

/// Get MusicBrainz ID for an artist, first checking the cache
pub fn get_artist_mbid(artist_name: &str) -> Option<String> {
    // Try to get MBID from cache first
    let cache_key = format!("artist::{}::mbid", artist_name);
    
    match attributecache::get::<String>(&cache_key) {
        Ok(Some(mbid)) => {
            debug!("Found MusicBrainz ID for '{}' in cache: {}", artist_name, mbid);
            Some(mbid)
        },
        _ => {
            // Not in cache, search MusicBrainz
            match search_musicbrainz_for_artist(artist_name, true) {
                MusicBrainzSearchResult::Found(mbids) => {
                    if !mbids.is_empty() {
                        debug!("Found {} MBID(s) for '{}', using the first one", mbids.len(), artist_name);
                        Some(mbids[0].clone())
                    } else {
                        None
                    }
                },
                _ => None,
            }
        }
    }
}

/// Get all MusicBrainz IDs for an artist or artists, first checking the cache
pub fn get_artist_mbids(artist_name: &str) -> Vec<String> {
    // Try to get MBID from cache first
    let cache_key = format!("artist::{}::mbid", artist_name);
    
    match attributecache::get::<String>(&cache_key) {
        Ok(Some(mbid)) => {
            debug!("Found MusicBrainz ID for '{}' in cache: {}", artist_name, mbid);
            vec![mbid]
        },
        _ => {
            // Not in cache, search MusicBrainz
            match search_musicbrainz_for_artist(artist_name, true) {
                MusicBrainzSearchResult::Found(mbids) => mbids,
                _ => Vec::new(),
            }
        }
    }
}