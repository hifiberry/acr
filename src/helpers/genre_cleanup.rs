use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;
use serde::{Deserialize, Serialize};
use log::{debug, warn, error};
use once_cell::sync::Lazy;
use parking_lot::Mutex;

/// Configuration for genre cleanup
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct GenreConfig {
    #[serde(rename = "_comment")]
    pub comment: Option<String>,
    #[serde(rename = "_ignore")]
    pub ignore: Vec<String>,
    pub mappings: HashMap<String, String>,
}

/// Genre cleanup service that consolidates and normalizes genre tags
pub struct GenreCleanup {
    ignore_set: HashSet<String>,
    mapping_lowercase: HashMap<String, String>,
}

// Global instance
static GENRE_CLEANUP: Lazy<Mutex<Option<GenreCleanup>>> = Lazy::new(|| Mutex::new(None));

impl GenreCleanup {
    /// Create a new GenreCleanup instance from a config file
    pub fn from_config_file<P: AsRef<Path>>(config_path: P) -> Result<Self, Box<dyn std::error::Error>> {
        let config_content = fs::read_to_string(config_path.as_ref())
            .map_err(|e| format!("Failed to read genre config file: {}", e))?;
        
        let config: GenreConfig = serde_json::from_str(&config_content)
            .map_err(|e| format!("Failed to parse genre config JSON: {}", e))?;
        
        Self::from_config(config)
    }

    /// Create a new GenreCleanup instance from a config object
    pub fn from_config(config: GenreConfig) -> Result<Self, Box<dyn std::error::Error>> {
        // Create case-insensitive ignore set
        let ignore_set: HashSet<String> = config.ignore.iter()
            .map(|s| s.to_lowercase())
            .collect();
        
        // Create case-insensitive mapping
        let mapping_lowercase: HashMap<String, String> = config.mappings.iter()
            .map(|(k, v)| (k.to_lowercase(), v.clone()))
            .collect();
        
        debug!("Genre cleanup initialized with {} ignore entries and {} mappings", 
               ignore_set.len(), mapping_lowercase.len());
        
        Ok(GenreCleanup {
            ignore_set,
            mapping_lowercase,
        })
    }

    /// Clean up a single genre string
    pub fn clean_genre(&self, genre: &str) -> Option<String> {
        let genre_lower = genre.trim().to_lowercase();
        
        // Check if genre should be ignored
        if self.ignore_set.contains(&genre_lower) {
            debug!("Ignoring genre: {}", genre);
            return None;
        }
        
        // Check if there's a mapping for this genre
        if let Some(mapped_genre) = self.mapping_lowercase.get(&genre_lower) {
            debug!("Mapped genre '{}' to '{}'", genre, mapped_genre);
            return Some(mapped_genre.clone());
        }
        
        // Return the original genre if no mapping found
        Some(genre.trim().to_string())
    }

    /// Clean up a list of genres, removing duplicates and applying mappings
    pub fn clean_genres(&self, genres: Vec<String>) -> Vec<String> {
        let mut cleaned_genres = HashSet::new();
        
        for genre in genres {
            if let Some(cleaned) = self.clean_genre(&genre) {
                cleaned_genres.insert(cleaned);
            }
        }
        
        let mut result: Vec<String> = cleaned_genres.into_iter().collect();
        result.sort();
        result
    }

    /// Clean up genres from a slice of strings
    pub fn clean_genres_slice(&self, genres: &[String]) -> Vec<String> {
        self.clean_genres(genres.to_vec())
    }
}

/// Initialize the global genre cleanup instance
pub fn initialize_genre_cleanup() -> Result<(), Box<dyn std::error::Error>> {
    initialize_genre_cleanup_with_config(None)
}

/// Initialize the global genre cleanup instance with an optional configuration
pub fn initialize_genre_cleanup_with_config(config: Option<&serde_json::Value>) -> Result<(), Box<dyn std::error::Error>> {
    // First try to get config path from the provided configuration
    if let Some(config_value) = config {
        if let Some(genre_config) = crate::config::get_service_config(config_value, "genre_cleanup") {
            if let Some(config_path) = genre_config.get("config_path").and_then(|p| p.as_str()) {
                if Path::new(config_path).exists() {
                    match GenreCleanup::from_config_file(config_path) {
                        Ok(cleanup) => {
                            let mut global_cleanup = GENRE_CLEANUP.lock();
                            *global_cleanup = Some(cleanup);
                            debug!("Genre cleanup initialized from configured path: {}", config_path);
                            return Ok(());
                        }
                        Err(e) => {
                            warn!("Failed to load genre config from configured path {}: {}", config_path, e);
                        }
                    }
                }
            }
        }
    }
    
    // Fall back to default config paths
    let config_paths = [
        "/etc/audiocontrol/genres.json",
    ];
    
    for path in &config_paths {
        if Path::new(path).exists() {
            match GenreCleanup::from_config_file(path) {
                Ok(cleanup) => {
                    let mut global_cleanup = GENRE_CLEANUP.lock();
                    *global_cleanup = Some(cleanup);
                    debug!("Genre cleanup initialized from: {}", path);
                    return Ok(());
                }
                Err(e) => {
                    warn!("Failed to load genre config from {}: {}", path, e);
                }
            }
        }
    }
    
    warn!("No valid genre config file found in any of the configured or default locations");
    warn!("Genre cleanup not initialized - genres will be returned without cleanup");
    Err("Genre cleanup configuration not found".into())
}

