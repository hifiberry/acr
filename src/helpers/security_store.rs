// Security store for ACR
// This module provides a secure key-value store for sensitive data
// using the SECRETS_ENCRYPTION_KEY from secrets.txt for encryption

use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use parking_lot::Mutex;
use parking_lot::RwLock;
use log::{debug, info, warn};
use serde::{Deserialize, Serialize};
use once_cell::sync::Lazy;
use thiserror::Error;
use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Key, Nonce
};
use base64::{engine::general_purpose::STANDARD, Engine};
use rand::{rngs::OsRng, RngCore};

// Compiled from secrets.txt at build time
#[cfg(not(test))]
pub fn default_encryption_key() -> String {
    crate::secrets::secrets_encryption_key()
}

#[cfg(test)]
pub fn default_encryption_key() -> String {
    "test_encryption_key".to_string()
}

// Error type for security store operations
#[derive(Error, Debug)]
pub enum SecurityStoreError {
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),

    #[error("Encryption error: {0}")]
    EncryptionError(String),

    #[error("Decryption error: {0}")]
    DecryptionError(String),

    #[error("Invalid encryption key: {0}")]
    InvalidKeyError(String),

    #[error("Store is locked: {0}")]
    StoreLocked(String),

    #[error("Key not found: {0}")]
    KeyNotFound(String),
}

// Type alias for results
pub type Result<T> = std::result::Result<T, SecurityStoreError>;

// In-memory representation of the security store
#[derive(Debug, Clone, Serialize, Deserialize)]
struct SecurityStoreData {
    // Values are stored as encrypted strings
    values: HashMap<String, String>,
    // Metadata about when values were last updated
    modified: HashMap<String, u64>,
    // File format version
    #[serde(default = "default_version")]
    version: u32,
    // Last updated timestamp
    #[serde(default)]
    last_updated: u64,
}

fn default_version() -> u32 {
    1
}

impl Default for SecurityStoreData {
    fn default() -> Self {
        SecurityStoreData {
            values: HashMap::new(),
            modified: HashMap::new(),
            version: default_version(),
            last_updated: 0,
        }
    }
}

// Security store singleton
pub struct SecurityStore {
    // Data store
    data: Mutex<SecurityStoreData>,
    // Path to the store file (wrapped in RwLock for safe interior mutability)
    file_path: RwLock<PathBuf>,
    // Encryption key (wrapped in RwLock for safe interior mutability)
    encryption_key: RwLock<String>,
    // Is the store initialized
    initialized: Mutex<bool>,
    // Cipher for encryption/decryption
    cipher: Mutex<Option<Aes256Gcm>>,
}

// Global singleton instance
pub static SECURITY_STORE: Lazy<Arc<SecurityStore>> = Lazy::new(|| {
    // Create with default path
    let default_path = PathBuf::from("secrets/security_store.json");
    Arc::new(SecurityStore::new(default_path))
});

impl SecurityStore {
    // Create a new security store with the given path
    fn new(file_path: PathBuf) -> Self {
        SecurityStore {
            data: Mutex::new(SecurityStoreData::default()),
            file_path: RwLock::new(file_path),
            encryption_key: RwLock::new(String::new()),
            initialized: Mutex::new(false),
            cipher: Mutex::new(None),
        }
    }

    // Derive a key from the string encryption key
    fn derive_key_bytes(&self, encryption_key: &str) -> [u8; 32] {
        // For this simple implementation, we'll pad or truncate the key to exactly 32 bytes
        // In a production system, you'd use a proper key derivation function (KDF)
        let mut key_bytes = [0u8; 32];
        let input = encryption_key.as_bytes();

        // Copy input bytes, or pad with zeros if too short
        for i in 0..32 {
            if i < input.len() {
                key_bytes[i] = input[i];
            } else {
                // Pad with a repeating pattern from the input
                key_bytes[i] = input[i % input.len()];
            }
        }

        key_bytes
    }

