use std::path::{Path, PathBuf};
use std::fs::{self, File};
use std::io::{self, Write, Read};
use std::sync::Mutex;
use lazy_static::lazy_static;
use log::{info, error, debug};

// Global singleton for the image cache
lazy_static! {
    static ref IMAGE_CACHE: Mutex<ImageCache> = Mutex::new(ImageCache::new());
}

/// A cache for storing image files
pub struct ImageCache {
    /// Base directory for storing images
    base_path: PathBuf,
    /// Whether the cache is enabled
    enabled: bool,
}

impl ImageCache {
    /// Create a new image cache with default settings
    pub fn new() -> Self {
        // Using the default path that matches our cache.image_cache_path setting
        let cache_dir = PathBuf::from("cache/images");
        Self::with_directory(cache_dir)
    }

    /// Create a new image cache with a specific directory
    pub fn with_directory<P: AsRef<Path>>(dir: P) -> Self {
        let base_path = dir.as_ref().to_path_buf();
        
        // Ensure the directory exists
        if let Err(e) = fs::create_dir_all(&base_path) {
            error!("Failed to create image cache directory at {:?}: {}", base_path, e);
        } else {
            info!("Successfully initialized image cache at {:?}", base_path);
        }

        ImageCache {
            base_path,
            enabled: true,
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
        self.base_path = base_path;
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
        if !self.is_enabled() {
            return Err("Image cache is disabled".to_string());
        }

        let full_path = self.get_full_path(path);
        
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
                Ok(())
            },
            Err(e) => Err(format!("Failed to create image file: {}", e)),
        }
    }

    /// Get an image from the cache
    pub fn get_image<P: AsRef<Path>>(&self, path: P) -> Result<Vec<u8>, String> {
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

        let full_path = self.get_full_path(path);
        
        if !full_path.exists() {
            // If the file doesn't exist, consider it a success
            return Ok(());
        }
        
        if let Err(e) = fs::remove_file(&full_path) {
            return Err(format!("Failed to delete image: {}", e));
        }
        
        debug!("Deleted image at {}", full_path.display());
        Ok(())
    }

    /// Get the full path for a relative path
    fn get_full_path<P: AsRef<Path>>(&self, path: P) -> PathBuf {
        self.base_path.join(path)
    }
}

// Global functions to access the image cache singleton

/// Get a reference to the global image cache
pub fn get_image_cache() -> std::sync::MutexGuard<'static, ImageCache> {
    IMAGE_CACHE.lock().unwrap()
}

/// Check if an image exists in the cache
pub fn image_exists<P: AsRef<Path>>(path: P) -> bool {
    get_image_cache().image_exists(path)
}

/// Store an image in the cache
pub fn store_image<P: AsRef<Path>>(path: P, data: &[u8]) -> Result<(), String> {
    get_image_cache().store_image(path, data)
}

/// Get an image from the cache
pub fn get_image<P: AsRef<Path>>(path: P) -> Result<Vec<u8>, String> {
    get_image_cache().get_image(path)
}

/// Delete an image from the cache
pub fn delete_image<P: AsRef<Path>>(path: P) -> Result<(), String> {
    get_image_cache().delete_image(path)
}