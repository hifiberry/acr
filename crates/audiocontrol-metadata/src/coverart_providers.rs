/// Cover art providers implementation
/// This module contains implementations of various cover art providers
use std::collections::HashSet;
use log::{debug, info, warn};
use crate::coverart::{CoverartProvider, CoverartMethod};
use crate::fanarttv::FanarttvCoverartProvider;
use crate::spotify::{Spotify, SpotifyError};
use crate::theaudiodb::TheAudioDbCoverartProvider;
use crate::lastfm::{LastfmClient, LastfmError, LastfmTrackInfoDetails};
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

/// Last.fm's image size slots, largest first.
const LASTFM_IMAGE_SIZES: [&str; 5] = ["mega", "extralarge", "large", "medium", "small"];

/// Pick the cover art from the album Last.fm reports for a track.
///
/// Last.fm returns the same picture once per size slot, each under its own URL
/// -- the size is a path segment, so the URLs differ and deduplicating them
/// achieves nothing. Every URL returned here costs the grader a network round
/// trip, taken while the cover art manager's lock is held, so only the largest
/// slot is worth returning.
// `pub` rather than `pub(crate)`: the Last.fm action plugin calls this and
// stayed in the player daemon, which is now a different crate.
pub fn album_image_urls(track_info: &LastfmTrackInfoDetails) -> Vec<String> {
    let Some(album) = &track_info.album else {
        return Vec::new();
    };

    let by_size = |wanted: &str| {
        album
            .image
            .iter()
            .find(|image| image.size == wanted && !image.url.is_empty())
            .map(|image| image.url.clone())
    };

    LASTFM_IMAGE_SIZES
        .iter()
        .find_map(|size| by_size(size))
        // A slot Last.fm did not label, or labelled in a way we do not know:
        // the last non-empty entry is its largest by convention.
        .or_else(|| {
            album
                .image
                .iter()
                .rev()
                .find(|image| !image.url.is_empty())
                .map(|image| image.url.clone())
        })
        .into_iter()
        .collect()
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
        methods.insert(CoverartMethod::Song);
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

    fn get_song_coverart_impl(&self, title: &str, artist: &str) -> Vec<String> {
        debug!("LastFM: Searching for song cover art: '{}' by '{}'", title, artist);

        let lastfm_client = match LastfmClient::get_instance() {
            Ok(client) => client,
            Err(LastfmError::ConfigError(_)) => {
                debug!("LastFM: Client not initialized for song search");
                return Vec::new();
            }
            Err(e) => {
                warn!("LastFM: Failed to get client for song search: {}", e);
                return Vec::new();
            }
        };

        // The unsigned lookup: cover art is not user-specific, and requiring a
        // linked account here would leave the song method as empty as the
        // Spotify-only arrangement it replaces.
        let track_info = match lastfm_client.get_track_album_info(artist, title) {
            Ok(info) => info,
            Err(e) => {
                warn!(
                    "LastFM: Failed to get track info for '{}' by '{}': {}",
                    title, artist, e
                );
                return Vec::new();
            }
        };

        let urls = album_image_urls(&track_info);
        debug!(
            "LastFM: Found {} song images for '{}' by '{}'",
            urls.len(),
            title,
            artist
        );
        urls
    }
}

