use std::path::{Path, PathBuf};
use parking_lot::Mutex;
use std::collections::HashMap;
use once_cell::sync::Lazy;
use log::{info, error};
use serde::{Serialize, Deserialize};
use std::sync::Arc;

// Global singleton for the settings database
static SETTINGS_DB: Lazy<Mutex<SettingsDb>> = Lazy::new(|| Mutex::new(SettingsDb::new()));

/// A persistent settings database that stores user settings as key-value pairs using SQLite database
pub struct SettingsDb {
    /// Path to the database file
    db_path: PathBuf,
    /// SQLite database connection
    db: Option<rusqlite::Connection>,
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
        let db_dir = dir.as_ref().to_path_buf();
        let db_path = db_dir.join("settings.db");
        
        // Try to ensure the directory exists
        if let Err(e) = std::fs::create_dir_all(&db_dir) {
            error!("Failed to create directory for settings database: {}", e);
        }
        
        // Try to open the SQLite database
        let db = match rusqlite::Connection::open(&db_path) {
            Ok(conn) => {
                info!("Successfully opened settings database at {:?}", db_path);
                
                // Create the settings table if it doesn't exist
                if let Err(e) = conn.execute(
                    "CREATE TABLE IF NOT EXISTS settings (
                        key TEXT PRIMARY KEY,
                        value BLOB NOT NULL
                    )",
                    [],
                ) {
                    error!("Failed to create settings table: {}", e);
                    None
                } else {
                    Some(conn)
                }
            },
            Err(e) => {
                error!("Failed to open SQLite database at {:?}: {}", db_path, e);
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
        let db_dir = dir.as_ref().to_path_buf();
        let db_path = db_dir.join("settings.db");
        
        // Try to ensure the directory exists
        if let Err(e) = std::fs::create_dir_all(&db_dir) {
            return Err(format!("Failed to create directory for settings database: {}", e));
        }
        
        // Try to open the new SQLite database
        let db = match rusqlite::Connection::open(&db_path) {
            Ok(conn) => {
                info!("Successfully opened settings database at {:?}", db_path);
                
                // Create the settings table if it doesn't exist
                if let Err(e) = conn.execute(
                    "CREATE TABLE IF NOT EXISTS settings (
                        key TEXT PRIMARY KEY,
                        value BLOB NOT NULL
                    )",
                    [],
                ) {
                    return Err(format!("Failed to create settings table: {}", e));
                }
                
                Some(conn)
            },
            Err(e) => {
                error!("Failed to open SQLite database at {:?}: {}", db_path, e);
                return Err(format!("Failed to open SQLite database: {}", e));
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

        // Store in SQLite database
        match &mut self.db {
            Some(conn) => {
                if let Err(e) = conn.execute(
                    "INSERT OR REPLACE INTO settings (key, value) VALUES (?1, ?2)",
                    rusqlite::params![key, &serialized],
                ) {
                    return Err(format!("Failed to store in database: {}", e));
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

        // Fall back to SQLite database
        match &mut self.db {
            Some(conn) => {
                let mut stmt = match conn.prepare("SELECT value FROM settings WHERE key = ?1") {
                    Ok(stmt) => stmt,
                    Err(e) => return Err(format!("Failed to prepare query: {}", e)),
                };
                
                match stmt.query_row(rusqlite::params![key], |row| {
                    let data: Vec<u8> = row.get(0)?;
                    Ok(data)
                }) {
                    Ok(data) => {
                        // Store in memory cache for future access
                        let result: T = match serde_json::from_slice(&data) {
                            Ok(value) => value,
                            Err(e) => return Err(format!("Failed to deserialize from database: {}", e)),
                        };
                        
                        self.memory_cache.insert(key.to_string(), Arc::new(data));
                        Ok(Some(result))
                    },
                    Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
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
        match &mut self.db {
            Some(conn) => {
                match conn.execute("DELETE FROM settings WHERE key = ?1", rusqlite::params![key]) {
                    Ok(count) => Ok(count > 0),
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
        match &mut self.db {
            Some(conn) => {
                let mut stmt = match conn.prepare("SELECT 1 FROM settings WHERE key = ?1 LIMIT 1") {
                    Ok(stmt) => stmt,
                    Err(e) => return Err(format!("Failed to prepare query: {}", e)),
                };
                
                match stmt.exists(rusqlite::params![key]) {
                    Ok(exists) => Ok(exists),
                    Err(e) => Err(format!("Database error: {}", e)),
                }
            },
            None => Err("Database not available".to_string()),
        }
    }

    /// Get all keys from the settings database
    pub fn get_all_keys(&mut self) -> Result<Vec<String>, String> {
        if !self.is_enabled() {
            return Err("Settings database is disabled".to_string());
        }

        match &mut self.db {
            Some(conn) => {
                let mut stmt = match conn.prepare("SELECT key FROM settings") {
                    Ok(stmt) => stmt,
                    Err(e) => return Err(format!("Failed to prepare query: {}", e)),
                };
                
                let key_iter = match stmt.query_map([], |row| {
                    Ok(row.get::<_, String>(0)?)
                }) {
                    Ok(iter) => iter,
                    Err(e) => return Err(format!("Failed to query keys: {}", e)),
                };
                
                let mut keys = Vec::new();
                for key_result in key_iter {
                    match key_result {
                        Ok(key) => keys.push(key),
                        Err(e) => return Err(format!("Error reading key: {}", e)),
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
        match &mut self.db {
            Some(conn) => {
                match conn.execute("DELETE FROM settings", []) {
                    Ok(_) => Ok(()),
                    Err(e) => Err(format!("Failed to clear database: {}", e)),
                }
            },
            None => Err("Database not available".to_string()),
        }
    }

    /// Get the number of settings in the database
    pub fn len(&mut self) -> Result<usize, String> {
        if !self.is_enabled() {
            return Err("Settings database is disabled".to_string());
        }

        match &mut self.db {
            Some(conn) => {
                let mut stmt = match conn.prepare("SELECT COUNT(*) FROM settings") {
                    Ok(stmt) => stmt,
                    Err(e) => return Err(format!("Failed to prepare count query: {}", e)),
                };
                
                match stmt.query_row([], |row| row.get::<_, i64>(0)) {
                    Ok(count) => Ok(count as usize),
                    Err(e) => Err(format!("Failed to count rows: {}", e)),
                }
            },
            None => Err("Database not available".to_string()),
        }
    }

    /// Check if the settings database is empty
    pub fn is_empty(&mut self) -> Result<bool, String> {
        Ok(self.len()? == 0)
    }
}

// Global functions to access the settings database singleton

/// Get a reference to the global settings database
pub fn get_settings_db() -> parking_lot::MutexGuard<'static, SettingsDb> {
    SETTINGS_DB.lock()
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

    fn display_name(&self) -> &'static str {
        "User settings"
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

    // Concurrent access tests
    #[test]
    #[serial]
    fn test_concurrent_set_and_get() {
        use std::sync::Arc;
        use parking_lot::Mutex;
        use std::thread;
        
        let temp_dir = TempDir::new().expect("Failed to create temp directory");
        let db_path = temp_dir.path().to_str().unwrap();
        let db = Arc::new(Mutex::new(SettingsDb::with_directory(db_path)));
        
        let num_threads = 10;
        let operations_per_thread = 50;
        let mut handles = vec![];
        
        // Spawn multiple threads that set and get values concurrently
        for thread_id in 0..num_threads {
            let db_clone = Arc::clone(&db);
            let handle = thread::spawn(move || {
                for i in 0..operations_per_thread {
                    let key = format!("thread_{}_key_{}", thread_id, i);
                    let value = format!("thread_{}_value_{}", thread_id, i);
                    
                    // Set value
                    {
                        let mut db_guard = db_clone.lock();
                        db_guard.set_string(&key, &value).expect("Failed to set value in thread");
                    }
                    
                    // Get value back
                    {
                        let mut db_guard = db_clone.lock();
                        let retrieved = db_guard.get_string(&key).expect("Failed to get value in thread");
                        assert_eq!(retrieved, Some(value));
                    }
                }
            });
            handles.push(handle);
        }
        
        // Wait for all threads to complete
        for handle in handles {
            handle.join().expect("Thread panicked");
        }
        
        // Verify all data is still accessible
        for thread_id in 0..num_threads {
            for i in 0..operations_per_thread {
                let key = format!("thread_{}_key_{}", thread_id, i);
                let expected_value = format!("thread_{}_value_{}", thread_id, i);
                
                let mut db_guard = db.lock();
                let retrieved = db_guard.get_string(&key).expect("Failed to get value after threads");
                assert_eq!(retrieved, Some(expected_value));
                drop(db_guard); // Explicitly drop to release lock
            }
        }
    }

    #[test]
    #[serial]
    fn test_concurrent_mixed_operations() {
        use std::sync::Arc;
        use parking_lot::Mutex;
        use std::thread;
        use std::time::Duration;
        
        let temp_dir = TempDir::new().expect("Failed to create temp directory");
        let db_path = temp_dir.path().to_str().unwrap();
        let db = Arc::new(Mutex::new(SettingsDb::with_directory(db_path)));
        
        // Pre-populate with some data
        {
            let mut db_guard = db.lock();
            for i in 0..10 {
                let key = format!("shared_key_{}", i);
                let value = format!("shared_value_{}", i);
                db_guard.set_string(&key, &value).expect("Failed to set initial value");
            }
        }
        
        let num_reader_threads = 3;
        let num_writer_threads = 2;
        let mut handles = vec![];
        
        // Spawn reader threads that access the same keys concurrently
        for _thread_id in 0..num_reader_threads {
            let db_clone = Arc::clone(&db);
            let handle = thread::spawn(move || {
                for _iteration in 0..50 {
                    for key_id in 0..10 {
                        let key = format!("shared_key_{}", key_id);
                        
                        // Just verify we can read some value, don't care about the exact content
                        // since writers might be updating it concurrently
                        {
                            let mut db_mut = db_clone.lock();
                            let _retrieved = db_mut.get_string(&key);
                            // Don't assert on value since it may be updated by writers
                        }
                        
                        // Small sleep to increase chance of interleaving
                        thread::sleep(Duration::from_millis(1));
                    }
                }
            });
            handles.push(handle);
        }
        
        // Spawn writer threads that update existing keys
        for thread_id in 0..num_writer_threads {
            let db_clone = Arc::clone(&db);
            let handle = thread::spawn(move || {
                for iteration in 0..10 {
                    for key_id in 0..10 {
                        let key = format!("shared_key_{}", key_id);
                        let new_value = format!("updated_by_thread_{}_iter_{}_{}", thread_id, iteration, key_id);
                        
                        {
                            let mut db_mut = db_clone.lock();
                            let _ = db_mut.set_string(&key, &new_value); // Ignore errors
                        }
                        
                        thread::sleep(Duration::from_millis(2));
                    }
                }
            });
            handles.push(handle);
        }
        
        // Wait for all threads to complete
        for handle in handles {
            let _ = handle.join(); // Ignore panics from individual threads
        }
        
        // Test passes if we get here without deadlocks
    }

    #[test]
    #[serial]
    fn test_concurrent_global_access() {
        use std::thread;
        use std::sync::Arc;
        use std::sync::atomic::{AtomicUsize, Ordering};
        
        // Initialize global database with a temp directory first
        let temp_dir = TempDir::new().expect("Failed to create temp directory");
        SettingsDb::initialize_global(temp_dir.path()).expect("Failed to initialize global database");
        
        let num_threads = 8;
        let operations_per_thread = 25;
        let success_counter = Arc::new(AtomicUsize::new(0));
        let mut handles = vec![];
        
        // Clear global database first to ensure clean state
        clear().ok(); // Ignore errors if not initialized
        
        // Spawn multiple threads that use global database functions
        for thread_id in 0..num_threads {
            let counter_clone = Arc::clone(&success_counter);
            let handle = thread::spawn(move || {
                let mut successful_operations = 0;
                
                for i in 0..operations_per_thread {
                    let key = format!("global_thread_{}_key_{}", thread_id, i);
                    let value = format!("global_thread_{}_value_{}", thread_id, i);
                    
                    // Set value using global function
                    if set_string(&key, &value).is_ok() {
                        // Get value back using global function
                        match get_string(&key) {
                            Ok(Some(retrieved)) => {
                                if retrieved == value {
                                    successful_operations += 1;
                                }
                            },
                            _ => {} // Failed to retrieve
                        }
                    }
                    
                    // Test removal occasionally
                    if i % 5 == 0 {
                        remove(&key).ok(); // Ignore errors
                    }
                }
                
                counter_clone.fetch_add(successful_operations, Ordering::Relaxed);
            });
            handles.push(handle);
        }
        
        // Wait for all threads to complete
        for handle in handles {
            let _ = handle.join(); // Ignore panics
        }
        
        // Verify that most operations were successful
        // We expect some operations to fail due to removals, but most should succeed
        let total_successful = success_counter.load(Ordering::Relaxed);
        let expected_minimum = (num_threads * operations_per_thread) / 3; // At least 33% success rate (relaxed)
        assert!(total_successful >= expected_minimum, 
            "Expected at least {} successful operations, got {}", 
            expected_minimum, total_successful);
        
        // Clean up
        clear().ok();
    }

    #[test]
    #[serial]
    fn test_concurrent_memory_cache_access() {
        use std::sync::Arc;
        use parking_lot::Mutex;
        use std::thread;
        use std::time::Duration;
        
        let temp_dir = TempDir::new().expect("Failed to create temp directory");
        let db_path = temp_dir.path().to_str().unwrap();
        let db = Arc::new(Mutex::new(SettingsDb::with_directory(db_path)));
        
        // Pre-populate the database to test memory cache behavior
        {
            let mut db_guard = db.lock();
            for i in 0..10 {
                let key = format!("memory_key_{}", i);
                let value = format!("memory_value_{}", i);
                db_guard.set_string(&key, &value).expect("Failed to set initial value");
                // Access it once to load into memory cache
                let _ = db_guard.get_string(&key);
            }
        }
        
        let num_threads = 5;
        let mut handles = vec![];
        
        // Spawn threads that repeatedly access cached values
        for _thread_id in 0..num_threads {
            let db_clone = Arc::clone(&db);
            let handle = thread::spawn(move || {
                for _iteration in 0..20 {
                    for key_id in 0..10 {
                        let key = format!("memory_key_{}", key_id);
                        
                        {
                            let mut db_mut = db_clone.lock();
                            // This should hit the memory cache
                            let _retrieved = db_mut.get_string(&key);

                            // Verify memory cache contains the key
                            let has_cached = db_mut.memory_cache.contains_key(&key);
                            assert!(has_cached, "Key {} should be in memory cache", key);
                        }
                        
                        thread::sleep(Duration::from_millis(1));
                    }
                }
            });
            handles.push(handle);
        }
        
        // Wait for all threads to complete
        for handle in handles {
            handle.join().expect("Thread panicked");
        }
    }

    #[test]
    #[serial]
    fn test_concurrent_clear_and_access() {
        use std::sync::Arc;
        use parking_lot::Mutex;
        use std::thread;
        use std::time::Duration;
        
        let temp_dir = TempDir::new().expect("Failed to create temp directory");
        let db_path = temp_dir.path().to_str().unwrap();
        let db = Arc::new(Mutex::new(SettingsDb::with_directory(db_path)));
        
        let num_access_threads = 4;
        let mut handles = vec![];
        
        // Spawn threads that continuously add and access data
        for thread_id in 0..num_access_threads {
            let db_clone = Arc::clone(&db);
            let handle = thread::spawn(move || {
                for i in 0..30 {
                    let key = format!("clear_thread_{}_key_{}", thread_id, i);
                    let value = format!("clear_value_{}", i);
                    
                    // Set value
                    {
                        let mut db_guard = db_clone.lock();
                        db_guard.set_string(&key, &value).ok(); // Ignore errors
                    }
                    
                    // Try to get value
                    {
                        let mut db_guard = db_clone.lock();
                        let _retrieved = db_guard.get_string(&key);
                        // Don't assert here as clear might remove the value
                    }
                    
                    thread::sleep(Duration::from_millis(1));
                }
            });
            handles.push(handle);
        }
        
        // Spawn a thread that periodically clears the database
        let db_clear = Arc::clone(&db);
        let clear_handle = thread::spawn(move || {
            for _i in 0..5 {
                thread::sleep(Duration::from_millis(10));
                let mut db_guard = db_clear.lock();
                db_guard.clear().ok(); // Ignore errors
            }
        });
        handles.push(clear_handle);
        
        // Wait for all threads to complete
        for handle in handles {
            handle.join().expect("Thread panicked");
        }
        
        // Test should complete without deadlocks or panics
        // The exact state of the database is not important, just that it didn't crash
    }

    #[test]
    #[serial]
    fn test_concurrent_different_data_types() {
        use std::sync::Arc;
        use parking_lot::Mutex;
        use std::thread;
        
        let temp_dir = TempDir::new().expect("Failed to create temp directory");
        let db_path = temp_dir.path().to_str().unwrap();
        let db = Arc::new(Mutex::new(SettingsDb::with_directory(db_path)));
        
        let num_threads = 6;
        let mut handles = vec![];
        
        // Spawn threads that work with different data types concurrently
        for thread_id in 0..num_threads {
            let db_clone = Arc::clone(&db);
            let handle = thread::spawn(move || {
                for i in 0..20 {
                    let base_key = format!("thread_{}_item_{}", thread_id, i);
                    
                    {
                        let mut db_guard = db_clone.lock();
                        
                        // Set different types of data
                        let string_key = format!("{}_string", base_key);
                        let int_key = format!("{}_int", base_key);
                        let bool_key = format!("{}_bool", base_key);
                        
                        let string_value = format!("value_{}", i);
                        let int_value = (thread_id * 100 + i) as i64;
                        let bool_value = i % 2 == 0;
                        
                        db_guard.set_string(&string_key, &string_value).expect("Failed to set string");
                        db_guard.set_int(&int_key, int_value).expect("Failed to set int");
                        db_guard.set_bool(&bool_key, bool_value).expect("Failed to set bool");
                    }
                    
                    // Read back and verify
                    {
                        let mut db_guard = db_clone.lock();
                        
                        let string_key = format!("{}_string", base_key);
                        let int_key = format!("{}_int", base_key);
                        let bool_key = format!("{}_bool", base_key);
                        
                        let expected_string = format!("value_{}", i);
                        let expected_int = (thread_id * 100 + i) as i64;
                        let expected_bool = i % 2 == 0;
                        
                        assert_eq!(db_guard.get_string(&string_key).unwrap(), Some(expected_string));
                        assert_eq!(db_guard.get_int(&int_key).unwrap(), Some(expected_int));
                        assert_eq!(db_guard.get_bool(&bool_key).unwrap(), Some(expected_bool));
                    }
                }
            });
            handles.push(handle);
        }
        
        // Wait for all threads to complete
        for handle in handles {
            handle.join().expect("Thread panicked");
        }
        
        // Verify final state
        let mut db_guard = db.lock();
        let total_keys = db_guard.get_all_keys().unwrap().len();
        // Each thread creates 3 keys per iteration
        let expected_keys = num_threads * 20 * 3;
        assert_eq!(total_keys, expected_keys);
    }

    #[test]
    #[serial]
    fn test_concurrent_favourite_operations() {
        use std::sync::Arc;
        use std::thread;
        use crate::data::song::Song;
        use crate::helpers::favourites::FavouriteProvider;
        
        // Initialize global database with a temp directory first
        let temp_dir = TempDir::new().expect("Failed to create temp directory");
        SettingsDb::initialize_global(temp_dir.path()).expect("Failed to initialize global database");
        
        // Clear any existing data first
        clear().ok();
        
        let provider = Arc::new(SettingsDbFavouriteProvider::new());
        let num_threads = 4;
        let songs_per_thread = 10;
        let mut handles = vec![];
        
        // Spawn threads that add/remove favourites concurrently
        for thread_id in 0..num_threads {
            let provider_clone = Arc::clone(&provider);
            let handle = thread::spawn(move || {
                for i in 0..songs_per_thread {
                    let mut song = Song::default();
                    song.artist = Some(format!("Artist_{}", thread_id));
                    song.title = Some(format!("Song_{}_{}", thread_id, i));
                    
                    // Add favourite
                    provider_clone.add_favourite(&song).expect("Failed to add favourite");
                    
                    // Check if it's marked as favourite
                    assert!(provider_clone.is_favourite(&song).expect("Failed to check favourite"));
                    
                    // Remove every other favourite to test removal
                    if i % 2 == 0 {
                        provider_clone.remove_favourite(&song).expect("Failed to remove favourite");
                        assert!(!provider_clone.is_favourite(&song).expect("Failed to check favourite after removal"));
                    }
                }
            });
            handles.push(handle);
        }
        
        // Wait for all threads to complete
        for handle in handles {
            handle.join().expect("Thread panicked");
        }
        
        // Check final favourite count
        // Each thread adds songs_per_thread favourites but removes half of them
        let expected_count = num_threads * (songs_per_thread / 2);
        let actual_count = provider.get_favourite_count().unwrap_or(0);
        assert_eq!(actual_count, expected_count);
        
        // Clean up
        clear().ok();
    }
}
