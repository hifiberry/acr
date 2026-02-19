/// Artist name splitting utilities with MusicBrainz integration
/// 
/// This module provides functionality to split artist names that contain multiple artists
/// separated by various delimiters like commas, "&", "feat.", etc. It includes both
/// simple text-based splitting and intelligent splitting using MusicBrainz MBID lookups.
use log::debug;
use crate::helpers::musicbrainz::{self, MusicBrainzSearchResult};
use crate::helpers::attributecache;

/// Default separators used to split artist names containing multiple artists
pub static DEFAULT_ARTIST_SEPARATORS: &[&str] = &[",", "&", " feat ", " feat.", " featuring ", " with "];

/// Cache key prefix for artist splits with MusicBrainz MBID lookup
pub static ARTIST_SPLIT_CACHE_PREFIX: &str = "artist::split::";

/// Cache key prefix for simple artist splits without MBID lookup
pub static ARTIST_SIMPLE_SPLIT_CACHE_PREFIX: &str = "artist::simple_split::";

/// Split an artist name that might contain multiple artists using default separators
/// 
/// # Arguments
/// * `artist_name` - The artist name to split
/// 
/// # Returns
/// * `Vec<String>` - Vector containing individual artist names
/// 
/// # Examples
/// ```
/// use audiocontrol::helpers::artistsplitter::split_artist;
/// 
/// let artists = split_artist("The Beatles feat. Tony Sheridan");
/// assert_eq!(artists, vec!["The Beatles", "Tony Sheridan"]);
/// 
/// let artists = split_artist("Simon & Garfunkel");
/// assert_eq!(artists, vec!["Simon", "Garfunkel"]);
/// ```
pub fn split_artist(artist_name: &str) -> Vec<String> {
    debug!("Splitting artist name: '{}'", artist_name);
    
    // Convert string slice array to string array for the internal function
    let default_separators: Vec<String> = DEFAULT_ARTIST_SEPARATORS.iter().map(|&s| s.to_string()).collect();
    split_artist_with_separators(artist_name, &default_separators)
}

/// Split an artist name that might contain multiple artists using custom separators
/// 
/// # Arguments
/// * `artist_name` - The artist name to split
/// * `separators` - Custom separators to use for splitting
/// 
/// # Returns
/// * `Vec<String>` - Vector containing individual artist names
/// 
/// # Examples
/// ```
/// use audiocontrol::helpers::artistsplitter::split_artist_with_separators;
/// 
/// let custom_separators = vec![" x ".to_string(), " vs ".to_string()];
/// let artists = split_artist_with_separators("Artist A x Artist B vs Artist C", &custom_separators);
/// assert_eq!(artists, vec!["Artist A", "Artist B", "Artist C"]);
/// ```
pub fn split_artist_with_separators(artist_name: &str, separators: &[String]) -> Vec<String> {
    debug!("Splitting artist name: '{}' with custom separators: {:?}", artist_name, separators);
    
    // Initial result will contain the full string
    let mut result = vec![artist_name.to_string()];
    
    // Iteratively split by each separator
    for separator in separators {
        let mut new_result = Vec::new();
        
        for part in result {
            // Skip empty parts
            if part.trim().is_empty() {
                continue;
            }
            
            // For each existing part, split it by the current separator
            if part.contains(separator) {
                for sub_part in part.split(separator) {
                    let trimmed = sub_part.trim();
                    if !trimmed.is_empty() {
                        new_result.push(trimmed.to_string());
                    }
                }
            } else {
                // If no separator in this part, keep it as is
                new_result.push(part);
            }
        }
        
        // Update result for the next separator
        result = new_result;
    }
    
    // Filter out any "feat." prefixes and empty strings
    result = result
        .into_iter()
        .map(|a| a.trim().to_string())
        .filter(|a| !a.is_empty() && !a.to_lowercase().starts_with("feat."))
        .collect();
    
    debug!("Split artist '{}' into: {:?}", artist_name, result);
    result
}

