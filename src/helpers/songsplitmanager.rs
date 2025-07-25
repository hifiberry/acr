use crate::helpers::songtitlesplitter::SongTitleSplitter;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
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
    /// or create a new one if it doesn't exist yet. Returns a cloned instance
    /// for read-only operations.
    /// 
    /// # Arguments
    /// * `splitter_id` - Unique identifier for the splitter (e.g., radio station URL)
    /// 
    /// # Returns
    /// * `Option<SongTitleSplitter>` - Cloned splitter instance, or None if locking fails or limit reached
    pub fn get_or_create_splitter(&self, splitter_id: &str) -> Option<SongTitleSplitter> {
        if let Ok(mut splitters) = self.splitters.lock() {
            // Check if we already have a splitter for this ID
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
            
            // Create a new splitter
            debug!("Creating new splitter for ID: {}", splitter_id);
            let new_splitter = SongTitleSplitter::new(splitter_id);
            
            // Store it in our map
            splitters.insert(splitter_id.to_string(), new_splitter.clone());
            
            info!("Created new song title splitter for '{}' (total splitters: {})", 
                  splitter_id, splitters.len());
            
            Some(new_splitter)
        } else {
            warn!("Failed to acquire lock on splitters map");
            None
        }
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
        if let Ok(mut splitters) = self.splitters.lock() {
            // Check if we already have a splitter for this ID
            if !splitters.contains_key(splitter_id) {
                // Check if we've reached the maximum number of splitters
                if splitters.len() >= self.max_splitters {
                    warn!("Maximum number of splitters ({}) reached, cannot create new splitter for ID: {}", 
                          self.max_splitters, splitter_id);
                    return None;
                }
                
                // Create a new splitter
                debug!("Creating new splitter for ID: {}", splitter_id);
                let new_splitter = SongTitleSplitter::new(splitter_id);
                splitters.insert(splitter_id.to_string(), new_splitter);
                info!("Created new song title splitter for '{}' (total splitters: {})", 
                      splitter_id, splitters.len());
            }
            
            // Now get mutable access to the splitter and split the song
            if let Some(splitter) = splitters.get_mut(splitter_id) {
                splitter.split_song(title)
            } else {
                warn!("Failed to get mutable access to splitter for ID '{}'", splitter_id);
                None
            }
        } else {
            warn!("Failed to acquire lock on splitters map for splitting title '{}'", title);
            None
        }
    }
    
    /// Get the number of active splitters
    pub fn get_splitter_count(&self) -> usize {
        if let Ok(splitters) = self.splitters.lock() {
            splitters.len()
        } else {
            0
        }
    }
    
    /// Get statistics for a specific splitter
    /// 
    /// # Returns
    /// * `Option<(u32, u32, u32, u32, bool)>` - Tuple of (artist_song_count, song_artist_count, unknown_count, undecided_count, has_default_order)
    pub fn get_splitter_stats(&self, splitter_id: &str) -> Option<(u32, u32, u32, u32, bool)> {
        if let Ok(splitters) = self.splitters.lock() {
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
        } else {
            None
        }
    }
    
    /// Get a list of all splitter IDs
    pub fn get_splitter_ids(&self) -> Vec<String> {
        if let Ok(splitters) = self.splitters.lock() {
            splitters.keys().cloned().collect()
        } else {
            Vec::new()
        }
    }
    
    /// Get statistics for all splitters
    /// 
    /// # Returns
    /// * `HashMap<String, (u32, u32, u32, u32, bool)>` - Map of splitter_id to statistics tuple
    pub fn get_all_splitter_stats(&self) -> HashMap<String, (u32, u32, u32, u32, bool)> {
        let mut stats = HashMap::new();
        
        if let Ok(splitters) = self.splitters.lock() {
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
        }
        
        stats
    }
    
    /// Clear all splitters (useful for testing or configuration changes)
    pub fn clear_all_splitters(&self) {
        if let Ok(mut splitters) = self.splitters.lock() {
            let count = splitters.len();
            splitters.clear();
            info!("Cleared {} song title splitters", count);
        } else {
            warn!("Failed to acquire lock for clearing splitters");
        }
    }
    
    /// Remove a specific splitter
    pub fn remove_splitter(&self, splitter_id: &str) -> bool {
        if let Ok(mut splitters) = self.splitters.lock() {
            if splitters.remove(splitter_id).is_some() {
                debug!("Removed splitter for ID: {}", splitter_id);
                true
            } else {
                debug!("No splitter found for ID: {}", splitter_id);
                false
            }
        } else {
            warn!("Failed to acquire lock for removing splitter");
            false
        }
    }
    
    /// Get the maximum number of splitters this manager will keep
    pub fn get_max_splitters(&self) -> usize {
        self.max_splitters
    }
    
    /// Set the maximum number of splitters to keep in memory
    /// 
    /// If the current number of splitters exceeds the new limit,
    /// excess splitters will be removed (no specific order guaranteed).
    pub fn set_max_splitters(&mut self, max_splitters: usize) {
        self.max_splitters = max_splitters;
        
        // If we currently have more splitters than the new limit, remove some
        if let Ok(mut splitters) = self.splitters.lock() {
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
    use std::time::Duration;

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
        let mut manager = SongSplitManager::with_max_splitters(2);
        
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
        let result = manager.split_song(id, "Artist - Song Title");
        
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
}
