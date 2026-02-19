use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use parking_lot::{Mutex, RwLock};
use std::time::Instant;
use log::{debug, info, warn, error};
use crate::data::{Album, AlbumArtists, Artist, LibraryError, LibraryInterface};
use crate::helpers::http_client;
use crate::players::lms::jsonrps::LmsRpcClient;
use crate::players::lms::lmsaudio::lms_image_url;

/// LMS library interface that provides access to albums and artists
#[derive(Clone)]
pub struct LMSLibrary {
    /// Client for communicating with the LMS server
    client: Arc<LmsRpcClient>,
    
    /// Cache of albums, key is album name
    albums: Arc<RwLock<HashMap<String, Album>>>,
    
    /// Cache of artists, key is artist name
    artists: Arc<RwLock<HashMap<String, Artist>>>,
    
    /// Album to artist relationships
    album_artists: Arc<RwLock<AlbumArtists>>,
    
    /// Flag indicating if library is loaded
    library_loaded: Arc<Mutex<bool>>,
    
    /// Library loading progress (0.0 - 1.0)
    loading_progress: Arc<Mutex<f32>>,
    
    /// Custom artist separators for splitting artist names
    artist_separators: Arc<Mutex<Option<Vec<String>>>>,
    
    /// Flag to control metadata enhancement
    enhance_metadata: bool,
}

impl LMSLibrary {
    /// Create a new LMS library interface with specific connection details
    pub fn with_connection(hostname: &str, port: u16) -> Self {
        debug!("Creating new LMSLibrary with connection {}:{}", hostname, port);
        
        // Create an LmsRpcClient for communicating with the server
        let client = Arc::new(LmsRpcClient::new(hostname, port));
        
        LMSLibrary {
            client,
            albums: Arc::new(RwLock::new(HashMap::new())),
            artists: Arc::new(RwLock::new(HashMap::new())),
            album_artists: Arc::new(RwLock::new(AlbumArtists::new())),
            library_loaded: Arc::new(Mutex::new(false)),
            loading_progress: Arc::new(Mutex::new(0.0)),
            artist_separators: Arc::new(Mutex::new(None)),
            enhance_metadata: true,
        }
    }
    /// Populate calculated fields in album objects
    /// 
    /// This adds derived fields like cover_art URL for albums that don't have them yet
    /// these calculates fields are not stored, but only calculated on demand
    pub fn populate_calculated_album_fields(&self, album: &mut Album) {
        // Add cover_art URL if not present
        if album.cover_art.is_none() {
            if let crate::data::Identifier::Numeric(album_id) = album.id {
                // Use the lms_image_url function from LMS audio controller
                let image_url = format!("{}/album:{}",
                    lms_image_url(), album_id);
                album.cover_art = Some(image_url);
            }
        }
    }

    /// Get the current library loading progress (0.0 to 1.0)
    pub fn get_loading_progress(&self) -> f32 {
        *self.loading_progress.lock()
    }
    
    /// Set custom artist separators for use in library operations
    pub fn set_artist_separators(&mut self, separators: Vec<String>) {
        debug!("Setting custom artist separators in LMSLibrary: {:?}", separators);
        let mut sep_guard = self.artist_separators.lock();
        *sep_guard = Some(separators);
    }
    /// Get custom artist separators for artist name splitting
    pub fn get_artist_separators(&self) -> Option<Vec<String>> {
        // Return the stored separators if available
        self.artist_separators.lock().clone()
    }
        
