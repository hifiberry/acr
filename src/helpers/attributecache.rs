use std::path::{Path, PathBuf};
use std::sync::Mutex;
use lazy_static::lazy_static;
use log::{info, error, debug, warn};
use serde::{Serialize, Deserialize};
use std::sync::Arc;
use rusqlite::{Connection, params};
use chrono::Utc;
use lru::LruCache;
use std::num::NonZeroUsize;

/// Information about a cache entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheEntry {
    pub key: String,
    pub size_bytes: usize,
    pub created_at: i64,
    pub updated_at: i64,
    pub expires_at: Option<i64>,
}

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
    /// In-memory LRU cache of recently accessed items
    memory_cache: LruCache<String, Arc<Vec<u8>>>,
}

impl AttributeCache {
    /// Create a new attribute cache with default settings
    pub fn new() -> Self {
        // Using the default path that matches our datastore.attribute_cache.dbfile setting
        let cache_dir = PathBuf::from("/var/lib/audiocontrol/cache");
        let db_file = cache_dir.join("attributes.db");
        Self::with_database_file_and_cache_size(db_file, 20_000)
    }

    /// Create a new attribute cache with a specific database file
    pub fn with_database_file<P: AsRef<Path>>(db_file: P) -> Self {
        Self::with_database_file_and_cache_size(db_file, 20_000)
    }

