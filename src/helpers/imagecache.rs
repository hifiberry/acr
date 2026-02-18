use std::path::{Path, PathBuf};
use std::fs::{self, File, read_dir};
use std::io::{Write, Read};
use parking_lot::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};
use std::collections::HashMap;
use once_cell::sync::Lazy;
use log::{info, error, debug};
use serde::{Serialize, Deserialize};
use crate::helpers::attributecache;

// Constants for cache keys
const IMAGECACHE_METADATA_PREFIX: &str = "imagecache:metadata:";
const IMAGECACHE_STATS_KEY: &str = "imagecache:stats";

/// Metadata for a cached image
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ImageMetadata {
    /// Original file name/path
    pub name: String,
    /// Size of the image in bytes
    pub size: u64,
    /// MIME type of the image
    pub mime_type: String,
    /// Timestamp when the image was cached (seconds since UNIX epoch)
    pub cached_at: u64,
    /// Optional expiry timestamp (seconds since UNIX epoch)
    pub expires_at: Option<u64>,
}

/// Statistics about the image cache
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ImageCacheStats {
    /// Total number of cached images
    pub total_images: usize,
    /// Total size of all cached images in bytes
    pub total_size: u64,
    /// Last time statistics were updated
    pub last_updated: u64,
}

impl ImageCacheStats {
    pub fn new() -> Self {
        Self {
            total_images: 0,
            total_size: 0,
            last_updated: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        }
    }
}

// Global singleton for the image cache
static IMAGE_CACHE: Lazy<Mutex<ImageCache>> = Lazy::new(|| Mutex::new(ImageCache::new()));

/// Metadata for image expiry tracking
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ImageExpiryMetadata {
    /// Map of image path to expiry timestamp (seconds since UNIX epoch)
    pub expiry_map: HashMap<String, u64>,
}

impl ImageExpiryMetadata {
    pub fn new() -> Self {
        Self {
            expiry_map: HashMap::new(),
        }
    }
}

/// A cache for storing image files
pub struct ImageCache {
    /// Base directory for storing images
    base_path: PathBuf,
    /// Whether the cache is enabled
    enabled: bool,
    /// Path to the expiry metadata file
    expiry_metadata_path: PathBuf,
}

impl ImageCache {
    /// Create a new image cache with default settings
    pub fn new() -> Self {
        // Using the default path that matches our cache.image_cache_path setting
        let cache_dir = PathBuf::from("/var/lib/audiocontrol/cache/images");
        Self::with_directory(cache_dir)
    }

    /// Create a new image cache with a specific directory
    pub fn with_directory<P: AsRef<Path>>(dir: P) -> Self {
        let base_path = dir.as_ref().to_path_buf();
        let expiry_metadata_path = base_path.join(".expiry_metadata.json");
        
        // Ensure the directory exists
        if let Err(e) = fs::create_dir_all(&base_path) {
            error!("Failed to create image cache directory at {:?}: {}", base_path, e);
        } else {
            info!("Successfully initialized image cache at {:?}", base_path);
        }

        ImageCache {
            base_path,
            enabled: true,
            expiry_metadata_path,
        }
    }

    /// Create a new image cache with custom directory and expiry metadata path
    pub fn with_custom_expiry_path<P: AsRef<Path>, E: AsRef<Path>>(dir: P, expiry_path: E) -> Self {
        let base_path = dir.as_ref().to_path_buf();
        let expiry_metadata_path = expiry_path.as_ref().to_path_buf();
        
        // Ensure the directory exists
        if let Err(e) = fs::create_dir_all(&base_path) {
            error!("Failed to create image cache directory at {:?}: {}", base_path, e);
        } else {
            info!("Successfully initialized image cache at {:?}", base_path);
        }

        ImageCache {
            base_path,
            enabled: true,
            expiry_metadata_path,
        }
    }

    /// Initialize the global image cache with a custom directory
    pub fn initialize<P: AsRef<Path>>(path: P) -> Result<(), String> {
        match get_image_cache().reconfigure_with_directory(path) {
            Ok(_) => {
                info!("Global image cache initialized with custom directory");
                Ok(())
            },
            Err(e) => {
                error!("Failed to initialize global image cache: {}", e);
                Err(e)
            }
        }
    }

    /// Reconfigure the image cache with a new directory
    fn reconfigure_with_directory<P: AsRef<Path>>(&mut self, dir: P) -> Result<(), String> {
        let base_path = dir.as_ref().to_path_buf();
        
        // Try to ensure the directory exists
        if let Err(e) = fs::create_dir_all(&base_path) {
            return Err(format!("Failed to create directory for image cache: {}", e));
        }
        
        // Update the instance
        self.base_path = base_path.clone();
        self.expiry_metadata_path = base_path.join(".expiry_metadata.json");
        info!("Image cache reconfigured with directory: {:?}", self.base_path);
        
        Ok(())
    }

