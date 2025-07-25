use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};

/// Provider information structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderInfo {
    pub name: String,
    pub display_name: String,
}

/// Cover art result from a specific provider
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoverartResult {
    pub provider: ProviderInfo,
    pub urls: Vec<String>,
}

/// Defines the types of cover art retrieval methods that a provider can support
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum CoverartMethod {
    /// Get cover art for an artist by name
    Artist,
    /// Get cover art for a song by title and artist
    Song,
    /// Get cover art for an album by title, artist, and optional year
    Album,
    /// Get cover art from a URL
    Url,
}

/// Trait for cover art providers that can retrieve cover art from various sources
pub trait CoverartProvider {
    /// Returns the internal name identifier for this provider
    fn name(&self) -> &str;
    
    /// Returns the human-readable display name for this provider
    fn display_name(&self) -> &str;
    
    /// Returns the set of methods this provider supports
    fn supported_methods(&self) -> HashSet<CoverartMethod>;

    /// Get cover art for an artist by name
    /// 
    /// # Arguments
    /// * `artist` - The artist name
    /// 
    /// # Returns
    /// * `Vec<String>` - URLs or local file paths to cover art
    fn get_artist_coverart(&self, artist: &str) -> Vec<String> {
        if self.supported_methods().contains(&CoverartMethod::Artist) {
            self.get_artist_coverart_impl(artist)
        } else {
            Vec::new()
        }
    }

    /// Get cover art for a song by title and artist
    /// 
    /// # Arguments
    /// * `title` - The song title
    /// * `artist` - The artist name
    /// 
    /// # Returns
    /// * `Vec<String>` - URLs or local file paths to cover art
    fn get_song_coverart(&self, title: &str, artist: &str) -> Vec<String> {
        if self.supported_methods().contains(&CoverartMethod::Song) {
            self.get_song_coverart_impl(title, artist)
        } else {
            Vec::new()
        }
    }

    /// Get cover art for an album by title, artist, and optional year
    /// 
    /// # Arguments
    /// * `title` - The album title
    /// * `artist` - The artist name
    /// * `year` - Optional release year
    /// 
    /// # Returns
    /// * `Vec<String>` - URLs or local file paths to cover art
    fn get_album_coverart(&self, title: &str, artist: &str, year: Option<i32>) -> Vec<String> {
        if self.supported_methods().contains(&CoverartMethod::Album) {
            self.get_album_coverart_impl(title, artist, year)
        } else {
            Vec::new()
        }
    }

    /// Get cover art from a URL
    /// 
    /// # Arguments
    /// * `url` - The URL to retrieve cover art from
    /// 
    /// # Returns
    /// * `Vec<String>` - URLs or local file paths to cover art
    fn get_url_coverart(&self, url: &str) -> Vec<String> {
        if self.supported_methods().contains(&CoverartMethod::Url) {
            self.get_url_coverart_impl(url)
        } else {
            Vec::new()
        }
    }

    // Implementation methods that providers must implement for supported methods
    // These are called only if the method is marked as supported

    /// Implementation for artist cover art retrieval
    /// Only called if CoverartMethod::Artist is in supported_methods()
    fn get_artist_coverart_impl(&self, _artist: &str) -> Vec<String> {
        Vec::new()
    }

    /// Implementation for song cover art retrieval
    /// Only called if CoverartMethod::Song is in supported_methods()
    fn get_song_coverart_impl(&self, _title: &str, _artist: &str) -> Vec<String> {
        Vec::new()
    }

    /// Implementation for album cover art retrieval
    /// Only called if CoverartMethod::Album is in supported_methods()
    fn get_album_coverart_impl(&self, _title: &str, _artist: &str, _year: Option<i32>) -> Vec<String> {
        Vec::new()
    }

    /// Implementation for URL cover art retrieval
    /// Only called if CoverartMethod::Url is in supported_methods()
    fn get_url_coverart_impl(&self, _url: &str) -> Vec<String> {
        Vec::new()
    }
}

/// Global coverart manager that maintains a registry of coverart providers
pub struct CoverartManager {
    providers: Vec<Arc<dyn CoverartProvider + Send + Sync>>,
}

impl CoverartManager {
    /// Create a new empty coverart manager
    pub fn new() -> Self {
        Self {
            providers: Vec::new(),
        }
    }

    /// Register a new coverart provider
    pub fn register_provider(&mut self, provider: Arc<dyn CoverartProvider + Send + Sync>) {
        self.providers.push(provider);
    }

    /// Get cover art for an artist from all registered providers
    pub fn get_artist_coverart(&self, artist: &str) -> Vec<CoverartResult> {
        self.providers
            .iter()
            .filter_map(|provider| {
                let urls = provider.get_artist_coverart(artist);
                if !urls.is_empty() {
                    Some(CoverartResult {
                        provider: ProviderInfo {
                            name: provider.name().to_string(),
                            display_name: provider.display_name().to_string(),
                        },
                        urls,
                    })
                } else {
                    None
                }
            })
            .collect()
    }

    /// Get cover art for a song from all registered providers
    pub fn get_song_coverart(&self, title: &str, artist: &str) -> Vec<CoverartResult> {
        self.providers
            .iter()
            .filter_map(|provider| {
                let urls = provider.get_song_coverart(title, artist);
                if !urls.is_empty() {
                    Some(CoverartResult {
                        provider: ProviderInfo {
                            name: provider.name().to_string(),
                            display_name: provider.display_name().to_string(),
                        },
                        urls,
                    })
                } else {
                    None
                }
            })
            .collect()
    }

    /// Get cover art for an album from all registered providers
    pub fn get_album_coverart(&self, title: &str, artist: &str, year: Option<i32>) -> Vec<CoverartResult> {
        self.providers
            .iter()
            .filter_map(|provider| {
                let urls = provider.get_album_coverart(title, artist, year);
                if !urls.is_empty() {
                    Some(CoverartResult {
                        provider: ProviderInfo {
                            name: provider.name().to_string(),
                            display_name: provider.display_name().to_string(),
                        },
                        urls,
                    })
                } else {
                    None
                }
            })
            .collect()
    }

    /// Get cover art from a URL from all registered providers
    pub fn get_url_coverart(&self, url: &str) -> Vec<CoverartResult> {
        self.providers
            .iter()
            .filter_map(|provider| {
                let urls = provider.get_url_coverart(url);
                if !urls.is_empty() {
                    Some(CoverartResult {
                        provider: ProviderInfo {
                            name: provider.name().to_string(),
                            display_name: provider.display_name().to_string(),
                        },
                        urls,
                    })
                } else {
                    None
                }
            })
            .collect()
    }

    /// Get all registered providers (for debugging/inspection)
    pub fn get_providers(&self) -> &Vec<Arc<dyn CoverartProvider + Send + Sync>> {
        &self.providers
    }
}

impl Default for CoverartManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Global singleton instance of the coverart manager
static COVERART_MANAGER: Lazy<Arc<Mutex<CoverartManager>>> = Lazy::new(|| {
    Arc::new(Mutex::new(CoverartManager::new()))
});

/// Get a reference to the global coverart manager
pub fn get_coverart_manager() -> Arc<Mutex<CoverartManager>> {
    COVERART_MANAGER.clone()
}