use std::error::Error;
use std::fmt;
use acr_types::song::Song;
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
    manager.add_provider(Box::new(crate::lastfm::LastfmFavouriteProvider::new()));
    
    // Add SettingsDB provider
    manager.add_provider(Box::new(acr_store::settingsdb::SettingsDbFavouriteProvider::new()));
    
    // Add Spotify provider
    manager.add_provider(Box::new(crate::spotify::SpotifyFavouriteProvider::new()));
    
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

/// `FavouriteProvider` for the settings DB, backed by acr-store.
///
/// This impl lives here rather than beside [`acr_store::settingsdb::SettingsDbFavouriteProvider`]
/// itself: `settingsdb` moved into the `acr-store` crate, which cannot depend
/// back on this crate for the `FavouriteProvider` trait. Implementing a local
/// trait for a foreign type is exactly what Rust's orphan rule allows, so the
/// impl stays here instead.
impl FavouriteProvider for acr_store::settingsdb::SettingsDbFavouriteProvider {
    fn is_favourite(&self, song: &Song) -> Result<bool, FavouriteError> {
        let artist = song.artist.as_ref()
            .ok_or_else(|| FavouriteError::InvalidSong("Artist is required".to_string()))?;
        let title = song.title.as_ref()
            .ok_or_else(|| FavouriteError::InvalidSong("Title is required".to_string()))?;

        match acr_store::settingsdb::is_favourite_song(artist, title) {
            Ok(is_fav) => Ok(is_fav),
            Err(e) => Err(FavouriteError::StorageError(e)),
        }
    }

    fn add_favourite(&self, song: &Song) -> Result<(), FavouriteError> {
        let artist = song.artist.as_ref()
            .ok_or_else(|| FavouriteError::InvalidSong("Artist is required".to_string()))?;
        let title = song.title.as_ref()
            .ok_or_else(|| FavouriteError::InvalidSong("Title is required".to_string()))?;

        match acr_store::settingsdb::add_favourite_song(artist, title) {
            Ok(()) => Ok(()),
            Err(e) => Err(FavouriteError::StorageError(e)),
        }
    }

    fn remove_favourite(&self, song: &Song) -> Result<(), FavouriteError> {
        let artist = song.artist.as_ref()
            .ok_or_else(|| FavouriteError::InvalidSong("Artist is required".to_string()))?;
        let title = song.title.as_ref()
            .ok_or_else(|| FavouriteError::InvalidSong("Title is required".to_string()))?;

        match acr_store::settingsdb::remove_favourite_song(artist, title) {
            Ok(()) => Ok(()),
            Err(e) => Err(FavouriteError::StorageError(e)),
        }
    }

    fn get_favourite_count(&self) -> Option<usize> {
        // Use the existing get_all_favourite_songs function to count favorites
        match acr_store::settingsdb::get_all_favourite_songs() {
            Ok(songs) => Some(songs.len()),
            Err(_) => None, // Return None if we can't access the database
        }
    }