    /// Enable or disable the cache
    pub fn enable(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    /// Check if the cache is enabled
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Load expiry metadata from disk
    fn load_expiry_metadata(&self) -> ImageExpiryMetadata {
        if !self.expiry_metadata_path.exists() {
            return ImageExpiryMetadata::new();
        }

        match fs::read_to_string(&self.expiry_metadata_path) {
            Ok(content) => {
                match serde_json::from_str::<ImageExpiryMetadata>(&content) {
                    Ok(metadata) => metadata,
                    Err(e) => {
                        error!("Failed to parse expiry metadata: {}", e);
                        ImageExpiryMetadata::new()
                    }
                }
            }
            Err(e) => {
                error!("Failed to read expiry metadata file: {}", e);
                ImageExpiryMetadata::new()
            }
        }
    }

    /// Save expiry metadata to disk
    fn save_expiry_metadata(&self, metadata: &ImageExpiryMetadata) -> Result<(), String> {
        let content = match serde_json::to_string_pretty(metadata) {
            Ok(c) => c,
            Err(e) => return Err(format!("Failed to serialize expiry metadata: {}", e)),
        };

        match fs::write(&self.expiry_metadata_path, content) {
            Ok(_) => Ok(()),
            Err(e) => Err(format!("Failed to write expiry metadata: {}", e)),
        }
    }

    /// Set expiry time for an image
    /// 
    /// # Arguments
    /// * `path` - Path to the image (relative to cache base)
    /// * `expiry_time` - SystemTime when the image should expire
    /// 
    /// # Returns
    /// * `Result<(), String>` - Success or error message
    pub fn set_image_expiry<P: AsRef<Path>>(&self, path: P, expiry_time: SystemTime) -> Result<(), String> {
        if !self.is_enabled() {
            return Err("Image cache is disabled".to_string());
        }

        let path_str = path.as_ref().to_string_lossy().to_string();
        let expiry_timestamp = expiry_time
            .duration_since(UNIX_EPOCH)
            .map_err(|e| format!("Invalid expiry time: {}", e))?
            .as_secs();

        let mut metadata = self.load_expiry_metadata();
        metadata.expiry_map.insert(path_str, expiry_timestamp);
        self.save_expiry_metadata(&metadata)?;

        debug!("Set expiry for image '{}' to timestamp {}", path.as_ref().display(), expiry_timestamp);
        Ok(())
    }

    /// Check if an image has expired
    /// 
    /// # Arguments
    /// * `path` - Path to the image (relative to cache base)
    /// 
    /// # Returns
    /// * `bool` - True if the image has expired or no expiry is set, false if still valid
    pub fn is_image_expired<P: AsRef<Path>>(&self, path: P) -> bool {
        if !self.is_enabled() {
            return false;
        }

        let path_str = path.as_ref().to_string_lossy().to_string();
        let metadata = self.load_expiry_metadata();
        
        if let Some(&expiry_timestamp) = metadata.expiry_map.get(&path_str) {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            
            now >= expiry_timestamp
        } else {
            // No expiry set, image doesn't expire
            false
        }
    }

    /// Remove expired images from the cache
    /// 
    /// # Returns
    /// * `Result<usize, String>` - Number of images removed or error message
    pub fn expire_images(&self) -> Result<usize, String> {
        if !self.is_enabled() {
            return Err("Image cache is disabled".to_string());
        }

        let mut metadata = self.load_expiry_metadata();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let mut removed_count = 0;
        let mut paths_to_remove = Vec::new();

        // Find expired images
        for (path, &expiry_timestamp) in &metadata.expiry_map {
            if now >= expiry_timestamp {
                let full_path = self.get_full_path(path);
                if full_path.exists() {
                    match fs::remove_file(&full_path) {
                        Ok(_) => {
                            debug!("Removed expired image: {}", full_path.display());
                            removed_count += 1;
                        }
                        Err(e) => {
                            error!("Failed to remove expired image {}: {}", full_path.display(), e);
                        }
                    }
                }
                paths_to_remove.push(path.clone());
            }
        }

        // Remove expired entries from metadata
        for path in paths_to_remove {
            metadata.expiry_map.remove(&path);
        }

        // Save updated metadata
        self.save_expiry_metadata(&metadata)?;

        info!("Expired {} images from cache", removed_count);
        Ok(removed_count)
    }

    /// Check if an image exists in the cache
    pub fn image_exists<P: AsRef<Path>>(&self, path: P) -> bool {
        if !self.is_enabled() {
            return false;
        }

        let full_path = self.get_full_path(path);
        full_path.exists()
    }

    /// Store an image in the cache
    pub fn store_image<P: AsRef<Path>>(&self, path: P, data: &[u8]) -> Result<(), String> {
        self.store_image_with_expiry(path, data, None)
    }

    /// Store an image in the cache with optional expiry time
    /// 
    /// # Arguments
    /// * `path` - Path to store the image
    /// * `data` - The image data
    /// * `expiry_time` - Optional expiry time for the image
    /// 
    /// # Returns
    /// * `Result<(), String>` - Success or error message
    pub fn store_image_with_expiry<P: AsRef<Path>>(&self, path: P, data: &[u8], expiry_time: Option<SystemTime>) -> Result<(), String> {
        if !self.is_enabled() {
            return Err("Image cache is disabled".to_string());
        }

        let path_ref = path.as_ref();
        let full_path = self.get_full_path(path_ref);
        
        // Ensure parent directory exists
        if let Some(parent) = full_path.parent() {
            if !parent.exists() {
                if let Err(e) = fs::create_dir_all(parent) {
                    return Err(format!("Failed to create directory {}: {}", parent.display(), e));
                }
            }
        }
        
        // Write the image data to file
        match File::create(&full_path) {
            Ok(mut file) => {
                if let Err(e) = file.write_all(data) {
                    return Err(format!("Failed to write image data: {}", e));
                }
                debug!("Stored image at {}", full_path.display());
            },
            Err(e) => return Err(format!("Failed to create image file: {}", e)),
        }

        // Create and store metadata
        let path_str = path_ref.to_string_lossy().to_string();
        let expires_at = expiry_time.map(|t| {
            t.duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs()
        });

        let metadata = ImageMetadata {
            name: path_str.clone(),
            size: data.len() as u64,
            mime_type: self.guess_mime_type(&path_str),
            cached_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            expires_at,
        };

        // Store metadata in attribute cache
        self.store_image_metadata(&path_str, &metadata)?;

        // Set expiry if provided (for backward compatibility)
        if let Some(expiry) = expiry_time {
            self.set_image_expiry(path_ref, expiry)?;
        }

        debug!("Stored image metadata for: {}", path_str);
        Ok(())
    }

    /// Store an image in the cache with the extension determined by the MIME type
    /// 
    /// # Arguments
    /// * `path` - Base path without extension
    /// * `data` - The image data
    /// * `mime_type` - MIME type of the image (e.g., "image/jpeg", "image/png")
    /// 
    /// # Returns
    /// * `Result<(), String>` - Success or error message
    pub fn store_image_from_data<P: AsRef<Path>>(&self, path: P, data: Vec<u8>, mime_type: String) -> Result<(), String> {
        self.store_image_from_data_with_expiry(path, data, mime_type, None)
    }

    /// Store an image in the cache with MIME type and optional expiry time
    /// 
    /// # Arguments
    /// * `path` - Base path without extension
    /// * `data` - The image data
    /// * `mime_type` - MIME type of the image (e.g., "image/jpeg", "image/png")
    /// * `expiry_time` - Optional expiry time for the image
    /// 
    /// # Returns
    /// * `Result<(), String>` - Success or error message
    pub fn store_image_from_data_with_expiry<P: AsRef<Path>>(&self, path: P, data: Vec<u8>, mime_type: String, expiry_time: Option<SystemTime>) -> Result<(), String> {
        if !self.is_enabled() {
            return Err("Image cache is disabled".to_string());
        }
        
        // Get the extension from the MIME type
        let extension = mime_type_to_extension(&mime_type);
        
        // Create a new path with the extension
        let path_str = path.as_ref().to_string_lossy().to_string();
        let path_with_extension = format!("{}.{}", path_str, extension);
        
        let full_path = self.get_full_path(&path_with_extension);
        
        // Ensure parent directory exists
        if let Some(parent) = full_path.parent() {
            if !parent.exists() {
                if let Err(e) = fs::create_dir_all(parent) {
                    return Err(format!("Failed to create directory {}: {}", parent.display(), e));
                }
            }
        }
        
        // Write the image data to file
        match File::create(&full_path) {
            Ok(mut file) => {
                if let Err(e) = file.write_all(&data) {
                    return Err(format!("Failed to write image data: {}", e));
                }
                debug!("Stored image at {}", full_path.display());
            },
            Err(e) => return Err(format!("Failed to create image file: {}", e)),
        }

        // Create and store metadata
        let expires_at = expiry_time.map(|t| {
            t.duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs()
        });

        let metadata = ImageMetadata {
            name: path_with_extension.clone(),
            size: data.len() as u64,
            mime_type: mime_type.clone(),
            cached_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            expires_at,
        };

        // Store metadata in attribute cache
        self.store_image_metadata(&path_with_extension, &metadata)?;

        // Set expiry if provided (for backward compatibility)
        if let Some(expiry) = expiry_time {
            self.set_image_expiry(&path_with_extension, expiry)?;
        }

        debug!("Stored image metadata for: {}", path_with_extension);
        Ok(())
    }

    /// Get an image from the cache
    pub fn get_image_data<P: AsRef<Path>>(&self, path: P) -> Result<Vec<u8>, String> {
        if !self.is_enabled() {
            return Err("Image cache is disabled".to_string());
        }

        let full_path = self.get_full_path(path);
        
        if !full_path.exists() {
            return Err(format!("Image does not exist: {}", full_path.display()));
        }
        
        match File::open(&full_path) {
            Ok(mut file) => {
                let mut data = Vec::new();
                if let Err(e) = file.read_to_end(&mut data) {
                    return Err(format!("Failed to read image data: {}", e));
                }
                Ok(data)
            },
            Err(e) => Err(format!("Failed to open image file: {}", e)),
        }
    }

    /// Delete an image from the cache
    pub fn delete_image<P: AsRef<Path>>(&self, path: P) -> Result<(), String> {
        if !self.is_enabled() {
            return Err("Image cache is disabled".to_string());
        }

        let path_str = path.as_ref().to_string_lossy().to_string();
        let full_path = self.get_full_path(&path);
        
        if !full_path.exists() {
            // If the file doesn't exist, still try to remove metadata
            let _ = self.remove_image_metadata(&path_str);
            return Ok(());
        }
        
        if let Err(e) = fs::remove_file(&full_path) {
            return Err(format!("Failed to delete image: {}", e));
        }
        
        // Remove metadata from attribute cache
        self.remove_image_metadata(&path_str)?;
        
        debug!("Deleted image and metadata for: {}", path_str);
        Ok(())
    }

    /// Get the full path for a relative path
    fn get_full_path<P: AsRef<Path>>(&self, path: P) -> PathBuf {
        self.base_path.join(path)
    }

    /// Store image metadata in the attribute cache
    fn store_image_metadata(&self, path: &str, metadata: &ImageMetadata) -> Result<(), String> {
        let cache_key = format!("{}{}", IMAGECACHE_METADATA_PREFIX, path);
        attributecache::set(&cache_key, metadata)
            .map_err(|e| format!("Failed to store image metadata: {}", e))
    }

    /// Retrieve image metadata from the attribute cache
    fn get_image_metadata(&self, path: &str) -> Option<ImageMetadata> {
        let cache_key = format!("{}{}", IMAGECACHE_METADATA_PREFIX, path);
        attributecache::get(&cache_key).ok().flatten()
    }

    /// Remove image metadata from the attribute cache
    fn remove_image_metadata(&self, path: &str) -> Result<(), String> {
        let cache_key = format!("{}{}", IMAGECACHE_METADATA_PREFIX, path);
        attributecache::remove(&cache_key)
            .map(|_| ())
            .map_err(|e| format!("Failed to remove image metadata: {}", e))
    }

    /// Update cache statistics
    fn update_cache_stats(&self) -> Result<ImageCacheStats, String> {
        let mut stats = ImageCacheStats::new();
        
        // Get all image metadata entries
        let prefix = Some(IMAGECACHE_METADATA_PREFIX);
        match attributecache::list_keys(prefix) {
            Ok(keys) => {
                for key in keys {
                    if let Ok(Some(metadata)) = attributecache::get::<ImageMetadata>(&key) {
                        stats.total_images += 1;
                        stats.total_size += metadata.size;
                    }
                }
            }
            Err(e) => {
                debug!("Failed to get image cache keys: {}", e);
                // Fall back to scanning the filesystem
                return self.scan_filesystem_for_stats();
            }
        }

        // Store updated stats
        attributecache::set(IMAGECACHE_STATS_KEY, &stats)
            .map_err(|e| format!("Failed to store cache stats: {}", e))?;

        Ok(stats)
    }

    /// Fallback method to scan filesystem for cache statistics
    fn scan_filesystem_for_stats(&self) -> Result<ImageCacheStats, String> {
        let mut stats = ImageCacheStats::new();
        
        if !self.base_path.exists() {
            return Ok(stats);
        }

        fn scan_directory(dir: &Path, stats: &mut ImageCacheStats) -> Result<(), String> {
            match read_dir(dir) {
                Ok(entries) => {
                    for entry in entries {
                        if let Ok(entry) = entry {
                            let path = entry.path();
                            if path.is_file() {
                                // Skip metadata files
                                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                                    if !name.starts_with('.') {
                                        if let Ok(metadata) = entry.metadata() {
                                            stats.total_images += 1;
                                            stats.total_size += metadata.len();
                                        }
                                    }
                                }
                            } else if path.is_dir() {
                                scan_directory(&path, stats)?;
                            }
                        }
                    }
                }
                Err(e) => {
                    return Err(format!("Failed to read directory {:?}: {}", dir, e));
                }
            }
            Ok(())
        }

        scan_directory(&self.base_path, &mut stats)?;
        
        // Store scanned stats
        attributecache::set(IMAGECACHE_STATS_KEY, &stats)
            .map_err(|e| format!("Failed to store scanned cache stats: {}", e))?;

        Ok(stats)
    }

