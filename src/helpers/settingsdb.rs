use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::collections::HashMap;
use lazy_static::lazy_static;
use log::{info, error};
use serde::{Serialize, Deserialize};
use std::sync::Arc;

// Global singleton for the settings database
lazy_static! {
    static ref SETTINGS_DB: Mutex<SettingsDb> = Mutex::new(SettingsDb::new());
}

/// A persistent settings database that stores user settings as key-value pairs using Sled database
pub struct SettingsDb {
    /// Path to the database directory
    db_path: PathBuf,
    /// Sled database instance
    db: Option<sled::Db>,
    /// Whether the database is enabled
    enabled: bool,
    /// In-memory cache of recently accessed settings
    memory_cache: HashMap<String, Arc<Vec<u8>>>,
}

impl SettingsDb {
    /// Create a new settings database with default settings
    pub fn new() -> Self {
        // Using the default path
        let db_dir = PathBuf::from("/var/lib/audiocontrol/db");
        Self::with_directory(db_dir)
    }

    /// Create a new settings database with a specific directory
    pub fn with_directory<P: AsRef<Path>>(dir: P) -> Self {
        let db_path = dir.as_ref().to_path_buf();
        
        // Try to open the sled database
        let db = match sled::open(&db_path) {
            Ok(db) => {
                info!("Successfully opened settings database at {:?}", db_path);
                Some(db)
            },
            Err(e) => {
                error!("Failed to open sled database at {:?}: {}", db_path, e);
                None
            }
        };

        SettingsDb {
            db_path,
            db,
            enabled: true,
            memory_cache: HashMap::new(),
        }
    }

    /// Initialize the global settings database with a custom directory
    pub fn initialize_global<P: AsRef<Path>>(dir: P) -> Result<(), String> {
        match get_settings_db().reconfigure_with_directory(dir) {
            Ok(_) => {
                info!("Global settings database initialized with custom directory");
                Ok(())
            },
            Err(e) => {
                error!("Failed to initialize global settings database: {}", e);
                Err(e)
            }
        }
    }
    
    /// Initialize the global settings database with a custom directory path as string
    pub fn initialize<P: AsRef<Path>>(path: P) -> Result<(), String> {
        Self::initialize_global(path)
    }

    /// Reconfigure the settings database with a new directory
    /// This will close the existing database and open a new one
    fn reconfigure_with_directory<P: AsRef<Path>>(&mut self, dir: P) -> Result<(), String> {
        let db_path = dir.as_ref().to_path_buf();
        
        // Try to ensure the directory exists
        if let Err(e) = std::fs::create_dir_all(&db_path) {
            return Err(format!("Failed to create directory for settings database: {}", e));
        }
        
        // Try to open the new sled database
        let db = match sled::open(&db_path) {
            Ok(db) => {
                info!("Successfully opened settings database at {:?}", db_path);
                Some(db)
            },
            Err(e) => {
                error!("Failed to open sled database at {:?}: {}", db_path, e);
                return Err(format!("Failed to open sled database: {}", e));
            }
        };
        
        // Update the instance
        self.db_path = db_path;
        self.db = db;
        self.memory_cache.clear(); // Clear memory cache as we have a new DB
        
        Ok(())
    }