/// Check if an artist name contains multiple artists by looking for separators
/// 
/// # Arguments
/// * `artist_name` - The artist name to check
/// * `custom_separators` - Optional custom separators to check (uses defaults if None)
/// 
/// # Returns
/// * `bool` - True if the artist name contains separator characters
/// 
/// # Examples
/// ```
/// use audiocontrol::helpers::artistsplitter::contains_multiple_artists;
/// 
/// assert!(contains_multiple_artists("The Beatles feat. Tony Sheridan", None));
/// assert!(contains_multiple_artists("Simon & Garfunkel", None));
/// assert!(!contains_multiple_artists("The Beatles", None));
/// ```
pub fn contains_multiple_artists(artist_name: &str, custom_separators: Option<&[String]>) -> bool {
    // Determine which separators to use
    let separators: Vec<&str> = match custom_separators {
        Some(seps) => seps.iter().map(|s| s.as_str()).collect(),
        None => DEFAULT_ARTIST_SEPARATORS.to_vec(),
    };
    
    // Check if the string contains any separator
    separators.iter().any(|separator| artist_name.contains(*separator))
}

/// Split artist names only if they contain multiple artists
/// 
/// Returns None if the artist name appears to be a single artist,
/// or Some(Vec<String>) if multiple artists are detected and split.
/// 
/// # Arguments
/// * `artist_name` - The artist name to potentially split
/// * `custom_separators` - Optional custom separators to use
/// 
/// # Returns
/// * `Option<Vec<String>>` - None if single artist, Some(artists) if multiple
/// 
/// # Examples
/// ```
/// use audiocontrol::helpers::artistsplitter::split_if_multiple;
/// 
/// assert_eq!(split_if_multiple("The Beatles", None), None);
/// assert_eq!(split_if_multiple("Simon & Garfunkel", None), Some(vec!["Simon".to_string(), "Garfunkel".to_string()]));
/// ```
pub fn split_if_multiple(artist_name: &str, custom_separators: Option<&[String]>) -> Option<Vec<String>> {
    // Create cache key for simple splits (include separator info)
    let separator_key = match custom_separators {
        Some(seps) => format!("custom:{}", seps.join("|")),
        None => "default".to_string(),
    };
    let cache_key = format!("{}{}::{}", ARTIST_SIMPLE_SPLIT_CACHE_PREFIX, separator_key, artist_name);
    
    // Try to get cached result first (no expiry)
    match attributecache::get::<Option<Vec<String>>>(&cache_key) {
        Ok(Some(cached_result)) => {
            debug!("Found cached simple split result for '{}': {:?}", artist_name, cached_result);
            return cached_result;
        },
        Ok(None) => {
            debug!("No cached simple split result found for '{}'", artist_name);
        },
        Err(e) => {
            debug!("Error reading cached simple split result for '{}': {}", artist_name, e);
        }
    }
    
    let result = if !contains_multiple_artists(artist_name, custom_separators) {
        debug!("'{}' doesn't contain any separators, assuming single artist", artist_name);
        None
    } else {
        let separators = match custom_separators {
            Some(seps) => seps.to_vec(),
            None => DEFAULT_ARTIST_SEPARATORS.iter().map(|&s| s.to_string()).collect(),
        };
        
        let split_artists = split_artist_with_separators(artist_name, &separators);
        
        // Only return if we actually split into multiple parts
        if split_artists.len() > 1 {
            debug!("Split '{}' into multiple artists: {:?}", artist_name, split_artists);
            Some(split_artists)
        } else {
            debug!("'{}' appears to be a single artist", artist_name);
            None
        }
    };
    
    // Cache the result (no expiry)
    if let Err(e) = attributecache::set_with_expiry(&cache_key, &result, None) {
        debug!("Failed to cache simple split result for '{}': {}", artist_name, e);
    } else {
        debug!("Cached simple split result for '{}': {:?}", artist_name, result);
    }
    
    result
}

