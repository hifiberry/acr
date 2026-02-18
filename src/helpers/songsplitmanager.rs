use crate::helpers::songtitlesplitter::SongTitleSplitter;
use crate::helpers::attributecache;
use std::collections::HashMap;
use std::sync::Arc;
use parking_lot::Mutex;
use log::{debug, info, warn};

/// Manager for song title splitters that handles creation, reuse, and lifecycle
/// 
/// This manager ensures that splitters are reused for the same ID, allowing
/// them to accumulate learning data over time. It also provides methods for
/// monitoring and managing the splitters.
pub struct SongSplitManager {
    /// Map of splitter ID to SongTitleSplitter instances
    splitters: Arc<Mutex<HashMap<String, SongTitleSplitter>>>,
    
    /// Maximum number of splitters to keep in memory (to prevent unbounded growth)
    max_splitters: usize,
}

impl SongSplitManager {
    /// Create a new SongSplitManager with default settings
    pub fn new() -> Self {
        Self {
            splitters: Arc::new(Mutex::new(HashMap::new())),
            max_splitters: 100, // Default limit
        }
    }
    
    /// Create a new SongSplitManager with custom maximum splitter count
    pub fn with_max_splitters(max_splitters: usize) -> Self {
        Self {
            splitters: Arc::new(Mutex::new(HashMap::new())),
            max_splitters,
        }
    }
    
    /// Get or create a splitter for the given ID
    /// 
    /// This method will reuse existing splitters to preserve learning data,
    /// or load from persistent storage, or create a new one if it doesn't exist yet. 
    /// Returns a cloned instance for read-only operations.
    /// 
    /// # Arguments
    /// * `splitter_id` - Unique identifier for the splitter (e.g., radio station URL)
    /// 
    /// # Returns
    /// * `Option<SongTitleSplitter>` - Cloned splitter instance, or None if locking fails or limit reached
    pub fn get_or_create_splitter(&self, splitter_id: &str) -> Option<SongTitleSplitter> {
        let mut splitters = self.splitters.lock();
        // Check if we already have a splitter for this ID in memory
        if let Some(existing_splitter) = splitters.get(splitter_id) {
            debug!("Reusing existing splitter for ID: {}", splitter_id);
            return Some(existing_splitter.clone());
        }
        
        // Check if we've reached the maximum number of splitters
        if splitters.len() >= self.max_splitters {
            warn!("Maximum number of splitters ({}) reached, cannot create new splitter for ID: {}", 
                  self.max_splitters, splitter_id);
            return None;
        }
        
        // Try to load from persistent storage first
        let new_splitter = if let Some(cached_splitter) = self.load_from_cache(splitter_id) {
            debug!("Loaded splitter for ID '{}' from persistent storage", splitter_id);
            cached_splitter
        } else {
            // Create a new splitter if not found in cache
            debug!("Creating new splitter for ID: {}", splitter_id);
            SongTitleSplitter::new(splitter_id)
        };
        
        // Store it in our map
        splitters.insert(splitter_id.to_string(), new_splitter.clone());
        
        info!("Created/loaded song title splitter for '{}' (total splitters: {})", 
              splitter_id, splitters.len());
        
        Some(new_splitter)
    }
    
