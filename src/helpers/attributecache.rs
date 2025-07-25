use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::collections::HashMap;
use lazy_static::lazy_static;
use log::{info, error, debug};
use serde::{Serialize, Deserialize};
use std::sync::Arc;
use rusqlite::{Connection, params};

// Global singleton for the attribute cache
lazy_static! {
    static ref ATTRIBUTE_CACHE: Mutex<AttributeCache> = Mutex::new(AttributeCache::new());
}

/// A persistent attribute cache that stores key-value pairs using SQLite database
pub struct AttributeCache {
    /// Path to the database file
    db_path: PathBuf,
    /// SQLite database connection
    db: Option<Connection>,
    /// Whether the cache is enabled
    enabled: bool,
    /// Max age of cached items in days
    max_age_days: u64,
    /// In-memory cache of recently accessed items
    memory_cache: HashMap<String, Arc<Vec<u8>>>,
}

impl AttributeCache {
    /// Create a new attribute cache with default settings
    pub fn new() -> Self {
        // Using the default path that matches our cache.attribute_cache_path setting
        let cache_dir = PathBuf::from("/var/lib/audiocontrol/cache");
        let db_file = cache_dir.join("attributes.db");
        Self::with_database_file(db_file)
    }

    /// Create a new attribute cache with a specific database file
    pub fn with_database_file<P: AsRef<Path>>(db_file: P) -> Self {
        let db_path = db_file.as_ref().to_path_buf();
        
        // Try to ensure the directory exists
        if let Some(parent) = db_path.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                error!("Failed to create directory for attribute cache: {}", e);
            }
        }
        
        // Try to open the SQLite database
        let db = match Connection::open(&db_path) {
            Ok(conn) => {
                info!("Successfully opened attribute cache database at {:?}", db_path);
                
                // Create the cache table if it doesn't exist
                if let Err(e) = conn.execute(
                    "CREATE TABLE IF NOT EXISTS cache (
                        key TEXT PRIMARY KEY,
                        value BLOB NOT NULL,
                        created_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now'))
                    )",
                    [],
                ) {
                    error!("Failed to create cache table: {}", e);
                    None
                } else {
                    debug!("Cache table created or already exists");
                    Some(conn)
                }
            },
            Err(e) => {
                error!("Failed to open SQLite database at {:?}: {}", db_path, e);
                None
            }
        };

        AttributeCache {
            db_path,
            db,
            enabled: true,
            max_age_days: 30, // Default to 30 days
            memory_cache: HashMap::new(),
        }
    }

    /// Initialize the global attribute cache with a custom directory
    pub fn initialize_global<P: AsRef<Path>>(dir: P) -> Result<(), String> {
        match get_attribute_cache().reconfigure_with_directory(dir) {
            Ok(_) => {
                info!("Global attribute cache initialized with custom directory");
                Ok(())
            },
            Err(e) => {
                error!("Failed to initialize global attribute cache: {}", e);
                Err(e)
            }
        }
    }
    
    /// Initialize the global attribute cache with a custom directory path as string
    pub fn initialize<P: AsRef<Path>>(path: P) -> Result<(), String> {
        Self::initialize_global(path)
    }

    /// Reconfigure the attribute cache with a new directory
    /// This will close the existing database and open a new one
    fn reconfigure_with_directory<P: AsRef<Path>>(&mut self, dir: P) -> Result<(), String> {
        let cache_dir = dir.as_ref().to_path_buf();
        let db_file = cache_dir.join("attributes.db");
        
        // Try to ensure the directory exists
        if let Err(e) = std::fs::create_dir_all(&cache_dir) {
            return Err(format!("Failed to create directory for attribute cache: {}", e));
        }
        
        // Try to open the new SQLite database
        let db = match Connection::open(&db_file) {
            Ok(conn) => {
                info!("Successfully opened attribute cache database at {:?}", db_file);
                
                // Create the cache table if it doesn't exist
                if let Err(e) = conn.execute(
                    "CREATE TABLE IF NOT EXISTS cache (
                        key TEXT PRIMARY KEY,
                        value BLOB NOT NULL,
                        created_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now'))
                    )",
                    [],
                ) {
                    return Err(format!("Failed to create cache table: {}", e));
                }
                
                debug!("Cache table created or already exists");
                Some(conn)
            },
            Err(e) => {
                error!("Failed to open SQLite database at {:?}: {}", db_file, e);
                return Err(format!("Failed to open SQLite database: {}", e));
            }
        };
        
        // Update the instance
        self.db_path = db_file;
        self.db = db;
        self.memory_cache.clear(); // Clear memory cache as we have a new DB
        
        Ok(())
    }

    /// Set the maximum age for cached items in days
    pub fn set_max_age(&mut self, days: u64) {
        self.max_age_days = days;
    }

    /// Enable or disable the cache
    pub fn enable(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    /// Check if the cache is enabled
    pub fn is_enabled(&self) -> bool {
        self.enabled && self.db.is_some()
    }

    /// Store a serializable value in the cache
    pub fn set<T: Serialize>(&mut self, key: &str, value: &T) -> Result<(), String> {
        if !self.is_enabled() {
            return Err("Cache is disabled".to_string());
        }

        let serialized = match serde_json::to_vec(value) {
            Ok(data) => data,
            Err(e) => return Err(format!("Failed to serialize value: {}", e)),
        };

        // Store in memory cache
        self.memory_cache.insert(key.to_string(), Arc::new(serialized.clone()));

        // Store in SQLite database
        match &mut self.db {
            Some(db) => {
                if let Err(e) = db.execute(
                    "INSERT OR REPLACE INTO cache (key, value) VALUES (?1, ?2)",
                    params![key, serialized],
                ) {
                    return Err(format!("Failed to store in database: {}", e));
                }
                
                debug!("Stored key '{}' in SQLite cache", key);
                Ok(())
            },
            None => Err("Database not available".to_string()),
        }
    }

    /// Get a value from the cache and deserialize it
    pub fn get<T: for<'de> Deserialize<'de>>(&mut self, key: &str) -> Result<Option<T>, String> {
        if !self.is_enabled() {
            return Err("Cache is disabled".to_string());
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
            Some(db) => {
                let mut stmt = match db.prepare("SELECT value FROM cache WHERE key = ?1") {
                    Ok(stmt) => stmt,
                    Err(e) => return Err(format!("Failed to prepare SQL statement: {}", e)),
                };
                
                match stmt.query_row(params![key], |row| {
                    let data: Vec<u8> = row.get(0)?;
                    Ok(data)
                }) {
                    Ok(data_vec) => {
                        // Store in memory cache for future access
                        let result: T = match serde_json::from_slice(&data_vec) {
                            Ok(value) => value,
                            Err(e) => return Err(format!("Failed to deserialize from database: {}", e)),
                        };
                        
                        self.memory_cache.insert(key.to_string(), Arc::new(data_vec));
                        debug!("Retrieved key '{}' from SQLite cache", key);
                        Ok(Some(result))
                    },
                    Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
                    Err(e) => Err(format!("Database error: {}", e)),
                }
            },
            None => Err("Database not available".to_string()),
        }
    }

    /// Remove an item from the cache
    pub fn remove(&mut self, key: &str) -> Result<bool, String> {
        if !self.is_enabled() {
            return Err("Cache is disabled".to_string());
        }

        // Remove from memory cache
        self.memory_cache.remove(key);

        // Remove from database
        match &mut self.db {
            Some(db) => {
                match db.execute("DELETE FROM cache WHERE key = ?1", params![key]) {
                    Ok(affected_rows) => {
                        let removed = affected_rows > 0;
                        if removed {
                            debug!("Removed key '{}' from SQLite cache", key);
                        }
                        Ok(removed)
                    },
                    Err(e) => Err(format!("Failed to remove from database: {}", e)),
                }
            },
            None => Err("Database not available".to_string()),
        }
    }

    /// Clear the entire cache
    pub fn clear(&mut self) -> Result<(), String> {
        if !self.is_enabled() {
            return Err("Cache is disabled".to_string());
        }

        // Clear memory cache
        self.memory_cache.clear();

        // Clear database
        match &mut self.db {
            Some(db) => {
                match db.execute("DELETE FROM cache", []) {
                    Ok(affected_rows) => {
                        debug!("Cleared {} entries from SQLite cache", affected_rows);
                        Ok(())
                    },
                    Err(e) => Err(format!("Failed to clear database: {}", e)),
                }
            },
            None => Err("Database not available".to_string()),
        }
    }

    /// Clean up old entries that exceed the maximum age
    pub fn cleanup(&mut self) -> Result<usize, String> {
        if !self.is_enabled() {
            return Err("Cache is disabled".to_string());
        }

        match &mut self.db {
            Some(db) => {
                // Calculate the cutoff timestamp (current time - max_age_days)
                let cutoff_timestamp = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_err(|e| format!("Failed to get current time: {}", e))?
                    .as_secs() as i64 - (self.max_age_days as i64 * 24 * 60 * 60);

                match db.execute(
                    "DELETE FROM cache WHERE created_at < ?1",
                    params![cutoff_timestamp]
                ) {
                    Ok(affected_rows) => {
                        if affected_rows > 0 {
                            info!("Cleaned up {} old entries from attribute cache", affected_rows);
                            // Clear memory cache as some entries might have been removed
                            self.memory_cache.clear();
                        }
                        Ok(affected_rows)
                    },
                    Err(e) => Err(format!("Failed to cleanup database: {}", e)),
                }
            },
            None => Err("Database not available".to_string()),
        }
    }
}

// Global functions to access the attribute cache singleton

/// Get a reference to the global attribute cache
pub fn get_attribute_cache() -> std::sync::MutexGuard<'static, AttributeCache> {
    ATTRIBUTE_CACHE.lock().unwrap()
}

/// Store a value in the attribute cache
pub fn set<T: Serialize>(key: &str, value: &T) -> Result<(), String> {
    get_attribute_cache().set(key, value)
}

/// Get a value from the attribute cache
pub fn get<T: for<'de> Deserialize<'de>>(key: &str) -> Result<Option<T>, String> {
    get_attribute_cache().get(key)
}

/// Remove a value from the attribute cache
pub fn remove(key: &str) -> Result<bool, String> {
    get_attribute_cache().remove(key)
}

/// Clear the entire attribute cache
pub fn clear() -> Result<(), String> {
    get_attribute_cache().clear()
}

/// Clean up old entries from the attribute cache
pub fn cleanup() -> Result<usize, String> {
    get_attribute_cache().cleanup()
}