    // Generate a random nonce for AES-GCM
    fn generate_nonce(&self) -> [u8; 12] {
        let mut nonce = [0u8; 12];
        OsRng.fill_bytes(&mut nonce);
        nonce
    }

    // Initialize the security store with the given encryption key
    pub fn initialize(encryption_key: &str, file_path: Option<PathBuf>) -> Result<()> {
        let store = SECURITY_STORE.clone();

        // Check if the encryption key is valid (non-empty)
        if encryption_key.is_empty() {
            return Err(SecurityStoreError::InvalidKeyError("Empty encryption key".to_string()));
        }

        // Set the encryption key using RwLock (safe interior mutability)
        {
            let mut key = store.encryption_key.write();
            *key = encryption_key.to_string();
        }

        // Initialize the cipher
        let key_bytes = store.derive_key_bytes(encryption_key);
        let key = Key::<Aes256Gcm>::from_slice(&key_bytes);
        let cipher = Aes256Gcm::new(key);

        let mut cipher_guard = store.cipher.lock();
        *cipher_guard = Some(cipher);
        drop(cipher_guard);

        // Update file path if provided
        if let Some(path) = file_path {
            let parent = path.parent().ok_or_else(|| {
                SecurityStoreError::IoError(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "Invalid file path",
                ))
            })?;

            // Create directory if it doesn't exist
            if !parent.exists() {
                fs::create_dir_all(parent)?;
            }

            // Update file path using RwLock (safe interior mutability)
            {
                let mut fp = store.file_path.write();
                *fp = path;
            }
        }

        // Mark as initialized first so load_from_file check passes
        {
            let mut initialized = store.initialized.lock();
            *initialized = true;
        }

        // Try to load existing data
        let file_exists = store.file_path.read().exists();
        if file_exists {
            match store.load_from_file() {
                Ok(_) => info!("Security store loaded from {}", store.file_path.read().display()),
                Err(e) => {
                    warn!("Failed to load security store: {}", e);
                    warn!("Starting with empty security store");
                }
            }
        } else {
            info!("No existing security store found, starting with empty store");
        }