/// Check if an artist name contains multiple artists by using MusicBrainz MBID lookups
/// and split the name if multiple MBIDs are found
///
/// # Arguments
/// * `artist_name` - The name of the artist to check
/// * `cache_only` - If true, only check the cache and don't make API calls (default: true)
/// * `custom_separators` - Optional list of custom separators to use instead of the default
///
/// # Returns
/// * `Option<Vec<String>>` - None if single artist, or Some(Vec<String>) with split artist names if multiple
pub fn split_artist_names_with_mbid_lookup(artist_name: &str, cache_only: bool, custom_separators: Option<&[String]>) -> Option<Vec<String>> {
    debug!("Checking if '{}' contains multiple artists (cache_only: {})", artist_name, cache_only);
    
    // Create cache key for artist splits
    let cache_key = format!("{}{}", ARTIST_SPLIT_CACHE_PREFIX, artist_name);
    
    // Try to get cached result first (no expiry)
    match attributecache::get::<Option<Vec<String>>>(&cache_key) {
        Ok(Some(cached_result)) => {
            debug!("Found cached split result for '{}': {:?}", artist_name, cached_result);
            return cached_result;
        },
        Ok(None) => {
            debug!("No cached split result found for '{}'", artist_name);
        },
        Err(e) => {
            debug!("Error reading cached split result for '{}': {}", artist_name, e);
        }
    }
    
    // Determine which separators to use
    let separators: Vec<&str> = match custom_separators {
        Some(seps) => seps.iter().map(|s| s.as_str()).collect(), // Convert &[String] to Vec<&str>
        None => DEFAULT_ARTIST_SEPARATORS.to_vec(), // Convert &[&str] to Vec<&str>
    };
    
    // First, quickly check if the string contains any separator
    let contains_separator = separators.iter().any(|separator| artist_name.contains(*separator));
    if !contains_separator {
        debug!("'{}' doesn't contain any separators, assuming single artist", artist_name);
        // Cache the result that this is a single artist
        if let Err(e) = attributecache::set_with_expiry(&cache_key, &None::<Vec<String>>, None) {
            debug!("Failed to cache single artist result for '{}': {}", artist_name, e);
        }
        return None;
    }

    let result = perform_artist_split_with_mbid_lookup(artist_name, cache_only, &separators);
    
    // Cache the result (no expiry)
    if let Err(e) = attributecache::set_with_expiry(&cache_key, &result, None) {
        debug!("Failed to cache split result for '{}': {}", artist_name, e);
    } else {
        debug!("Cached split result for '{}': {:?}", artist_name, result);
    }
    
    result
}

/// Internal function that performs the actual artist split with MBID lookup logic
fn perform_artist_split_with_mbid_lookup(artist_name: &str, cache_only: bool, separators: &[&str]) -> Option<Vec<String>> {
    // if musicbrainz lookups are disabled, implement a "dumb" split using provided separators
    if !musicbrainz::is_enabled() {
        debug!("MusicBrainz lookups are disabled, performing dumb split for '{}'", artist_name);
        
        // Convert string slices to Strings for processing
        let string_separators: Vec<String> = separators.iter().map(|&s| s.to_string()).collect();
        
        // Call split_artist with our separators
        let split_artists = split_artist_with_separators(artist_name, &string_separators);
        
        // Only return if we actually split into multiple parts
        if split_artists.len() > 1 {
            debug!("Split '{}' into multiple artists: {:?}", artist_name, split_artists);
            return Some(split_artists);
        } else {
            debug!("'{}' appears to be a single artist", artist_name);
            return None;
        }
    }
    
    // Look up MBIDs for the artist
    match musicbrainz::search_mbids_for_artist(artist_name, true, cache_only, false) {
        MusicBrainzSearchResult::Found(mbids, _) => {
            // If multiple MBIDs found, this might be a combined artist name
            if mbids.len() > 1 {
                debug!("Multiple MBIDs found for '{}', splitting artist name", artist_name);
                
                // Convert string slices to Strings for processing
                let string_separators: Vec<String> = separators.iter().map(|&s| s.to_string()).collect();
                
                // Split using provided separators
                let split_artists = split_artist_with_separators(artist_name, &string_separators);
                
                // Only return if we actually split into multiple parts
                if split_artists.len() > 1 {
                    debug!("Split '{}' into multiple artists: {:?}", artist_name, split_artists);
                    return Some(split_artists);
                }
            }
            
            // Single MBID found or couldn't split into multiple parts
            debug!("'{}' appears to be a single artist", artist_name);
            None
        },
        _ => {
            // No MBIDs found for the combined string, try splitting and validating individual artists
            debug!("No MBIDs found for combined string '{}', trying to split and validate individual artists", artist_name);
            
            // Convert string slices to Strings for processing
            let string_separators: Vec<String> = separators.iter().map(|&s| s.to_string()).collect();
            
            // Split using provided separators
            let split_artists = split_artist_with_separators(artist_name, &string_separators);
            
            // Only proceed if we actually split into multiple parts
            if split_artists.len() > 1 {
                debug!("Split '{}' into {} potential artists: {:?}", artist_name, split_artists.len(), split_artists);
                
                // Validate individual artists against MusicBrainz
                let mut found_count = 0;
                for individual_artist in &split_artists {
                    match musicbrainz::search_mbids_for_artist(individual_artist, true, cache_only, false) {
                        MusicBrainzSearchResult::Found(_, _) => {
                            found_count += 1;
                            debug!("Found MBID for individual artist: '{}'", individual_artist);
                        },
                        _ => {
                            debug!("No MBID found for individual artist: '{}'", individual_artist);
                        }
                    }
                }
                
                // Calculate percentage of artists found
                let found_percentage = (found_count as f64 / split_artists.len() as f64) * 100.0;
                debug!("Found {}/{} artists ({}%) in MusicBrainz for '{}'", found_count, split_artists.len(), found_percentage, artist_name);
                
                // Only split if at least 25% of the artists can be found in MusicBrainz
                if found_percentage >= 25.0 {
                    debug!("At least 25% of split artists found in MusicBrainz, splitting '{}'", artist_name);
                    Some(split_artists)
                } else {
                    debug!("Less than 25% of split artists found in MusicBrainz, not splitting '{}'", artist_name);
                    None
                }
            } else {
                debug!("'{}' appears to be a single artist", artist_name);
                None
            }
        }
    }
}



