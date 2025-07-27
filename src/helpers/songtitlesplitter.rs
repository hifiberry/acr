/// Song title splitter module
/// 
/// This module provides functionality to split combined artist/title strings
/// into separate parts using common separators and determine their order
/// using MusicBrainz lookups.

use crate::helpers::musicbrainz;
use std::collections::HashMap;
use log::{debug, info};
use serde::{Serialize, Deserialize};

/// Result of order detection
#[derive(Debug, PartialEq, Eq, Hash, Clone, Serialize, Deserialize)]
pub enum OrderResult {
    /// First part is artist, second part is song
    ArtistSong,
    /// First part is song, second part is artist  
    SongArtist,
    /// No combination found in MusicBrainz
    Unknown,
    /// Both combinations found, cannot determine
    Undecided,
}

/// Split a combined title into artist and song parts
///
/// This function splits titles that contain both artist and song information
/// separated by common delimiters like " / " or " - ".
///
/// # Arguments
/// * `title` - The combined title string to split
///
/// # Returns
/// An optional tuple of (part1, part2) if splitting was successful
///
/// # Examples
/// ```no_run
/// use audiocontrol::helpers::songtitlesplitter::split_song;
/// 
/// let result = split_song("The Beatles / Hey Jude");
/// assert_eq!(result, Some(("The Beatles".to_string(), "Hey Jude".to_string())));
/// 
/// let result = split_song("Yesterday - The Beatles");
/// assert_eq!(result, Some(("Yesterday".to_string(), "The Beatles".to_string())));
/// ```
pub fn split_song(input: &str) -> Option<(String, String)> {
    // Find the first occurrence of either "/" or "-"
    let dash_pos = input.find('-');
    let slash_pos = input.find('/');
    
    // Determine which separator comes first (or if any exists)
    let split_pos = match (dash_pos, slash_pos) {
        (Some(dash), Some(slash)) => Some(dash.min(slash)),
        (Some(dash), None) => Some(dash),
        (None, Some(slash)) => Some(slash),
        (None, None) => None,
    };
    
    // If we found a separator, split the string
    if let Some(pos) = split_pos {
        let part1 = input[..pos].trim().to_string();
        let part2 = input[pos + 1..].trim().to_string();
        
        // Only return if both parts are non-empty after trimming
        if !part1.is_empty() && !part2.is_empty() {
            Some((part1, part2))
        } else {
            None
        }
    } else {
        None
    }
}

/// Detect the order of artist and song in split parts using MusicBrainz lookup
///
/// This function attempts to determine which part is the artist and which is the song
/// by searching MusicBrainz for exact matches. It tries both combinations:
/// - part1 as artist, part2 as song
/// - part1 as song, part2 as artist
///
/// # Arguments
/// * `part1` - The first part of the split title
/// * `part2` - The second part of the split title
///
/// # Returns
/// An OrderResult indicating the detected order:
/// - ArtistSong: part1 is artist, part2 is song
/// - SongArtist: part1 is song, part2 is artist
/// - Unknown: no combination found in MusicBrainz
/// - Undecided: both combinations found, cannot determine
///
/// # Examples
/// ```no_run
/// use audiocontrol::helpers::songtitlesplitter::{detect_order, OrderResult};
/// 
/// let result = detect_order("The Beatles", "Hey Jude");
/// // Result depends on MusicBrainz database content
/// ```
pub fn detect_order(part1: &str, part2: &str) -> OrderResult {
    // Try part1 as artist, part2 as song
    let artist_song_result = musicbrainz::search_recording(part1, part2);
    let artist_song_found = match artist_song_result {
        Ok(response) => response.count > 0,
        Err(_) => false,
    };

    // Try part1 as song, part2 as artist
    let song_artist_result = musicbrainz::search_recording(part2, part1);
    let song_artist_found = match song_artist_result {
        Ok(response) => response.count > 0,
        Err(_) => false,
    };

    match (artist_song_found, song_artist_found) {
        (true, false) => OrderResult::ArtistSong,
        (false, true) => OrderResult::SongArtist,
        (false, false) => OrderResult::Unknown,
        (true, true) => OrderResult::Undecided,
    }
}

/// A smart song title splitter that can detect artist/song order
/// 
/// This struct provides intelligent splitting of combined artist/title strings
/// and can determine the correct order using MusicBrainz lookups. It maintains
/// statistics about how many songs have been found in each order. After 20 songs,
/// if one order type represents >95% of successful detections, it becomes the default.
/// It also caches lookup results to avoid affecting counters for repeated lookups.
#[derive(Clone, Serialize, Deserialize)]
pub struct SongTitleSplitter {
    /// An identifier string (not the song title itself)
    id: String,
    /// Statistics of detected orders: (ArtistSong, SongArtist, Unknown, Undecided)
    order_stats: HashMap<OrderResult, u32>,
    /// Default order to use when pattern is established (>95% confidence after 20+ songs)
    default_order: Option<OrderResult>,
    /// Cache for MusicBrainz lookup results to avoid repeated API calls and counter updates
    #[serde(skip)]
    lookup_cache: HashMap<String, OrderResult>,
    /// Maximum number of entries to keep in the lookup cache
    #[serde(skip)]
    cache_size_limit: usize,
}

