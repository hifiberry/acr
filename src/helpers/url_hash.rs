/*!
 * URL hash mapping system for shortening long URLs
 * 
 * This module provides functionality to create short hash-based identifiers
 * for long URLs, particularly useful for MPD library image URLs that can
 * be very long due to URL-encoded file paths.
 */

use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::hash::{Hash, Hasher};
use std::collections::hash_map::DefaultHasher;
use log::{debug, warn};

/// URL hash mapper that maintains bidirectional mapping between hashes and URLs
#[derive(Clone)]
pub struct UrlHashMapper {
    /// Map from hash to original URL
    hash_to_url: Arc<RwLock<HashMap<String, String>>>,
    /// Map from URL to hash for efficient lookups
    url_to_hash: Arc<RwLock<HashMap<String, String>>>,
}

impl UrlHashMapper {
    /// Create a new URL hash mapper
    pub fn new() -> Self {
        Self {
            hash_to_url: Arc::new(RwLock::new(HashMap::new())),
            url_to_hash: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Generate a 64-bit hex hash for a given URL
    /// Returns a 16-character hexadecimal string
    fn generate_hash(url: &str) -> String {
        let mut hasher = DefaultHasher::new();
        url.hash(&mut hasher);
        format!("{:016x}", hasher.finish())
    }

    /// Get or create a hash for a URL
    /// If the URL already has a hash, return it. Otherwise, create a new one.
    pub fn get_or_create_hash(&self, url: &str) -> Result<String, String> {
        // First check if we already have a hash for this URL
        if let Ok(url_to_hash) = self.url_to_hash.read() {
            if let Some(existing_hash) = url_to_hash.get(url) {
                debug!("Found existing hash '{}' for URL: {}", existing_hash, url);
                return Ok(existing_hash.clone());
            }
        } else {
            return Err("Failed to acquire read lock on url_to_hash".to_string());
        }

        // Generate a new hash
        let hash = Self::generate_hash(url);
        debug!("Generated new hash '{}' for URL: {}", hash, url);

        // Store the mapping in both directions
        match (self.hash_to_url.write(), self.url_to_hash.write()) {
            (Ok(mut hash_to_url), Ok(mut url_to_hash)) => {
                // Check for hash collision (very unlikely with 64-bit hash)
                if hash_to_url.contains_key(&hash) {
                    warn!("Hash collision detected for hash '{}'. This is extremely unlikely!", hash);
                    return Err(format!("Hash collision for hash '{}'", hash));
                }

                hash_to_url.insert(hash.clone(), url.to_string());
                url_to_hash.insert(url.to_string(), hash.clone());
                
                debug!("Stored bidirectional mapping for hash '{}'", hash);
                Ok(hash)
            }
            _ => Err("Failed to acquire write locks for hash storage".to_string())
        }
    }

    /// Resolve a hash back to the original URL
    pub fn resolve_hash(&self, hash: &str) -> Option<String> {
        if let Ok(hash_to_url) = self.hash_to_url.read() {
            let result = hash_to_url.get(hash).cloned();
            if result.is_some() {
                debug!("Resolved hash '{}' to URL", hash);
            } else {
                debug!("Hash '{}' not found in mapping", hash);
            }
            result
        } else {
            warn!("Failed to acquire read lock on hash_to_url");
            None
        }
    }

    /// Get statistics about the hash mapper
    pub fn get_stats(&self) -> HashMap<String, usize> {
        let mut stats = HashMap::new();
        
        if let Ok(hash_to_url) = self.hash_to_url.read() {
            stats.insert("total_mappings".to_string(), hash_to_url.len());
        } else {
            stats.insert("total_mappings".to_string(), 0);
        }
        
        stats
    }

    /// Clear all mappings (useful for testing)
    pub fn clear(&self) {
        if let (Ok(mut hash_to_url), Ok(mut url_to_hash)) = 
            (self.hash_to_url.write(), self.url_to_hash.write()) {
            hash_to_url.clear();
            url_to_hash.clear();
            debug!("Cleared all URL hash mappings");
        } else {
            warn!("Failed to acquire write locks for clearing hash mappings");
        }
    }

    /// Check if a string looks like one of our hashes (16 hex characters)
    pub fn is_hash_format(s: &str) -> bool {
        s.len() == 16 && s.chars().all(|c| c.is_ascii_hexdigit())
    }
}

impl Default for UrlHashMapper {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_generation() {
        let url1 = "music/test/file.flac";
        let url2 = "music/test/different.flac";
        
        let hash1 = UrlHashMapper::generate_hash(url1);
        let hash2 = UrlHashMapper::generate_hash(url2);
        
        // Hashes should be 16 characters long
        assert_eq!(hash1.len(), 16);
        assert_eq!(hash2.len(), 16);
        
        // Different URLs should produce different hashes
        assert_ne!(hash1, hash2);
        
        // Same URL should produce same hash
        let hash1_again = UrlHashMapper::generate_hash(url1);
        assert_eq!(hash1, hash1_again);
    }

    #[test]
    fn test_hash_format_detection() {
        // Valid hash format
        assert!(UrlHashMapper::is_hash_format("0123456789abcdef"));
        assert!(UrlHashMapper::is_hash_format("fedcba9876543210"));
        
        // Invalid formats
        assert!(!UrlHashMapper::is_hash_format("0123456789abcdeg")); // invalid character
        assert!(!UrlHashMapper::is_hash_format("0123456789abcde")); // too short
        assert!(!UrlHashMapper::is_hash_format("0123456789abcdef0")); // too long
        assert!(!UrlHashMapper::is_hash_format("music/file.flac")); // not hex
    }

    #[test]
    fn test_url_mapping() {
        let mapper = UrlHashMapper::new();
        let url = "music/Andrea%20Farri%2C%20Woodkid/test.flac";
        
        // Get hash for URL
        let hash = mapper.get_or_create_hash(url).expect("Should create hash");
        assert_eq!(hash.len(), 16);
        
        // Should get same hash for same URL
        let hash2 = mapper.get_or_create_hash(url).expect("Should return existing hash");
        assert_eq!(hash, hash2);
        
        // Should be able to resolve hash back to URL
        let resolved = mapper.resolve_hash(&hash).expect("Should resolve hash");
        assert_eq!(resolved, url);
        
        // Non-existent hash should return None
        assert!(mapper.resolve_hash("0000000000000000").is_none());
    }

    #[test]
    fn test_stats() {
        let mapper = UrlHashMapper::new();
        
        // Initially empty
        let stats = mapper.get_stats();
        assert_eq!(stats.get("total_mappings"), Some(&0));
        
        // Add some mappings
        mapper.get_or_create_hash("url1").unwrap();
        mapper.get_or_create_hash("url2").unwrap();
        
        let stats = mapper.get_stats();
        assert_eq!(stats.get("total_mappings"), Some(&2));
    }

    #[test]
    fn test_clear() {
        let mapper = UrlHashMapper::new();
        
        // Add mapping
        let hash = mapper.get_or_create_hash("test_url").unwrap();
        assert!(mapper.resolve_hash(&hash).is_some());
        
        // Clear mappings
        mapper.clear();
        
        // Should be empty now
        assert!(mapper.resolve_hash(&hash).is_none());
        let stats = mapper.get_stats();
        assert_eq!(stats.get("total_mappings"), Some(&0));
    }
}