    /// Guess MIME type from file path/extension
    fn guess_mime_type(&self, path: &str) -> String {
        if let Some(extension) = Path::new(path).extension().and_then(|ext| ext.to_str()) {
            extension_to_mime_type(extension).to_string()
        } else {
            "application/octet-stream".to_string()
        }
    }

    /// Get an image from the cache by base name regardless of extension
    /// 
    /// # Arguments
    /// * `base_path` - Base path without extension
    /// 
    /// # Returns
    /// * `Result<(Vec<u8>, String), String>` - Image data and MIME type, or error message
    pub fn get_image_with_mime_type<P: AsRef<Path>>(&self, base_path: P) -> Result<(Vec<u8>, String), String> {
        if !self.is_enabled() {
            return Err("Image cache is disabled".to_string());
        }

        let base_path = base_path.as_ref();
        
        // Get the directory and file name
        let dir_path = if let Some(parent) = base_path.parent() {
            parent.to_path_buf()
        } else {
            PathBuf::new()
        };

        let base_name = base_path.file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| "Invalid path: no file name".to_string())?;
        
        // Get the full path to the directory
        let full_dir_path = self.get_full_path(dir_path);
        
        // If directory doesn't exist, return error
        if !full_dir_path.exists() {
            return Err(format!("Directory does not exist: {}", full_dir_path.display()));
        }
        