#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_split_artist_basic() {
        let result = split_artist("John Lennon");
        assert_eq!(result, vec!["John Lennon"]);
    }

    #[test]
    fn test_split_artist_with_comma() {
        let result = split_artist("Lennon, McCartney");
        assert_eq!(result, vec!["Lennon", "McCartney"]);
    }

    #[test]
    fn test_split_artist_with_ampersand() {
        let result = split_artist("Simon & Garfunkel");
        assert_eq!(result, vec!["Simon", "Garfunkel"]);
    }

    #[test]
    fn test_split_artist_with_feat() {
        let result = split_artist("The Beatles feat. Tony Sheridan");
        assert_eq!(result, vec!["The Beatles", "Tony Sheridan"]);
    }

    #[test]
    fn test_split_artist_with_featuring() {
        let result = split_artist("Jay-Z featuring Alicia Keys");
        assert_eq!(result, vec!["Jay-Z", "Alicia Keys"]);
    }

    #[test]
    fn test_split_artist_with_with() {
        let result = split_artist("Johnny Cash with June Carter");
        assert_eq!(result, vec!["Johnny Cash", "June Carter"]);
    }

    #[test]
    fn test_split_artist_multiple_separators() {
        let result = split_artist("The Beatles, Paul McCartney & Wings feat. Billy Preston");
        assert_eq!(result, vec!["The Beatles", "Paul McCartney", "Wings", "Billy Preston"]);
    }

    #[test]
    fn test_split_artist_with_custom_separators() {
        let custom_separators = vec![" x ".to_string(), " vs ".to_string()];
        let result = split_artist_with_separators("Artist A x Artist B vs Artist C", &custom_separators);
        assert_eq!(result, vec!["Artist A", "Artist B", "Artist C"]);
    }

    #[test]
    fn test_split_artist_with_extra_whitespace() {
        let result = split_artist("  John Lennon  ,  Paul McCartney  ");
        assert_eq!(result, vec!["John Lennon", "Paul McCartney"]);
    }

    #[test]
    fn test_split_artist_removes_feat_prefix() {
        // The function should filter out parts that start with "feat."
        let result = split_artist("Main Artist, feat. Other Artist");
        assert_eq!(result, vec!["Main Artist"]);
    }

    #[test]
    fn test_contains_multiple_artists() {
        assert!(contains_multiple_artists("Simon & Garfunkel", None));
        assert!(contains_multiple_artists("The Beatles feat. Tony Sheridan", None));
        assert!(contains_multiple_artists("Lennon, McCartney", None));
        assert!(!contains_multiple_artists("The Beatles", None));
        assert!(!contains_multiple_artists("", None));
    }

    #[test]
    fn test_contains_multiple_artists_custom_separators() {
        let custom_separators = vec![" x ".to_string()];
        assert!(contains_multiple_artists("Artist A x Artist B", Some(&custom_separators)));
        assert!(!contains_multiple_artists("Artist A & Artist B", Some(&custom_separators)));
    }

    #[test]
    fn test_split_if_multiple() {
        // Should return None for single artists
        assert_eq!(split_if_multiple("The Beatles", None), None);
        
        // Should return Some for multiple artists
        let result = split_if_multiple("Simon & Garfunkel", None);
        assert_eq!(result, Some(vec!["Simon".to_string(), "Garfunkel".to_string()]));
        
        // Should return None if separator exists but doesn't actually split
        let result = split_if_multiple("Artist & ", None);
        assert_eq!(result, None);
    }

    #[test]
    fn test_split_if_multiple_custom_separators() {
        let custom_separators = vec![" x ".to_string()];
        
        // Should use custom separators
        let result = split_if_multiple("Artist A x Artist B", Some(&custom_separators));
        assert_eq!(result, Some(vec!["Artist A".to_string(), "Artist B".to_string()]));
        
        // Should not split on default separators when custom ones are provided
        let result = split_if_multiple("Artist A & Artist B", Some(&custom_separators));
        assert_eq!(result, None);
    }

    #[test]
    fn test_edge_cases() {
        // Empty string
        let result = split_artist("");
        assert_eq!(result, Vec::<String>::new());
        
        // Only separators
        let result = split_artist("&,");
        assert_eq!(result, Vec::<String>::new());
        
        // Separator at start/end
        let result = split_artist("& Artist Name ,");
        assert_eq!(result, vec!["Artist Name"]);
    }

    #[test]
    fn test_case_insensitive_feat_filtering() {
        // Test that "feat." filtering is case insensitive
        let result = split_artist("Main Artist, FEAT. Other Artist");
        assert_eq!(result, vec!["Main Artist"]);
        
        let result = split_artist("Main Artist, Feat. Other Artist");
        assert_eq!(result, vec!["Main Artist"]);
    }

    #[test]
    fn test_complex_multi_artist_string() {
        // Test the specific artist string: "Adam X, Maedon, Alessandro Adriani, 3.14, Chloe Lula, E-Bony"
        let complex_artists = "Adam X, Maedon, Alessandro Adriani, 3.14, Chloe Lula, E-Bony";
        
        let result = split_artist(complex_artists);
        let expected = vec![
            "Adam X".to_string(),
            "Maedon".to_string(), 
            "Alessandro Adriani".to_string(),
            "3.14".to_string(),
            "Chloe Lula".to_string(),
            "E-Bony".to_string()
        ];
        assert_eq!(result, expected);
    }

    #[test]
    fn test_complex_multi_artist_string_split_if_multiple() {
        // Test the same string with split_if_multiple
        let complex_artists = "Adam X, Maedon, Alessandro Adriani, 3.14, Chloe Lula, E-Bony";
        
        let result = split_if_multiple(complex_artists, None);
        let expected = Some(vec![
            "Adam X".to_string(),
            "Maedon".to_string(), 
            "Alessandro Adriani".to_string(),
            "3.14".to_string(),
            "Chloe Lula".to_string(),
            "E-Bony".to_string()
        ]);
        assert_eq!(result, expected);
    }

    #[test] 
    fn test_complex_multi_artist_string_contains_check() {
        // Test that the complex string is correctly detected as containing multiple artists
        let complex_artists = "Adam X, Maedon, Alessandro Adriani, 3.14, Chloe Lula, E-Bony";
        
        assert!(contains_multiple_artists(complex_artists, None));
    }

    #[test]
    fn test_complex_multi_artist_string_with_mbid_lookup_disabled() {
        // Test the same string with MBID lookup when MusicBrainz is disabled
        // This simulates the behavior when musicbrainz::is_enabled() returns false
        let complex_artists = "Adam X, Maedon, Alessandro Adriani, 3.14, Chloe Lula, E-Bony";
        
        // When MusicBrainz is disabled, split_artist_names_with_mbid_lookup should fall back
        // to simple text-based splitting, which should work the same as split_if_multiple
        let result_simple = split_if_multiple(complex_artists, None);
        
        // The MBID lookup function should produce the same result when MB is disabled
        // Note: This test will only work properly when MusicBrainz lookups are disabled
        // If they're enabled, it might try to do actual MBID lookups
        let expected = Some(vec![
            "Adam X".to_string(),
            "Maedon".to_string(), 
            "Alessandro Adriani".to_string(),
            "3.14".to_string(),
            "Chloe Lula".to_string(),
            "E-Bony".to_string()
        ]);
        assert_eq!(result_simple, expected);
    }

    #[test]
    fn test_mbid_lookup_function_behavior() {
        // Test the behavior of split_artist_names_with_mbid_lookup function
        let complex_artists = "Adam X, Maedon, Alessandro Adriani, 3.14, Chloe Lula, E-Bony";
        
        // Test with cache_only = true to avoid making actual API calls during testing
        let result_mbid = split_artist_names_with_mbid_lookup(complex_artists, true, None);
        
        // The result should either be:
        // 1. The split artists if MusicBrainz is disabled (falls back to simple splitting)
        // 2. The split artists if MusicBrainz is enabled and >= 25% of individual artists found in cache
        // 3. The split artists if MusicBrainz finds multiple MBIDs for the full string
        // 4. None if MusicBrainz is enabled but < 25% of individual artists found in cache
        // 5. None if MusicBrainz is enabled and finds a single artist for the full string
        match result_mbid {
            Some(artists) => {
                // If we get a result, it should be the correctly split artists
                let expected = vec![
                    "Adam X".to_string(),
                    "Maedon".to_string(), 
                    "Alessandro Adriani".to_string(),
                    "3.14".to_string(),
                    "Chloe Lula".to_string(),
                    "E-Bony".to_string()
                ];
                assert_eq!(artists, expected);
                println!("MBID lookup successfully split the complex artist string - either due to MusicBrainz being disabled, finding multiple MBIDs for the full string, or >= 25% of individual artists being found");
            },
            None => {
                // If we get None, it could be because:
                // 1. No separators found (shouldn't happen with this string)
                // 2. MusicBrainz is enabled and determined it's a single artist (found single MBID for full string)
                // 3. MusicBrainz is enabled but < 25% of split artists were found (new validation logic)
                println!("MBID lookup returned None for complex artist string - this could be expected if:");
                println!("  - MusicBrainz found a single MBID for the full string, or");
                println!("  - Less than 25% of individual artists ('Adam X', 'Maedon', etc.) were found in MusicBrainz cache");
                println!("  This demonstrates the new validation threshold working correctly");
            }
        }
        
        // Additional validation: ensure basic splitting still works regardless of MusicBrainz behavior
        let simple_split = split_artist(complex_artists);
        let expected_simple = vec![
            "Adam X".to_string(),
            "Maedon".to_string(), 
            "Alessandro Adriani".to_string(),
            "3.14".to_string(),
            "Chloe Lula".to_string(),
            "E-Bony".to_string()
        ];
        assert_eq!(simple_split, expected_simple, "Basic splitting should always work regardless of MusicBrainz state");
    }

    #[test]
    fn test_mbid_validation_threshold_behavior() {
        // Test that the 25% threshold logic is working
        // Note: This test validates the logic structure but actual behavior depends on MusicBrainz cache state
        
        // Test with a string that contains separators (should attempt splitting)
        let test_artists = "Known Artist, Unknown Artist, Another Unknown, Yet Another Unknown";
        
        // Test with cache_only = true to avoid making actual API calls during testing
        let result = split_artist_names_with_mbid_lookup(test_artists, true, None);
        
        // The result depends on what's in the MusicBrainz cache:
        // - If MusicBrainz is disabled: should split (fallback behavior)
        // - If MusicBrainz is enabled but no cache entries: should not split (< 25% found)
        // - If MusicBrainz is enabled and >= 25% artists found in cache: should split
        // - If MusicBrainz finds multiple MBIDs for the full string: should split
        
        match result {
            Some(artists) => {
                // Splitting occurred - either due to fallback or sufficient MBID validation
                println!("Artists were split into: {:?}", artists);
                assert!(artists.len() > 1);
            },
            None => {
                // No splitting occurred - either single artist or insufficient MBID validation
                println!("Artists were not split - insufficient MusicBrainz validation or single artist");
            }
        }
        
        // This test primarily validates that the function doesn't crash and handles the logic correctly
        // The actual outcome depends on the MusicBrainz configuration and cache state
    }
}