/// Initialize and register all cover art providers
pub fn register_all_providers() {
    use crate::coverart::get_coverart_manager;
    
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

    // Register every configured external endpoint. These are slow by
    // declaration, so the fan-out keeps them off the fast path; they are here
    // so /api/coverart can serve their cached answers and honour
    // ?include_slow=true.
    for provider in crate::external_coverart::configured_providers() {
        info!(
            "Registering external coverart provider: {} ({}), methods {:?}",
            provider.name(),
            provider.display_name(),
            provider.supported_methods()
        );
        manager_lock.register_provider(provider);
    }

    info!("Final provider count: {}", manager_lock.provider_count());
    info!("Registered all cover art providers");
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Track info as Last.fm returns it, with the given image URLs on the album.
    fn track_info_with_images(images: &[(&str, &str)]) -> LastfmTrackInfoDetails {
        let image: Vec<_> = images
            .iter()
            .map(|(size, url)| serde_json::json!({ "#text": url, "size": size }))
            .collect();
        serde_json::from_value(serde_json::json!({
            "name": "Listen To The News",
            "url": "https://www.last.fm/music/example",
            "duration": "0",
            "listeners": "1",
            "playcount": "1",
            "artist": {
                "name": "Radical Friendship Theory",
                "url": "https://www.last.fm/music/example"
            },
            "album": {
                "artist": "Radical Friendship Theory",
                "title": "Radical Friendship Theory",
                "url": "https://www.last.fm/music/example/album",
                "image": image
            }
        }))
        .expect("track info fixture should deserialize")
    }

    /// Track info without an album block at all.
    fn track_info_without_album() -> LastfmTrackInfoDetails {
        serde_json::from_value(serde_json::json!({
            "name": "Listen To The News",
            "url": "https://www.last.fm/music/example",
            "duration": "0",
            "listeners": "1",
            "playcount": "1",
            "artist": {
                "name": "Radical Friendship Theory",
                "url": "https://www.last.fm/music/example"
            }
        }))
        .expect("track info fixture should deserialize")
    }

    /// Last.fm serves one picture per size slot, the size being a path segment,
    /// so the URLs differ. Only the largest is worth returning: the grader
    /// fetches each one over the network while holding the manager's lock.
    #[test]
    fn only_the_largest_album_image_is_returned() {
        let info = track_info_with_images(&[
            ("small", "https://lastfm.example/i/u/34s/cover.png"),
            ("medium", "https://lastfm.example/i/u/64s/cover.png"),
            ("large", "https://lastfm.example/i/u/174s/cover.png"),
            ("extralarge", "https://lastfm.example/i/u/300x300/cover.png"),
        ]);

        assert_eq!(
            album_image_urls(&info),
            vec!["https://lastfm.example/i/u/300x300/cover.png".to_string()]
        );
    }

    /// Not every album carries every slot.
    #[test]
    fn the_largest_available_slot_is_used_when_bigger_ones_are_missing() {
        let info = track_info_with_images(&[
            ("small", "https://lastfm.example/i/u/34s/cover.png"),
            ("large", "https://lastfm.example/i/u/174s/cover.png"),
        ]);

        assert_eq!(
            album_image_urls(&info),
            vec!["https://lastfm.example/i/u/174s/cover.png".to_string()]
        );
    }

    /// Last.fm pads its image array with empty entries for sizes it has none of.
    #[test]
    fn empty_slots_are_skipped() {
        let info = track_info_with_images(&[
            ("extralarge", ""),
            ("large", "https://lastfm.example/i/u/174s/cover.png"),
        ]);

        assert_eq!(
            album_image_urls(&info),
            vec!["https://lastfm.example/i/u/174s/cover.png".to_string()]
        );
    }

    /// A slot Last.fm labels in a way this code does not know still yields an
    /// image, and the fallback takes the last entry rather than the first --
    /// Last.fm orders its slots smallest to largest, so the last is the
    /// biggest. A single-entry fixture would pass either way and prove nothing.
    #[test]
    fn an_unknown_size_label_yields_the_last_entry() {
        let info = track_info_with_images(&[
            ("", "https://lastfm.example/i/u/34s/cover.png"),
            ("", "https://lastfm.example/i/u/cover.png"),
        ]);

        assert_eq!(
            album_image_urls(&info),
            vec!["https://lastfm.example/i/u/cover.png".to_string()]
        );
    }

    /// Last.fm knows plenty of tracks it cannot place on an album.
    #[test]
    fn no_album_block_means_no_song_images() {
        assert!(album_image_urls(&track_info_without_album()).is_empty());
    }

    /// Without this the song method has only Spotify behind it, so a device
    /// with no Spotify link can never resolve cover art for a track.
    #[test]
    fn lastfm_supports_song_lookup() {
        assert!(LastfmCoverartProvider::new()
            .supported_methods()
            .contains(&CoverartMethod::Song));
    }
}