        debug!("Security store initialized successfully");
        Ok(())
    }

    pub fn initialize_with_defaults(file_path: Option<PathBuf>) -> Result<()> {
        let encryption_key = default_encryption_key();

        if encryption_key == "unknown" {
            debug!("Using unknown encryption key");
        } else {
            debug!("Using default encryption key");
        }

        Self::initialize(&encryption_key, file_path)
    }

    // Check if the store is initialized
    fn ensure_initialized(&self) -> Result<()> {
        let initialized = self.initialized.lock();
        if !*initialized {
            return Err(SecurityStoreError::StoreLocked(
                "Security store is not initialized".to_string(),
            ));
        }
        Ok(())
    }

    // Encrypt a value using AES-GCM
    fn encrypt_value(&self, value: &str) -> Result<String> {
        self.ensure_initialized()?;

        if self.encryption_key.read().is_empty() {
            return Err(SecurityStoreError::EncryptionError(
                "Encryption key is not set".to_string(),
            ));
        }

        // Get the cipher
        let cipher_guard = self.cipher.lock();
        let cipher = cipher_guard.as_ref().ok_or_else(|| {
            SecurityStoreError::EncryptionError("Cipher not initialized".to_string())
        })?;

        // Generate a random nonce
        let nonce_bytes = self.generate_nonce();
        let nonce = Nonce::from_slice(&nonce_bytes);

        // Encrypt the value
        let plaintext = value.as_bytes();
        let ciphertext = cipher.encrypt(nonce, plaintext)
            .map_err(|e| SecurityStoreError::EncryptionError(e.to_string()))?;

        // Combine nonce and ciphertext for storage
        let mut combined = Vec::with_capacity(nonce_bytes.len() + ciphertext.len());
        combined.extend_from_slice(&nonce_bytes);
        combined.extend_from_slice(&ciphertext);

        // Encode as base64 for storage
        Ok(STANDARD.encode(combined))
    }

    // Decrypt a value using AES-GCM
    fn decrypt_value(&self, encrypted_base64: &str) -> Result<String> {
        self.ensure_initialized()?;

        if self.encryption_key.read().is_empty() {
            return Err(SecurityStoreError::DecryptionError(
                "Encryption key is not set".to_string(),
            ));
        }

        // Get the cipher
        let cipher_guard = self.cipher.lock();
        let cipher = cipher_guard.as_ref().ok_or_else(|| {
            SecurityStoreError::DecryptionError("Cipher not initialized".to_string())
        })?;

        // Decode from base64
        let combined = STANDARD.decode(encrypted_base64)
            .map_err(|e| SecurityStoreError::DecryptionError(format!("Base64 decode error: {}", e)))?;

        // Extract nonce and ciphertext
        if combined.len() < 12 {
            return Err(SecurityStoreError::DecryptionError(
                "Invalid encrypted data format".to_string(),
            ));
        }

        let (nonce_bytes, ciphertext) = combined.split_at(12);
        let nonce = Nonce::from_slice(nonce_bytes);

        // Decrypt the value
        let plaintext = cipher.decrypt(nonce, ciphertext)
            .map_err(|e| SecurityStoreError::DecryptionError(format!("Decryption error: {}", e)))?;

        // Convert to string
        String::from_utf8(plaintext)
            .map_err(|e| SecurityStoreError::DecryptionError(format!("UTF-8 decode error: {}", e)))
    }

    // Load the security store from a file
    fn load_from_file(&self) -> Result<()> {
        self.ensure_initialized()?;

        let file_path = self.file_path.read();
        let mut file = File::open(&*file_path)?;
        let mut json_contents = String::new();
        file.read_to_string(&mut json_contents)?;

        // Parse the JSON
        let store_data: SecurityStoreData = serde_json::from_str(&json_contents)?;

        // Update the in-memory store
        let mut data = self.data.lock();
        *data = store_data;

        debug!("Security store loaded from {}", file_path.display());
        Ok(())
    }

    // Save the security store to a file
    fn save_to_file(&self) -> Result<()> {
        self.ensure_initialized()?;

        let file_path = self.file_path.read();

        // Create parent directory if it doesn't exist
        if let Some(parent) = file_path.parent() {
            if !parent.exists() {
                fs::create_dir_all(parent)?;
            }
        }

        // Get the data
        let mut data = self.data.lock();

        // Update the last_updated timestamp
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        data.last_updated = now;

        // Serialize to pretty JSON for better readability
        let json_string = serde_json::to_string_pretty(&*data)?;

        // Write to file
        let mut file = File::create(&*file_path)?;
        file.write_all(json_string.as_bytes())?;

        debug!("Security store saved to {}", file_path.display());
        Ok(())
    }

    // Store a value in the security store
    pub fn set(key: &str, value: &str) -> Result<()> {
        let store = SECURITY_STORE.clone();
        store.ensure_initialized()?;

        // Encrypt the value
        let encrypted_value = store.encrypt_value(value)?;

        // Update the in-memory store
        let mut data = store.data.lock();
        data.values.insert(key.to_string(), encrypted_value);

        // Update the modified timestamp
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        data.modified.insert(key.to_string(), now);
        data.last_updated = now;

        // Save to file
        drop(data); // Release the lock before saving
        store.save_to_file()?;

        debug!("Stored value for key: {}", key);
        Ok(())
    }

    // Get a value from the security store
    pub fn get(key: &str) -> Result<String> {
        let store = SECURITY_STORE.clone();
        store.ensure_initialized()?;

        // Get from the in-memory store
        let data = store.data.lock();

        match data.values.get(key) {
            Some(encrypted_value) => {
                // Decrypt the value
                store.decrypt_value(encrypted_value)
            },
            None => Err(SecurityStoreError::KeyNotFound(key.to_string())),
        }
    }

    // Check if a key exists in the security store
    pub fn contains_key(key: &str) -> Result<bool> {
        let store = SECURITY_STORE.clone();
        store.ensure_initialized()?;

        let data = store.data.lock();
        Ok(data.values.contains_key(key))
    }

    // Remove a key from the security store
    pub fn remove(key: &str) -> Result<bool> {
        let store = SECURITY_STORE.clone();
        store.ensure_initialized()?;

        let mut data = store.data.lock();
        let existed = data.values.remove(key).is_some();

        if existed {
            data.modified.remove(key);

            // Save to file
            drop(data); // Release the lock before saving
            store.save_to_file()?;

            debug!("Removed key: {}", key);
        }

        Ok(existed)
    }

    // Get all keys in the security store
    pub fn get_all_keys() -> Result<Vec<String>> {
        let store = SECURITY_STORE.clone();
        store.ensure_initialized()?;

        let data = store.data.lock();
        Ok(data.values.keys().cloned().collect())
    }

    // Get the last modified timestamp for a key
    pub fn get_last_modified(key: &str) -> Result<Option<u64>> {
        let store = SECURITY_STORE.clone();
        store.ensure_initialized()?;

        let data = store.data.lock();
        Ok(data.modified.get(key).cloned())
    }

    // Change the encryption key and re-encrypt all values
    pub fn change_encryption_key(new_key: &str) -> Result<()> {
        let store = SECURITY_STORE.clone();
        store.ensure_initialized()?;

        if new_key.is_empty() {
            return Err(SecurityStoreError::InvalidKeyError("Empty encryption key".to_string()));
        }

        // Get all current key-value pairs
        let data = store.data.lock();
        let mut pairs = Vec::new();

        for (key, encrypted_value) in &data.values {
            let value = store.decrypt_value(encrypted_value)?;
            pairs.push((key.clone(), value));
        }

        // Release the current lock
        drop(data);

        // Update the encryption key using RwLock (safe interior mutability)
        {
            let mut enc_key = store.encryption_key.write();
            *enc_key = new_key.to_string();
        }

        // Initialize new cipher with the new key
        let key_bytes = store.derive_key_bytes(new_key);
        let key = Key::<Aes256Gcm>::from_slice(&key_bytes);
        let cipher = Aes256Gcm::new(key);

        let mut cipher_guard = store.cipher.lock();
        *cipher_guard = Some(cipher);
        drop(cipher_guard);

        // Re-encrypt and update all values
        let mut new_data = store.data.lock();

        for (key, value) in pairs {
            let encrypted = store.encrypt_value(&value)?;
            new_data.values.insert(key, encrypted);
        }

        // Save with the new encryption
        drop(new_data);
        store.save_to_file()?;

        info!("Encryption key changed and all values re-encrypted");
        Ok(())
    }

    // Clear all values in the security store
    pub fn clear() -> Result<()> {
        let store = SECURITY_STORE.clone();
        store.ensure_initialized()?;

        let mut data = store.data.lock();
        data.values.clear();
        data.modified.clear();

        // Save to file
        drop(data);
        store.save_to_file()?;

        info!("Security store cleared");
        Ok(())
    }
}

