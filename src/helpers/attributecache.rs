use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::collections::HashMap;
use lazy_static::lazy_static;
use log::{info, error};
use serde::{Serialize, Deserialize};
use std::sync::Arc;

// Global singleton for the attribute cache
lazy_static! {
    static ref ATTRIBUTE_CACHE: Mutex<AttributeCache> = Mutex::new(AttributeCache::new());
}

/// A persistent attribute cache that stores key-value pairs using Sled database
pub struct AttributeCache {
    /// Path to the database directory
    db_path: PathBuf,
    /// Sled database instance
    db: Option<sled::Db>,
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
        let cache_dir = PathBuf::from("cache/attributes");
        Self::with_directory(cache_dir)
    }

    /// Create a new attribute cache with a specific directory
    pub fn with_directory<P: AsRef<Path>>(dir: P) -> Self {
        let db_path = dir.as_ref().to_path_buf();
        
        // Try to open the sled database
        let db = match sled::open(&db_path) {
            Ok(db) => {
                info!("Successfully opened attribute cache database at {:?}", db_path);
                Some(db)
            },
            Err(e) => {
                error!("Failed to open sled database at {:?}: {}", db_path, e);
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
        let db_path = dir.as_ref().to_path_buf();
        
        // Try to ensure the directory exists
        if let Err(e) = std::fs::create_dir_all(&db_path) {
            return Err(format!("Failed to create directory for attribute cache: {}", e));
        }
        
        // Try to open the new sled database
        let db = match sled::open(&db_path) {
            Ok(db) => {
                info!("Successfully opened attribute cache database at {:?}", db_path);
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

    /// Remove an item from the cache
    pub fn remove(&mut self, key: &str) -> Result<bool, String> {
        if !self.is_enabled() {
            return Err("Cache is disabled".to_string());
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

    /// Clear the entire cache
    pub fn clear(&mut self) -> Result<(), String> {
        if !self.is_enabled() {
            return Err("Cache is disabled".to_string());
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

    /// Clean up old entries that exceed the maximum age
    pub fn cleanup(&mut self) -> Result<usize, String> {
        // TODO: Implement cleanup of old entries based on timestamps
        // This will require storing timestamps with entries
        Ok(0)
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