    /// Enable or disable the database
    pub fn enable(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    /// Check if the database is enabled
    pub fn is_enabled(&self) -> bool {
        self.enabled && self.db.is_some()
    }

    /// Store a serializable value in the settings database
    pub fn set<T: Serialize>(&mut self, key: &str, value: &T) -> Result<(), String> {
        if !self.is_enabled() {
            return Err("Settings database is disabled".to_string());
        }

        let serialized = match serde_json::to_vec(value) {
            Ok(data) => data,
            Err(e) => return Err(format!("Failed to serialize value: {}", e)),
        };

        // Store in memory cache
        self.memory_cache.insert(key.to_string(), Arc::new(serialized.clone()));

        // Store in sled database
        match &self.db {
            Some(db) => {
                if let Err(e) = db.insert(key.as_bytes(), serialized) {
                    return Err(format!("Failed to store in database: {}", e));
                }
                
                // Flush to ensure persistence
                if let Err(e) = db.flush() {
                    return Err(format!("Failed to flush database: {}", e));
                }
                
                Ok(())
            },
            None => Err("Database not available".to_string()),
        }
    }

    /// Store a string value in the settings database
    pub fn set_string(&mut self, key: &str, value: &str) -> Result<(), String> {
        self.set(key, &value.to_string())
    }

    /// Store an integer value in the settings database
    pub fn set_int(&mut self, key: &str, value: i64) -> Result<(), String> {
        self.set(key, &value)
    }

    /// Store a boolean value in the settings database
    pub fn set_bool(&mut self, key: &str, value: bool) -> Result<(), String> {
        self.set(key, &value)
    }

    /// Get a value from the settings database and deserialize it
    pub fn get<T: for<'de> Deserialize<'de>>(&mut self, key: &str) -> Result<Option<T>, String> {
        if !self.is_enabled() {
            return Err("Settings database is disabled".to_string());
        }

        // Try memory cache first
        if let Some(data) = self.memory_cache.get(key) {
            return match serde_json::from_slice(&data) {
                Ok(value) => Ok(Some(value)),
                Err(e) => Err(format!("Failed to deserialize from memory cache: {}", e)),
            };
        }

        // Fall back to sled database
        match &self.db {
            Some(db) => {
                match db.get(key.as_bytes()) {
                    Ok(Some(data)) => {
                        // Store in memory cache for future access
                        let data_vec = data.to_vec();
                        let result: T = match serde_json::from_slice(&data_vec) {
                            Ok(value) => value,
                            Err(e) => return Err(format!("Failed to deserialize from database: {}", e)),
                        };
                        
                        self.memory_cache.insert(key.to_string(), Arc::new(data_vec));
                        Ok(Some(result))
                    },
                    Ok(None) => Ok(None),
                    Err(e) => Err(format!("Database error: {}", e)),
                }
            },
            None => Err("Database not available".to_string()),
        }
    }

    /// Get a string value from the settings database
    pub fn get_string(&mut self, key: &str) -> Result<Option<String>, String> {
        self.get::<String>(key)
    }

    /// Get an integer value from the settings database
    pub fn get_int(&mut self, key: &str) -> Result<Option<i64>, String> {
        self.get::<i64>(key)
    }

    /// Get a boolean value from the settings database
    pub fn get_bool(&mut self, key: &str) -> Result<Option<bool>, String> {
        self.get::<bool>(key)
    }

    /// Get a string value from the settings database with a default value
    pub fn get_string_with_default(&mut self, key: &str, default: &str) -> Result<String, String> {
        match self.get_string(key)? {
            Some(value) => Ok(value),
            None => Ok(default.to_string()),
        }
    }

    /// Get an integer value from the settings database with a default value
    pub fn get_int_with_default(&mut self, key: &str, default: i64) -> Result<i64, String> {
        match self.get_int(key)? {
            Some(value) => Ok(value),
            None => Ok(default),
        }
    }

    /// Get a boolean value from the settings database with a default value
    pub fn get_bool_with_default(&mut self, key: &str, default: bool) -> Result<bool, String> {
        match self.get_bool(key)? {
            Some(value) => Ok(value),
            None => Ok(default),
        }
    }

    /// Remove a setting from the database
    pub fn remove(&mut self, key: &str) -> Result<bool, String> {
        if !self.is_enabled() {
            return Err("Settings database is disabled".to_string());
        }

        // Remove from memory cache
        self.memory_cache.remove(key);

        // Remove from database
        match &self.db {
            Some(db) => {
                match db.remove(key.as_bytes()) {
                    Ok(Some(_)) => Ok(true),
                    Ok(None) => Ok(false),
                    Err(e) => Err(format!("Failed to remove from database: {}", e)),
                }
            },
            None => Err("Database not available".to_string()),
        }
    }

    /// Check if a key exists in the settings database
    pub fn contains_key(&mut self, key: &str) -> Result<bool, String> {
        if !self.is_enabled() {
            return Err("Settings database is disabled".to_string());
        }

        // Check memory cache first
        if self.memory_cache.contains_key(key) {
            return Ok(true);
        }

        // Check database
        match &self.db {
            Some(db) => {
                match db.contains_key(key.as_bytes()) {
                    Ok(exists) => Ok(exists),
                    Err(e) => Err(format!("Database error: {}", e)),
                }
            },
            None => Err("Database not available".to_string()),
        }
    }

    /// Get all keys from the settings database
    pub fn get_all_keys(&self) -> Result<Vec<String>, String> {
        if !self.is_enabled() {
            return Err("Settings database is disabled".to_string());
        }

        match &self.db {
            Some(db) => {
                let mut keys = Vec::new();
                for item in db.iter() {
                    match item {
                        Ok((key, _)) => {
                            match String::from_utf8(key.to_vec()) {
                                Ok(key_str) => keys.push(key_str),
                                Err(_) => continue, // Skip non-UTF8 keys
                            }
                        },
                        Err(e) => return Err(format!("Database iteration error: {}", e)),
                    }
                }
                Ok(keys)
            },
            None => Err("Database not available".to_string()),
        }
    }

    /// Clear all settings from the database
    pub fn clear(&mut self) -> Result<(), String> {
        if !self.is_enabled() {
            return Err("Settings database is disabled".to_string());
        }

        // Clear memory cache
        self.memory_cache.clear();

        // Clear database
        match &self.db {
            Some(db) => {
                match db.clear() {
                    Ok(_) => Ok(()),
                    Err(e) => Err(format!("Failed to clear database: {}", e)),
                }
            },
            None => Err("Database not available".to_string()),
        }
    }

    /// Get the number of settings in the database
    pub fn len(&self) -> Result<usize, String> {
        if !self.is_enabled() {
            return Err("Settings database is disabled".to_string());
        }

        match &self.db {
            Some(db) => {
                Ok(db.len())
            },
            None => Err("Database not available".to_string()),
        }
    }

    /// Check if the settings database is empty
    pub fn is_empty(&self) -> Result<bool, String> {
        Ok(self.len()? == 0)
    }
}

// Global functions to access the settings database singleton

/// Get a reference to the global settings database
pub fn get_settings_db() -> std::sync::MutexGuard<'static, SettingsDb> {
    SETTINGS_DB.lock().unwrap()
}