// Helper function to set the module path to a default location
pub fn set_default_store_path(path: &Path) -> Result<()> {
    let store = SECURITY_STORE.clone();

    // Only allow changing the path if not initialized
    let initialized = *store.initialized.lock();
    if initialized {
        return Err(SecurityStoreError::StoreLocked(
            "Cannot change store path after initialization".to_string(),
        ));
    }

    // Update file path using RwLock (safe interior mutability)
    {
        let mut fp = store.file_path.write();
        *fp = path.to_path_buf();
    }

    debug!("Security store path set to {}", path.display());
    Ok(())
}

// Tests for the security store
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use std::sync::Mutex as StdMutex;

    // Use a mutex to synchronize test execution that touches the singleton
    static TEST_MUTEX: StdMutex<()> = StdMutex::new(());

    #[test]
    fn test_store_and_retrieve() {
        // Lock mutex to prevent other tests from interfering
        let _lock = TEST_MUTEX.lock().unwrap();

        let dir = tempdir().unwrap();
        let file_path = dir.path().join("test_store.json");

        // Reset any previous state using safe RwLock/Mutex writes
        {
            let store = SECURITY_STORE.clone();
            *store.initialized.lock() = false;
            *store.encryption_key.write() = String::new();
            *store.cipher.lock() = None;
            *store.data.lock() = SecurityStoreData::default();
        }

        // Initialize with a test key
        SecurityStore::initialize("test_key_123", Some(file_path.clone())).unwrap();

        // Store some values
        SecurityStore::set("username", "testuser").unwrap();
        SecurityStore::set("password", "p@ssw0rd").unwrap();

        // Retrieve and verify
        assert_eq!(SecurityStore::get("username").unwrap(), "testuser");
        assert_eq!(SecurityStore::get("password").unwrap(), "p@ssw0rd");

        // Check if keys exist
        assert!(SecurityStore::contains_key("username").unwrap());
        assert!(!SecurityStore::contains_key("nonexistent").unwrap());

        // Get all keys
        let keys = SecurityStore::get_all_keys().unwrap();
        assert_eq!(keys.len(), 2);
        assert!(keys.contains(&"username".to_string()));
        assert!(keys.contains(&"password".to_string()));

        // Remove a key
        assert!(SecurityStore::remove("username").unwrap());
        assert!(!SecurityStore::contains_key("username").unwrap());

        // Try to get a removed key
        assert!(SecurityStore::get("username").is_err());
    }

    #[test]
    fn test_change_encryption_key() {
        // Lock mutex to prevent other tests from interfering
        let _lock = TEST_MUTEX.lock().unwrap();

        let dir = tempdir().unwrap();
        let file_path = dir.path().join("test_store.json");

        // Reset any previous state using safe RwLock/Mutex writes
        {
            let store = SECURITY_STORE.clone();
            *store.initialized.lock() = false;
            *store.encryption_key.write() = String::new();
            *store.cipher.lock() = None;
            *store.data.lock() = SecurityStoreData::default();
        }

        // Initialize with a test key
        SecurityStore::initialize("test_key_123", Some(file_path.clone())).unwrap();

        // Store a value
        SecurityStore::set("secret", "myvalue").unwrap();

        // Change the encryption key
        SecurityStore::change_encryption_key("new_key_456").unwrap();

        // Verify we can still access the value
        assert_eq!(SecurityStore::get("secret").unwrap(), "myvalue");
    }

    #[test]
    fn test_persistence() {
        // Lock mutex to prevent other tests from interfering
        let _lock = TEST_MUTEX.lock().unwrap();

        let dir = tempdir().unwrap();
        let file_path = dir.path().join("test_store.json");

        // Reset any previous state using safe RwLock/Mutex writes
        {
            let store = SECURITY_STORE.clone();
            *store.initialized.lock() = false;
            *store.encryption_key.write() = String::new();
            *store.cipher.lock() = None;
            *store.data.lock() = SecurityStoreData::default();
            *store.file_path.write() = file_path.clone();
        }

        // First initialization
        {
            SecurityStore::initialize("test_key_123", Some(file_path.clone())).unwrap();
            SecurityStore::set("key1", "value1").unwrap();
            SecurityStore::set("key2", "value2").unwrap();

            // Make sure it's saved
            let store = SECURITY_STORE.clone();
            store.save_to_file().unwrap();
        }

        // Clear the in-memory singleton for testing using safe writes
        {
            let store = SECURITY_STORE.clone();
            *store.initialized.lock() = false;
            *store.encryption_key.write() = String::new();
            *store.cipher.lock() = None;
            *store.data.lock() = SecurityStoreData::default();
            // Keep the file path
        }

        // Reinitialize with the same key
        SecurityStore::initialize("test_key_123", Some(file_path.clone())).unwrap();

        // Verify values are still there
        assert_eq!(SecurityStore::get("key1").unwrap(), "value1");
        assert_eq!(SecurityStore::get("key2").unwrap(), "value2");
    }
}