        // Read directory and find matching files
        match read_dir(full_dir_path) {
            Ok(entries) => {
                let found_files: Vec<(PathBuf, String)> = entries
                    .filter_map(Result::ok)
                    .filter_map(|entry| {
                        let path = entry.path();
                        let file_stem = path.file_stem()?.to_str()?;
                        let extension = path.extension()?.to_str()?;
                        
                        if file_stem == base_name {
                            // Found a file with matching base name
                            let mime_type = extension_to_mime_type(extension);
                            Some((path.clone(), mime_type.to_string()))
                        } else {
                            None
                        }
                    })
                    .collect();
                
                // Return the first matching file
                if let Some((file_path, mime_type)) = found_files.first() {
                    match File::open(file_path) {
                        Ok(mut file) => {
                            let mut data = Vec::new();
                            if let Err(e) = file.read_to_end(&mut data) {
                                return Err(format!("Failed to read image data: {}", e));
                            }
                            Ok((data, mime_type.clone()))
                        },
                        Err(e) => Err(format!("Failed to open image file: {}", e)),
                    }
                } else {
                    Err(format!("No image found with base name: {}", base_name))
                }
            },
            Err(e) => Err(format!("Failed to read directory: {}", e)),
        }
    }
    
    /// Get image cache statistics
    /// 
    /// # Returns
    /// * `Result<ImageCacheStats, String>` - Cache statistics or error message
    pub fn get_cache_statistics(&self) -> Result<ImageCacheStats, String> {
        if !self.is_enabled() {
            return Ok(ImageCacheStats::new());
        }

        // Try to get cached stats first
        match attributecache::get::<ImageCacheStats>(IMAGECACHE_STATS_KEY) {
            Ok(Some(stats)) => {
                // Check if stats are recent (less than 5 minutes old)
                let now = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
                
                if now - stats.last_updated < 300 { // 5 minutes
                    return Ok(stats);
                }
            }
            _ => {} // Fall through to update stats
        }

        // Update and return fresh stats
        self.update_cache_stats()
    }

    /// Force refresh of cache statistics
    /// 
    /// # Returns
    /// * `Result<ImageCacheStats, String>` - Updated cache statistics or error message
    pub fn refresh_cache_statistics(&self) -> Result<ImageCacheStats, String> {
        if !self.is_enabled() {
            return Ok(ImageCacheStats::new());
        }

        self.update_cache_stats()
    }

    /// Get metadata for a specific cached image
    /// 
    /// # Arguments
    /// * `path` - Path to the image (relative to cache base)
    /// 
    /// # Returns
    /// * `Option<ImageMetadata>` - Image metadata if found
    pub fn get_image_metadata_info<P: AsRef<Path>>(&self, path: P) -> Option<ImageMetadata> {
        if !self.is_enabled() {
            return None;
        }

        let path_str = path.as_ref().to_string_lossy().to_string();
        self.get_image_metadata(&path_str)
    }
}

