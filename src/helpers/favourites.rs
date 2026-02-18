use std::error::Error;
use std::fmt;
use crate::data::song::Song;
use parking_lot::Mutex;
use once_cell::sync::Lazy;

// Global favourite manager instance
static GLOBAL_FAVOURITE_MANAGER: Lazy<Mutex<FavouriteManager>> = Lazy::new(|| Mutex::new(FavouriteManager::new()));

/// Error types for favourite operations
#[derive(Debug)]
pub enum FavouriteError {
    /// Network-related error (for remote providers like Last.fm)
    NetworkError(String),
    /// Database/storage error (for local providers like settingsdb)
    StorageError(String),
    /// Authentication error (for providers requiring authentication)
    AuthError(String),
    /// Provider not configured or disabled
    NotConfigured(String),
    /// Invalid song data (missing artist or title)
    InvalidSong(String),
    /// Generic error
    Other(String),
}

impl fmt::Display for FavouriteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FavouriteError::NetworkError(msg) => write!(f, "Network error: {}", msg),
            FavouriteError::StorageError(msg) => write!(f, "Storage error: {}", msg),
            FavouriteError::AuthError(msg) => write!(f, "Authentication error: {}", msg),
            FavouriteError::NotConfigured(msg) => write!(f, "Not configured: {}", msg),
            FavouriteError::InvalidSong(msg) => write!(f, "Invalid song: {}", msg),
            FavouriteError::Other(msg) => write!(f, "Error: {}", msg),
        }
    }
}

impl Error for FavouriteError {}

/// Trait for services that can manage favourite songs
pub trait FavouriteProvider {
    /// Check if a song is marked as favourite
    /// 
    /// # Arguments
    /// * `song` - The song to check
    /// 
    /// # Returns
    /// `Ok(true)` if the song is a favourite, `Ok(false)` if not, or an error
    fn is_favourite(&self, song: &Song) -> Result<bool, FavouriteError>;

    /// Add a song to favourites
    /// 
    /// # Arguments
    /// * `song` - The song to add as favourite
    /// 
    /// # Returns
    /// `Ok(())` if successful, or an error
    fn add_favourite(&self, song: &Song) -> Result<(), FavouriteError>;

    /// Remove a song from favourites
    /// 
    /// # Arguments
    /// * `song` - The song to remove from favourites
    /// 
    /// # Returns
    /// `Ok(())` if successful, or an error
    fn remove_favourite(&self, song: &Song) -> Result<(), FavouriteError>;

    /// Get the total number of favourite songs
    /// 
    /// # Returns
    /// `Some(count)` if the provider supports counting, `None` if not supported
    fn get_favourite_count(&self) -> Option<usize>;

    /// Get the name/identifier of this provider
    fn provider_name(&self) -> &'static str;

    /// Get the human-readable display name of this provider
    fn display_name(&self) -> &'static str;

    /// Check if this provider is currently enabled/configured
    fn is_enabled(&self) -> bool;

    /// Check if this provider is currently active (e.g., user logged in for remote providers)
    /// This is different from is_enabled - a provider can be enabled but not active
    fn is_active(&self) -> bool;
}

/// Validate that a song has both artist and title
fn validate_song(song: &Song) -> Result<(), FavouriteError> {
    let artist = song.artist.as_ref()
        .ok_or_else(|| FavouriteError::InvalidSong("Artist is required".to_string()))?;
    
    let title = song.title.as_ref()
        .ok_or_else(|| FavouriteError::InvalidSong("Title is required".to_string()))?;
    
    if artist.trim().is_empty() {
        return Err(FavouriteError::InvalidSong("Artist cannot be empty".to_string()));
    }
    if title.trim().is_empty() {
        return Err(FavouriteError::InvalidSong("Title cannot be empty".to_string()));
    }
    Ok(())
}

/// Multi-provider favourite manager
pub struct FavouriteManager {
    providers: Vec<Box<dyn FavouriteProvider + Send + Sync>>,
}

impl FavouriteManager {
    /// Create a new favourite manager with no providers
    pub fn new() -> Self {
        Self {
            providers: Vec::new(),
        }
    }

    /// Add a provider to the manager
    pub fn add_provider(&mut self, provider: Box<dyn FavouriteProvider + Send + Sync>) {
        self.providers.push(provider);
    }

    /// Check if a song is favourite in any of the providers
    /// Returns true if the song is favourite in at least one provider
    pub fn is_favourite(&self, song: &Song) -> Result<bool, FavouriteError> {
        validate_song(song)?;

        for provider in &self.providers {
            if !provider.is_enabled() {
                continue;
            }

            match provider.is_favourite(song) {
                Ok(true) => return Ok(true),
                Ok(false) => continue,
                Err(e) => {
                    log::warn!("Error checking favourite in provider {}: {}", 
                              provider.provider_name(), e);
                    continue;
                }
            }
        }

        Ok(false)
    }

    /// Check which providers have the song marked as favourite (with display names)
    /// Returns a tuple of (is_favourite, list_of_provider_display_names_with_favourite)
    pub fn get_favourite_providers_display_names(&self, song: &Song) -> Result<(bool, Vec<String>), FavouriteError> {
        validate_song(song)?;

        let mut favourite_provider_display_names = Vec::new();

        for provider in &self.providers {
            if !provider.is_enabled() {
                continue;
            }

            match provider.is_favourite(song) {
                Ok(true) => {
                    favourite_provider_display_names.push(provider.display_name().to_string());
                }
                Ok(false) => continue,
                Err(e) => {
                    log::warn!("Error checking favourite in provider {}: {}", 
                              provider.provider_name(), e);
                    continue;
                }
            }
        }

        let is_favourite = !favourite_provider_display_names.is_empty();
        Ok((is_favourite, favourite_provider_display_names))
    }