/// Get the global genre cleanup instance
pub fn get_genre_cleanup() -> Result<parking_lot::MutexGuard<'static, Option<GenreCleanup>>, Box<dyn std::error::Error>> {
    Ok(GENRE_CLEANUP.lock())
}

/// Clean up genres using the global instance
pub fn clean_genres_global(genres: Vec<String>) -> Vec<String> {
    match get_genre_cleanup() {
        Ok(cleanup_guard) => {
            if let Some(ref cleanup) = *cleanup_guard {
                cleanup.clean_genres(genres)
            } else {
                // Remove duplicates at least
                let mut unique_genres: Vec<String> = genres.into_iter().collect::<HashSet<_>>().into_iter().collect();
                unique_genres.sort();
                unique_genres
            }
        }
        Err(e) => {
            error!("Failed to access genre cleanup: {}", e);
            // Fallback: just remove duplicates
            let mut unique_genres: Vec<String> = genres.into_iter().collect::<HashSet<_>>().into_iter().collect();
            unique_genres.sort();
            unique_genres
        }
    }
}

/// Clean up a single genre using the global instance
pub fn clean_genre_global(genre: &str) -> Option<String> {
    match get_genre_cleanup() {
        Ok(cleanup_guard) => {
            if let Some(ref cleanup) = *cleanup_guard {
                cleanup.clean_genre(genre)
            } else {
                Some(genre.trim().to_string())
            }
        }
        Err(e) => {
            error!("Failed to access genre cleanup: {}", e);
            Some(genre.trim().to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;
    use std::io::Write;

    #[test]
    fn test_genre_cleanup_basic() {
        let config = GenreConfig {
            comment: Some("Test config".to_string()),
            ignore: vec!["seen live".to_string(), "80s".to_string()],
            mappings: {
                let mut map = HashMap::new();
                map.insert("hip hop".to_string(), "hip-hop".to_string());
                map.insert("heavy metal".to_string(), "heavy metal".to_string());
                map.insert("thrash metal".to_string(), "thrash metal".to_string());
                map
            },
        };

        let cleanup = GenreCleanup::from_config(config).unwrap();

        // Test ignoring
        assert_eq!(cleanup.clean_genre("seen live"), None);
        assert_eq!(cleanup.clean_genre("80s"), None);

        // Test mapping
        assert_eq!(cleanup.clean_genre("hip hop"), Some("hip-hop".to_string()));
        assert_eq!(cleanup.clean_genre("Hip Hop"), Some("hip-hop".to_string()));

        // Test passthrough
        assert_eq!(cleanup.clean_genre("jazz"), Some("jazz".to_string()));
    }

    #[test]
    fn test_genre_cleanup_list() {
        let config = GenreConfig {
            comment: None,
            ignore: vec!["seen live".to_string()],
            mappings: {
                let mut map = HashMap::new();
                map.insert("hip hop".to_string(), "hip-hop".to_string());
                map.insert("rap".to_string(), "hip-hop".to_string());
                map
            },
        };

        let cleanup = GenreCleanup::from_config(config).unwrap();

        let input = vec![
            "hip hop".to_string(),
            "rap".to_string(),
            "jazz".to_string(),
            "seen live".to_string(),
            "hip hop".to_string(), // duplicate
        ];

        let result = cleanup.clean_genres(input);
        
        // Should have hip-hop, jazz (no duplicates, no ignored)
        assert_eq!(result.len(), 2);
        assert!(result.contains(&"hip-hop".to_string()));
        assert!(result.contains(&"jazz".to_string()));
    }

    #[test]
    fn test_config_from_file() {
        let config_json = r#"{
            "_comment": "Test config",
            "_ignore": ["seen live", "80s"],
            "mappings": {
                "hip hop": "hip-hop",
                "heavy metal": "heavy metal"
            }
        }"#;

        let mut temp_file = NamedTempFile::new().unwrap();
        temp_file.write_all(config_json.as_bytes()).unwrap();

        let cleanup = GenreCleanup::from_config_file(temp_file.path()).unwrap();
        
        assert_eq!(cleanup.clean_genre("seen live"), None);
        assert_eq!(cleanup.clean_genre("hip hop"), Some("hip-hop".to_string()));
    }
}