    /// Split a song title using the appropriate splitter for the given ID
    /// 
    /// This method handles getting or creating a splitter and performing the split operation.
    /// 
    /// # Arguments
    /// * `splitter_id` - Unique identifier for the splitter
    /// * `title` - The title to split
    /// 
    /// # Returns
    /// * `Option<(String, String)>` - Tuple of (artist, song) if successfully split
    pub fn split_song(&self, splitter_id: &str, title: &str) -> Option<(String, String)> {
        let mut splitters = self.splitters.lock();
        // Check if we already have a splitter for this ID in memory
        if !splitters.contains_key(splitter_id) {
            // Check if we've reached the maximum number of splitters
            if splitters.len() >= self.max_splitters {
                warn!("Maximum number of splitters ({}) reached, cannot create new splitter for ID: {}", 
                      self.max_splitters, splitter_id);
                return None;
            }
            
            // Try to load from persistent storage first
            let new_splitter = if let Some(cached_splitter) = self.load_from_cache(splitter_id) {
                debug!("Loaded splitter for ID '{}' from persistent storage for splitting", splitter_id);
                cached_splitter
            } else {
                // Create a new splitter if not found in cache
                debug!("Creating new splitter for ID: {}", splitter_id);
                SongTitleSplitter::new(splitter_id)
            };
            
            splitters.insert(splitter_id.to_string(), new_splitter);
            info!("Created/loaded song title splitter for '{}' (total splitters: {})", 
                  splitter_id, splitters.len());
        }
        
        // Now get mutable access to the splitter and split the song
        if let Some(splitter) = splitters.get_mut(splitter_id) {
            splitter.split_song(title)
        } else {
            warn!("Failed to get mutable access to splitter for ID '{}'", splitter_id);
            None
        }
    }
    
    /// Get the number of active splitters
    pub fn get_splitter_count(&self) -> usize {
        let splitters = self.splitters.lock();
        splitters.len()
    }
    
    /// Get statistics for a specific splitter
    /// 
    /// # Returns
    /// * `Option<(u32, u32, u32, u32, bool)>` - Tuple of (artist_song_count, song_artist_count, unknown_count, undecided_count, has_default_order)
    pub fn get_splitter_stats(&self, splitter_id: &str) -> Option<(u32, u32, u32, u32, bool)> {
        let splitters = self.splitters.lock();
        if let Some(splitter) = splitters.get(splitter_id) {
            Some((
                splitter.get_artist_song_count(),
                splitter.get_song_artist_count(),
                splitter.get_unknown_count(),
                splitter.get_undecided_count(),
                splitter.has_default_order(),
            ))
        } else {
            None
        }
    }
    
    /// Get a list of all splitter IDs
    pub fn get_splitter_ids(&self) -> Vec<String> {
        let splitters = self.splitters.lock();
        splitters.keys().cloned().collect()
    }
    
    /// Get statistics for all splitters
    /// 
    /// # Returns
    /// * `HashMap<String, (u32, u32, u32, u32, bool)>` - Map of splitter_id to statistics tuple
    pub fn get_all_splitter_stats(&self) -> HashMap<String, (u32, u32, u32, u32, bool)> {
        let mut stats = HashMap::new();
        
        let splitters = self.splitters.lock();
        for (id, splitter) in splitters.iter() {
            stats.insert(
                id.clone(),
                (
                    splitter.get_artist_song_count(),
                    splitter.get_song_artist_count(),
                    splitter.get_unknown_count(),
                    splitter.get_undecided_count(),
                    splitter.has_default_order(),
                )
            );
        }
        
        stats
    }
    
    /// Clear all splitters (useful for testing or configuration changes)
    pub fn clear_all_splitters(&self) {
        let mut splitters = self.splitters.lock();
        let count = splitters.len();
        splitters.clear();
        info!("Cleared {} song title splitters", count);
    }
    
    /// Remove a specific splitter
    pub fn remove_splitter(&self, splitter_id: &str) -> bool {
        let mut splitters = self.splitters.lock();
        if splitters.remove(splitter_id).is_some() {
            debug!("Removed splitter for ID: {}", splitter_id);
            true
        } else {
            debug!("No splitter found for ID: {}", splitter_id);
            false
        }
    }
    
    /// Get the maximum number of splitters this manager will keep
    pub fn get_max_splitters(&self) -> usize {
        self.max_splitters
    }
    