    /// Create artist objects from all album artist data
    ///
    /// This method scans all albums in the library, extracts all artist names
    /// from the album artists list, and creates Artist objects for each if they 
    /// don't already exist. It also updates the album-artist relationships.
    pub fn create_artists(&self) -> Result<usize, LibraryError> {
        debug!("Creating artist objects from album artist data");
        let start_time = Instant::now();
        
        let mut created_count = 0;
        
        // First, get a read lock on the albums to extract all artist names
        let albums = self.albums.read();
        
        // Collect all artist names from albums and their IDs
        let mut artist_names = HashSet::new();
        let mut album_artist_relations = Vec::new();
        
        // Go through all albums and collect artist names
        for album in albums.values() {
            // Extract artist names from the album's artists list
            let album_artists = album.artists.lock();
            for artist_name in album_artists.iter() {
                artist_names.insert(artist_name.clone());

                // Store album-artist relationship for later
                album_artist_relations.push((album.id.clone(), artist_name.clone()));
            }
        }
        
        debug!("Found {} unique artist names in albums", artist_names.len());
        
        // Now, get a write lock on the artists collection to add new artists
        let mut artists = self.artists.write();

        // Get a write lock on the album_artists relationships
        let mut album_artists = self.album_artists.write();
        
        // Create a new artist object for each name that doesn't already exist
        for artist_name in artist_names {
            // Skip if the artist already exists
            if artists.contains_key(&artist_name) {
                continue;
            }
            
            // Create a unique ID for the artist based on the name
            use std::collections::hash_map::DefaultHasher;
            use std::hash::{Hash, Hasher};
            use crate::data::Identifier;
            
            let mut hasher = DefaultHasher::new();
            artist_name.hash(&mut hasher);
            let artist_id = hasher.finish();
            
            // Create a new Artist object
            let artist = Artist {
                id: Identifier::Numeric(artist_id),
                name: artist_name.clone(),
                is_multi: false,  // Default to false, can be updated later if needed
                metadata: None,
            };

            // lookup cache_key "artist::metadata::<artistname>" for cached metadata
            let cache_key = format!("artist::metadata::{}", artist_name);
            
            // Try to load metadata from the attribute cache
            let mut artist_with_metadata = artist;
            match crate::helpers::attributecache::get::<crate::data::ArtistMeta>(&cache_key) {
                Ok(Some(cached_metadata)) => {
                    debug!("Loaded metadata for artist {} from attribute cache", artist_name);
                    artist_with_metadata.metadata = Some(cached_metadata);
                    
                    // Check if this is a multi-artist (having multiple MBIDs or partial match)
                    if let Some(ref meta) = artist_with_metadata.metadata {
                        if meta.mbid.len() > 1 || meta.is_partial_match {
                            artist_with_metadata.is_multi = true;
                            debug!("Marked {} as multi-artist based on cached metadata", artist_name);
                        }
                    }
                },
                Ok(None) => {
                    debug!("No cached metadata found for artist {}", artist_name);
                },
                Err(e) => {
                    warn!("Error loading cached metadata for artist {}: {}", artist_name, e);
                }
            }

            // Insert the artist with potentially loaded metadata
            artists.insert(artist_name.clone(), artist_with_metadata);
            created_count += 1;
        }
        
        // Update album-artist relationships
        for (album_id, artist_name) in album_artist_relations {
            // Get artist ID (if it exists)
            if let Some(artist) = artists.get(&artist_name) {
                // Add relationship between album and artist
                album_artists.add_mapping(album_id, artist.id.clone());
            }
        }
        
        let elapsed = start_time.elapsed();
        info!("Created {} new artists in {:?}", created_count, elapsed);
        
        Ok(created_count)
    }
    
    /// Get artists collection as Arc for direct updating
    pub fn get_artists_arc(&self) -> Arc<RwLock<HashMap<String, Artist>>> {
        self.artists.clone()
    }

    /// Get album by ID
    pub fn get_album_by_id(&self, id: &crate::data::Identifier) -> Option<Album> {
        let albums = self.albums.read();
        // Search through all albums to find one with matching ID
        for album in albums.values() {
            if &album.id == id {
                let mut album_clone = album.clone();
                self.populate_calculated_album_fields(&mut album_clone);
                return Some(album_clone);
            }
        }
        None
    }

    /// Get albums by artist ID
    pub fn get_albums_by_artist_id(&self, artist_id: &crate::data::Identifier) -> Vec<Album> {
        let mut result = Vec::new();

        // Get albums associated with this artist ID from album_artists mapping
        let album_artists_mapping = self.album_artists.read();
        let album_ids = album_artists_mapping.get_albums_for_artist(artist_id);

        // Get all albums and fetch the ones with matching IDs
        let albums = self.albums.read();
        for album in albums.values() {
            if album_ids.contains(&album.id) {
                let mut album_clone = album.clone();
                self.populate_calculated_album_fields(&mut album_clone);
                result.push(album_clone);
            }
        }

        result
    }