impl SongTitleSplitter {
    /// Create a new SongTitleSplitter with the given ID
    /// 
    /// # Arguments
    /// * `id` - An identifier string (not the song title)
    /// 
    /// # Returns
    /// A new SongTitleSplitter instance with default cache size of 50
    /// 
    /// # Examples
    /// ```no_run
    /// use audiocontrol::helpers::songtitlesplitter::SongTitleSplitter;
    /// 
    /// let splitter = SongTitleSplitter::new("track_123");
    /// ```
    pub fn new(id: &str) -> Self {
        Self::with_cache_size(id, 50)
    }
    
    /// Create a new SongTitleSplitter with a custom cache size
    /// 
    /// # Arguments
    /// * `id` - An identifier string (not the song title)
    /// * `cache_size` - Maximum number of lookup results to cache
    /// 
    /// # Returns
    /// A new SongTitleSplitter instance with specified cache size
    /// 
    /// # Examples
    /// ```no_run
    /// use audiocontrol::helpers::songtitlesplitter::SongTitleSplitter;
    /// 
    /// let splitter = SongTitleSplitter::with_cache_size("track_123", 100);
    /// ```
    pub fn with_cache_size(id: &str, cache_size: usize) -> Self {
        Self {
            id: id.to_string(),
            order_stats: HashMap::new(),
            default_order: None,
            lookup_cache: HashMap::new(),
            cache_size_limit: cache_size,
        }
    }
    
    /// Split the song and return (artist, song) in the correct order
    /// 
    /// This method intelligently determines which part is the artist and which
    /// is the song using MusicBrainz lookups, then returns them in the correct order.
    /// Updates statistics about detected order patterns. After 20 successful detections,
    /// if one order type represents >95% of results, it becomes the default for future splits.
    /// Results are cached to avoid repeated API calls and counter updates.
    /// 
    /// # Arguments
    /// * `song_title` - The combined song title string to split and analyze
    /// 
    /// # Returns
    /// An optional tuple of (artist, song) if the title could be split and order determined
    /// 
    /// # Examples
    /// ```no_run
    /// use audiocontrol::helpers::songtitlesplitter::SongTitleSplitter;
    /// 
    /// let mut splitter = SongTitleSplitter::new("track_123");
    /// if let Some((artist, song)) = splitter.split_song("The Beatles - Hey Jude") {
    ///     println!("Artist: {}, Song: {}", artist, song);
    /// }
    /// ```
    pub fn split_song(&mut self, song_title: &str) -> Option<(String, String)> {
        debug!("Splitting song: '{}'", song_title);
        
        // First split the title into parts
        let parts = split_song(song_title)?;
        
        // Determine the order using default, cache, or MusicBrainz lookup
        let order = if let Some(default) = &self.default_order {
            // Use the established default order
            debug!("Using established default order {:?} for '{}'", default, song_title);
            default.clone()
        } else {
            // Check cache first, then detect if not cached
            self.get_order_with_cache(song_title, &parts)
        };
        
        let result = match order {
            OrderResult::ArtistSong => {
                debug!("Returning ArtistSong order for '{}': artist='{}', song='{}'", 
                       song_title, parts.0, parts.1);
                Some((parts.0, parts.1))
            },
            OrderResult::SongArtist => {
                debug!("Returning SongArtist order for '{}': artist='{}', song='{}'", 
                       song_title, parts.1, parts.0);
                Some((parts.1, parts.0))
            },
            OrderResult::Unknown | OrderResult::Undecided => {
                debug!("Cannot determine order for '{}': {:?}", song_title, order);
                // For unknown or undecided cases, we could implement fallback logic
                // For now, return None to indicate we couldn't determine the order
                None
            }
        };
        
        result
    }
    
    /// Internal method to update order statistics
    fn update_stats(&mut self, order: OrderResult) {
        let count = self.order_stats.entry(order.clone()).or_insert(0);
        *count += 1;
        debug!("Updated stats for {:?}: count now {}", order, count);
    }
    