/// Store a value in the settings database
pub fn set<T: Serialize>(key: &str, value: &T) -> Result<(), String> {
    get_settings_db().set(key, value)
}

/// Store a string value in the settings database
pub fn set_string(key: &str, value: &str) -> Result<(), String> {
    get_settings_db().set_string(key, value)
}

/// Store an integer value in the settings database
pub fn set_int(key: &str, value: i64) -> Result<(), String> {
    get_settings_db().set_int(key, value)
}

/// Store a boolean value in the settings database
pub fn set_bool(key: &str, value: bool) -> Result<(), String> {
    get_settings_db().set_bool(key, value)
}

/// Get a value from the settings database
pub fn get<T: for<'de> Deserialize<'de>>(key: &str) -> Result<Option<T>, String> {
    get_settings_db().get(key)
}

/// Get a string value from the settings database
pub fn get_string(key: &str) -> Result<Option<String>, String> {
    get_settings_db().get_string(key)
}

/// Get an integer value from the settings database
pub fn get_int(key: &str) -> Result<Option<i64>, String> {
    get_settings_db().get_int(key)
}

/// Get a boolean value from the settings database
pub fn get_bool(key: &str) -> Result<Option<bool>, String> {
    get_settings_db().get_bool(key)
}

/// Get a string value with a default
pub fn get_string_with_default(key: &str, default: &str) -> Result<String, String> {
    get_settings_db().get_string_with_default(key, default)
}

/// Get an integer value with a default
pub fn get_int_with_default(key: &str, default: i64) -> Result<i64, String> {
    get_settings_db().get_int_with_default(key, default)
}

/// Get a boolean value with a default
pub fn get_bool_with_default(key: &str, default: bool) -> Result<bool, String> {
    get_settings_db().get_bool_with_default(key, default)
}

/// Remove a setting from the database
pub fn remove(key: &str) -> Result<bool, String> {
    get_settings_db().remove(key)
}

/// Check if a key exists in the settings database
pub fn contains_key(key: &str) -> Result<bool, String> {
    get_settings_db().contains_key(key)
}

/// Get all keys from the settings database
pub fn get_all_keys() -> Result<Vec<String>, String> {
    get_settings_db().get_all_keys()
}

