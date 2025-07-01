use std::path::{Path, PathBuf};
use std::fs::{self, File, read_dir};
use std::io::{Write, Read};
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
        let cache_dir = PathBuf::from("/var/lib/audiocontrol/cache/images");
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
        if !self.is_enabled() {
            return Err("Image cache is disabled".to_string());
        }
        
        // Get the extension from the MIME type
        let extension = mime_type_to_extension(&mime_type);
        
        // Create a new path with the extension
        let path_str = path.as_ref().to_string_lossy().to_string();
        let path_with_extension = format!("{}.{}", path_str, extension);
        
        // Store the image using the existing method
        self.store_image(path_with_extension, &data)
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
pub fn get_image_cache() -> std::sync::MutexGuard<'static, ImageCache> {
    IMAGE_CACHE.lock().unwrap()
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

/// Store an image in the cache with the extension determined by the MIME type
pub fn store_image_from_data<P: AsRef<Path>>(path: P, data: Vec<u8>, mime_type: String) -> Result<(), String> {
    get_image_cache().store_image_from_data(path, data, mime_type)
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