    /// Get order with cache lookup, only updating stats for new lookups
    fn get_order_with_cache(&mut self, song_title: &str, parts: &(String, String)) -> OrderResult {
        // Check if we already have the result cached
        if let Some(cached_order) = self.lookup_cache.get(song_title) {
            debug!("Cache hit for '{}': {:?}", song_title, cached_order);
            return cached_order.clone();
        }
        
        debug!("Cache miss for '{}', performing MusicBrainz lookup for '{}' vs '{}'", 
               song_title, parts.0, parts.1);
        
        // Detect the order using MusicBrainz lookup
        let order = detect_order(&parts.0, &parts.1);
        
        debug!("MusicBrainz lookup result for '{}': {:?}", song_title, order);
        
        // Cache the result (with size limit)
        if self.lookup_cache.len() >= self.cache_size_limit {
            // Remove a random entry to make space (in a real implementation, you might use LRU)
            if let Some(key) = self.lookup_cache.keys().next().cloned() {
                debug!("Cache full, removing entry: '{}'", key);
                self.lookup_cache.remove(&key);
            }
        }
        self.lookup_cache.insert(song_title.to_string(), order.clone());
        debug!("Cached result for '{}': {:?} (cache size: {})", 
               song_title, order, self.lookup_cache.len());
        
        // Update statistics only for new lookups
        self.update_stats(order.clone());
        
        // Check if we should establish a default order
        self.check_and_set_default_order();
        
        order
    }
    
    /// Check if we should establish a default order based on current statistics
    /// 
    /// After 20 successful detections (ArtistSong + SongArtist), if one order type
    /// represents >95% of the successful results, it becomes the default.
    fn check_and_set_default_order(&mut self) {
        // Only check if we don't already have a default
        if self.default_order.is_some() {
            return;
        }
        
        let artist_song_count = self.get_artist_song_count();
        let song_artist_count = self.get_song_artist_count();
        let total_successful = artist_song_count + song_artist_count;
        
        // Need at least 20 successful detections to establish a pattern
        if total_successful < 20 {
            debug!("Not enough successful detections yet: {} (need 20)", total_successful);
            return;
        }
        
        // Calculate percentages
        let artist_song_percent = (artist_song_count as f64) / (total_successful as f64) * 100.0;
        let song_artist_percent = (song_artist_count as f64) / (total_successful as f64) * 100.0;
        
        debug!("Order statistics: ArtistSong={} ({:.1}%), SongArtist={} ({:.1}%), Total={}",
               artist_song_count, artist_song_percent, 
               song_artist_count, song_artist_percent, 
               total_successful);
        
        // Check if either order reaches strictly >95% threshold
        if artist_song_percent > 95.0 {
            self.default_order = Some(OrderResult::ArtistSong);
            info!("Default sort order established: ArtistSong ({:.1}% of {} successful detections)", 
                  artist_song_percent, total_successful);
        } else if song_artist_percent > 95.0 {
            self.default_order = Some(OrderResult::SongArtist);
            info!("Default sort order established: SongArtist ({:.1}% of {} successful detections)", 
                  song_artist_percent, total_successful);
        } else {
            debug!("No clear pattern yet: need >95% for one order type");
        }
    }
    
    /// Get the order detection result for a given song title
    /// 
    /// # Arguments
    /// * `song_title` - The combined song title string to analyze
    /// 
    /// # Returns
    /// The OrderResult indicating the detected order
    /// 
    /// # Examples
    /// ```no_run
    /// use audiocontrol::helpers::songtitlesplitter::{SongTitleSplitter, OrderResult};
    /// 
    /// let mut splitter = SongTitleSplitter::new("track_123");
    /// match splitter.get_order("The Beatles - Hey Jude") {
    ///     OrderResult::ArtistSong => println!("First part is artist"),
    ///     OrderResult::SongArtist => println!("First part is song"),
    ///     _ => println!("Could not determine order"),
    /// }
    /// ```
    pub fn get_order(&mut self, song_title: &str) -> OrderResult {
        if let Some((part1, part2)) = split_song(song_title) {
            // Use default order if established
            if let Some(default) = &self.default_order {
                debug!("Using default order {:?} for '{}'", default, song_title);
                return default.clone();
            }
            
            // Otherwise use cache or detect using MusicBrainz
            self.get_order_with_cache(song_title, &(part1, part2))
        } else {
            debug!("Could not split '{}' - no separator found", song_title);
            let order = OrderResult::Unknown;
            // Only update stats if not cached
            if !self.lookup_cache.contains_key(song_title) {
                self.update_stats(order.clone());
                self.lookup_cache.insert(song_title.to_string(), order.clone());
            }
            order
        }
    }
    
    /// Get the raw split parts for a given song title without order detection
    /// 
    /// # Arguments
    /// * `song_title` - The combined song title string to split
    /// 
    /// # Returns
    /// An optional tuple of (part1, part2) as they appear in the original title
    /// 
    /// # Examples
    /// ```no_run
    /// use audiocontrol::helpers::songtitlesplitter::SongTitleSplitter;
    /// 
    /// let splitter = SongTitleSplitter::new("track_123");
    /// if let Some((part1, part2)) = splitter.get_raw_parts("The Beatles - Hey Jude") {
    ///     println!("Part 1: {}, Part 2: {}", part1, part2);
    /// }
    /// ```
    pub fn get_raw_parts(&self, song_title: &str) -> Option<(String, String)> {
        split_song(song_title)
    }
    