/// Clear all settings from the database
pub fn clear() -> Result<(), String> {
    get_settings_db().clear()
}

/// Get the number of settings in the database
pub fn len() -> Result<usize, String> {
    get_settings_db().len()
}

/// Check if the settings database is empty
pub fn is_empty() -> Result<bool, String> {
    get_settings_db().is_empty()
}

/// Add a song to favourites in the settings database
pub fn add_favourite_song(artist: &str, title: &str) -> Result<(), String> {
    let key = format!("favourite_song:{}:{}", sanitize_key_component(artist), sanitize_key_component(title));
    set_bool(&key, true)
}

/// Remove a song from favourites in the settings database
pub fn remove_favourite_song(artist: &str, title: &str) -> Result<(), String> {
    let key = format!("favourite_song:{}:{}", sanitize_key_component(artist), sanitize_key_component(title));
    remove(&key).map(|_| ()) // Convert Result<bool, String> to Result<(), String>
}

/// Check if a song is marked as favourite in the settings database
pub fn is_favourite_song(artist: &str, title: &str) -> Result<bool, String> {
    let key = format!("favourite_song:{}:{}", sanitize_key_component(artist), sanitize_key_component(title));
    match get_bool(&key)? {
        Some(value) => Ok(value),
        None => Ok(false),
    }
}

/// Get all favourite songs from the settings database
pub fn get_all_favourite_songs() -> Result<Vec<(String, String)>, String> {
    let all_keys = get_all_keys()?;
    let mut favourite_songs = Vec::new();
    
    for key in all_keys {
        if key.starts_with("favourite_song:") {
            // Extract artist and title from the key
            let parts: Vec<&str> = key.strip_prefix("favourite_song:").unwrap().splitn(2, ':').collect();
            if parts.len() == 2 {
                // Reverse the sanitization (basic approach - may not be perfect for all cases)
                let artist = parts[0].replace("_", " ");
                let title = parts[1].replace("_", " ");
                favourite_songs.push((artist, title));
            }
        }
    }
    
    Ok(favourite_songs)
}

/// Sanitize a key component by replacing problematic characters
fn sanitize_key_component(input: &str) -> String {
    input
        .replace(":", "_")
        .replace("/", "_")
        .replace("\\", "_")
        .replace(" ", "_")
        .to_lowercase()
}

/// Settings DB implementation of FavouriteProvider
pub struct SettingsDbFavouriteProvider;

impl SettingsDbFavouriteProvider {
    pub fn new() -> Self {
        Self
    }
}

impl crate::helpers::favourites::FavouriteProvider for SettingsDbFavouriteProvider {
    fn is_favourite(&self, song: &crate::data::song::Song) -> Result<bool, crate::helpers::favourites::FavouriteError> {
        let artist = song.artist.as_ref()
            .ok_or_else(|| crate::helpers::favourites::FavouriteError::InvalidSong("Artist is required".to_string()))?;
        let title = song.title.as_ref()
            .ok_or_else(|| crate::helpers::favourites::FavouriteError::InvalidSong("Title is required".to_string()))?;

        match is_favourite_song(artist, title) {
            Ok(is_fav) => Ok(is_fav),
            Err(e) => Err(crate::helpers::favourites::FavouriteError::StorageError(e)),
        }
    }

    fn add_favourite(&self, song: &crate::data::song::Song) -> Result<(), crate::helpers::favourites::FavouriteError> {
        let artist = song.artist.as_ref()
            .ok_or_else(|| crate::helpers::favourites::FavouriteError::InvalidSong("Artist is required".to_string()))?;
        let title = song.title.as_ref()
            .ok_or_else(|| crate::helpers::favourites::FavouriteError::InvalidSong("Title is required".to_string()))?;

        match add_favourite_song(artist, title) {
            Ok(()) => Ok(()),
            Err(e) => Err(crate::helpers::favourites::FavouriteError::StorageError(e)),
        }
    }

