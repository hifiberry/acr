use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Instant;
use log::{debug, info, warn, error};
use crate::data::{Album, Artist, AlbumArtists, LibraryInterface, LibraryError};
use crate::helpers::memory_report::MemoryUsage;
use crate::players::mpd::mpd::MPDPlayerController;

/// MPD library interface that provides access to albums and artists
#[derive(Clone)]
pub struct MPDLibrary {
    /// MPD server hostname
    hostname: String,
    
    /// MPD server port
    port: u16,
    
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
    
    /// Reference to the MPDPlayerController that owns this library
    controller: Arc<MPDPlayerController>,
}

impl MPDLibrary {
    /// Create a new MPD library interface with specific connection details
    pub fn with_connection(hostname: &str, port: u16, controller: Arc<MPDPlayerController>) -> Self {
        debug!("Creating new MPDLibrary with connection {}:{}", hostname, port);
        
        MPDLibrary {
            hostname: hostname.to_string(),
            port,
            albums: Arc::new(RwLock::new(HashMap::new())),
            artists: Arc::new(RwLock::new(HashMap::new())),
            album_artists: Arc::new(RwLock::new(AlbumArtists::new())),
            library_loaded: Arc::new(Mutex::new(false)),
            loading_progress: Arc::new(Mutex::new(0.0)),
            artist_separators: Arc::new(Mutex::new(None)),
            controller,
        }
    }
    
    /// Get the current library loading progress (0.0 to 1.0)
    pub fn get_loading_progress(&self) -> f32 {
        if let Ok(progress) = self.loading_progress.lock() {
            *progress
        } else {
            0.0 // Default to 0 if we can't get the lock
        }
    }
    
    /// Set custom artist separators for use in library operations
    pub fn set_artist_separators(&mut self, separators: Vec<String>) {
        debug!("Setting custom artist separators in MPDLibrary: {:?}", separators);
        if let Ok(mut sep_guard) = self.artist_separators.lock() {
            *sep_guard = Some(separators);
        } else {
            warn!("Failed to acquire lock for setting artist separators");
        }
    }
    
    /// Get custom artist separators for artist name splitting
    pub fn get_artist_separators(&self) -> Option<Vec<String>> {
        // Return the stored separators if available
        if let Ok(sep_guard) = self.artist_separators.lock() {
            sep_guard.clone()
        } else {
            warn!("Failed to acquire lock for artist separators");
            None
        }
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
        let albums = match self.albums.read() {
            Ok(albums) => albums,
            Err(_) => {
                error!("Failed to acquire read lock on albums");
                return Err(LibraryError::InternalError("Failed to acquire lock on albums".to_string()));
            }
        };
        
        // Collect all artist names from albums and their IDs
        let mut artist_names = HashSet::new();
        let mut album_artist_relations = Vec::new();
        
        // Go through all albums and collect artist names
        for album in albums.values() {
            // Extract artist names from the album's artists list
            if let Ok(album_artists) = album.artists.lock() {
                for artist_name in album_artists.iter() {
                    artist_names.insert(artist_name.clone());
                    
                    // Store album-artist relationship for later
                    album_artist_relations.push((album.id, artist_name.clone()));
                }
            }
        }
        
        debug!("Found {} unique artist names in albums", artist_names.len());
        
        // Now, get a write lock on the artists collection to add new artists
        let mut artists = match self.artists.write() {
            Ok(artists) => artists,
            Err(_) => {
                error!("Failed to acquire write lock on artists");
                return Err(LibraryError::InternalError("Failed to acquire lock on artists".to_string()));
            }
        };
        
        // Get a write lock on the album_artists relationships
        let mut album_artists = match self.album_artists.write() {
            Ok(album_artists) => album_artists,
            Err(_) => {
                error!("Failed to acquire write lock on album_artists");
                return Err(LibraryError::InternalError("Failed to acquire lock on album_artists".to_string()));
            }
        };
        
        // Create a new artist object for each name that doesn't already exist
        for artist_name in artist_names {
            // Skip if the artist already exists
            if artists.contains_key(&artist_name) {
                continue;
            }
            
            // Create a unique ID for the artist based on the name
            use std::collections::hash_map::DefaultHasher;
            use std::hash::{Hash, Hasher};
            
            let mut hasher = DefaultHasher::new();
            artist_name.hash(&mut hasher);
            let artist_id = hasher.finish();
            
            // Create a new Artist object
            let artist = Artist {
                id: artist_id,
                name: artist_name.clone(),
                is_multi: false,  // Default to false, can be updated later if needed
                track_count: 0,  // Will be updated when processing tracks
                metadata: None,
            };
            
            // Insert the new artist
            artists.insert(artist_name.clone(), artist);
            created_count += 1;
        }
        
        // Update album-artist relationships
        for (album_id, artist_name) in album_artist_relations {
            // Get artist ID (if it exists)
            if let Some(artist) = artists.get(&artist_name) {
                // Add relationship between album and artist
                album_artists.add_mapping(album_id, artist.id);
                
                // No longer adding album names to artist.albums since we removed that attribute
            }
        }
        
        let elapsed = start_time.elapsed();
        info!("Created {} new artists in {:?}", created_count, elapsed);
        
        Ok(created_count)
    }
    