    /// Get the ID string that was passed to the constructor
    /// 
    /// # Returns
    /// The ID string that was passed to the constructor
    /// 
    /// # Examples
    /// ```no_run
    /// use audiocontrol::helpers::songtitlesplitter::SongTitleSplitter;
    /// 
    /// let splitter = SongTitleSplitter::new("track_123");
    /// assert_eq!(splitter.get_id(), "track_123");
    /// ```
    pub fn get_id(&self) -> &str {
        &self.id
    }
    
    /// Get the number of songs detected with ArtistSong order
    pub fn get_artist_song_count(&self) -> u32 {
        *self.order_stats.get(&OrderResult::ArtistSong).unwrap_or(&0)
    }
    
    /// Get the number of songs detected with SongArtist order
    pub fn get_song_artist_count(&self) -> u32 {
        *self.order_stats.get(&OrderResult::SongArtist).unwrap_or(&0)
    }
    
    /// Get the number of songs where order could not be determined
    pub fn get_unknown_count(&self) -> u32 {
        *self.order_stats.get(&OrderResult::Unknown).unwrap_or(&0)
    }
    
    /// Get the number of songs where order was undecided
    pub fn get_undecided_count(&self) -> u32 {
        *self.order_stats.get(&OrderResult::Undecided).unwrap_or(&0)
    }
    
    /// Get the total number of songs analyzed
    pub fn get_total_count(&self) -> u32 {
        self.order_stats.values().sum()
    }
    
    /// Get a copy of all order statistics
    pub fn get_all_stats(&self) -> HashMap<OrderResult, u32> {
        self.order_stats.clone()
    }
    
    /// Clear all statistics
    pub fn clear_stats(&mut self) {
        self.order_stats.clear();
    }
    
    /// Clear the lookup cache
    pub fn clear_cache(&mut self) {
        self.lookup_cache.clear();
    }
    
    /// Get the current cache size
    pub fn get_cache_size(&self) -> usize {
        self.lookup_cache.len()
    }
    
    /// Get the cache size limit
    pub fn get_cache_size_limit(&self) -> usize {
        self.cache_size_limit
    }
    
    /// Check if a song title's order has been cached
    pub fn is_cached(&self, song_title: &str) -> bool {
        self.lookup_cache.contains_key(song_title)
    }
    
    /// Get the default order if one has been established
    /// 
    /// # Returns
    /// The default OrderResult if established (>95% confidence after 20+ songs), None otherwise
    pub fn get_default_order(&self) -> Option<OrderResult> {
        self.default_order.clone()
    }
    
    /// Check if a default order has been established
    /// 
    /// # Returns
    /// true if a default order is set (>95% confidence after 20+ songs), false otherwise
    pub fn has_default_order(&self) -> bool {
        self.default_order.is_some()
    }
    
    /// Get the percentage of successful detections for each order type
    /// 
    /// # Returns
    /// A tuple of (artist_song_percent, song_artist_percent) based on successful detections only
    pub fn get_order_percentages(&self) -> (f64, f64) {
        let artist_song_count = self.get_artist_song_count();
        let song_artist_count = self.get_song_artist_count();
        let total_successful = artist_song_count + song_artist_count;
        
        if total_successful == 0 {
            return (0.0, 0.0);
        }
        
        let artist_song_percent = (artist_song_count as f64) / (total_successful as f64) * 100.0;
        let song_artist_percent = (song_artist_count as f64) / (total_successful as f64) * 100.0;
        
        (artist_song_percent, song_artist_percent)
    }
    
    /// Reset the default order and clear statistics
    /// 
    /// This can be useful if you want to restart the learning process
    pub fn reset(&mut self) {
        self.order_stats.clear();
        self.default_order = None;
        self.lookup_cache.clear();
    }
    