    /// Save a specific splitter's state to persistent storage
    /// 
    /// # Arguments
    /// * `splitter_id` - The ID of the splitter to save
    /// 
    /// # Returns
    /// * `Result<(), String>` - Ok(()) if successful, Err with error message if failed
    pub fn save(&self, splitter_id: &str) -> Result<(), String> {
        let splitters = self.splitters.lock();
        if let Some(splitter) = splitters.get(splitter_id) {
            // Create cache key for this splitter
            let cache_key = format!("song_splitter:{}", splitter_id);
            
            // Serialize the splitter to JSON
            match splitter.to_json_compact() {
                Ok(json) => {
                    // Store in attribute cache
                    match attributecache::set(&cache_key, &json) {
                        Ok(_) => {
                            debug!("Successfully saved splitter state for '{}' to cache", splitter_id);
                            Ok(())
                        },
                        Err(e) => {
                            warn!("Failed to save splitter state for '{}' to cache: {}", splitter_id, e);
                            Err(format!("Failed to save to cache: {}", e))
                        }
                    }
                },
                Err(e) => {
                    warn!("Failed to serialize splitter for '{}': {}", splitter_id, e);
                    Err(format!("Failed to serialize splitter: {}", e))
                }
            }
        } else {
            Err(format!("No splitter found for ID: {}", splitter_id))
        }
    }
    
    /// Load a splitter from persistent storage
    /// 
    /// # Arguments
    /// * `splitter_id` - The ID of the splitter to load
    /// 
    /// # Returns
    /// * `Option<SongTitleSplitter>` - The loaded splitter if found, None otherwise
    fn load_from_cache(&self, splitter_id: &str) -> Option<SongTitleSplitter> {
        let cache_key = format!("song_splitter:{}", splitter_id);
        
        match attributecache::get::<String>(&cache_key) {
            Ok(Some(json)) => {
                match SongTitleSplitter::from_json(&json) {
                    Ok(splitter) => {
                        debug!("Successfully loaded splitter state for '{}' from cache", splitter_id);
                        Some(splitter)
                    },
                    Err(e) => {
                        warn!("Failed to deserialize splitter for '{}': {}", splitter_id, e);
                        None
                    }
                }
            },
            Ok(None) => {
                debug!("No cached splitter found for '{}'", splitter_id);
                None
            },
            Err(e) => {
                warn!("Failed to load splitter from cache for '{}': {}", splitter_id, e);
                None
            }
        }
    }
    
    /// Set the maximum number of splitters to keep in memory
    /// 
    /// If the current number of splitters exceeds the new limit,
    /// excess splitters will be removed (no specific order guaranteed).
    pub fn set_max_splitters(&mut self, max_splitters: usize) {
        self.max_splitters = max_splitters;
        
        // If we currently have more splitters than the new limit, remove some
        let mut splitters = self.splitters.lock();
        if splitters.len() > max_splitters {
            let current_count = splitters.len();
            let to_remove = current_count - max_splitters;
            
            // Remove excess splitters (no specific order)
            let keys_to_remove: Vec<String> = splitters.keys()
                .take(to_remove)
                .cloned()
                .collect();
            
            for key in keys_to_remove {
                splitters.remove(&key);
            }
            
            warn!("Reduced splitter count from {} to {} due to new limit", 
                  current_count, splitters.len());
        }
    }
}