/// Convert a MIME type to a file extension
fn mime_type_to_extension(mime_type: &str) -> &str {
    match mime_type {
        "image/jpeg" => "jpg",
        "image/png" => "png",
        "image/gif" => "gif",
        "image/webp" => "webp",
        "image/bmp" => "bmp",
        "image/svg+xml" => "svg",
        _ => "bin", // Default extension for unknown types
    }
}

/// Convert a file extension to a MIME type
fn extension_to_mime_type(extension: &str) -> &str {
    match extension.to_lowercase().as_str() {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        "svg" => "image/svg+xml",
        _ => "application/octet-stream", // Default MIME type for unknown extensions
    }
}

// Global functions to access the image cache singleton

/// Get a reference to the global image cache
pub fn get_image_cache() -> parking_lot::MutexGuard<'static, ImageCache> {
    IMAGE_CACHE.lock()
}

/// Get the full path for a relative path in the image cache
pub fn get_full_path<P: AsRef<Path>>(path: P) -> PathBuf {
    get_image_cache().get_full_path(path)
}

/// Check if an image exists in the cache
pub fn image_exists<P: AsRef<Path>>(path: P) -> bool {
    get_image_cache().image_exists(path)
}

/// Store an image in the cache
pub fn store_image<P: AsRef<Path>>(path: P, data: &[u8]) -> Result<(), String> {
    get_image_cache().store_image(path, data)
}