    /// Get albums by artist name
    pub fn get_albums_by_artist(&self, artist_name: &str) -> Vec<Album> {
        let mut result = Vec::new();

        // First get the artist by name to get the artist ID
        if let Some(artist) = self.get_artist_by_name(artist_name) {
            let artist_id = artist.id;

            // Get albums associated with this artist from album_artists mapping
            let album_artists_mapping = self.album_artists.read();
            let album_ids = album_artists_mapping.get_albums_for_artist(&artist_id);

            // Get all albums and fetch the ones with matching IDs
            let albums = self.albums.read();
            for album in albums.values() {
                if album_ids.contains(&album.id) {
                    let mut album_clone = album.clone();
                    self.populate_calculated_album_fields(&mut album_clone);
                    result.push(album_clone);
                }
            }
        }

        result
    }

    /// Get album by artist and album name
    pub fn get_album_by_artist_and_name(&self, artist: &str, album: &str) -> Option<Album> {
        // Implementation to find album by both artist and album name
        let albums = self.albums.read();
        // Look for an album with the specified name
        if let Some(album_obj) = albums.get(album) {
            // If we found the album, check if it has the specified artist
            let album_artists = album_obj.artists.lock();
            // If the album has the specified artist (case-insensitive comparison)
            if album_artists.iter().any(|a| a.to_lowercase() == artist.to_lowercase()) {
                let mut album_clone = album_obj.clone();
                self.populate_calculated_album_fields(&mut album_clone);
                return Some(album_clone);
            }
        }

        // Album not found or artist doesn't match
        None
    }

    /// Get artist by name
    pub fn get_artist_by_name(&self, name: &str) -> Option<Artist> {
        self.artists.read().get(name).cloned()
    }    /// Returns the URL for a track's cover artwork
    /// 
    /// # Arguments
    /// * `track_id` - The ID of the track, which can be extracted from track URI
    /// 
    /// # Returns
    /// A string containing the URL for the track's cover artwork in the format:
    /// `http://<server>:<port>/music/<track_id>/cover.jpg`
    pub fn track_cover_url(&self, track_id: &str) -> String {
        // Get server address from the client
        let server_addr = match self.client.get_server_address() {
            Ok(addr) => addr,
            Err(_) => "localhost".to_string(), // Default to localhost if we can't get the address
        };
        
        // Get server port from the client
        let port = self.client.get_server_port();
        
        // Extract the track ID from the URI if it's in a format like file:///path/to/track
        let id = if track_id.contains("://") {
            // For file URIs, extract just the file path part
            if let Some(path) = track_id.split("://").nth(1) {
                path
            } else {
                track_id
            }
        } else {
            track_id
        };
        
        // Construct and return the URL
        format!("http://{}:{}/music/{}/cover.jpg", server_addr, port, id)
    }
}

impl LibraryInterface for LMSLibrary {
    fn new() -> Self {
        debug!("Creating new LMSLibrary with default connection");
        Self::with_connection("localhost", 9000) // Default LMS port is 9000
    }
    fn is_loaded(&self) -> bool {
        let loaded = self.library_loaded.lock();
        debug!("Library is_loaded check returning: {}", *loaded);
        *loaded
    }
    
    fn refresh_library(&self) -> Result<(), LibraryError> {
        debug!("Refreshing LMS library data using LMSLibraryLoader");
        let start_time = Instant::now();
        
        // Use our LMSLibraryLoader to load albums
        let loader = super::libraryloader::LMSLibraryLoader::new(
            self.client.clone()
        );
        
        // Get artist separators from the configuration, if any
        let artist_separators = self.get_artist_separators();
        
        let result = match loader.load_albums_from_lms(artist_separators) {
            Ok(albums) => {
                // Mark as not loaded during update
                { let mut loaded = self.library_loaded.lock(); *loaded = false; }

                // Reset loading progress to 0
                { let mut progress = self.loading_progress.lock(); *progress = 0.0; }
                
                // Update albums collection
                {
                    let mut self_albums = self.albums.write();
                    self_albums.clear();

                    // Add each album to the collection with name as key
                    for mut album in albums {
                        self.populate_calculated_album_fields(&mut album);
                        self_albums.insert(album.name.clone(), album);
                    }

                    info!("Updated library with {} albums", self_albums.len());
                }
                
                // Create artists and update album-artist relationships
                if let Err(e) = self.create_artists() {
                    error!("Error creating artists: {}", e);
                }
                // Mark as loaded and update progress
                {
                    let mut loaded = self.library_loaded.lock();
                    *loaded = true;
                    info!("Setting library_loaded flag to true");
                }

                { let mut progress = self.loading_progress.lock(); *progress = 1.0; }
                
                let total_time = start_time.elapsed();
                info!("Library load complete in {:.2?}", total_time);
                
                // Start background update of artist metadata now that the library is fully loaded
                if self.enhance_metadata {
                    info!("Starting background metadata update for artists");
                    crate::helpers::artistupdater::update_library_artists_metadata_in_background(
                        self.artists.clone()
                    );
                }
                
                Ok(())
            },
            Err(e) => {
                error!("Error loading LMS library: {}", e);
                Err(e)
            }
        };
        
        result
    }
    
