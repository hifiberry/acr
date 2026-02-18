/// Cover art providers implementation
/// This module contains implementations of various cover art providers

use std::collections::HashSet;
use log::{debug, info, warn};
use crate::helpers::coverart::{CoverartProvider, CoverartMethod};
use crate::helpers::fanarttv::FanarttvCoverartProvider;
use crate::helpers::spotify::{Spotify, SpotifyError};
use crate::helpers::theaudiodb::TheAudioDbCoverartProvider;
use crate::helpers::lastfm::{LastfmClient, LastfmError};
use std::sync::Arc;

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

/// LastFM Cover Art Provider
/// Uses LastFM's Artist.getInfo API to find cover art for artists
pub struct LastfmCoverartProvider {
    name: String,
    display_name: String,
}

impl LastfmCoverartProvider {
    pub fn new() -> Self {
        Self {
            name: "lastfm".to_string(),
            display_name: "Last.fm".to_string(),
        }
    }
}

impl Default for LastfmCoverartProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl CoverartProvider for LastfmCoverartProvider {
    fn name(&self) -> &str {
        &self.name
    }
    
    fn display_name(&self) -> &str {
        &self.display_name
    }
    
    fn supported_methods(&self) -> HashSet<CoverartMethod> {
        let mut methods = HashSet::new();
        methods.insert(CoverartMethod::Artist);
        methods
    }
    
    fn get_artist_coverart_impl(&self, artist: &str) -> Vec<String> {
        debug!("LastFM: Searching for artist images: {}", artist);
        
        let lastfm_client = match LastfmClient::get_instance() {
            Ok(client) => client,
            Err(LastfmError::ConfigError(_)) => {
                debug!("LastFM: Client not initialized for artist search");
                return Vec::new();
            }
            Err(e) => {
                warn!("LastFM: Failed to get client for artist search: {}", e);
                return Vec::new();
            }
        };
        
        let artist_info = match lastfm_client.get_artist_info(artist) {
            Ok(info) => info,
            Err(e) => {
                warn!("LastFM: Failed to get artist info for '{}': {}", artist, e);
                return Vec::new();
            }
        };
        
        // Extract image URLs from artist info
        let mut urls = Vec::new();
        for image in &artist_info.image {
            if !image.url.is_empty() {
                urls.push(image.url.clone());
                debug!("LastFM: Found {} image for artist '{}': {}", image.size, artist, image.url);
            }
        }
        
        debug!("LastFM: Found {} artist images for '{}'", urls.len(), artist);
        urls
    }
}

/// Initialize and register all cover art providers
pub fn register_all_providers() {
    use crate::helpers::coverart::get_coverart_manager;
    
    info!("Starting provider registration...");
    
    let manager = get_coverart_manager();
    let mut manager_lock = manager.lock();
    
    info!("Manager lock acquired, current provider count: {}", manager_lock.provider_count());
    
    // Register Spotify cover art provider
    info!("Creating Spotify coverart provider...");
    let spotify_coverart = Arc::new(SpotifyCoverartProvider::new());
    info!("Registering Spotify coverart provider: {} ({})", spotify_coverart.name(), spotify_coverart.display_name());
    info!("Spotify coverart supported methods: {:?}", spotify_coverart.supported_methods());
    manager_lock.register_provider(spotify_coverart);
    
    // Register LastFM cover art provider
    info!("Creating LastFM coverart provider...");
    let lastfm_coverart = Arc::new(LastfmCoverartProvider::new());
    info!("Registering LastFM coverart provider: {} ({})", lastfm_coverart.name(), lastfm_coverart.display_name());
    info!("LastFM coverart supported methods: {:?}", lastfm_coverart.supported_methods());
    manager_lock.register_provider(lastfm_coverart);
    
    // Register TheAudioDB cover art provider
    info!("Creating TheAudioDB coverart provider...");
    let theaudiodb_coverart = Arc::new(TheAudioDbCoverartProvider::new());
    info!("Registering TheAudioDB coverart provider: {} ({})", theaudiodb_coverart.name(), theaudiodb_coverart.display_name());
    info!("TheAudioDB coverart supported methods: {:?}", theaudiodb_coverart.supported_methods());
    manager_lock.register_provider(theaudiodb_coverart);
    
    // Register FanArt.tv cover art provider
    info!("Creating FanArt.tv coverart provider...");
    let fanarttv_coverart = Arc::new(FanarttvCoverartProvider::new());
    info!("Registering FanArt.tv coverart provider: {} ({})", fanarttv_coverart.name(), fanarttv_coverart.display_name());
    info!("FanArt.tv coverart supported methods: {:?}", fanarttv_coverart.supported_methods());
    manager_lock.register_provider(fanarttv_coverart);
    
    info!("Final provider count: {}", manager_lock.provider_count());
    info!("Registered all cover art providers");
}