    fn provider_name(&self) -> &'static str {
        "settingsdb"
    }

    fn display_name(&self) -> &'static str {
        "User settings"
    }

    fn is_enabled(&self) -> bool {
        // Settings DB is always enabled if the database is accessible
        acr_store::settingsdb::settings_db_enabled()
    }

    fn is_active(&self) -> bool {
        // Settings DB is always active when enabled since it's a local database
        // No authentication or external connectivity required
        self.is_enabled() && acr_store::settingsdb::settings_db_has_connection()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use acr_store::settingsdb::{clear, SettingsDb, SettingsDbFavouriteProvider};
    use tempfile::TempDir;
    use serial_test::serial;

    #[test]
    #[serial]
    fn test_favourite_provider_count() {
        // Initialize the global settings database with a temporary path for testing
        let temp_dir = TempDir::new().unwrap();
        let test_path = temp_dir.path().to_str().unwrap();

        // Initialize the global database
        SettingsDb::initialize(test_path).ok();

        // Leaked deliberately, like test_concurrent_favourite_operations
        // below: this repoints the process-wide settings singleton, and
        // dropping the directory here would leave it pointing at nothing for
        // whichever test runs next.
        std::mem::forget(temp_dir);

        // Clear any existing data first
        clear().ok(); // Ignore errors if not initialized

        let provider = SettingsDbFavouriteProvider::new();

        // Initially should have 0 favorites
        assert_eq!(provider.get_favourite_count(), Some(0));

        // Create test songs
        let mut song1 = Song::default();
        song1.artist = Some("Test Artist 1".to_string());
        song1.title = Some("Test Song 1".to_string());

        let mut song2 = Song::default();
        song2.artist = Some("Test Artist 2".to_string());
        song2.title = Some("Test Song 2".to_string());

        let mut song3 = Song::default();
        song3.artist = Some("Test Artist 3".to_string());
        song3.title = Some("Test Song 3".to_string());

        // Add first favorite
        assert!(provider.add_favourite(&song1).is_ok());
        assert_eq!(provider.get_favourite_count(), Some(1));

        // Add second favorite
        assert!(provider.add_favourite(&song2).is_ok());
        assert_eq!(provider.get_favourite_count(), Some(2));

        // Add third favorite
        assert!(provider.add_favourite(&song3).is_ok());
        assert_eq!(provider.get_favourite_count(), Some(3));

        // Remove one favorite
        assert!(provider.remove_favourite(&song2).is_ok());
        assert_eq!(provider.get_favourite_count(), Some(2));

        // Remove another favorite
        assert!(provider.remove_favourite(&song1).is_ok());
        assert_eq!(provider.get_favourite_count(), Some(1));

        // Remove last favorite
        assert!(provider.remove_favourite(&song3).is_ok());
        assert_eq!(provider.get_favourite_count(), Some(0));

        // Clean up
        clear().ok();
    }

    #[test]
    #[serial]
    fn test_concurrent_favourite_operations() {
        use std::sync::Arc;
        use std::thread;

        // Initialize global database with a temp directory first.
        //
        // Leaked rather than dropped: this re-points the process-wide
        // singleton, and every later test that writes a setting keeps using
        // whatever this leaves behind. Letting the directory be deleted at the
        // end of this test would leave the global pointing at a path that is
        // gone, and the next writer anywhere in the suite fails with
        // "attempt to write a readonly database".
        let temp_dir = TempDir::new().expect("Failed to create temp directory");
        SettingsDb::initialize_global(temp_dir.path()).expect("Failed to initialize global database");
        std::mem::forget(temp_dir);

        // Clear any existing data first
        clear().ok();

        let provider = Arc::new(SettingsDbFavouriteProvider::new());
        let num_threads = 4;
        let songs_per_thread = 10;
        let mut handles = vec![];

        // Spawn threads that add/remove favourites concurrently
        for thread_id in 0..num_threads {
            let provider_clone = Arc::clone(&provider);
            let handle = thread::spawn(move || {
                for i in 0..songs_per_thread {
                    let mut song = Song::default();
                    song.artist = Some(format!("Artist_{}", thread_id));
                    song.title = Some(format!("Song_{}_{}", thread_id, i));

                    // Add favourite
                    provider_clone.add_favourite(&song).expect("Failed to add favourite");

                    // Check if it's marked as favourite
                    assert!(provider_clone.is_favourite(&song).expect("Failed to check favourite"));

                    // Remove every other favourite to test removal
                    if i % 2 == 0 {
                        provider_clone.remove_favourite(&song).expect("Failed to remove favourite");
                        assert!(!provider_clone.is_favourite(&song).expect("Failed to check favourite after removal"));
                    }
                }
            });
            handles.push(handle);
        }

        // Wait for all threads to complete
        for handle in handles {
            handle.join().expect("Thread panicked");
        }

        // Check final favourite count
        // Each thread adds songs_per_thread favourites but removes half of them
        let expected_count = num_threads * (songs_per_thread / 2);
        let actual_count = provider.get_favourite_count().unwrap_or(0);
        assert_eq!(actual_count, expected_count);

        // Clean up
        clear().ok();
    }
}