    fn get_albums(&self) -> Vec<Album> {
        warn!("Retrieving all albums from LMSLibrary");
        let albums = self.albums.read();
        info!("LMSLibrary contains {} albums", albums.len());
        albums.values().cloned().map(|mut album| {
            self.populate_calculated_album_fields(&mut album);
            album
        }).collect()
    }
    fn get_artists(&self) -> Vec<Artist> {
        let artists = self.artists.read();
        info!("LMSLibrary returning {} artists from get_artists", artists.len());
        artists.values().cloned().collect()
    }
    
    fn get_album_by_artist_and_name(&self, artist: &str, album: &str) -> Option<Album> {
        self.get_album_by_artist_and_name(artist, album)
    }
    
    fn get_artist_by_name(&self, name: &str) -> Option<Artist> {
        self.get_artist_by_name(name)
    }
    
    fn update_artist_metadata(&self) {
        if self.enhance_metadata {
            info!("Starting background metadata update for LMSLibrary artists");
            // Use the generic function from artistupdater with only the artists collection
            crate::helpers::artistupdater::update_library_artists_metadata_in_background(self.artists.clone());
        }
    }
    
    fn get_album_by_id(&self, id: &crate::data::Identifier) -> Option<Album> {
        self.get_album_by_id(id)
    }
    
    fn get_albums_by_artist_id(&self, artist_id: &crate::data::Identifier) -> Vec<Album> {
        self.get_albums_by_artist_id(artist_id)
    }
    fn get_image(&self, identifier: String) -> Option<(Vec<u8>, String)> {
        debug!("Retrieving image for identifier: {}", identifier);
        
        // Check if the identifier starts with "album:"
        if let Some(album_id_str) = identifier.strip_prefix("album:") {
            debug!("Detected album identifier: {}", album_id_str);
            
            // Parse the album ID as a numeric ID
            match album_id_str.parse::<u64>() {
                Ok(album_id_num) => {
                    let album_id = crate::data::Identifier::Numeric(album_id_num);
                    warn!("Parsed album ID: {}", album_id);
                    let album = self.get_album_by_id(&album_id);
                    
                    // Get the first track from the album (if any) with proper lifetime handling
                    let track = album.and_then(|a| {
                        let tracks_guard = a.tracks.lock();
                        tracks_guard.first().cloned()
                    });

                    // Extract the track ID if available, otherwise fall back to URI
                    let track_id = track.and_then(|t| {
                        // First try the id field
                        t.id.map(|id| match id {
                            crate::data::Identifier::String(s) => s,
                            crate::data::Identifier::Numeric(n) => n.to_string(),
                        })
                        // Fall back to URI if no ID is available
                        .or_else(|| t.uri.clone())
                    })
                    .unwrap_or_else(|| "0".to_string());
                    let track_cover_url = self.track_cover_url(&track_id);
                    warn!("Track cover URL: {}", track_cover_url);
                    // Fetch the image data from the URL using  http client
                    match http_client::new_http_client(2).get_binary(&track_cover_url) {
                        Ok((data, content_type)) => {
                            debug!("Successfully retrieved image data");
                            return Some((data, content_type));
                        },
                        Err(e) => {
                            warn!("Failed to retrieve image data: {}", e);
                            return None;
                        }
                    }
                    
                },
                Err(e) => {
                    warn!("Failed to parse album ID '{}' as a number: {}", album_id_str, e);
                    return None;
                }
            }
        }
        
        // If we reach here, the identifier is not supported
        warn!("Unsupported image identifier format: {}", identifier);
        None
    }