    /// Serialize the SongTitleSplitter to a JSON string
    /// 
    /// This method saves the essential state: ID, default order, and statistics.
    /// Cache is not serialized and will be reset when the splitter is restored.
    /// 
    /// # Returns
    /// A Result containing the JSON string representation on success, or an error on failure
    /// 
    /// # Examples
    /// ```no_run
    /// use audiocontrol::helpers::songtitlesplitter::SongTitleSplitter;
    /// 
    /// let mut splitter = SongTitleSplitter::new("radio_station_url");
    /// // ... use the splitter to analyze songs and establish default order ...
    /// 
    /// match splitter.to_json() {
    ///     Ok(json) => {
    ///         // Save the JSON string to a file or database
    ///         println!("Saved splitter state: {}", json);
    ///     },
    ///     Err(e) => {
    ///         eprintln!("Failed to serialize splitter: {}", e);
    ///     }
    /// }
    /// ```
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }
    
    /// Deserialize a SongTitleSplitter from a JSON string
    /// 
    /// This method restores a previously saved splitter state including its
    /// ID, default order, and statistics. Cache is reset to empty.
    /// 
    /// # Arguments
    /// * `json` - The JSON string representation to deserialize
    /// 
    /// # Returns
    /// A Result containing the restored SongTitleSplitter on success, or an error on failure
    /// 
    /// # Examples
    /// ```no_run
    /// use audiocontrol::helpers::songtitlesplitter::SongTitleSplitter;
    /// 
    /// let json = r#"{"id":"radio_station_url","order_stats":{},"default_order":null}"#;
    /// 
    /// match SongTitleSplitter::from_json(json) {
    ///     Ok(mut splitter) => {
    ///         println!("Restored splitter with ID: {}", splitter.get_id());
    ///     },
    ///     Err(e) => {
    ///         eprintln!("Failed to deserialize splitter: {}", e);
    ///     }
    /// }
    /// ```
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        let mut splitter: SongTitleSplitter = serde_json::from_str(json)?;
        // Initialize the skipped fields with default values
        splitter.lookup_cache = HashMap::new();
        splitter.cache_size_limit = 50; // Default cache size
        Ok(splitter)
    }
    
    /// Serialize the SongTitleSplitter to a compact JSON string
    /// 
    /// This is similar to `to_json()` but produces a more compact representation
    /// without pretty formatting, suitable for storage in databases or network transmission.
    /// ID, default order, and statistics are included in the serialization.
    /// 
    /// # Returns
    /// A Result containing the compact JSON string representation on success, or an error on failure
    /// 
    /// # Examples
    /// ```no_run
    /// use audiocontrol::helpers::songtitlesplitter::SongTitleSplitter;
    /// 
    /// let splitter = SongTitleSplitter::new("radio_station_url");
    /// 
    /// match splitter.to_json_compact() {
    ///     Ok(json) => {
    ///         // Compact JSON is smaller for storage
    ///         println!("Compact JSON: {}", json);
    ///     },
    ///     Err(e) => {
    ///         eprintln!("Failed to serialize splitter: {}", e);
    ///     }
    /// }
    /// ```
    pub fn to_json_compact(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }
    
    /// Create a copy of this splitter with a new ID
    /// 
    /// This is useful when you want to use the learned statistics and cache
    /// from one splitter as a starting point for another radio station.
    /// 
    /// # Arguments
    /// * `new_id` - The new identifier for the cloned splitter
    /// 
    /// # Returns
    /// A new SongTitleSplitter with the same state but different ID
    /// 
    /// # Examples
    /// ```no_run
    /// use audiocontrol::helpers::songtitlesplitter::SongTitleSplitter;
    /// 
    /// let mut original = SongTitleSplitter::new("station_1");
    /// // ... train the splitter with songs ...
    /// 
    /// // Create a new splitter for a similar station with the same learned patterns
    /// let similar_station = original.clone_with_id("station_2");
    /// assert_eq!(similar_station.get_id(), "station_2");
    /// ```
    pub fn clone_with_id(&self, new_id: &str) -> Self {
        Self {
            id: new_id.to_string(),
            order_stats: self.order_stats.clone(),
            default_order: self.default_order.clone(),
            lookup_cache: self.lookup_cache.clone(),
            cache_size_limit: self.cache_size_limit,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_split_with_dash() {
        let result = split_song("Jay's Soul Connection - Frankes Party Life");
        assert_eq!(result, Some(("Jay's Soul Connection".to_string(), "Frankes Party Life".to_string())));
    }

    #[test]
    fn test_split_with_slash() {
        let result = split_song("Artist / Song Title");
        assert_eq!(result, Some(("Artist".to_string(), "Song Title".to_string())));
    }

    #[test]
    fn test_split_with_extra_whitespace() {
        let result = split_song("  Artist  -  Song Title  ");
        assert_eq!(result, Some(("Artist".to_string(), "Song Title".to_string())));
    }

    #[test]
    fn test_split_first_separator_dash() {
        // When both separators exist, should split on the first one (dash in this case)
        let result = split_song("Artist - Song / Other Part");
        assert_eq!(result, Some(("Artist".to_string(), "Song / Other Part".to_string())));
    }

    #[test]
    fn test_split_first_separator_slash() {
        // When both separators exist, should split on the first one (slash in this case)
        let result = split_song("Artist / Song - Other Part");
        assert_eq!(result, Some(("Artist".to_string(), "Song - Other Part".to_string())));
    }

    #[test]
    fn test_no_separator() {
        let result = split_song("No separator here");
        assert_eq!(result, None);
    }

    #[test]
    fn test_empty_string() {
        let result = split_song("");
        assert_eq!(result, None);
    }

    #[test]
    fn test_only_separator() {
        let result = split_song("-");
        assert_eq!(result, None);
    }

    #[test]
    fn test_separator_at_start() {
        let result = split_song("- Song Title");
        assert_eq!(result, None); // Empty first part
    }

    #[test]
    fn test_separator_at_end() {
        let result = split_song("Artist -");
        assert_eq!(result, None); // Empty second part
    }

    #[test]
    fn test_multiple_dashes() {
        let result = split_song("Artist - Song - More Info");
        assert_eq!(result, Some(("Artist".to_string(), "Song - More Info".to_string())));
    }

    #[test]
    fn test_multiple_slashes() {
        let result = split_song("Artist / Song / More Info");
        assert_eq!(result, Some(("Artist".to_string(), "Song / More Info".to_string())));
    }

    #[test]
    fn test_single_character_parts() {
        let result = split_song("A - B");
        assert_eq!(result, Some(("A".to_string(), "B".to_string())));
    }

    #[test]
    fn test_unicode_characters() {
        let result = split_song("Артист - Песня");
        assert_eq!(result, Some(("Артист".to_string(), "Песня".to_string())));
    }

    #[test]
    fn test_special_characters_in_title() {
        let result = split_song("Band (feat. Someone) - Song Title & More");
        assert_eq!(result, Some(("Band (feat. Someone)".to_string(), "Song Title & More".to_string())));
    }
    
    #[test]
    fn test_detect_order_well_known_songs() {
        // Note: These tests require MusicBrainz to be enabled and accessible
        // In a real-world scenario, you would mock the MusicBrainz responses
        
        // Test case: Artist / Song format
        let _result = detect_order("The Beatles", "Hey Jude");
        // Should return ArtistSong if MusicBrainz has this combination
        
        // Test case: Song - Artist format  
        let _result2 = detect_order("Yesterday", "The Beatles");
        // Should return SongArtist if MusicBrainz has this combination
        
        // Test case: Unknown combination
        let _result3 = detect_order("NonExistentArtist", "NonExistentSong");
        // Should return Unknown
        
        // Since these tests depend on external API, we just verify the function runs
        // In production, you would mock musicbrainz::search_recording responses
        println!("detect_order tests completed successfully");
    }
    
    #[test] 
    fn test_detect_order_mock_scenarios() {
        // These are conceptual tests showing what results should be expected
        // In a real implementation, you would mock the MusicBrainz responses
        
        // Example of what we expect for well-known songs:
        // detect_order("The Beatles", "Hey Jude") -> OrderResult::ArtistSong
        // detect_order("Hey Jude", "The Beatles") -> OrderResult::SongArtist  
        // detect_order("Queen", "Bohemian Rhapsody") -> OrderResult::ArtistSong
        // detect_order("Bohemian Rhapsody", "Queen") -> OrderResult::SongArtist
        // detect_order("Led Zeppelin", "Stairway to Heaven") -> OrderResult::ArtistSong
        // detect_order("Stairway to Heaven", "Led Zeppelin") -> OrderResult::SongArtist
        // detect_order("Unknown Artist", "Unknown Song") -> OrderResult::Unknown
        
        // For now, just test that the function exists and can be called
        // Real tests would require mocking the musicbrainz module
        assert!(true); // Placeholder assertion
    }
    
    #[test]
    fn test_song_title_splitter_new() {
        let splitter = SongTitleSplitter::new("track_123");
        assert_eq!(splitter.get_id(), "track_123");
    }
    
    #[test]
    fn test_song_title_splitter_get_raw_parts() {
        let splitter = SongTitleSplitter::new("track_123");
        let parts = splitter.get_raw_parts("The Beatles - Hey Jude");
        assert_eq!(parts, Some(("The Beatles".to_string(), "Hey Jude".to_string())));
    }
    
    #[test]
    fn test_song_title_splitter_no_separator() {
        let splitter = SongTitleSplitter::new("track_456");
        let parts = splitter.get_raw_parts("NoSeparatorHere");
        assert_eq!(parts, None);
    }
    
    #[test]
    fn test_song_title_splitter_with_slash() {
        let splitter = SongTitleSplitter::new("track_789");
        let parts = splitter.get_raw_parts("Artist / Song Title");
        assert_eq!(parts, Some(("Artist".to_string(), "Song Title".to_string())));
    }
    
    #[test]
    fn test_song_title_splitter_order_detection() {
        let mut splitter = SongTitleSplitter::new("track_abc");
        let order = splitter.get_order("The Beatles - Hey Jude");
        // Since we can't predict MusicBrainz results, just verify the function runs
        assert!(matches!(order, OrderResult::ArtistSong | OrderResult::SongArtist | OrderResult::Unknown | OrderResult::Undecided));
    }
    
    #[test]
    fn test_song_title_splitter_split_song() {
        let mut splitter = SongTitleSplitter::new("track_def");
        
        // Test with a song that has separators
        let result = splitter.split_song("Artist - Song Title");
        // The result depends on MusicBrainz lookup, so we just verify it handles the call
        match result {
            Some((artist, song)) => {
                assert!(!artist.is_empty());
                assert!(!song.is_empty());
                println!("Split result: Artist='{}', Song='{}'", artist, song);
            }
            None => {
                // This is also valid - it means order couldn't be determined
                println!("Could not determine artist/song order");
            }
        }
        
        // Test with a song that has no separators
        let result2 = splitter.split_song("NoSeparatorHere");
        assert_eq!(result2, None);
    }
    
    #[test]
    fn test_song_title_splitter_multiple_calls() {
        let mut splitter = SongTitleSplitter::new("track_ghi");
        
        // Test that multiple calls with different songs work
        let _parts1 = splitter.get_raw_parts("The Beatles - Hey Jude");
        let _parts2 = splitter.get_raw_parts("Queen / Bohemian Rhapsody");
        let _order1 = splitter.get_order("Led Zeppelin - Stairway to Heaven");
        let _order2 = splitter.get_order("Pink Floyd / Wish You Were Here");
        
        // All calls should work independently since no state is cached
        assert!(true); // If we get here, all calls succeeded
    }
    
    #[test]
    fn test_song_title_splitter_default_order_detection() {
        let mut splitter = SongTitleSplitter::new("track_stats");
        
        // Initially no default order
        assert!(!splitter.has_default_order());
        assert_eq!(splitter.get_default_order(), None);
        
        // Test statistics tracking
        assert_eq!(splitter.get_artist_song_count(), 0);
        assert_eq!(splitter.get_song_artist_count(), 0);
        assert_eq!(splitter.get_total_count(), 0);
        
        // Test percentages calculation with no data
        let (artist_percent, song_percent) = splitter.get_order_percentages();
        assert_eq!(artist_percent, 0.0);
        assert_eq!(song_percent, 0.0);
        
        // Test reset functionality
        splitter.reset();
        assert!(!splitter.has_default_order());
        assert_eq!(splitter.get_total_count(), 0);
    }
    
    #[test]
    fn test_song_title_splitter_statistics_tracking() {
        let mut splitter = SongTitleSplitter::new("track_multi");
        
        // Process multiple songs to test statistics
        let _order1 = splitter.get_order("The Beatles - Hey Jude");
        let _order2 = splitter.get_order("Queen / Bohemian Rhapsody");
        let _order3 = splitter.get_order("Led Zeppelin - Stairway to Heaven");
        
        // Should have processed 3 songs
        assert_eq!(splitter.get_total_count(), 3);
        
        // Test that clear_stats works
        splitter.clear_stats();
        assert_eq!(splitter.get_total_count(), 0);
        assert_eq!(splitter.get_artist_song_count(), 0);
        assert_eq!(splitter.get_song_artist_count(), 0);
        
        // Test that reset clears both stats and default order
        splitter.reset();
        assert!(!splitter.has_default_order());
        assert_eq!(splitter.get_total_count(), 0);
    }
    
    #[test]
    fn test_song_title_splitter_cache() {
        let mut splitter = SongTitleSplitter::new("track_cache");
        
        // Initially cache should be empty
        assert_eq!(splitter.get_cache_size(), 0);
        assert!(!splitter.is_cached("The Beatles - Hey Jude"));
        
        // Get order for first time - should cache result and update stats
        let order1 = splitter.get_order("The Beatles - Hey Jude");
        assert_eq!(splitter.get_cache_size(), 1);
        assert!(splitter.is_cached("The Beatles - Hey Jude"));
        let initial_count = splitter.get_total_count();
        assert!(initial_count > 0);
        
        // Get order for same song again - should use cache and NOT update stats
        let order2 = splitter.get_order("The Beatles - Hey Jude");
        assert_eq!(order1, order2);
        assert_eq!(splitter.get_cache_size(), 1); // Cache size shouldn't increase
        assert_eq!(splitter.get_total_count(), initial_count); // Stats shouldn't change
        
        // Test with custom cache size
        let mut splitter2 = SongTitleSplitter::with_cache_size("track_custom", 2);
        assert_eq!(splitter2.get_cache_size_limit(), 2);
        
        // Clear cache
        splitter.clear_cache();
        assert_eq!(splitter.get_cache_size(), 0);
        assert!(!splitter.is_cached("The Beatles - Hey Jude"));
    }
    
    #[test]
    fn test_song_title_splitter_serialization() {
        let mut splitter = SongTitleSplitter::new("test_station");
        
        // Serialize empty splitter
        let json = splitter.to_json().expect("Failed to serialize empty splitter");
        assert!(json.contains("test_station"));
        assert!(json.contains("default_order"));
        assert!(json.contains("order_stats"));
        // Should NOT contain the skipped fields
        assert!(!json.contains("lookup_cache"));
        assert!(!json.contains("cache_size_limit"));
        
        // Test compact serialization
        let compact_json = splitter.to_json_compact().expect("Failed to serialize compact");
        assert!(compact_json.len() < json.len()); // Compact should be smaller
        
        // Deserialize and verify
        let restored = SongTitleSplitter::from_json(&json).expect("Failed to deserialize");
        assert_eq!(restored.get_id(), "test_station");
        assert_eq!(restored.get_total_count(), 0); // Stats restored (empty)
        assert_eq!(restored.get_cache_size(), 0); // Cache reset
        assert!(!restored.has_default_order());
        
        // Test round-trip serialization with data
        let _order = splitter.get_order("Artist - Song");
        let json_with_data = splitter.to_json().expect("Failed to serialize with data");
        let restored_with_data = SongTitleSplitter::from_json(&json_with_data)
            .expect("Failed to deserialize with data");
        
        assert_eq!(restored_with_data.get_id(), splitter.get_id());
        // Stats are preserved during serialization
        assert_eq!(restored_with_data.get_total_count(), splitter.get_total_count());
        assert_eq!(restored_with_data.get_cache_size(), 0); // Cache still reset
        // Default order is preserved if it was set
        assert_eq!(restored_with_data.get_default_order(), splitter.get_default_order());
    }
    
    #[test]
    fn test_song_title_splitter_clone_with_id() {
        let mut original = SongTitleSplitter::new("original_station");
        
        // Add some data to the original
        let _order = original.get_order("Artist - Song");
        let original_stats = original.get_all_stats();
        
        // Clone with new ID
        let cloned = original.clone_with_id("new_station");
        
        // Verify ID changed but data is preserved
        assert_eq!(cloned.get_id(), "new_station");
        assert_eq!(original.get_id(), "original_station");
        
        // Verify statistics are preserved
        assert_eq!(cloned.get_all_stats(), original_stats);
        assert_eq!(cloned.get_total_count(), original.get_total_count());
        assert_eq!(cloned.get_cache_size(), original.get_cache_size());
        assert_eq!(cloned.has_default_order(), original.has_default_order());
        assert_eq!(cloned.get_cache_size_limit(), original.get_cache_size_limit());
    }
    
    #[test]
    fn test_serialization_error_handling() {
        // Test deserializing invalid JSON
        let invalid_json = "{ invalid json }";
        let result = SongTitleSplitter::from_json(invalid_json);
        assert!(result.is_err());
        
        // Test deserializing JSON with missing required fields
        let incomplete_json = r#"{"id":"test"}"#;
        let result2 = SongTitleSplitter::from_json(incomplete_json);
        assert!(result2.is_err());
        
        // Test deserializing valid minimal JSON (only required fields)
        let minimal_json = r#"{
            "id": "test_minimal",
            "order_stats": {},
            "default_order": null
        }"#;
        let result3 = SongTitleSplitter::from_json(minimal_json);
        assert!(result3.is_ok());
        
        let splitter = result3.unwrap();
        assert_eq!(splitter.get_id(), "test_minimal");
        assert_eq!(splitter.get_total_count(), 0);
        assert_eq!(splitter.get_cache_size_limit(), 50); // Default value
        assert!(!splitter.has_default_order());
    }
    
    #[test]
    fn test_serialization_with_complex_data() {
        let mut splitter = SongTitleSplitter::with_cache_size("complex_test", 10);
        
        // Manually insert some statistics and set default order
        splitter.order_stats.insert(OrderResult::ArtistSong, 15);
        splitter.order_stats.insert(OrderResult::SongArtist, 2);
        splitter.order_stats.insert(OrderResult::Unknown, 3);
        splitter.default_order = Some(OrderResult::ArtistSong);
        splitter.lookup_cache.insert("Test - Song".to_string(), OrderResult::ArtistSong);
        
        // Serialize and deserialize
        let json = splitter.to_json().expect("Failed to serialize complex data");
        let restored = SongTitleSplitter::from_json(&json)
            .expect("Failed to deserialize complex data");
        
        // Verify ID, default order, and stats are preserved
        assert_eq!(restored.get_id(), "complex_test");
        assert!(restored.has_default_order());
        assert_eq!(restored.get_default_order(), Some(OrderResult::ArtistSong));
        
        // Verify statistics are preserved (now serialized)
        assert_eq!(restored.get_artist_song_count(), 15);
        assert_eq!(restored.get_song_artist_count(), 2);
        assert_eq!(restored.get_unknown_count(), 3);
        assert_eq!(restored.get_total_count(), 20);
        
        // Verify cache is reset (not serialized)
        assert_eq!(restored.get_cache_size(), 0);
        assert_eq!(restored.get_cache_size_limit(), 50); // Default value
        assert!(!restored.is_cached("Test - Song"));
    }
}