    /// Add a song as favourite in all enabled providers
    /// Returns a list of providers that were successfully updated
    pub fn add_favourite(&self, song: &Song) -> Result<Vec<String>, FavouriteError> {
        validate_song(song)?;

        let mut errors = Vec::new();
        let mut successful_providers = Vec::new();

        for provider in &self.providers {
            if !provider.is_enabled() {
                continue;
            }

            match provider.add_favourite(song) {
                Ok(()) => {
                    successful_providers.push(provider.provider_name().to_string());
                    log::info!("Successfully added favourite to {}", provider.provider_name());
                }
                Err(e) => {
                    log::error!("Failed to add favourite in provider {}: {}", 
                               provider.provider_name(), e);
                    errors.push(format!("{}: {}", provider.provider_name(), e));
                }
            }
        }

        if successful_providers.is_empty() && !errors.is_empty() {
            return Err(FavouriteError::Other(format!(
                "Failed to add favourite in all providers: {}",
                errors.join(", ")
            )));
        }

        Ok(successful_providers)
    }

    /// Remove a song from favourites in all enabled providers
    /// Returns a list of providers that were successfully updated
    pub fn remove_favourite(&self, song: &Song) -> Result<Vec<String>, FavouriteError> {
        validate_song(song)?;

        let mut errors = Vec::new();
        let mut successful_providers = Vec::new();

        for provider in &self.providers {
            if !provider.is_enabled() {
                continue;
            }

            match provider.remove_favourite(song) {
                Ok(()) => {
                    successful_providers.push(provider.provider_name().to_string());
                    log::info!("Successfully removed favourite from {}", provider.provider_name());
                }
                Err(e) => {
                    log::error!("Failed to remove favourite in provider {}: {}", 
                               provider.provider_name(), e);
                    errors.push(format!("{}: {}", provider.provider_name(), e));
                }
            }
        }

        if successful_providers.is_empty() && !errors.is_empty() {
            return Err(FavouriteError::Other(format!(
                "Failed to remove favourite in all providers: {}",
                errors.join(", ")
            )));
        }

        Ok(successful_providers)
    }

    /// Get list of enabled providers
    pub fn get_enabled_providers(&self) -> Vec<&str> {
        self.providers
            .iter()
            .filter(|p| p.is_enabled())
            .map(|p| p.provider_name())
            .collect()
    }

    /// Get total number of providers (enabled and disabled)
    pub fn provider_count(&self) -> usize {
        self.providers.len()
    }

    /// Get number of enabled providers
    pub fn enabled_provider_count(&self) -> usize {
        self.providers.iter().filter(|p| p.is_enabled()).count()
    }

    /// Get detailed provider information including favorite counts
    pub fn get_provider_details(&self) -> Vec<serde_json::Value> {
        self.providers
            .iter()
            .map(|provider| {
                serde_json::json!({
                    "name": provider.provider_name(),
                    "display_name": provider.display_name(),
                    "enabled": provider.is_enabled(),
                    "active": provider.is_active(),
                    "favourite_count": provider.get_favourite_count()
                })
            })
            .collect()
    }
}

impl Default for FavouriteManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Initialize the global favourite manager with default providers
pub fn initialize_favourite_providers() {
    let mut manager = GLOBAL_FAVOURITE_MANAGER.lock();
    
    // Clear any existing providers
    manager.providers.clear();
    
    // Add Last.fm provider
    manager.add_provider(Box::new(crate::helpers::lastfm::LastfmFavouriteProvider::new()));
    
    // Add SettingsDB provider
    manager.add_provider(Box::new(crate::helpers::settingsdb::SettingsDbFavouriteProvider::new()));
    
    // Add Spotify provider
    manager.add_provider(Box::new(crate::helpers::spotify::SpotifyFavouriteProvider::new()));
    
    log::info!("Initialized favourite providers: {} total, {} enabled", 
               manager.provider_count(), 
               manager.enabled_provider_count());
}

/// Get a reference to the global favourite manager
pub fn get_favourite_manager() -> parking_lot::MutexGuard<'static, FavouriteManager> {
    GLOBAL_FAVOURITE_MANAGER.lock()
}

/// Check if a song is favourite using the global manager
pub fn is_favourite(song: &Song) -> Result<bool, FavouriteError> {
    get_favourite_manager().is_favourite(song)
}

/// Get which providers have the song marked as favourite (with display names) using the global manager
pub fn get_favourite_providers_display_names(song: &Song) -> Result<(bool, Vec<String>), FavouriteError> {
    get_favourite_manager().get_favourite_providers_display_names(song)
}

/// Add a song to favourites using the global manager
pub fn add_favourite(song: &Song) -> Result<Vec<String>, FavouriteError> {
    get_favourite_manager().add_favourite(song)
}

/// Remove a song from favourites using the global manager
pub fn remove_favourite(song: &Song) -> Result<Vec<String>, FavouriteError> {
    get_favourite_manager().remove_favourite(song)
}

/// Get enabled providers from the global manager
pub fn get_enabled_providers() -> Vec<String> {
    get_favourite_manager().get_enabled_providers().into_iter().map(|s| s.to_string()).collect()
}

/// Get provider count from the global manager
pub fn get_provider_count() -> (usize, usize) {
    let manager = get_favourite_manager();
    (manager.provider_count(), manager.enabled_provider_count())
}

/// Get detailed provider information from the global manager
pub fn get_provider_details() -> Vec<serde_json::Value> {
    get_favourite_manager().get_provider_details()
}