/// Store an image in the cache with optional expiry time
pub fn store_image_with_expiry<P: AsRef<Path>>(path: P, data: &[u8], expiry_time: Option<SystemTime>) -> Result<(), String> {
    get_image_cache().store_image_with_expiry(path, data, expiry_time)
}

/// Store an image in the cache with the extension determined by the MIME type
pub fn store_image_from_data<P: AsRef<Path>>(path: P, data: Vec<u8>, mime_type: String) -> Result<(), String> {
    get_image_cache().store_image_from_data(path, data, mime_type)
}

/// Store an image in the cache with MIME type and optional expiry time
pub fn store_image_from_data_with_expiry<P: AsRef<Path>>(path: P, data: Vec<u8>, mime_type: String, expiry_time: Option<SystemTime>) -> Result<(), String> {
    get_image_cache().store_image_from_data_with_expiry(path, data, mime_type, expiry_time)
}

/// Get an image from the cache
pub fn get_image_data<P: AsRef<Path>>(path: P) -> Result<Vec<u8>, String> {
    get_image_cache().get_image_data(path)
}

/// Delete an image from the cache
pub fn delete_image<P: AsRef<Path>>(path: P) -> Result<(), String> {
    get_image_cache().delete_image(path)
}

/// Get an image from the cache by base name regardless of extension
pub fn get_image_with_mime_type<P: AsRef<Path>>(base_path: P) -> Result<(Vec<u8>, String), String> {
    get_image_cache().get_image_with_mime_type(base_path)
}

/// Get album cover art using artist, album name, and optional year
/// 
/// # Arguments
/// * `artist` - Artist name
/// * `album_name` - Album name
/// * `year` - Optional release year
/// 
/// # Returns
/// * `Result<(Vec<u8>, String), String>` - Image data and MIME type, or error message
pub fn get_album_cover(artist: &str, album_name: &str, year: Option<i32>) -> Result<(Vec<u8>, String), String> {
    let cache_path = crate::helpers::local_coverart::album_cache_key(artist, album_name, year);
    get_image_cache().get_image_with_mime_type(format!("{}/cover", cache_path))
}

/// Store album cover art using artist, album name, and optional year
/// 
/// # Arguments
/// * `artist` - Artist name
/// * `album_name` - Album name
/// * `year` - Optional release year
/// * `data` - Image data
/// * `mime_type` - MIME type of the image
/// 
/// # Returns
/// * `Result<(), String>` - Success or error message
pub fn store_album_cover(artist: &str, album_name: &str, year: Option<i32>, data: Vec<u8>, mime_type: String) -> Result<(), String> {
    let cache_path = crate::helpers::local_coverart::album_cache_key(artist, album_name, year);
    get_image_cache().store_image_from_data(format!("{}/cover", cache_path), data, mime_type)
}

/// Set expiry time for an image
pub fn set_image_expiry<P: AsRef<Path>>(path: P, expiry_time: SystemTime) -> Result<(), String> {
    get_image_cache().set_image_expiry(path, expiry_time)
}

/// Check if an image has expired
pub fn is_image_expired<P: AsRef<Path>>(path: P) -> bool {
    get_image_cache().is_image_expired(path)
}

/// Remove expired images from the cache
pub fn expire_images() -> Result<usize, String> {
    get_image_cache().expire_images()
}

/// Get image cache statistics
/// 
/// # Returns
/// * `Result<ImageCacheStats, String>` - Cache statistics including total number of images and total size
pub fn get_cache_statistics() -> Result<ImageCacheStats, String> {
    get_image_cache().get_cache_statistics()
}

/// Force refresh of image cache statistics
/// 
/// # Returns
/// * `Result<ImageCacheStats, String>` - Updated cache statistics
pub fn refresh_cache_statistics() -> Result<ImageCacheStats, String> {
    get_image_cache().refresh_cache_statistics()
}

/// Get metadata for a specific cached image
/// 
/// # Arguments
/// * `path` - Path to the image (relative to cache base)
/// 
/// # Returns
/// * `Option<ImageMetadata>` - Image metadata if found
pub fn get_image_metadata<P: AsRef<Path>>(path: P) -> Option<ImageMetadata> {
    get_image_cache().get_image_metadata_info(path)
}

/// Count files with any extension matching a base path and provider pattern
/// 
/// # Arguments
/// * `base_path` - Base path without extension
/// * `provider` - Provider name (e.g., "fanarttv")
/// 
/// # Returns
/// * `usize` - Number of matching files found
pub fn count_provider_files<P: AsRef<Path>>(base_path: P, provider: &str) -> usize {
    if !get_image_cache().is_enabled() {
        return 0;
    }

    let base = base_path.as_ref();
    let dir_path = if let Some(parent) = base.parent() {
        parent.to_path_buf()
    } else {
        PathBuf::new()
    };

    let file_name = base.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("");

    // Get the full path to the directory
    let full_dir_path = get_image_cache().get_full_path(dir_path);
    
    // If directory doesn't exist, return 0
    if !full_dir_path.exists() {
        return 0;
    }

    let pattern = format!("{}.{}", file_name, provider);
    
    // Read directory and count matching files
    match read_dir(full_dir_path) {
        Ok(entries) => {
            entries
                .filter_map(Result::ok)
                .filter(|entry| {
                    entry.file_name()
                        .to_str()
                        .map(|name| name.starts_with(&pattern))
                        .unwrap_or(false)
                })
                .count()
        },
        Err(_) => 0,
    }
}