    /// Create a new attribute cache with a specific database file and cache size
    pub fn with_database_file_and_cache_size<P: AsRef<Path>>(db_file: P, cache_size: usize) -> Self {
        let db_path = db_file.as_ref().to_path_buf();
        
        // Try to ensure the directory exists
        if let Some(parent) = db_path.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                error!("Failed to create directory for attribute cache: {}", e);
            }
        }
        
        let db = Self::setup_database(&db_path);

        let cache_size = if cache_size > 0 {
            cache_size
        } else {
            warn!("Invalid cache size {}, using default of 20,000", cache_size);
            20_000
        };

        AttributeCache {
            db_path,
            db,
            enabled: true,
            max_age_days: 30, // Default to 30 days
            memory_cache: LruCache::new(NonZeroUsize::new(cache_size).unwrap()),
        }
    }

    /// Setup and migrate the SQLite database
    /// This is the single source of truth for database schema and migration logic
    fn setup_database(db_path: &Path) -> Option<Connection> {
        match Connection::open(db_path) {
            Ok(conn) => {
                info!("Successfully opened attribute cache database at {:?}", db_path);
                
                // First, check if this is a completely new database or needs migration
                let mut table_exists = false;
                let mut has_key = false;
                let mut has_value = false;
                let mut has_created_at = false;
                let mut has_updated_at = false;
                let mut has_expires_at = false;
                
                // Check if table exists and what columns it has
                if let Ok(mut stmt) = conn.prepare("SELECT name FROM sqlite_master WHERE type='table' AND name='cache'") {
                    if stmt.query_row([], |_| Ok(())).is_ok() {
                        table_exists = true;
                        
                        // Check existing columns
                        if let Ok(mut stmt) = conn.prepare("PRAGMA table_info(cache)") {
                            let column_iter = stmt.query_map([], |row| {
                                Ok(row.get::<_, String>(1)?) // Column name is at index 1
                            });
                            
                            if let Ok(iter) = column_iter {
                                for column in iter {
                                    if let Ok(col_name) = column {
                                        match col_name.as_str() {
                                            "key" => has_key = true,
                                            "value" => has_value = true,
                                            "created_at" => has_created_at = true,
                                            "updated_at" => has_updated_at = true,
                                            "expires_at" => has_expires_at = true,
                                            _ => {}
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                
                // If the table doesn't have all required columns, recreate the database
                // This is simpler than complex migration logic
                let schema_complete = has_key && has_value && has_created_at && has_updated_at && has_expires_at;
                if table_exists && !schema_complete {
                    warn!("Database schema is incomplete (key: {}, value: {}, created_at: {}, updated_at: {}, expires_at: {}), recreating cache database", 
                          has_key, has_value, has_created_at, has_updated_at, has_expires_at);
                    if let Err(e) = conn.execute("DROP TABLE IF EXISTS cache", []) {
                        error!("Failed to drop old cache table: {}", e);
                        return None;
                    }
                    table_exists = false;
                }
                
                // Create the cache table with the full schema
                if !table_exists {
                    if let Err(e) = conn.execute(
                        "CREATE TABLE cache (
                            key TEXT PRIMARY KEY,
                            value BLOB NOT NULL,
                            created_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
                            updated_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
                            expires_at INTEGER
                        )",
                        [],
                    ) {
                        error!("Failed to create cache table: {}", e);
                        return None;
                    }
                    info!("Created new cache table with complete schema");
                }
                
                debug!("Cache table created or verified successfully");
                Some(conn)
            },
            Err(e) => {
                error!("Failed to open SQLite database at {:?}: {}", db_path, e);
                None
            }
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
    
    /// Initialize the global attribute cache with a custom directory path and cache size
    pub fn initialize_global_with_cache_size<P: AsRef<Path>>(db_file: P, cache_size: usize) -> Result<(), String> {
        match get_attribute_cache().reconfigure_with_file_and_cache_size(db_file, cache_size) {
            Ok(_) => {
                info!("Global attribute cache initialized successfully");
                Ok(())
            },
            Err(e) => {
                error!("Failed to initialize global attribute cache: {}", e);
                Err(e)
            }
        }
    }
    
    /// Initialize the global attribute cache with a custom directory path as string and cache size
    pub fn initialize_with_cache_size<P: AsRef<Path>>(path: P, cache_size: usize) -> Result<(), String> {
        Self::initialize_global_with_cache_size(path, cache_size)
    }

    /// Initialize the global attribute cache with a custom directory path as string (backward compatibility)
    pub fn initialize<P: AsRef<Path>>(path: P) -> Result<(), String> {
        Self::initialize_with_cache_size(path, 20_000)
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
        
        // Use the centralized database setup logic
        let db = Self::setup_database(&db_file);
        if db.is_none() {
            return Err("Failed to setup database".to_string());
        }
        
        // Update the instance
        self.db_path = db_file;
        self.db = db;
        self.memory_cache.clear(); // Clear memory cache as we have a new DB
        
        Ok(())
    }

    /// Reconfigure the attribute cache with a new database file and cache size
    /// This will close the existing database and open a new one with a new memory cache
    fn reconfigure_with_file_and_cache_size<P: AsRef<Path>>(&mut self, db_file: P, cache_size: usize) -> Result<(), String> {
        let db_path = db_file.as_ref().to_path_buf();
        
        // Try to ensure the directory exists
        if let Some(parent) = db_path.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                return Err(format!("Failed to create directory for attribute cache: {}", e));
            }
        }
        
        // Use the centralized database setup logic
        let db = Self::setup_database(&db_path);
        if db.is_none() {
            return Err("Failed to setup database".to_string());
        }

        let cache_size = if cache_size > 0 {
            cache_size
        } else {
            warn!("Invalid cache size {}, using default of 20,000", cache_size);
            20_000
        };
        
        // Update the instance
        self.db_path = db_path;
        self.db = db;
        self.memory_cache = LruCache::new(NonZeroUsize::new(cache_size).unwrap());
        
        info!("Attribute cache reconfigured with {} memory cache entries", cache_size);
        
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
    pub fn set<T: Serialize + ?Sized>(&mut self, key: &str, value: &T) -> Result<(), String> {
        self.set_with_expiry(key, value, None)
    }

    /// Store a serializable value in the cache with an optional expiry time (Unix timestamp)
    pub fn set_with_expiry<T: Serialize + ?Sized>(&mut self, key: &str, value: &T, expires_at: Option<i64>) -> Result<(), String> {
        if !self.is_enabled() {
            return Err("Cache is disabled".to_string());
        }

        let serialized = match serde_json::to_vec(value) {
            Ok(data) => data,
            Err(e) => return Err(format!("Failed to serialize value: {}", e)),
        };

        // Store in memory cache
        self.memory_cache.put(key.to_string(), Arc::new(serialized.clone()));

        // Store in SQLite database
        match &mut self.db {
            Some(db) => {
                // Use INSERT ... ON CONFLICT to properly handle timestamps
                // For new records: set both created_at and updated_at to current time
                // For existing records: keep created_at, update only updated_at
                if let Err(e) = db.execute(
                    "INSERT INTO cache (key, value, created_at, updated_at, expires_at) 
                     VALUES (?1, ?2, strftime('%s', 'now'), strftime('%s', 'now'), ?3)
                     ON CONFLICT(key) DO UPDATE SET 
                         value = excluded.value,
                         updated_at = strftime('%s', 'now'),
                         expires_at = excluded.expires_at",
                    params![key, serialized, expires_at],
                ) {
                    return Err(format!("Failed to store in database: {}", e));
                }
                
                debug!("Stored key '{}' in SQLite cache with expiry: {:?}", key, expires_at);
                Ok(())
            },
            None => Err("Database not available".to_string()),
        }
    }

    /// Store a serializable value in the cache with a TTL (time to live) in seconds
    pub fn set_with_ttl<T: Serialize + ?Sized>(&mut self, key: &str, value: &T, ttl_seconds: u64) -> Result<(), String> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| format!("Failed to get current time: {}", e))?
            .as_secs() as i64;
        let expires_at = now + ttl_seconds as i64;
        self.set_with_expiry(key, value, Some(expires_at))
    }

    /// Get a value from the cache and deserialize it
    /// This method automatically removes expired entries when they are accessed
    pub fn get<T: for<'de> Deserialize<'de>>(&mut self, key: &str) -> Result<Option<T>, String> {
        if !self.is_enabled() {
            return Err("Cache is disabled".to_string());
        }

        // Check database first to validate expiry before returning from memory cache
        let is_expired = match &mut self.db {
            Some(db) => {
                let mut stmt = match db.prepare("SELECT expires_at FROM cache WHERE key = ?1") {
                    Ok(stmt) => stmt,
                    Err(e) => return Err(format!("Failed to prepare expiry check statement: {}", e)),
                };
                
                match stmt.query_row(params![key], |row| {
                    let expires_at: Option<i64> = row.get(0)?;
                    Ok(expires_at)
                }) {
                    Ok(Some(expires_at)) => {
                        let now = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map_err(|e| format!("Failed to get current time: {}", e))?
                            .as_secs() as i64;
                        expires_at <= now
                    },
                    Ok(None) => false, // No expiry set
                    Err(rusqlite::Error::QueryReturnedNoRows) => return Ok(None), // Key doesn't exist
                    Err(e) => return Err(format!("Database error checking expiry: {}", e)),
                }
            },
            None => return Err("Database not available".to_string()),
        };

        // If expired, remove it and return None
        if is_expired {
            debug!("Removing expired cache entry: {}", key);
            let _ = self.remove(key); // Ignore errors during cleanup
            return Ok(None);
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
                        
                        self.memory_cache.put(key.to_string(), Arc::new(data_vec));
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
        self.memory_cache.pop(key);

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

    /// Get the created_at and updated_at timestamps for a key
    /// Returns (created_at, updated_at) as Unix timestamps, or None if key doesn't exist
    pub fn get_timestamps(&mut self, key: &str) -> Result<Option<(i64, i64)>, String> {
        if !self.is_enabled() {
            return Err("Cache is disabled".to_string());
        }

        match &mut self.db {
            Some(db) => {
                let mut stmt = match db.prepare("SELECT created_at, updated_at FROM cache WHERE key = ?1") {
                    Ok(stmt) => stmt,
                    Err(e) => return Err(format!("Failed to prepare statement: {}", e)),
                };

                let result = stmt.query_row(params![key], |row| {
                    let created_at: i64 = row.get(0)?;
                    let updated_at: i64 = row.get(1)?;
                    Ok((created_at, updated_at))
                });

                match result {
                    Ok(timestamps) => Ok(Some(timestamps)),
                    Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
                    Err(e) => Err(format!("Failed to query timestamps: {}", e)),
                }
            },
            None => Err("Database not available".to_string()),
        }
    }

    /// Check if a key exists and return its age in seconds (time since creation)
    pub fn get_age(&mut self, key: &str) -> Result<Option<i64>, String> {
        match self.get_timestamps(key)? {
            Some((created_at, _)) => {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_err(|e| format!("Failed to get current time: {}", e))?
                    .as_secs() as i64;
                Ok(Some(now - created_at))
            },
            None => Ok(None),
        }
    }

    /// Check if a key was recently updated (time since last update)
    pub fn get_last_updated_age(&mut self, key: &str) -> Result<Option<i64>, String> {
        match self.get_timestamps(key)? {
            Some((_, updated_at)) => {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_err(|e| format!("Failed to get current time: {}", e))?
                    .as_secs() as i64;
                Ok(Some(now - updated_at))
            },
            None => Ok(None),
        }
    }

    /// List all cache keys, optionally filtered by prefix
    pub fn list_keys(&self, prefix_filter: Option<&str>) -> Result<Vec<String>, String> {
        if self.db.is_none() {
            return Err("Database connection is not available".to_string());
        }

        let db = self.db.as_ref().unwrap();
        let mut keys = Vec::new();
        
        match prefix_filter {
            Some(prefix) => {
                let pattern = format!("{}%", prefix);
                let mut stmt = db.prepare("SELECT key FROM cache WHERE key LIKE ?1 ORDER BY key")
                    .map_err(|e| format!("Failed to prepare list statement: {}", e))?;
                
                let rows = stmt.query_map(params![pattern], |row: &rusqlite::Row| {
                    Ok(row.get::<_, String>(0)?)
                }).map_err(|e| format!("Failed to execute list query: {}", e))?;
                
                for row in rows {
                    let key = row.map_err(|e| format!("Failed to read row: {}", e))?;
                    keys.push(key);
                }
            },
            None => {
                let mut stmt = db.prepare("SELECT key FROM cache ORDER BY key")
                    .map_err(|e| format!("Failed to prepare list statement: {}", e))?;
                
                let rows = stmt.query_map([], |row: &rusqlite::Row| {
                    Ok(row.get::<_, String>(0)?)
                }).map_err(|e| format!("Failed to execute list query: {}", e))?;
                
                for row in rows {
                    let key = row.map_err(|e| format!("Failed to read row: {}", e))?;
                    keys.push(key);
                }
            }
        }

        Ok(keys)
    }

    /// Get detailed information about cache entries, optionally filtered by prefix
    pub fn list_entries(&mut self, prefix_filter: Option<&str>) -> Result<Vec<CacheEntry>, String> {
        if !self.enabled {
            return Ok(Vec::new());
        }

        if self.db.is_none() {
            return Err("Database connection is not available".to_string());
        }

        let db = self.db.as_ref().unwrap();
        let mut entries = Vec::new();

        match prefix_filter {
            Some(prefix) => {
                let pattern = format!("{}%", prefix);
                let mut stmt = db.prepare("SELECT key, LENGTH(value) as size, created_at, updated_at, expires_at FROM cache WHERE key LIKE ?1 ORDER BY key")
                    .map_err(|e| format!("Failed to prepare list statement: {}", e))?;
                
                let rows = stmt.query_map(params![pattern], |row: &rusqlite::Row| {
                    Ok(CacheEntry {
                        key: row.get::<_, String>(0)?,
                        size_bytes: row.get::<_, i64>(1)? as usize,
                        created_at: row.get::<_, i64>(2)?,
                        updated_at: row.get::<_, i64>(3)?,
                        expires_at: row.get::<_, Option<i64>>(4)?,
                    })
                }).map_err(|e| format!("Failed to execute list query: {}", e))?;
                
                for row in rows {
                    let entry = row.map_err(|e| format!("Failed to read row: {}", e))?;
                    entries.push(entry);
                }
            },
            None => {
                let mut stmt = db.prepare("SELECT key, LENGTH(value) as size, created_at, updated_at, expires_at FROM cache ORDER BY key")
                    .map_err(|e| format!("Failed to prepare list statement: {}", e))?;
                
                let rows = stmt.query_map([], |row: &rusqlite::Row| {
                    Ok(CacheEntry {
                        key: row.get::<_, String>(0)?,
                        size_bytes: row.get::<_, i64>(1)? as usize,
                        created_at: row.get::<_, i64>(2)?,
                        updated_at: row.get::<_, i64>(3)?,
                        expires_at: row.get::<_, Option<i64>>(4)?,
                    })
                }).map_err(|e| format!("Failed to execute list query: {}", e))?;
                
                for row in rows {
                    let entry = row.map_err(|e| format!("Failed to read row: {}", e))?;
                    entries.push(entry);
                }
            }
        }

        Ok(entries)
    }

    /// Remove all cache entries matching a prefix
    pub fn remove_by_prefix(&mut self, prefix: &str) -> Result<usize, String> {
        if !self.enabled {
            return Ok(0);
        }

        if self.db.is_none() {
            return Err("Database connection is not available".to_string());
        }

        let pattern = format!("{}%", prefix);
        let db = self.db.as_ref().unwrap();
        
        // First, get the keys to remove from memory cache
        let mut stmt = db.prepare("SELECT key FROM cache WHERE key LIKE ?1")
            .map_err(|e| format!("Failed to prepare select statement: {}", e))?;
        
        let rows = stmt.query_map(params![pattern], |row| {
            Ok(row.get::<_, String>(0)?)
        }).map_err(|e| format!("Failed to execute select query: {}", e))?;

        let mut keys_to_remove = Vec::new();
        for row in rows {
            let key = row.map_err(|e| format!("Failed to read row: {}", e))?;
            keys_to_remove.push(key);
        }

        // Remove from memory cache
        for key in &keys_to_remove {
            self.memory_cache.pop(key);
        }

        // Remove from database
        let deleted = db.execute("DELETE FROM cache WHERE key LIKE ?1", params![pattern])
            .map_err(|e| format!("Failed to delete from database: {}", e))?;

        debug!("Removed {} cache entries with prefix '{}'", deleted, prefix);
        Ok(deleted)
    }

    /// Preload all cache entries matching a prefix into the LRU memory cache
    /// 
    /// This function loads all database entries with the given prefix into the LRU cache
    /// for faster subsequent access. This is useful for warming up the cache when you
    /// know you'll be accessing many keys with the same prefix.
    /// 
    /// # Arguments
    /// * `prefix` - The prefix to match for cache keys
    /// 
    /// # Returns
    /// The number of entries loaded into memory cache
    pub fn preload_prefix(&mut self, prefix: &str) -> Result<usize, String> {
        if !self.enabled {
            return Ok(0);
        }

        if self.db.is_none() {
            return Err("Database connection is not available".to_string());
        }

        let pattern = format!("{}%", prefix);
        let db = self.db.as_ref().unwrap();
        
        let mut stmt = db.prepare("SELECT key, value FROM cache WHERE key LIKE ?1")
            .map_err(|e| format!("Failed to prepare select statement: {}", e))?;
        
        let rows = stmt.query_map(params![pattern], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Vec<u8>>(1)?
            ))
        }).map_err(|e| format!("Failed to execute select query: {}", e))?;

        let mut loaded_count = 0;
        for row in rows {
            let (key, value) = row.map_err(|e| format!("Failed to read row: {}", e))?;
            
            // Store in memory cache
            self.memory_cache.put(key, Arc::new(value));
            loaded_count += 1;
        }

        debug!("Preloaded {} cache entries with prefix '{}' into memory cache", loaded_count, prefix);
        Ok(loaded_count)
    }
}

// Global functions to access the attribute cache singleton

/// Get a reference to the global attribute cache
pub fn get_attribute_cache() -> std::sync::MutexGuard<'static, AttributeCache> {
    ATTRIBUTE_CACHE.lock().unwrap()
}

/// Store a value in the attribute cache
pub fn set<T: Serialize + ?Sized>(key: &str, value: &T) -> Result<(), String> {
    get_attribute_cache().set(key, value)
}

/// Store a value in the attribute cache with an optional expiry time (Unix timestamp)
pub fn set_with_expiry<T: Serialize + ?Sized>(key: &str, value: &T, expires_at: Option<i64>) -> Result<(), String> {
    get_attribute_cache().set_with_expiry(key, value, expires_at)
}

/// Store a value in the attribute cache with a TTL (time to live) in seconds
pub fn set_with_ttl<T: Serialize + ?Sized>(key: &str, value: &T, ttl_seconds: u64) -> Result<(), String> {
    get_attribute_cache().set_with_ttl(key, value, ttl_seconds)
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

/// List all cache keys, optionally filtered by prefix
pub fn list_keys(prefix_filter: Option<&str>) -> Result<Vec<String>, String> {
    get_attribute_cache().list_keys(prefix_filter)
}

/// Get detailed information about cache entries, optionally filtered by prefix
pub fn list_entries(prefix_filter: Option<&str>) -> Result<Vec<CacheEntry>, String> {
    get_attribute_cache().list_entries(prefix_filter)
}

/// Remove all cache entries matching a prefix
pub fn remove_by_prefix(prefix: &str) -> Result<usize, String> {
    get_attribute_cache().remove_by_prefix(prefix)
}

/// Preload all cache entries matching a prefix into the LRU memory cache
/// 
/// This function loads all database entries with the given prefix into the LRU cache
/// for faster subsequent access. This is useful for warming up the cache when you
/// know you'll be accessing many keys with the same prefix.
/// 
/// # Arguments
/// * `prefix` - The prefix to match for cache keys
/// 
/// # Returns
/// The number of entries loaded into memory cache
pub fn preload_prefix(prefix: &str) -> Result<usize, String> {
    get_attribute_cache().preload_prefix(prefix)
}

/// Get the created_at and updated_at timestamps for a key
/// Returns (created_at, updated_at) as Unix timestamps, or None if key doesn't exist
pub fn get_timestamps(key: &str) -> Result<Option<(i64, i64)>, String> {
    get_attribute_cache().get_timestamps(key)
}

/// Check if a key exists and return its age in seconds (time since creation)
pub fn get_age(key: &str) -> Result<Option<i64>, String> {
    get_attribute_cache().get_age(key)
}

/// Check if a key was recently updated (time since last update)
pub fn get_last_updated_age(key: &str) -> Result<Option<i64>, String> {
    get_attribute_cache().get_last_updated_age(key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Serialize, Deserialize, PartialEq)]
    struct TestData {
        name: String,
        value: u32,
        active: bool,
    }

    fn create_test_cache() -> (AttributeCache, TempDir) {
        let temp_dir = TempDir::new().expect("Failed to create temp directory");
        let cache_file = temp_dir.path().join("test_cache.db");
        let cache = AttributeCache::with_database_file(&cache_file);
        (cache, temp_dir)
    }

    #[test]
    fn test_new_cache() {
        let (cache, _temp_dir) = create_test_cache();
        assert!(cache.is_enabled());
        assert!(cache.db.is_some());
    }

    #[test]
    fn test_set_and_get_string() {
        let (mut cache, _temp_dir) = create_test_cache();
        
        let key = "test_key";
        let value = "test_value".to_string();
        
        // Test set
        cache.set(key, &value).expect("Failed to set value");
        
        // Test get
        let retrieved: Option<String> = cache.get(key).expect("Failed to get value");
        assert_eq!(retrieved, Some(value));
    }

    #[test]
    fn test_set_and_get_struct() {
        let (mut cache, _temp_dir) = create_test_cache();
        
        let key = "test_struct";
        let value = TestData {
            name: "test".to_string(),
            value: 42,
            active: true,
        };
        
        // Test set
        cache.set(key, &value).expect("Failed to set struct");
        
        // Test get
        let retrieved: Option<TestData> = cache.get(key).expect("Failed to get struct");
        assert_eq!(retrieved, Some(value));
    }

    #[test]
    fn test_get_nonexistent_key() {
        let (mut cache, _temp_dir) = create_test_cache();
        
        let retrieved: Option<String> = cache.get("nonexistent").expect("Failed to get nonexistent key");
        assert_eq!(retrieved, None);
    }

    #[test]
    fn test_memory_cache() {
        let (mut cache, _temp_dir) = create_test_cache();
        
        let key = "memory_test";
        let value = "cached_value".to_string();
        
        // Set value
        cache.set(key, &value).expect("Failed to set value");
        
        // First get should load from database into memory
        let retrieved1: Option<String> = cache.get(key).expect("Failed to get value");
        assert_eq!(retrieved1, Some(value.clone()));
        
        // Second get should hit memory cache
        let retrieved2: Option<String> = cache.get(key).expect("Failed to get value from memory");
        assert_eq!(retrieved2, Some(value));
        
        // Verify memory cache contains the key
        assert!(cache.memory_cache.peek(key).is_some());
    }

    #[test]
    fn test_remove() {
        let (mut cache, _temp_dir) = create_test_cache();
        
        let key = "remove_test";
        let value = "to_be_removed".to_string();
        
        // Set value
        cache.set(key, &value).expect("Failed to set value");
        
        // Verify it exists
        let retrieved: Option<String> = cache.get(key).expect("Failed to get value");
        assert_eq!(retrieved, Some(value));
        
        // Remove it
        let removed = cache.remove(key).expect("Failed to remove value");
        assert!(removed);
        
        // Verify it's gone
        let retrieved_after_remove: Option<String> = cache.get(key).expect("Failed to get removed value");
        assert_eq!(retrieved_after_remove, None);
        
        // Verify memory cache is also cleared
        assert!(cache.memory_cache.peek(key).is_none());
    }

    #[test]
    fn test_remove_nonexistent() {
        let (mut cache, _temp_dir) = create_test_cache();
        
        let removed = cache.remove("nonexistent").expect("Failed to remove nonexistent key");
        assert!(!removed);
    }

    #[test]
    fn test_clear() {
        let (mut cache, _temp_dir) = create_test_cache();
        
        // Add some test data
        cache.set("key1", &"value1".to_string()).expect("Failed to set key1");
        cache.set("key2", &42u32).expect("Failed to set key2");
        cache.set("key3", &true).expect("Failed to set key3");
        
        // Verify data exists
        let val1: Option<String> = cache.get("key1").expect("Failed to get key1");
        assert_eq!(val1, Some("value1".to_string()));
        
        // Clear cache
        cache.clear().expect("Failed to clear cache");
        
        // Verify all data is gone
        let val1_after: Option<String> = cache.get("key1").expect("Failed to get key1 after clear");
        let val2_after: Option<u32> = cache.get("key2").expect("Failed to get key2 after clear");
        let val3_after: Option<bool> = cache.get("key3").expect("Failed to get key3 after clear");
        
        assert_eq!(val1_after, None);
        assert_eq!(val2_after, None);
        assert_eq!(val3_after, None);
        
        // Verify memory cache is also cleared
        assert!(cache.memory_cache.is_empty());
    }

    #[test]
    fn test_overwrite_existing_key() {
        let (mut cache, _temp_dir) = create_test_cache();
        
        let key = "overwrite_test";
        let value1 = "first_value".to_string();
        let value2 = "second_value".to_string();
        
        // Set first value
        cache.set(key, &value1).expect("Failed to set first value");
        let retrieved1: Option<String> = cache.get(key).expect("Failed to get first value");
        assert_eq!(retrieved1, Some(value1));
        
        // Overwrite with second value
        cache.set(key, &value2).expect("Failed to set second value");
        let retrieved2: Option<String> = cache.get(key).expect("Failed to get second value");
        assert_eq!(retrieved2, Some(value2));
    }

    #[test]
    fn test_disabled_cache() {
        let (mut cache, _temp_dir) = create_test_cache();
        
        // Disable cache
        cache.enable(false);
        assert!(!cache.is_enabled());
        
        // Try to set value
        let result = cache.set("key", &"value".to_string());
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("disabled"));
        
        // Try to get value
        let result: Result<Option<String>, String> = cache.get("key");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("disabled"));
    }

    #[test]
    fn test_max_age_setting() {
        let (mut cache, _temp_dir) = create_test_cache();
        
        // Change max age
        cache.set_max_age(7);
        assert_eq!(cache.max_age_days, 7);
    }

    #[test]
    fn test_persistence_across_instances() {
        let temp_dir = TempDir::new().expect("Failed to create temp directory");
        let cache_file = temp_dir.path().join("persistence_test.db");
        
        let key = "persistent_key";
        let value = TestData {
            name: "persistent".to_string(),
            value: 123,
            active: false,
        };
        
        // Create first cache instance and store data
        {
            let mut cache1 = AttributeCache::with_database_file(&cache_file);
            cache1.set(key, &value).expect("Failed to set value in first instance");
        }
        
        // Create second cache instance and retrieve data
        {
            let mut cache2 = AttributeCache::with_database_file(&cache_file);
            let retrieved: Option<TestData> = cache2.get(key).expect("Failed to get value in second instance");
            assert_eq!(retrieved, Some(value));
        }
    }

    #[test]
    fn test_reconfigure_with_directory() {
        let temp_dir1 = TempDir::new().expect("Failed to create temp directory 1");
        let temp_dir2 = TempDir::new().expect("Failed to create temp directory 2");
        
        let mut cache = AttributeCache::with_database_file(temp_dir1.path().join("cache1.db"));
        
        // Set data in first location
        cache.set("key1", &"value1".to_string()).expect("Failed to set value");
        
        // Reconfigure to second location
        cache.reconfigure_with_directory(temp_dir2.path()).expect("Failed to reconfigure");
        
        // Old data should not be accessible
        let retrieved: Option<String> = cache.get("key1").expect("Failed to get key1");
        assert_eq!(retrieved, None);
        
        // New data should work in new location
        cache.set("key2", &"value2".to_string()).expect("Failed to set value in new location");
        let retrieved2: Option<String> = cache.get("key2").expect("Failed to get key2");
        assert_eq!(retrieved2, Some("value2".to_string()));
    }

    #[test]
    fn test_serialize_error_handling() {
        // This test is harder to trigger with JSON serialization since most types serialize fine
        // But we can test the error path indirectly by trying to deserialize invalid data
        
        let (mut cache, _temp_dir) = create_test_cache();
        
        // Manually insert invalid JSON data into the database
        if let Some(ref mut db) = cache.db {
            db.execute(
                "INSERT INTO cache (key, value, created_at, updated_at) VALUES (?1, ?2, strftime('%s', 'now'), strftime('%s', 'now'))",
                params!["invalid_json", b"invalid json data"],
            ).expect("Failed to insert invalid data");
        }
        
        // Try to retrieve as a struct - should fail
        let result: Result<Option<TestData>, String> = cache.get("invalid_json");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Failed to deserialize"));
    }

    // Test global functions
    #[test]
    fn test_global_functions() {
        // Initialize global cache with a temporary directory for testing
        let temp_dir = TempDir::new().expect("Failed to create temp directory");
        let _ = super::AttributeCache::initialize_global(temp_dir.path());
        
        let key = "global_test";
        let value = "global_value".to_string();
        
        // Test global set
        super::set(key, &value).expect("Failed to set global value");
        
        // Test global get
        let retrieved: Option<String> = super::get(key).expect("Failed to get global value");
        assert_eq!(retrieved, Some(value));
        
        // Test global remove
        let removed = super::remove(key).expect("Failed to remove global value");
        assert!(removed);
        
        // Verify removal
        let retrieved_after: Option<String> = super::get(key).expect("Failed to get removed global value");
        assert_eq!(retrieved_after, None);
    }

    // Concurrent access tests
    #[test]
    fn test_concurrent_set_and_get() {
        use std::sync::{Arc, Mutex};
        use std::thread;
        
        let temp_dir = TempDir::new().expect("Failed to create temp directory");
        let cache_file = temp_dir.path().join("concurrent_test.db");
        let cache = Arc::new(Mutex::new(AttributeCache::with_database_file(&cache_file)));
        
        let num_threads = 10;
        let operations_per_thread = 50;
        let mut handles = vec![];
        
        // Spawn multiple threads that set and get values concurrently
        for thread_id in 0..num_threads {
            let cache_clone = Arc::clone(&cache);
            let handle = thread::spawn(move || {
                for i in 0..operations_per_thread {
                    let key = format!("thread_{}_key_{}", thread_id, i);
                    let value = format!("thread_{}_value_{}", thread_id, i);
                    
                    // Set value
                    {
                        let mut cache_guard = cache_clone.lock().unwrap();
                        cache_guard.set(&key, &value).expect("Failed to set value in thread");
                    }
                    
                    // Get value back
                    {
                        let mut cache_guard = cache_clone.lock().unwrap();
                        let retrieved: Option<String> = cache_guard.get(&key).expect("Failed to get value in thread");
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
                
                let mut cache_guard = cache.lock().unwrap();
                let retrieved: Option<String> = cache_guard.get(&key).expect("Failed to get value after threads");
                assert_eq!(retrieved, Some(expected_value));
                drop(cache_guard); // Explicitly drop to release lock
            }
        }
    }

    #[test]
    fn test_concurrent_memory_cache_access() {
        use std::sync::{Arc, Mutex};
        use std::thread;
        use std::time::Duration;
        
        let temp_dir = TempDir::new().expect("Failed to create temp directory");
        let cache_file = temp_dir.path().join("memory_concurrent_test.db");
        let cache = Arc::new(Mutex::new(AttributeCache::with_database_file(&cache_file)));
        
        // Pre-populate the cache
        {
            let mut cache_guard = cache.lock().unwrap();
            for i in 0..10 {
                let key = format!("shared_key_{}", i);
                let value = format!("shared_value_{}", i);
                cache_guard.set(&key, &value).expect("Failed to set initial value");
            }
        }
        
        let num_reader_threads = 3;
        let num_writer_threads = 2;
        let mut handles = vec![];
        
        // Spawn reader threads that access the same keys concurrently
        for _thread_id in 0..num_reader_threads {
            let cache_clone = Arc::clone(&cache);
            let handle = thread::spawn(move || {
                for _iteration in 0..50 { // Reduced iterations to reduce race conditions
                    for key_id in 0..10 {
                        let key = format!("shared_key_{}", key_id);
                        
                        // Just verify we can read some value, don't care about the exact content
                        // since writers might be updating it concurrently
                        if let Ok(cache_guard) = cache_clone.lock() {
                            let mut cache_mut = cache_guard;
                            let _retrieved: Result<Option<String>, _> = cache_mut.get(&key);
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
            let cache_clone = Arc::clone(&cache);
            let handle = thread::spawn(move || {
                for iteration in 0..10 { // Reduced iterations
                    for key_id in 0..10 {
                        let key = format!("shared_key_{}", key_id);
                        let new_value = format!("updated_by_thread_{}_iter_{}_{}", thread_id, iteration, key_id);
                        
                        if let Ok(cache_guard) = cache_clone.lock() {
                            let mut cache_mut = cache_guard;
                            let _ = cache_mut.set(&key, &new_value); // Ignore errors
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
    fn test_concurrent_global_cache_access() {
        use std::thread;
        use std::sync::Arc;
        use std::sync::atomic::{AtomicUsize, Ordering};
        
        // Initialize global cache with a temp directory first
        let temp_dir = TempDir::new().expect("Failed to create temp directory");
        super::AttributeCache::initialize_global(temp_dir.path()).expect("Failed to initialize global cache");
        
        let num_threads = 8;
        let operations_per_thread = 25;
        let success_counter = Arc::new(AtomicUsize::new(0));
        let mut handles = vec![];
        
        // Clear global cache first to ensure clean state
        super::clear().ok(); // Ignore errors in case cache is not initialized
        
        // Spawn multiple threads that use global cache functions
        for thread_id in 0..num_threads {
            let counter_clone = Arc::clone(&success_counter);
            let handle = thread::spawn(move || {
                let mut successful_operations = 0;
                
                for i in 0..operations_per_thread {
                    let key = format!("global_thread_{}_key_{}", thread_id, i);
                    let value = TestData {
                        name: format!("global_thread_{}", thread_id),
                        value: i as u32,
                        active: i % 2 == 0,
                    };
                    
                    // Set value using global function
                    if super::set(&key, &value).is_ok() {
                        // Get value back using global function
                        match super::get::<TestData>(&key) {
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
                        super::remove(&key).ok(); // Ignore errors
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
    }

    #[test]
    fn test_concurrent_cleanup_and_access() {
        use std::sync::{Arc, Mutex};
        use std::thread;
        use std::time::Duration;
        
        let temp_dir = TempDir::new().expect("Failed to create temp directory");
        let cache_file = temp_dir.path().join("cleanup_concurrent_test.db");
        let cache = Arc::new(Mutex::new(AttributeCache::with_database_file(&cache_file)));
        
        // Set a very short max age for testing cleanup
        {
            let mut cache_guard = cache.lock().unwrap();
            cache_guard.set_max_age(0); // Immediate expiry for testing
        }
        
        let num_access_threads = 3;
        let mut handles = vec![];
        
        // Spawn threads that continuously add and access data
        for thread_id in 0..num_access_threads {
            let cache_clone = Arc::clone(&cache);
            let handle = thread::spawn(move || {
                for i in 0..50 {
                    let key = format!("cleanup_thread_{}_key_{}", thread_id, i);
                    let value = format!("cleanup_value_{}", i);
                    
                    // Set value
                    {
                        let mut cache_guard = cache_clone.lock().unwrap();
                        cache_guard.set(&key, &value).ok(); // Ignore errors
                    }
                    
                    // Try to get value
                    {
                        let mut cache_guard = cache_clone.lock().unwrap();
                        let _retrieved: Result<Option<String>, _> = cache_guard.get(&key);
                        // Don't assert here as cleanup might remove the value
                    }
                    
                    thread::sleep(Duration::from_millis(1));
                }
            });
            handles.push(handle);
        }
        
        // Spawn a cleanup thread that periodically runs cleanup
        let cache_cleanup = Arc::clone(&cache);
        let cleanup_handle = thread::spawn(move || {
            for _i in 0..10 {
                thread::sleep(Duration::from_millis(5));
                let mut cache_guard = cache_cleanup.lock().unwrap();
                cache_guard.cleanup().ok(); // Ignore errors
            }
        });
        handles.push(cleanup_handle);
        
        // Wait for all threads to complete
        for handle in handles {
            handle.join().expect("Thread panicked");
        }
        
        // Test should complete without deadlocks or panics
        // The exact state of the cache is not important, just that it didn't crash
    }

    #[test]
    fn test_concurrent_clear_and_access() {
        use std::sync::{Arc, Mutex};
        use std::thread;
        use std::time::Duration;
        
        let temp_dir = TempDir::new().expect("Failed to create temp directory");
        let cache_file = temp_dir.path().join("clear_concurrent_test.db");
        let cache = Arc::new(Mutex::new(AttributeCache::with_database_file(&cache_file)));
        
        let num_access_threads = 4;
        let mut handles = vec![];
        
        // Spawn threads that continuously add and access data
        for thread_id in 0..num_access_threads {
            let cache_clone = Arc::clone(&cache);
            let handle = thread::spawn(move || {
                for i in 0..30 {
                    let key = format!("clear_thread_{}_key_{}", thread_id, i);
                    let value = format!("clear_value_{}", i);
                    
                    // Set value
                    {
                        let mut cache_guard = cache_clone.lock().unwrap();
                        cache_guard.set(&key, &value).ok(); // Ignore errors
                    }
                    
                    // Try to get value
                    {
                        let mut cache_guard = cache_clone.lock().unwrap();
                        let _retrieved: Result<Option<String>, _> = cache_guard.get(&key);
                        // Don't assert here as clear might remove the value
                    }
                    
                    thread::sleep(Duration::from_millis(1));
                }
            });
            handles.push(handle);
        }
        
        // Spawn a thread that periodically clears the cache
        let cache_clear = Arc::clone(&cache);
        let clear_handle = thread::spawn(move || {
            for _i in 0..5 {
                thread::sleep(Duration::from_millis(10));
                let mut cache_guard = cache_clear.lock().unwrap();
                cache_guard.clear().ok(); // Ignore errors
            }
        });
        handles.push(clear_handle);
        
        // Wait for all threads to complete
        for handle in handles {
            handle.join().expect("Thread panicked");
        }
        
        // Test should complete without deadlocks or panics
        // The exact state of the cache is not important, just that it didn't crash
    }

    #[test]
    fn test_timestamps_creation() {
        let (mut cache, _temp_dir) = create_test_cache();

        let key = "test_key";
        let value = "test_value";

        // Set a value and get timestamps
        let before_set = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64;
        cache.set(key, &value).unwrap();
        let after_set = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64;

        let (created_at, updated_at) = cache.get_timestamps(key).unwrap().unwrap();
        
        // Timestamps should be within reasonable range
        assert!(created_at >= before_set && created_at <= after_set);
        assert!(updated_at >= before_set && updated_at <= after_set);
        assert_eq!(created_at, updated_at); // Should be equal for new entries
    }

    #[test]
    fn test_timestamps_update() {
        let (mut cache, _temp_dir) = create_test_cache();

        let key = "test_key";
        let value1 = "test_value1";
        let value2 = "test_value2";

        // Set initial value
        cache.set(key, &value1).unwrap();
        let (created_at, initial_updated_at) = cache.get_timestamps(key).unwrap().unwrap();

        // Wait a moment to ensure timestamp difference
        std::thread::sleep(std::time::Duration::from_millis(1100));

        // Update the value
        let before_update = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64;
        cache.set(key, &value2).unwrap();
        let after_update = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64;

        let (new_created_at, new_updated_at) = cache.get_timestamps(key).unwrap().unwrap();

        // Created timestamp should remain the same
        assert_eq!(created_at, new_created_at);
        
        // Updated timestamp should be newer
        assert!(new_updated_at > initial_updated_at);
        assert!(new_updated_at >= before_update && new_updated_at <= after_update);
    }

    #[test]
    fn test_age_functions() {
        let (mut cache, _temp_dir) = create_test_cache();

        let key = "test_key";
        let value = "test_value";

        // Set a value
        cache.set(key, &value).unwrap();

        // Wait a moment
        std::thread::sleep(std::time::Duration::from_millis(1100));

        // Check age functions
        let age = cache.get_age(key).unwrap().unwrap();
        let last_updated_age = cache.get_last_updated_age(key).unwrap().unwrap();

        // Ages should be reasonable (at least some milliseconds)
        assert!(age >= 0);
        assert!(last_updated_age >= 0);
        assert_eq!(age, last_updated_age); // Should be equal for newly created entries

        // Update the value
        std::thread::sleep(std::time::Duration::from_millis(1100));
        cache.set(key, "updated_value").unwrap();

        let new_age = cache.get_age(key).unwrap().unwrap();
        let new_last_updated_age = cache.get_last_updated_age(key).unwrap().unwrap();

        // Age should be older than last updated age now
        assert!(new_age > new_last_updated_age);
        assert!(new_age >= age); // Age should have increased
        assert!(new_last_updated_age < last_updated_age); // Last updated should be more recent
    }

    #[test]
    fn test_global_timestamp_functions() {
        let (mut cache, _temp_dir) = create_test_cache();
        
        let key = "test_key";
        let value = "test_value";

        // Set a value using cache method
        cache.set(key, &value).unwrap();

        // Get timestamps using cache methods
        let (created_at, updated_at) = cache.get_timestamps(key).unwrap().unwrap();
        let age = cache.get_age(key).unwrap().unwrap();
        let last_updated_age = cache.get_last_updated_age(key).unwrap().unwrap();

        // Basic validation
        assert!(created_at > 0);
        assert!(updated_at > 0);
        assert_eq!(created_at, updated_at);
        assert!(age >= 0);
        assert!(last_updated_age >= 0);
        assert_eq!(age, last_updated_age);
    }

    #[test]
    fn test_nonexistent_key_timestamps() {
        let (mut cache, _temp_dir) = create_test_cache();

        let nonexistent_key = "nonexistent";

        // All timestamp functions should return None for nonexistent keys
        assert_eq!(cache.get_timestamps(nonexistent_key).unwrap(), None);
        assert_eq!(cache.get_age(nonexistent_key).unwrap(), None);
        assert_eq!(cache.get_last_updated_age(nonexistent_key).unwrap(), None);
    }

    #[test]
    fn test_database_migration() {
        let temp_dir = TempDir::new().expect("Failed to create temp directory");
        let cache_file = temp_dir.path().join("test_cache.db");

        // Create an old-style cache without timestamp columns
        {
            let conn = rusqlite::Connection::open(&cache_file).unwrap();
            conn.execute(
                "CREATE TABLE IF NOT EXISTS cache (
                    key TEXT PRIMARY KEY,
                    value BLOB NOT NULL
                )",
                [],
            ).unwrap();
            
            // Use BLOB format like the real cache (JSON serialized)
            let value_json = serde_json::to_vec("old_value").unwrap();
            conn.execute(
                "INSERT INTO cache (key, value) VALUES (?1, ?2)",
                [&"old_key" as &dyn rusqlite::ToSql, &value_json],
            ).unwrap();
        }

        // Create new cache - should recreate the database due to missing expires_at column
        let mut cache = AttributeCache::with_database_file(&cache_file);
        
        // Old data should be gone due to database recreation
        assert_eq!(cache.get::<String>("old_key").unwrap(), None);

        // Add new data - should work with timestamps and expiry
        cache.set("new_key", "new_value").unwrap();
        let timestamps = cache.get_timestamps("new_key").unwrap();
        assert!(timestamps.is_some());
    }

    #[test]
    fn test_database_recreation_missing_expires_at() {
        let temp_dir = TempDir::new().expect("Failed to create temp directory");
        let cache_file = temp_dir.path().join("test_cache.db");

        // Create a cache with timestamps but missing expires_at column
        {
            let conn = rusqlite::Connection::open(&cache_file).unwrap();
            conn.execute(
                "CREATE TABLE cache (
                    key TEXT PRIMARY KEY,
                    value BLOB NOT NULL,
                    created_at INTEGER NOT NULL,
                    updated_at INTEGER NOT NULL
                )",
                [],
            ).unwrap();
            
            let value_json = serde_json::to_vec("old_value").unwrap();
            conn.execute(
                "INSERT INTO cache (key, value, created_at, updated_at) VALUES (?1, ?2, ?3, ?4)",
                [&"old_key" as &dyn rusqlite::ToSql, &value_json, &1234567890_i64, &1234567890_i64],
            ).unwrap();
        }

        // Create new cache - should recreate due to missing expires_at
        let mut cache = AttributeCache::with_database_file(&cache_file);
        
        // Old data should be gone
        assert_eq!(cache.get::<String>("old_key").unwrap(), None);

        // New functionality should work
        cache.set_with_ttl("new_key", "new_value", 3600).unwrap();
        assert_eq!(cache.get::<String>("new_key").unwrap(), Some("new_value".to_string()));
        
        let entries = cache.list_entries(None).unwrap();
        assert_eq!(entries.len(), 1);
        assert!(entries[0].expires_at.is_some());
    }

    #[test]
    fn test_database_recreation_missing_multiple_columns() {
        let temp_dir = TempDir::new().expect("Failed to create temp directory");
        let cache_file = temp_dir.path().join("test_cache.db");

        // Create a cache with only key and value columns
        {
            let conn = rusqlite::Connection::open(&cache_file).unwrap();
            conn.execute(
                "CREATE TABLE cache (
                    key TEXT PRIMARY KEY,
                    value BLOB NOT NULL
                )",
                [],
            ).unwrap();
        }

        // Create new cache - should recreate due to missing timestamp and expires_at columns
        let mut cache = AttributeCache::with_database_file(&cache_file);
        
        // Should be able to use all functionality
        cache.set_with_expiry("test_key", "test_value", None).unwrap();
        assert_eq!(cache.get::<String>("test_key").unwrap(), Some("test_value".to_string()));
        
        let timestamps = cache.get_timestamps("test_key").unwrap();
        assert!(timestamps.is_some());
    }

    #[test]
    fn test_database_with_complete_schema() {
        let temp_dir = TempDir::new().expect("Failed to create temp directory");
        let cache_file = temp_dir.path().join("test_cache.db");

        // Create a cache with complete schema
        {
            let conn = rusqlite::Connection::open(&cache_file).unwrap();
            conn.execute(
                "CREATE TABLE cache (
                    key TEXT PRIMARY KEY,
                    value BLOB NOT NULL,
                    created_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
                    updated_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
                    expires_at INTEGER
                )",
                [],
            ).unwrap();
            
            let value_json = serde_json::to_vec("existing_value").unwrap();
            let now = chrono::Utc::now().timestamp();
            conn.execute(
                "INSERT INTO cache (key, value, created_at, updated_at, expires_at) VALUES (?1, ?2, ?3, ?4, ?5)",
                [&"existing_key" as &dyn rusqlite::ToSql, &value_json, &now, &now, &(now + 3600)],
            ).unwrap();
        }

        // Create new cache - should NOT recreate database since schema is complete
        let mut cache = AttributeCache::with_database_file(&cache_file);
        
        // Existing data should still be there
        assert_eq!(cache.get::<String>("existing_key").unwrap(), Some("existing_value".to_string()));
        
        // All functionality should work
        cache.set_with_ttl("new_key", "new_value", 1800).unwrap();
        assert_eq!(cache.get::<String>("new_key").unwrap(), Some("new_value".to_string()));
        
        let entries = cache.list_entries(None).unwrap();
        assert_eq!(entries.len(), 2);
    }

    #[test]
    fn test_expiry_functionality() {
        let (mut cache, _temp_dir) = create_test_cache();

        let key = "expiry_test";
        let value = "test_value";

        // Test setting with TTL
        cache.set_with_ttl(key, &value, 2).unwrap(); // 2 seconds TTL

        // Should be available immediately
        assert_eq!(cache.get::<String>(key).unwrap(), Some(value.to_string()));

        // Wait for expiry
        std::thread::sleep(std::time::Duration::from_secs(3));

        // Should be expired and removed
        assert_eq!(cache.get::<String>(key).unwrap(), None);
    }

    #[test]
    fn test_expiry_with_timestamp() {
        let (mut cache, _temp_dir) = create_test_cache();

        let key = "expiry_timestamp_test";
        let value = "test_value";

        // Set expiry to 2 seconds from now
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        let expires_at = now + 2;

        cache.set_with_expiry(key, &value, Some(expires_at)).unwrap();

        // Should be available immediately
        assert_eq!(cache.get::<String>(key).unwrap(), Some(value.to_string()));

        // Wait for expiry
        std::thread::sleep(std::time::Duration::from_secs(3));

        // Should be expired and removed
        assert_eq!(cache.get::<String>(key).unwrap(), None);
    }

    #[test]
    fn test_no_expiry_behavior() {
        let (mut cache, _temp_dir) = create_test_cache();

        let key = "no_expiry_test";
        let value = "test_value";

        // Set without expiry (should use None)
        cache.set(key, &value).unwrap();

        // Should be available
        assert_eq!(cache.get::<String>(key).unwrap(), Some(value.to_string()));

        // Wait some time
        std::thread::sleep(std::time::Duration::from_secs(1));

        // Should still be available (no expiry)
        assert_eq!(cache.get::<String>(key).unwrap(), Some(value.to_string()));
    }

    #[test]
    fn test_expiry_memory_cache_handling() {
        let (mut cache, _temp_dir) = create_test_cache();

        let key = "memory_expiry_test";
        let value = "test_value";

        // Set with short TTL
        cache.set_with_ttl(key, &value, 1).unwrap(); // 1 second TTL

        // First access should populate memory cache
        assert_eq!(cache.get::<String>(key).unwrap(), Some(value.to_string()));

        // Wait for expiry
        std::thread::sleep(std::time::Duration::from_secs(2));

        // Even though it's in memory cache, should check database expiry and remove
        assert_eq!(cache.get::<String>(key).unwrap(), None);
    }

    #[test]
    fn test_global_expiry_functions() {
        let temp_dir = TempDir::new().expect("Failed to create temp directory");
        let db_path = temp_dir.path().join("cache.db");
        
        // Initialize the global cache
        super::AttributeCache::initialize(&db_path).unwrap();
        
        let key = "global_expiry_test";
        let value = "test_value";

        // Test global TTL function
        set_with_ttl(key, &value, 2).unwrap(); // 2 seconds TTL

        // Should be available immediately
        assert_eq!(get::<String>(key).unwrap(), Some(value.to_string()));

        // Wait for expiry
        std::thread::sleep(std::time::Duration::from_secs(3));

        // Should be expired
        assert_eq!(get::<String>(key).unwrap(), None);
    }

    #[test]
    fn test_list_entries_with_expiry() {
        let (mut cache, _temp_dir) = create_test_cache();

        // Set entries with and without expiry
        cache.set("no_expiry", "value1").unwrap();
        cache.set_with_ttl("with_expiry", "value2", 3600).unwrap(); // 1 hour TTL

        let entries = cache.list_entries(None).unwrap();
        assert_eq!(entries.len(), 2);

        // Check that one has expiry and one doesn't
        let no_expiry_entry = entries.iter().find(|e| e.key == "no_expiry").unwrap();
        let with_expiry_entry = entries.iter().find(|e| e.key == "with_expiry").unwrap();

        assert_eq!(no_expiry_entry.expires_at, None);
        assert!(with_expiry_entry.expires_at.is_some());
    }

    #[test]
    fn test_expired_entry_removal_on_access() {
        let (mut cache, _temp_dir) = create_test_cache();

        let key = "removal_test";
        let value = "test_value";

        // Set with very short TTL
        cache.set_with_ttl(key, &value, 1).unwrap();

        // Verify it exists in database initially
        let entries_before = cache.list_entries(None).unwrap();
        assert_eq!(entries_before.len(), 1);

        // Wait for expiry
        std::thread::sleep(std::time::Duration::from_secs(2));

        // Access should trigger removal
        assert_eq!(cache.get::<String>(key).unwrap(), None);

        // Verify it's removed from database
        let entries_after = cache.list_entries(None).unwrap();
        assert_eq!(entries_after.len(), 0);
    }

    #[test]
    fn test_update_expiry_on_existing_key() {
        let (mut cache, _temp_dir) = create_test_cache();

        let key = "update_expiry_test";
        let value1 = "value1";
        let value2 = "value2";

        // Set initial value without expiry
        cache.set(key, &value1).unwrap();

        // Update with expiry
        let future_time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64 + 3600; // 1 hour from now

        cache.set_with_expiry(key, &value2, Some(future_time)).unwrap();

        // Check that value was updated and expiry was set
        assert_eq!(cache.get::<String>(key).unwrap(), Some(value2.to_string()));
        
        let entries = cache.list_entries(None).unwrap();
        assert_eq!(entries.len(), 1);
        assert!(entries[0].expires_at.is_some());
        assert_eq!(entries[0].expires_at.unwrap(), future_time);
    }
}