    /// Transfer data from another MPDLibrary instance
    fn transfer_data_from(&self, other: &MPDLibrary) -> Result<(), LibraryError> {
        debug!("Transferring library data from newly loaded library");
        
        // Reset loading progress to 0
        if let Ok(mut progress) = self.loading_progress.lock() {
            *progress = 0.0;
        }
        
        // Mark as not loaded during transfer
        *self.library_loaded.lock().unwrap() = false;
        
        // Transfer albums
        {
            if let (Ok(mut self_albums), Ok(other_albums)) = (self.albums.write(), other.albums.read()) {
                self_albums.clear();
                for (key, value) in other_albums.iter() {
                    self_albums.insert(key.clone(), value.clone());
                }
                debug!("Transferred {} albums", self_albums.len());
            } else {
                error!("Failed to acquire locks for album transfer");
                return Err(LibraryError::InternalError("Failed to acquire locks".to_string()));
            }
        }
        
        // Transfer artists
        {
            if let (Ok(mut self_artists), Ok(other_artists)) = (self.artists.write(), other.artists.read()) {
                self_artists.clear();
                for (key, value) in other_artists.iter() {
                    self_artists.insert(key.clone(), value.clone());
                }
                debug!("Transferred {} artists", self_artists.len());
            } else {
                error!("Failed to acquire locks for artist transfer");
                return Err(LibraryError::InternalError("Failed to acquire locks".to_string()));
            }
        }
        
        // Transfer album-artist relationships
        {
            if let (Ok(mut self_relationships), Ok(other_relationships)) = 
                (self.album_artists.write(), other.album_artists.read()) {
                *self_relationships = other_relationships.clone();
                debug!("Transferred album-artist relationships");
            } else {
                error!("Failed to acquire locks for relationship transfer");
                return Err(LibraryError::InternalError("Failed to acquire locks".to_string()));
            }
        }
        
        // Mark as loaded and update progress
        *self.library_loaded.lock().unwrap() = true;
        if let Ok(mut progress) = self.loading_progress.lock() {
            *progress = 1.0;
        }
        
        debug!("Library data transfer complete");
        
        Ok(())
    }
    
    /// Get artists collection as Arc for direct updating
    pub fn get_artists_arc(&self) -> Arc<RwLock<HashMap<String, Artist>>> {
        self.artists.clone()
    }
}

impl LibraryInterface for MPDLibrary {
    fn new() -> Self {
        debug!("Creating new MPDLibrary with default connection");
        // Create a new default MPDPlayerController
        let controller = Arc::new(MPDPlayerController::new());
        
        Self::with_connection("localhost", 6600, controller)
    }
    
    fn is_loaded(&self) -> bool {
        if let Ok(loaded) = self.library_loaded.lock() {
            *loaded
        } else {
            false
        }
    }
    