    fn remove_favourite(&self, song: &crate::data::song::Song) -> Result<(), crate::helpers::favourites::FavouriteError> {
        let artist = song.artist.as_ref()
            .ok_or_else(|| crate::helpers::favourites::FavouriteError::InvalidSong("Artist is required".to_string()))?;
        let title = song.title.as_ref()
            .ok_or_else(|| crate::helpers::favourites::FavouriteError::InvalidSong("Title is required".to_string()))?;

        match remove_favourite_song(artist, title) {
            Ok(()) => Ok(()),
            Err(e) => Err(crate::helpers::favourites::FavouriteError::StorageError(e)),
        }
    }

    fn get_favourite_count(&self) -> Option<usize> {
        // Use the existing get_all_favourite_songs function to count favorites
        match get_all_favourite_songs() {
            Ok(songs) => Some(songs.len()),
            Err(_) => None, // Return None if we can't access the database
        }
    }

    fn provider_name(&self) -> &'static str {
        "settingsdb"
    }

    fn is_enabled(&self) -> bool {
        // Settings DB is always enabled if the database is accessible
        get_settings_db().enabled
    }

    fn is_active(&self) -> bool {
        // Settings DB is always active when enabled since it's a local database
        // No authentication or external connectivity required
        self.is_enabled() && get_settings_db().db.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use serial_test::serial;

    #[test]
    #[serial]
    fn test_settings_db_basic_functionality() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().to_str().unwrap();
        
        let mut db = SettingsDb::with_directory(db_path);
        
        // Test string storage and retrieval
        assert!(db.set_string("user_name", "alice").is_ok());
        assert_eq!(db.get_string("user_name").unwrap(), Some("alice".to_string()));
        
        // Test integer storage and retrieval
        assert!(db.set_int("volume", 75).is_ok());
        assert_eq!(db.get_int("volume").unwrap(), Some(75));
        
        // Test boolean storage and retrieval
        assert!(db.set_bool("shuffle_enabled", true).is_ok());
        assert_eq!(db.get_bool("shuffle_enabled").unwrap(), Some(true));
        
        // Test non-existent key
        assert_eq!(db.get_string("non_existent").unwrap(), None);
        
        // Test key existence
        assert!(db.contains_key("user_name").unwrap());
        assert!(!db.contains_key("non_existent").unwrap());
        
        // Test key removal
        assert!(db.remove("user_name").unwrap());
        assert!(!db.contains_key("user_name").unwrap());
        
        // Test get all keys
        let keys = db.get_all_keys().unwrap();
        assert_eq!(keys.len(), 2);
        assert!(keys.contains(&"volume".to_string()));
        assert!(keys.contains(&"shuffle_enabled".to_string()));
        
        // Test length
        assert_eq!(db.len().unwrap(), 2);
        assert!(!db.is_empty().unwrap());
        
        // Test clear
        assert!(db.clear().is_ok());
        assert_eq!(db.len().unwrap(), 0);
        assert!(db.is_empty().unwrap());
    }

    #[test]
    #[serial]
    fn test_settings_db_with_defaults() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().to_str().unwrap();
        
        let mut db = SettingsDb::with_directory(db_path);
        
        // Test defaults for non-existent keys
        assert_eq!(db.get_string_with_default("missing_string", "default").unwrap(), "default");
        assert_eq!(db.get_int_with_default("missing_int", 42).unwrap(), 42);
        assert_eq!(db.get_bool_with_default("missing_bool", false).unwrap(), false);
        
        // Test defaults when values exist
        db.set_string("existing_string", "value").unwrap();
        db.set_int("existing_int", 123).unwrap();
        db.set_bool("existing_bool", true).unwrap();
        
        assert_eq!(db.get_string_with_default("existing_string", "default").unwrap(), "value");
        assert_eq!(db.get_int_with_default("existing_int", 42).unwrap(), 123);
        assert_eq!(db.get_bool_with_default("existing_bool", false).unwrap(), true);
    }

    #[test]
    #[serial]
    fn test_settings_db_persistence() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().to_str().unwrap();
        
        // Store some data
        {
            let mut db = SettingsDb::with_directory(db_path);
            db.set_string("persistent_key", "persistent_value").unwrap();
            db.set_int("persistent_number", 999).unwrap();
        }
        
        // Create new instance and verify data persists
        {
            let mut db = SettingsDb::with_directory(db_path);
            assert_eq!(db.get_string("persistent_key").unwrap(), Some("persistent_value".to_string()));
            assert_eq!(db.get_int("persistent_number").unwrap(), Some(999));
        }
    }

    #[test]
    #[serial]
    fn test_settings_db_complex_types() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().to_str().unwrap();
        
        let mut db = SettingsDb::with_directory(db_path);
        
        // Test storing complex JSON-serializable types
        let settings = serde_json::json!({
            "theme": "dark",
            "volume": 85,
            "equalizer": {
                "bass": 2,
                "treble": -1
            }
        });
        
        assert!(db.set("user_preferences", &settings).is_ok());
        let retrieved: serde_json::Value = db.get("user_preferences").unwrap().unwrap();
        assert_eq!(retrieved, settings);
    }

    #[test]
    #[serial]
    fn test_global_functions() {
        // Initialize the global settings database with a temporary path for testing
        let temp_dir = TempDir::new().unwrap();
        let test_path = temp_dir.path().to_str().unwrap();
        
        // Initialize the global database
        SettingsDb::initialize(test_path).ok();
        
        // Clear any existing data first
        clear().ok(); // Ignore errors if not initialized
        
        // Test global functions
        assert!(set_string("global_test", "value").is_ok());
        assert_eq!(get_string("global_test").unwrap(), Some("value".to_string()));
        
        assert!(set_int("global_int", 42).is_ok());
        assert_eq!(get_int("global_int").unwrap(), Some(42));
        
        assert!(set_bool("global_bool", true).is_ok());
        assert_eq!(get_bool("global_bool").unwrap(), Some(true));
        
        // Test with defaults
        assert_eq!(get_string_with_default("missing", "default").unwrap(), "default");
        assert_eq!(get_int_with_default("missing", 100).unwrap(), 100);
        assert_eq!(get_bool_with_default("missing", false).unwrap(), false);
        
        // Test key operations
        assert!(contains_key("global_test").unwrap());
        let all_keys = get_all_keys().unwrap();
        assert!(all_keys.contains(&"global_test".to_string()));
        
        assert!(remove("global_test").unwrap());
        assert!(!contains_key("global_test").unwrap());
        
        // Clean up
        clear().ok();
    }

    #[test]
    #[serial]
    fn test_favourite_provider_count() {
        use crate::helpers::favourites::FavouriteProvider;
        use crate::data::song::Song;

        // Initialize the global settings database with a temporary path for testing
        let temp_dir = TempDir::new().unwrap();
        let test_path = temp_dir.path().to_str().unwrap();
        
        // Initialize the global database
        SettingsDb::initialize(test_path).ok();
        
        // Clear any existing data first
        clear().ok(); // Ignore errors if not initialized
        
        let provider = SettingsDbFavouriteProvider::new();
        
        // Initially should have 0 favorites
        assert_eq!(provider.get_favourite_count(), Some(0));
        
        // Create test songs
        let mut song1 = Song::default();
        song1.artist = Some("Test Artist 1".to_string());
        song1.title = Some("Test Song 1".to_string());
        
        let mut song2 = Song::default();
        song2.artist = Some("Test Artist 2".to_string());
        song2.title = Some("Test Song 2".to_string());
        
        let mut song3 = Song::default();
        song3.artist = Some("Test Artist 3".to_string());
        song3.title = Some("Test Song 3".to_string());
        
        // Add first favorite
        assert!(provider.add_favourite(&song1).is_ok());
        assert_eq!(provider.get_favourite_count(), Some(1));
        
        // Add second favorite
        assert!(provider.add_favourite(&song2).is_ok());
        assert_eq!(provider.get_favourite_count(), Some(2));
        
        // Add third favorite
        assert!(provider.add_favourite(&song3).is_ok());
        assert_eq!(provider.get_favourite_count(), Some(3));
        
        // Remove one favorite
        assert!(provider.remove_favourite(&song2).is_ok());
        assert_eq!(provider.get_favourite_count(), Some(2));
        
        // Remove another favorite
        assert!(provider.remove_favourite(&song1).is_ok());
        assert_eq!(provider.get_favourite_count(), Some(1));
        
        // Remove last favorite
        assert!(provider.remove_favourite(&song3).is_ok());
        assert_eq!(provider.get_favourite_count(), Some(0));
        
        // Clean up
        clear().ok();
    }
}