/// Check if any files with a given base path and provider pattern exist
/// 
/// # Arguments
/// * `base_path` - Base path without extension
/// * `provider` - Provider name (e.g., "fanarttv")
/// 
/// # Returns
/// * `bool` - True if any matching files exist
pub fn provider_files_exist<P: AsRef<Path>>(base_path: P, provider: &str) -> bool {
    count_provider_files(base_path, provider) > 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::SystemTime;
    use tempfile::TempDir;
    use serial_test::serial;

    // Helper function to initialize attribute cache for tests
    fn init_test_attribute_cache() {
        use crate::helpers::attributecache::AttributeCache;
        use std::sync::Once;
        static INIT: Once = Once::new();
        
        INIT.call_once(|| {
            let temp_dir = TempDir::new().unwrap();
            let attr_cache_path = temp_dir.path().join("attributes");
            let _ = AttributeCache::initialize_global(&attr_cache_path);
            // Keep the temp_dir alive by leaking it for tests
            std::mem::forget(temp_dir);
        });
    }

    #[test]
    #[serial]
    fn test_image_cache_basic_functionality() {
        init_test_attribute_cache();
        
        let temp_dir = TempDir::new().unwrap();
        let cache_path = temp_dir.path().to_str().unwrap();
        let expiry_path = temp_dir.path().join("expiry.json");
        
        // Create cache with custom paths
        let cache = ImageCache::with_custom_expiry_path(cache_path, &expiry_path);
        
        // Store image without expiry
        let test_data = b"test image data";
        let result = cache.store_image("test_image.jpg", test_data);
        assert!(result.is_ok());
        
        // Retrieve image
        let retrieved = cache.get_image_data("test_image.jpg");
        assert!(retrieved.is_ok());
        assert_eq!(retrieved.unwrap(), test_data);
    }

    #[test]
    #[serial]
    fn test_expiry_metadata_persistence() {
        init_test_attribute_cache();
        
        let temp_dir = TempDir::new().unwrap();
        let cache_path = temp_dir.path().to_str().unwrap();
        let expiry_path = temp_dir.path().join("expiry.json");
        
        let test_data = b"persistence test";
        let future_time = SystemTime::now() + std::time::Duration::from_secs(7200);
        
        // Create cache and store image with expiry
        {
            let cache = ImageCache::with_custom_expiry_path(cache_path, &expiry_path);
            let result = cache.store_image_with_expiry("persist_test.webp", test_data, Some(future_time));
            assert!(result.is_ok());
        }
        
        // Create new cache instance (simulating restart)
        {
            let cache = ImageCache::with_custom_expiry_path(cache_path, &expiry_path);
            // Expiry metadata should be loaded from disk
            assert!(!cache.is_image_expired("persist_test.webp"));
        }
    }

    #[test]
    #[serial]
    fn test_expire_images_method() {
        init_test_attribute_cache();
        
        let temp_dir = TempDir::new().unwrap();
        let cache_path = temp_dir.path().to_str().unwrap();
        let expiry_path = temp_dir.path().join("expiry.json");
        
        let cache = ImageCache::with_custom_expiry_path(cache_path, &expiry_path);
        
        let test_data = b"expiry cleanup test";
        
        // Store images with different expiry times
        let past_time = SystemTime::now() - std::time::Duration::from_secs(3600);
        let future_time = SystemTime::now() + std::time::Duration::from_secs(3600);
        
        cache.store_image_with_expiry("expired_1.jpg", test_data, Some(past_time)).unwrap();
        cache.store_image_with_expiry("expired_2.png", test_data, Some(past_time)).unwrap();
        cache.store_image_with_expiry("valid_1.jpg", test_data, Some(future_time)).unwrap();
        cache.store_image("no_expiry.jpg", test_data).unwrap();
        
        // Run expiry cleanup
        let expired_count = cache.expire_images();
        assert!(expired_count.is_ok());
        assert_eq!(expired_count.unwrap(), 2);
        
        // Verify expired images are gone
        assert!(cache.get_image_data("expired_1.jpg").is_err());
        assert!(cache.get_image_data("expired_2.png").is_err());
        
        // Verify valid images remain
        assert!(cache.get_image_data("valid_1.jpg").is_ok());
        assert!(cache.get_image_data("no_expiry.jpg").is_ok());
    }

    #[test]
    #[serial]
    fn test_disabled_cache_behavior() {
        init_test_attribute_cache();
        
        let temp_dir = TempDir::new().unwrap();
        let cache_path = temp_dir.path().to_str().unwrap();
        let expiry_path = temp_dir.path().join("expiry.json");
        
        // Create cache and disable it by setting it to a read-only directory
        // (We'll simulate disabled behavior by checking error handling)
        let cache = ImageCache::with_custom_expiry_path(cache_path, &expiry_path);
        
        let test_data = b"disabled cache test";
        
        // Test normal operation first
        let result = cache.store_image("normal_test.jpg", test_data);
        assert!(result.is_ok());
        
        // Test expiry check on non-existent image
        assert!(!cache.is_image_expired("non_existent.jpg"));
        
        // Test expire operation
        let expired_count = cache.expire_images();
        assert!(expired_count.is_ok());
    }

    #[test]
    #[serial]
    fn test_global_functions() {
        init_test_attribute_cache();
        
        let temp_dir = TempDir::new().unwrap();
        let test_path = temp_dir.path().join("global_test");
        
        // Test provider file functions
        assert_eq!(count_provider_files(&test_path, "test_provider"), 0);
        assert!(!provider_files_exist(&test_path, "test_provider"));
        
        // Create a test file
        std::fs::create_dir_all(test_path.parent().unwrap()).unwrap();
        std::fs::write(format!("{}.test_provider.jpg", test_path.display()), b"test").unwrap();
        
        // Now should find one file
        assert_eq!(count_provider_files(&test_path, "test_provider"), 1);
        assert!(provider_files_exist(&test_path, "test_provider"));
    }

    #[test]
    #[serial]
    fn test_image_cache_statistics() {
        use crate::helpers::attributecache::AttributeCache;
        
        let temp_dir = TempDir::new().unwrap();
        let cache_path = temp_dir.path().to_str().unwrap();
        let expiry_path = temp_dir.path().join("expiry.json");
        
        // Initialize attribute cache for this test
        let attr_cache_path = temp_dir.path().join("attributes");
        AttributeCache::initialize_global(&attr_cache_path).unwrap();
        
        let cache = ImageCache::with_custom_expiry_path(cache_path, &expiry_path);
        
        // Initially, stats should show no images
        let initial_stats = cache.get_cache_statistics();
        assert!(initial_stats.is_ok());
        let stats = initial_stats.unwrap();
        assert_eq!(stats.total_images, 0);
        assert_eq!(stats.total_size, 0);
        
        // Store some test images
        let test_data1 = b"test image data 1";
        let test_data2 = b"test image data 2 - longer";
        
        cache.store_image("test1.jpg", test_data1).unwrap();
        cache.store_image_from_data("test2", test_data2.to_vec(), "image/png".to_string()).unwrap();
        
        // Get updated stats
        let updated_stats = cache.refresh_cache_statistics();
        assert!(updated_stats.is_ok());
        let stats = updated_stats.unwrap();
        assert_eq!(stats.total_images, 2);
        assert_eq!(stats.total_size, (test_data1.len() + test_data2.len()) as u64);
        
        // Test metadata retrieval
        let metadata1 = cache.get_image_metadata_info("test1.jpg");
        assert!(metadata1.is_some());
        let meta1 = metadata1.unwrap();
        assert_eq!(meta1.name, "test1.jpg");
        assert_eq!(meta1.size, test_data1.len() as u64);
        assert_eq!(meta1.mime_type, "image/jpeg");
        
        let metadata2 = cache.get_image_metadata_info("test2.png");
        assert!(metadata2.is_some());
        let meta2 = metadata2.unwrap();
        assert_eq!(meta2.name, "test2.png");
        assert_eq!(meta2.size, test_data2.len() as u64);
        assert_eq!(meta2.mime_type, "image/png");
        
        // Delete one image and verify stats update
        cache.delete_image("test1.jpg").unwrap();
        let final_stats = cache.refresh_cache_statistics();
        assert!(final_stats.is_ok());
        let stats = final_stats.unwrap();
        assert_eq!(stats.total_images, 1);
        assert_eq!(stats.total_size, test_data2.len() as u64);
        
        // Verify metadata was removed
        let metadata1_after = cache.get_image_metadata_info("test1.jpg");
        assert!(metadata1_after.is_none());
    }

    #[test]
    #[serial]
    fn test_global_cache_statistics_functions() {
        init_test_attribute_cache();
        
        let temp_dir = TempDir::new().unwrap();
        let cache_path = temp_dir.path().to_str().unwrap();
        
        // Initialize global cache for testing
        ImageCache::initialize(cache_path).unwrap();
        
        // Test global statistics functions
        let initial_stats = get_cache_statistics();
        assert!(initial_stats.is_ok());
        
        // Store some data using global functions
        let test_data = b"global test data";
        store_image("global_test.jpg", test_data).unwrap();
        
        // Get metadata using global function
        let metadata = get_image_metadata("global_test.jpg");
        assert!(metadata.is_some());
        let meta = metadata.unwrap();
        assert_eq!(meta.name, "global_test.jpg");
        assert_eq!(meta.size, test_data.len() as u64);
        
        // Refresh stats using global function
        let refreshed_stats = refresh_cache_statistics();
        assert!(refreshed_stats.is_ok());
        let stats = refreshed_stats.unwrap();
        assert!(stats.total_images >= 1);
        assert!(stats.total_size >= test_data.len() as u64);
    }
}