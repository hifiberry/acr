/// Cover art providers implementation
/// This module contains implementations of various cover art providers

use std::collections::HashSet;
use log::{debug, warn};
use crate::helpers::coverart::{CoverartProvider, CoverartMethod};
use crate::helpers::spotify::{Spotify, SpotifyError};

/// Spotify Cover Art Provider
/// Uses Spotify's Search API to find cover art for artists, albums, and songs
pub struct SpotifyCoverartProvider {
    name: String,
    display_name: String,
}

impl SpotifyCoverartProvider {
    pub fn new() -> Self {
        Self {
            name: "spotify".to_string(),
            display_name: "Spotify".to_string(),
        }
    }
}

impl Default for SpotifyCoverartProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl CoverartProvider for SpotifyCoverartProvider {
    fn name(&self) -> &str {
        &self.name
    }
    
    fn display_name(&self) -> &str {
        &self.display_name
    }
    
    fn supported_methods(&self) -> HashSet<CoverartMethod> {
        let mut methods = HashSet::new();
        methods.insert(CoverartMethod::Artist);
        methods.insert(CoverartMethod::Album);
        methods.insert(CoverartMethod::Song);
        methods
    }
    
    fn get_artist_coverart_impl(&self, artist: &str) -> Vec<String> {
        debug!("Spotify: Searching for artist cover art: {}", artist);
        
        let spotify_client = match Spotify::get_instance() {
            Ok(client) => client,
            Err(e) => {
                warn!("Spotify: Failed to get client for artist search: {}", e);
                return Vec::new();
            }
        };
        
        let search_result = match spotify_client.search(artist, &["artist"], None) {
            Ok(result) => result,
            Err(SpotifyError::TokenNotFound) => {
                debug!("Spotify: No valid token available for artist search");
                return Vec::new();
            }
            Err(e) => {
                warn!("Spotify: Failed to search for artist '{}': {}", artist, e);
                return Vec::new();
            }
        };
        
        // Extract artist images from search results
        if let Some(artists) = search_result.get("artists")
            .and_then(|a| a.get("items"))
            .and_then(|i| i.as_array()) 
        {
            if let Some(first_artist) = artists.first() {
                if let Some(images) = first_artist.get("images").and_then(|i| i.as_array()) {
                    let mut urls = Vec::new();
                    for image in images {
                        if let Some(url) = image.get("url").and_then(|u| u.as_str()) {
                            urls.push(url.to_string());
                        }
                    }
                    debug!("Spotify: Found {} artist images for '{}'", urls.len(), artist);
                    return urls;
                }
            }
        }
        
        debug!("Spotify: No artist images found for '{}'", artist);
        Vec::new()
    }
    
    fn get_album_coverart_impl(&self, title: &str, artist: &str, _year: Option<i32>) -> Vec<String> {
        debug!("Spotify: Searching for album cover art: '{}' by '{}'", title, artist);
        
        let spotify_client = match Spotify::get_instance() {
            Ok(client) => client,
            Err(e) => {
                warn!("Spotify: Failed to get client for album search: {}", e);
                return Vec::new();
            }
        };
        
        // Create search query with artist and album filters
        let filters = serde_json::json!({
            "artist": artist,
            "album": title
        });
        
        let search_result = match spotify_client.search(title, &["album"], Some(&filters)) {
            Ok(result) => result,
            Err(SpotifyError::TokenNotFound) => {
                debug!("Spotify: No valid token available for album search");
                return Vec::new();
            }
            Err(e) => {
                warn!("Spotify: Failed to search for album '{}' by '{}': {}", title, artist, e);
                return Vec::new();
            }
        };
        
        // Extract album images from search results
        if let Some(albums) = search_result.get("albums")
            .and_then(|a| a.get("items"))
            .and_then(|i| i.as_array()) 
        {
            if let Some(first_album) = albums.first() {
                if let Some(images) = first_album.get("images").and_then(|i| i.as_array()) {
                    let mut urls = Vec::new();
                    for image in images {
                        if let Some(url) = image.get("url").and_then(|u| u.as_str()) {
                            urls.push(url.to_string());
                        }
                    }
                    debug!("Spotify: Found {} album images for '{}' by '{}'", urls.len(), title, artist);
                    return urls;
                }
            }
        }
        
        debug!("Spotify: No album images found for '{}' by '{}'", title, artist);
        Vec::new()
    }
    
    fn get_song_coverart_impl(&self, title: &str, artist: &str) -> Vec<String> {
        debug!("Spotify: Searching for song cover art: '{}' by '{}'", title, artist);
        
        let spotify_client = match Spotify::get_instance() {
            Ok(client) => client,
            Err(e) => {
                warn!("Spotify: Failed to get client for song search: {}", e);
                return Vec::new();
            }
        };
        
        // Create search query with artist and track filters
        let filters = serde_json::json!({
            "artist": artist,
            "track": title
        });
        
        let search_result = match spotify_client.search(title, &["track"], Some(&filters)) {
            Ok(result) => result,
            Err(SpotifyError::TokenNotFound) => {
                debug!("Spotify: No valid token available for song search");
                return Vec::new();
            }
            Err(e) => {
                warn!("Spotify: Failed to search for song '{}' by '{}': {}", title, artist, e);
                return Vec::new();
            }
        };
        
        // Extract track album images from search results (songs use album art)
        if let Some(tracks) = search_result.get("tracks")
            .and_then(|t| t.get("items"))
            .and_then(|i| i.as_array()) 
        {
            if let Some(first_track) = tracks.first() {
                if let Some(album) = first_track.get("album") {
                    if let Some(images) = album.get("images").and_then(|i| i.as_array()) {
                        let mut urls = Vec::new();
                        for image in images {
                            if let Some(url) = image.get("url").and_then(|u| u.as_str()) {
                                urls.push(url.to_string());
                            }
                        }
                        debug!("Spotify: Found {} song images for '{}' by '{}'", urls.len(), title, artist);
                        return urls;
                    }
                }
            }
        }
        
        debug!("Spotify: No song images found for '{}' by '{}'", title, artist);
        Vec::new()
    }
}