    fn refresh_library(&self) -> Result<(), LibraryError> {
        debug!("Refreshing MPD library data using MPDLibraryLoader");
        let start_time = Instant::now();
        
        // Use our MPDLibraryLoader to load albums, passing the controller reference
        let loader = super::libraryloader::MPDLibraryLoader::new(&self.hostname, self.port, self.controller.clone());
        
        // Get artist separators from the MPD configuration, if any
        let artist_separators = self.get_artist_separators();
        
        let result = match loader.load_albums_from_mpd(artist_separators) {
            Ok(albums) => {
                // Mark as not loaded during update
                *self.library_loaded.lock().unwrap() = false;
                
                // Reset loading progress to 0
                if let Ok(mut progress) = self.loading_progress.lock() {
                    *progress = 0.0;
                }
                
                // Update albums collection
                {
                    if let Ok(mut self_albums) = self.albums.write() {
                        self_albums.clear();
                        
                        // Add each album to the collection with name as key
                        for album in albums {
                            self_albums.insert(album.name.clone(), album);
                        }
                        
                        debug!("Updated library with {} albums", self_albums.len());
                    } else {
                        error!("Failed to acquire write lock on albums");
                        return Err(LibraryError::InternalError("Failed to acquire locks".to_string()));
                    }
                }
                
                // Create artists and update album-artist relationships
                if let Err(e) = self.create_artists() {
                    error!("Error creating artists: {}", e);
                }
                
                // Mark as loaded and update progress
                *self.library_loaded.lock().unwrap() = true;
                if let Ok(mut progress) = self.loading_progress.lock() {
                    *progress = 1.0;
                }
                
                let total_time = start_time.elapsed();
                info!("Library load complete in {:.2?}", total_time);
                
                // Start background update of artist metadata now that the library is fully loaded
                info!("Starting background metadata update for artists");
                crate::helpers::artistupdater::update_library_artists_metadata_in_background(
                    self.artists.clone()
                );
                
                Ok(())
            },
            Err(e) => {
                error!("Error loading MPD library: {}", e);
                Err(e)
            }
        };
        
        // Send an update_database notification of 100% before exiting, even in case of errors
        self.controller.notify_database_update(None, None, None, Some(100.0));
        
        result
    }
    
    fn get_albums(&self) -> Vec<Album> {
        if let Ok(albums) = self.albums.read() {
            albums.values().cloned().collect()
        } else {
            warn!("Failed to acquire read lock on albums");
            Vec::new()
        }
    }
    
    fn get_artists(&self) -> Vec<Artist> {
        if let Ok(artists) = self.artists.read() {
            artists.values().cloned().collect()
        } else {
            warn!("Failed to acquire read lock on artists");
            Vec::new()
        }
    }
    
    fn get_album(&self, name: &str) -> Option<Album> {
        if let Ok(albums) = self.albums.read() {
            albums.get(name).cloned()
        } else {
            warn!("Failed to acquire read lock on albums");
            None
        }
    }
    
    fn get_artist(&self, name: &str) -> Option<Artist> {
        if let Ok(artists) = self.artists.read() {
            artists.get(name).cloned()
        } else {
            warn!("Failed to acquire read lock on artists");
            None
        }
    }
    
    fn get_albums_by_artist(&self, artist_name: &str) -> Vec<Album> {
        let mut result = Vec::new();
        
        // First get the artist by name to get the artist ID
        if let Some(artist) = self.get_artist(artist_name) {
            let artist_id = artist.id;
            
            // Get albums associated with this artist from album_artists mapping
            if let Ok(album_artists_mapping) = self.album_artists.read() {
                let album_ids = album_artists_mapping.get_albums_for_artist(&artist_id);
                
                // Get all albums and fetch the ones with matching IDs
                if let Ok(albums) = self.albums.read() {
                    for album in albums.values() {
                        if album_ids.contains(&album.id) {
                            result.push(album.clone());
                        }
                    }
                }
            }
        }
        
        result
    }
    
    fn get_album_cover(&self, _album_name: &str) -> Option<String> {
        None
    }
    
    fn update_artist_metadata(&self) {
        info!("Starting background metadata update for MPDLibrary artists");
        // Use the generic function from artistupdater with only the artists collection
        crate::helpers::artistupdater::update_library_artists_metadata_in_background(self.artists.clone());
    }
}
