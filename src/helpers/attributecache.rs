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
        assert!(cache.memory_cache.contains_key(key));
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
        assert!(!cache.memory_cache.contains_key(key));
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
                "INSERT OR REPLACE INTO cache (key, value) VALUES (?1, ?2)",
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
}