    fn force_update(&self) -> bool {
        // Send a rescan command to the LMS server
        match self.client.control_request("0:0:0:0:0:0:0:0", "rescan", vec![]) {
            Ok(_) => {
                debug!("Successfully sent rescan command to LMS server");
                true
            },
            Err(e) => {
                error!("Failed to send rescan command to LMS server: {}", e);
                false
            }
        }
    }

    fn get_meta_keys(&self) -> Vec<String> {
        vec![
            "memory_usage".to_string(),
            "album_count".to_string(),
            "artist_count".to_string(),
            "track_count".to_string(),
            "library_loaded".to_string(),
            "loading_progress".to_string(),
            "enhance_metadata".to_string(),
            "server_address".to_string(),
            "server_port".to_string(),
        ]
    }

    fn get_metadata_value(&self, key: &str) -> Option<String> {
        match key {
            "memory_usage" => {
                use crate::helpers::memory_report::MemoryUsage;
                
                // Create memory usage tracker
                let mut usage = MemoryUsage::new();
                
                // Calculate size of albums and tracks
                {
                    let albums = self.albums.read();
                    usage.album_count = albums.len();

                    for album in albums.values() {
                        usage.albums_memory += MemoryUsage::calculate_album_memory(album);
                        usage.tracks_memory += MemoryUsage::calculate_tracks_memory(&album.tracks);

                        // Count tracks
                        let tracks = album.tracks.lock();
                        usage.track_count += tracks.len();
                    }
                }

                // Calculate size of artists
                {
                    let artists = self.artists.read();
                    usage.artist_count = artists.len();
                    for artist in artists.values() {
                        usage.artists_memory += MemoryUsage::calculate_artist_memory(artist);
                    }
                }

                // Calculate album-artist relationships
                {
                    let album_artists = self.album_artists.read();
                    usage.album_artists_count = album_artists.len();
                    usage.overhead_memory += album_artists.memory_usage();
                }
                
                // Log the stats for debugging/monitoring
                usage.log_stats();
                
                // Return as JSON
                Some(serde_json::to_string_pretty(&serde_json::json!({
                    "name": "LMSLibrary",
                    "total_memory": usage.total(),
                    "total_memory_human": MemoryUsage::format_size(usage.total()),
                    "components": {
                        "artists": {
                            "count": usage.artist_count,
                            "memory": usage.artists_memory,
                            "memory_human": MemoryUsage::format_size(usage.artists_memory)
                        },
                        "albums": {
                            "count": usage.album_count,
                            "memory": usage.albums_memory,
                            "memory_human": MemoryUsage::format_size(usage.albums_memory)
                        },
                        "tracks": {
                            "count": usage.track_count,
                            "memory": usage.tracks_memory,
                            "memory_human": MemoryUsage::format_size(usage.tracks_memory)
                        },
                        "album_artist_mappings": {
                            "count": usage.album_artists_count,
                            "memory": usage.overhead_memory,
                            "memory_human": MemoryUsage::format_size(usage.overhead_memory)
                        }
                    }
                })).unwrap_or_else(|_| "{}".to_string()))
            },
            "album_count" => {
                Some(self.albums.read().len().to_string())
            },
            "artist_count" => {
                Some(self.artists.read().len().to_string())
            },
            "track_count" => {
                let mut total_tracks = 0;
                let albums = self.albums.read();
                for album in albums.values() {
                    let tracks = album.tracks.lock();
                    total_tracks += tracks.len();
                }
                Some(total_tracks.to_string())
            },
            "server_address" => {
                if let Ok(address) = self.client.get_server_address() {
                    Some(address)
                } else {
                    Some("unknown".to_string())
                }
            },
            "server_port" => Some(self.client.get_server_port().to_string()),
            "library_loaded" => {
                Some(self.library_loaded.lock().to_string())
            },
            "loading_progress" => {
                Some(format!("{:.2}", *self.loading_progress.lock()))
            },
            "enhance_metadata" => Some(self.enhance_metadata.to_string()),
            _ => None,
        }
    }
    
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}