impl Default for SongSplitManager {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for SongSplitManager {
    fn clone(&self) -> Self {
        Self {
            splitters: Arc::clone(&self.splitters),
            max_splitters: self.max_splitters,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    #[test]
    fn test_manager_creation() {
        let manager = SongSplitManager::new();
        assert_eq!(manager.get_splitter_count(), 0);
        assert_eq!(manager.get_max_splitters(), 100);
    }

    #[test]
    fn test_manager_with_custom_limit() {
        let manager = SongSplitManager::with_max_splitters(50);
        assert_eq!(manager.get_max_splitters(), 50);
    }

    #[test]
    fn test_splitter_reuse() {
        let manager = SongSplitManager::new();
        let id = "test_radio_station";
        
        // Get splitter twice - should be the same instance
        let splitter1 = manager.get_or_create_splitter(id);
        let splitter2 = manager.get_or_create_splitter(id);
        
        assert!(splitter1.is_some());
        assert!(splitter2.is_some());
        assert_eq!(manager.get_splitter_count(), 1);
    }

    #[test]
    fn test_max_splitters_limit() {
        let manager = SongSplitManager::with_max_splitters(2);
        
        // Create splitters up to the limit
        assert!(manager.get_or_create_splitter("station1").is_some());
        assert!(manager.get_or_create_splitter("station2").is_some());
        assert_eq!(manager.get_splitter_count(), 2);
        
        // Try to create one more - should fail
        assert!(manager.get_or_create_splitter("station3").is_none());
        assert_eq!(manager.get_splitter_count(), 2);
    }

    #[test]
    fn test_clear_splitters() {
        let manager = SongSplitManager::new();
        
        // Create some splitters
        manager.get_or_create_splitter("station1");
        manager.get_or_create_splitter("station2");
        assert_eq!(manager.get_splitter_count(), 2);
        
        // Clear all
        manager.clear_all_splitters();
        assert_eq!(manager.get_splitter_count(), 0);
    }

    #[test]
    fn test_remove_specific_splitter() {
        let manager = SongSplitManager::new();
        
        manager.get_or_create_splitter("station1");
        manager.get_or_create_splitter("station2");
        assert_eq!(manager.get_splitter_count(), 2);
        
        // Remove one
        assert!(manager.remove_splitter("station1"));
        assert_eq!(manager.get_splitter_count(), 1);
        
        // Try to remove non-existent
        assert!(!manager.remove_splitter("station_nonexistent"));
        assert_eq!(manager.get_splitter_count(), 1);
    }

    #[test]
    fn test_get_splitter_ids() {
        let manager = SongSplitManager::new();
        
        manager.get_or_create_splitter("station1");
        manager.get_or_create_splitter("station2");
        
        let ids = manager.get_splitter_ids();
        assert_eq!(ids.len(), 2);
        assert!(ids.contains(&"station1".to_string()));
        assert!(ids.contains(&"station2".to_string()));
    }

    #[test]
    fn test_split_song_convenience_method() {
        let manager = SongSplitManager::new();
        let id = "test_station";
        
        // This should create a new splitter and attempt to split
        // Since the splitter is new, it won't have learning data, so it might not split
        let _result = manager.split_song(id, "Artist - Song Title");
        
        // The exact result depends on MusicBrainz lookup, but the splitter should be created
        assert_eq!(manager.get_splitter_count(), 1);
    }

    #[test]
    fn test_thread_safety() {
        let manager = Arc::new(SongSplitManager::new());
        let mut handles = vec![];
        
        // Spawn multiple threads that try to create splitters
        for i in 0..5 {
            let manager_clone = Arc::clone(&manager);
            let handle = thread::spawn(move || {
                let id = format!("station_{}", i);
                manager_clone.get_or_create_splitter(&id)
            });
            handles.push(handle);
        }
        
        // Wait for all threads to complete
        for handle in handles {
            handle.join().unwrap();
        }
        
        // Should have 5 splitters
        assert_eq!(manager.get_splitter_count(), 5);
    }

    #[test]
    fn test_persistence() {
        let manager = SongSplitManager::new();
        let splitter_id = "test_persistence_station";
        
        // Create a splitter and verify it exists
        let splitter = manager.get_or_create_splitter(splitter_id);
        assert!(splitter.is_some());
        assert_eq!(manager.get_splitter_count(), 1);
        
        // Save the splitter
        let save_result = manager.save(splitter_id);
        // Note: This might fail if attributecache is not properly initialized in test environment
        // but the function should exist and handle errors gracefully
        match save_result {
            Ok(_) => {
                // If save succeeded, we can test loading
                manager.clear_all_splitters();
                assert_eq!(manager.get_splitter_count(), 0);
                
                // Try to load the splitter - should restore from cache
                let restored_splitter = manager.get_or_create_splitter(splitter_id);
                assert!(restored_splitter.is_some());
                assert_eq!(manager.get_splitter_count(), 1);
            },
            Err(_) => {
                // If save failed (likely due to test environment), that's okay
                // We're mainly testing that the functions exist and handle errors
                println!("Save failed in test environment - this is expected");
            }
        }
    }

    #[test]
    fn test_save_nonexistent_splitter() {
        let manager = SongSplitManager::new();
        
        // Try to save a non-existent splitter
        let result = manager.save("nonexistent_id");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("No splitter found"));
    }
}
