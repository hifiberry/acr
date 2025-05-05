use std::path::{Path, PathBuf};
use std::fs::{self, File};
use std::io::{self, Read, Write};
use url::Url;
use std::collections::HashMap;
use std::sync::Mutex;
use lazy_static::lazy_static;
use reqwest;
use bytes::Bytes;
use tokio::runtime::Runtime;
use log::{info, warn, error};
use std::time::{Duration, SystemTime};

lazy_static! {
    static ref IMAGE_CACHE: Mutex<ImageCache> = Mutex::new(ImageCache::new());
}

pub struct ImageCache {
    base_dir: PathBuf,
    cache_map: HashMap<String, String>,
    max_age_days: u64,
    enabled: bool,
}

impl ImageCache {
    pub fn new() -> Self {
        let cache_dir = PathBuf::from("cache/images");
        Self::with_directory(cache_dir)
    }

    pub fn with_directory<P: AsRef<Path>>(dir: P) -> Self {
        let base_dir = dir.as_ref().to_path_buf();
        
        // Ensure cache directory exists
        if !base_dir.exists() {
            if let Err(e) = fs::create_dir_all(&base_dir) {
                error!("Failed to create image cache directory: {:?}", e);
            }
        }
        
        ImageCache {
            base_dir,
            cache_map: HashMap::new(),
            max_age_days: 30, // Default cache retention time: 30 days
            enabled: true,
        }
    }

    pub fn set_max_age(&mut self, days: u64) {
        self.max_age_days = days;
    }

    pub fn enable(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }
    
    // Add image to cache from binary data
    pub fn add_image_from_binary(&mut self, key: &str, data: &[u8]) -> io::Result<String> {
        if !self.enabled {
            return Err(io::Error::new(io::ErrorKind::Other, "Cache is disabled"));
        }
        
        let file_name = self.compute_filename(key);
        let file_path = self.base_dir.join(&file_name);
        
        let mut file = File::create(&file_path)?;
        file.write_all(data)?;
        
        self.cache_map.insert(key.to_string(), file_name.clone());
        
        Ok(file_name)
    }
    
    // Add image to cache from a local file
    pub fn add_image_from_file<P: AsRef<Path>>(&mut self, key: &str, file_path: P) -> io::Result<String> {
        if !self.enabled {
            return Err(io::Error::new(io::ErrorKind::Other, "Cache is disabled"));
        }
        
        let mut file = File::open(file_path)?;
        let mut buffer = Vec::new();
        file.read_to_end(&mut buffer)?;
        
        self.add_image_from_binary(key, &buffer)
    }
    
    // Add image to cache from a URL
    pub fn add_image_from_url(&mut self, key: &str, url_string: &str) -> io::Result<String> {
        if !self.enabled {
            return Err(io::Error::new(io::ErrorKind::Other, "Cache is disabled"));
        }
        
        let url = match Url::parse(url_string) {
            Ok(url) => url,
            Err(_) => return Err(io::Error::new(io::ErrorKind::InvalidInput, "Invalid URL")),
        };
        
        // Check if we already have this image cached
        if let Some(filename) = self.cache_map.get(key) {
            let file_path = self.base_dir.join(filename);
            if file_path.exists() {
                return Ok(filename.clone());
            }
        }
        
        // Download the image
        let runtime = match Runtime::new() {
            Ok(rt) => rt,
            Err(e) => return Err(io::Error::new(io::ErrorKind::Other, format!("Failed to create runtime: {}", e))),
        };
        
        let response = match runtime.block_on(async {
            reqwest::get(url).await
        }) {
            Ok(resp) => resp,
            Err(e) => return Err(io::Error::new(io::ErrorKind::Other, format!("Failed to download image: {}", e))),
        };
        
        if !response.status().is_success() {
            return Err(io::Error::new(
                io::ErrorKind::Other, 
                format!("Failed to download image: HTTP status {}", response.status())
            ));
        }
        
        // Use bytes from reqwest, which implements Sized
        let image_bytes: Bytes = match runtime.block_on(async {
            response.bytes().await
        }) {
            Ok(data) => data,
            Err(e) => return Err(io::Error::new(io::ErrorKind::Other, format!("Failed to read image data: {}", e))),
        };
        
        // Convert bytes to a slice for writing to file
        self.add_image_from_binary(key, &image_bytes)
    }
    
    // Get the path for a cached image
    pub fn get_image_path(&self, key: &str) -> Option<PathBuf> {
        match self.cache_map.get(key) {
            Some(filename) => {
                let path = self.base_dir.join(filename);
                if path.exists() {
                    Some(path)
                } else {
                    None
                }
            },
            None => None,
        }
    }
    
    // Get the relative path for a cached image
    pub fn get_image_relative_path(&self, key: &str) -> Option<String> {
        self.cache_map.get(key).cloned()
    }
    
    // Compute a unique filename for a key
    fn compute_filename(&self, key: &str) -> String {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        
        let mut hasher = DefaultHasher::new();
        key.hash(&mut hasher);
        
        // Extract file extension from the key if it looks like a URL or file path
        let extension = if key.contains('.') {
            let parts: Vec<&str> = key.split('.').collect();
            let possible_ext = parts.last().unwrap();
            
            // Check if it's likely an image extension
            match *possible_ext {
                "jpg" | "jpeg" | "png" | "gif" | "bmp" | "webp" => Some(*possible_ext),
                _ => None,
            }
        } else {
            None
        };
        
        match extension {
            Some(ext) => format!("{:x}.{}", hasher.finish(), ext),
            None => format!("{:x}.jpg", hasher.finish()), // Default to jpg
        }
    }
    
    // Clean up old cache entries
    pub fn cleanup(&mut self) -> io::Result<()> {
        if !self.enabled {
            return Ok(());
        }
        
        let now = SystemTime::now();
        let max_age = Duration::from_secs(60 * 60 * 24 * self.max_age_days);
        
        let entries = fs::read_dir(&self.base_dir)?;
        for entry in entries {
            let entry = entry?;
            let path = entry.path();
            
            if path.is_file() {
                if let Ok(metadata) = fs::metadata(&path) {
                    if let Ok(created_time) = metadata.created() {
                        if let Ok(age) = now.duration_since(created_time) {
                            if age > max_age {
                                if let Err(e) = fs::remove_file(&path) {
                                    warn!("Failed to remove old cache file {:?}: {}", path, e);
                                } else {
                                    info!("Removed old cache file: {:?}", path);
                                }
                            }
                        }
                    }
                }
            }
        }
        
        Ok(())
    }
}

// Global functions to access the image cache
pub fn get_image_cache() -> std::sync::MutexGuard<'static, ImageCache> {
    IMAGE_CACHE.lock().unwrap()
}

pub fn add_image_from_binary(key: &str, data: &[u8]) -> io::Result<String> {
    get_image_cache().add_image_from_binary(key, data)
}

pub fn add_image_from_file<P: AsRef<Path>>(key: &str, file_path: P) -> io::Result<String> {
    get_image_cache().add_image_from_file(key, file_path)
}

pub fn add_image_from_url(key: &str, url: &str) -> io::Result<String> {
    get_image_cache().add_image_from_url(key, url)
}

pub fn get_image_path(key: &str) -> Option<PathBuf> {
    get_image_cache().get_image_path(key)
}

pub fn get_image_relative_path(key: &str) -> Option<String> {
    get_image_cache().get_image_relative_path(key)
}

pub fn cleanup_image_cache() -> io::Result<()> {
    get_image_cache().cleanup()
}