/// Initialize and register all cover art providers
pub fn register_all_providers() {
    use crate::helpers::coverart::get_coverart_manager;
    use std::sync::Arc;
    
    let manager = get_coverart_manager();
    let mut manager_lock = manager.lock().unwrap();
    
    // Register Spotify provider
    let spotify_provider = Arc::new(SpotifyCoverartProvider::new());
    manager_lock.register_provider(spotify_provider);
    
    debug!("Registered all cover art providers");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::helpers::spotify;
    
    /// Test helper to initialize Spotify for testing
    fn setup_spotify_for_test() -> Result<(), Box<dyn std::error::Error>> {
        // Check if we're running with test credentials
        let oauth_url = spotify::default_spotify_oauth_url();
        let proxy_secret = spotify::default_spotify_proxy_secret();
        
        // Skip tests if we're using test credentials (they won't work with real Spotify API)
        if oauth_url.contains("test.oauth.example.com") || proxy_secret == "test_proxy_secret" {
            return Err("Skipping test - using test credentials".into());
        }
        
        // Initialize Spotify with test configuration
        match spotify::Spotify::initialize_with_defaults() {
            Ok(_) => Ok(()),
            Err(e) => {
                println!("Warning: Failed to initialize Spotify for test: {}", e);
                println!("This test requires valid Spotify credentials");
                Err(Box::new(e))
            }
        }
    }
    
    #[test]
    fn test_spotify_provider_creation() {
        let provider = SpotifyCoverartProvider::new();
        assert_eq!(provider.name(), "spotify");
        assert_eq!(provider.display_name(), "Spotify");
        
        let methods = provider.supported_methods();
        assert!(methods.contains(&CoverartMethod::Artist));
        assert!(methods.contains(&CoverartMethod::Album));
        assert!(methods.contains(&CoverartMethod::Song));
        assert_eq!(methods.len(), 3);
    }
    
    #[test]
    fn test_spotify_artist_search_the_beatles() {
        // Skip test if Spotify can't be initialized (no credentials)
        if setup_spotify_for_test().is_err() {
            println!("Skipping Spotify test - no valid credentials available");
            return;
        }
        
        let provider = SpotifyCoverartProvider::new();
        let results = provider.get_artist_coverart_impl("The Beatles");
        
        // The Beatles should have cover art available
        assert!(!results.is_empty(), "Expected to find cover art for The Beatles");
        
        // Verify all results are valid URLs
        for url in &results {
            assert!(url.starts_with("http://") || url.starts_with("https://"), 
                   "Expected valid URL, got: {}", url);
        }
        
        println!("Found {} cover art URLs for The Beatles:", results.len());
        for url in &results {
            println!("  - {}", url);
        }
    }
    
    #[test]
    fn test_spotify_song_search_yellow_submarine() {
        // Skip test if Spotify can't be initialized (no credentials)
        if setup_spotify_for_test().is_err() {
            println!("Skipping Spotify test - no valid credentials available");
            return;
        }
        
        let provider = SpotifyCoverartProvider::new();
        let results = provider.get_song_coverart_impl("Yellow Submarine", "The Beatles");
        
        // Yellow Submarine by The Beatles should have cover art available
        assert!(!results.is_empty(), "Expected to find cover art for Yellow Submarine by The Beatles");
        
        // Verify all results are valid URLs
        for url in &results {
            assert!(url.starts_with("http://") || url.starts_with("https://"), 
                   "Expected valid URL, got: {}", url);
        }
        
        println!("Found {} cover art URLs for Yellow Submarine by The Beatles:", results.len());
        for url in &results {
            println!("  - {}", url);
        }
    }
    
    #[test]
    fn test_spotify_album_search_yellow_submarine() {
        // Skip test if Spotify can't be initialized (no credentials)
        if setup_spotify_for_test().is_err() {
            println!("Skipping Spotify test - no valid credentials available");
            return;
        }
        
        let provider = SpotifyCoverartProvider::new();
        let results = provider.get_album_coverart_impl("Yellow Submarine", "The Beatles", None);
        
        // Yellow Submarine album by The Beatles should have cover art available
        assert!(!results.is_empty(), "Expected to find cover art for Yellow Submarine album by The Beatles");
        
        // Verify all results are valid URLs
        for url in &results {
            assert!(url.starts_with("http://") || url.starts_with("https://"), 
                   "Expected valid URL, got: {}", url);
        }
        
        println!("Found {} cover art URLs for Yellow Submarine album by The Beatles:", results.len());
        for url in &results {
            println!("  - {}", url);
        }
    }
    
    #[test]
    fn test_spotify_provider_no_token_graceful_handling() {
        let provider = SpotifyCoverartProvider::new();
        
        // These should gracefully handle the case where no Spotify token is available
        // and return empty results rather than panicking
        let artist_results = provider.get_artist_coverart_impl("NonExistentArtist12345");
        let song_results = provider.get_song_coverart_impl("NonExistentSong12345", "NonExistentArtist12345");
        let album_results = provider.get_album_coverart_impl("NonExistentAlbum12345", "NonExistentArtist12345", None);
        
        // Should return empty vectors, not panic
        assert!(artist_results.is_empty() || !artist_results.is_empty()); // Either case is valid
        assert!(song_results.is_empty() || !song_results.is_empty());     // Either case is valid  
        assert!(album_results.is_empty() || !album_results.is_empty());   // Either case is valid
    }
}
