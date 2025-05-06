use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Instant;
use std::mem;
use std::io::{Write, BufRead};
use std::hash::{Hash, Hasher};
use std::collections::hash_map::DefaultHasher;
use log::{debug, info, warn, error};
use crate::data::{Album, Artist, AlbumArtists, LibraryInterface, LibraryError, Track};
use crate::helpers::memory_report::MemoryUsage;

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
}

impl MPDLibrary {
    /// Create a new MPD library interface with specific connection details
    pub fn with_connection(hostname: &str, port: u16) -> Self {
        debug!("Creating new MPDLibrary with connection {}:{}", hostname, port);
        
        MPDLibrary {
            hostname: hostname.to_string(),
            port,
            albums: Arc::new(RwLock::new(HashMap::new())),
            artists: Arc::new(RwLock::new(HashMap::new())),
            album_artists: Arc::new(RwLock::new(AlbumArtists::new())),
            library_loaded: Arc::new(Mutex::new(false)),
            loading_progress: Arc::new(Mutex::new(0.0)),
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
        
    /// Fetch all songs for a specific artist
    pub fn fetch_all_songs_for_artist(&self, artist_name: &str) -> Result<Vec<mpd::Song>, LibraryError> {
        debug!("Fetching all songs for artist: {}", artist_name);
        
        // Connect to MPD server
        let addr = format!("{}:{}", self.hostname, self.port);
        let mut stream = std::net::TcpStream::connect(&addr)
            .map_err(|e| LibraryError::ConnectionError(format!("Failed to connect to MPD: {}", e)))?;
        
        // Create find command for this artist
        let cmd = format!("find artist \"{}\"\n", artist_name.replace("\"", "\\\""));
        
        if let Err(e) = stream.write_all(cmd.as_bytes()) {
            return Err(LibraryError::ConnectionError(format!("Failed to send command: {}", e)));
        }
        
        // Read response
        let mut reader = std::io::BufReader::new(stream);
        let mut line = String::new();
        let mut songs = Vec::new();
        
        // Current song being processed
        let mut current_file = None;
        let mut current_title = None;
        let mut current_artist = None;
        let mut current_album = None;
        let mut current_album_artist = None;
        let mut current_track = None;
        let mut current_date = None;
        let mut current_duration = None;
        
        // Process response line by line
        while let Ok(bytes) = reader.read_line(&mut line) {
            if bytes == 0 {
                break; // End of stream
            }
            
            let line_trimmed = line.trim();
            if line_trimmed == "OK" {
                // End of response
                break;
            }
            
            // Process each line based on field
            if line_trimmed.starts_with("file: ") {
                // New song entry, save previous if exists
                if let Some(file) = current_file.take() {
                    // Create a song object from gathered data
                    let mut song = mpd::Song::default();
                    song.file = file;
                    song.title = current_title;
                    
                    // Add tags
                    if let Some(artist) = current_artist {
                        song.tags.push(("Artist".to_string(), artist));
                    }
                    
                    if let Some(album) = current_album {
                        song.tags.push(("Album".to_string(), album));
                    }
                    
                    if let Some(album_artist) = current_album_artist {
                        song.tags.push(("AlbumArtist".to_string(), album_artist));
                    }
                    
                    if let Some(track) = current_track {
                        song.tags.push(("Track".to_string(), track));
                    }
                    
                    if let Some(date) = current_date {
                        song.tags.push(("Date".to_string(), date));
                    }
                    
                    if let Some(duration) = current_duration {
                        song.duration = Some(std::time::Duration::from_secs_f64(duration));
                    }
                    
                    songs.push(song);
                }
                
                // Start new song
                current_file = Some(line_trimmed[6..].to_string());
                current_title = None;
                current_artist = None;
                current_album = None;
                current_album_artist = None;
                current_track = None;
                current_date = None;
                current_duration = None;
            } else if line_trimmed.starts_with("Title: ") {
                current_title = Some(line_trimmed[7..].to_string());
            } else if line_trimmed.starts_with("Artist: ") {
                current_artist = Some(line_trimmed[8..].to_string());
            } else if line_trimmed.starts_with("Album: ") {
                current_album = Some(line_trimmed[7..].to_string());
            } else if line_trimmed.starts_with("AlbumArtist: ") {
                current_album_artist = Some(line_trimmed[13..].to_string());
            } else if line_trimmed.starts_with("Track: ") {
                current_track = Some(line_trimmed[7..].to_string());
            } else if line_trimmed.starts_with("Date: ") {
                current_date = Some(line_trimmed[6..].to_string());
            } else if line_trimmed.starts_with("duration: ") {
                if let Ok(dur) = line_trimmed[10..].parse::<f64>() {
                    current_duration = Some(dur);
                }
            }
            
            line.clear();
        }
        
        // Process the last song if there's one in progress
        if let Some(file) = current_file.take() {
            // Create a song object from gathered data
            let mut song = mpd::Song::default();
            song.file = file;
            song.title = current_title;
            
            // Add tags
            if let Some(artist) = current_artist {
                song.tags.push(("Artist".to_string(), artist));
            }
            
            if let Some(album) = current_album {
                song.tags.push(("Album".to_string(), album));
            }
            
            if let Some(album_artist) = current_album_artist {
                song.tags.push(("AlbumArtist".to_string(), album_artist));
            }
            
            if let Some(track) = current_track {
                song.tags.push(("Track".to_string(), track));
            }
            
            if let Some(date) = current_date {
                song.tags.push(("Date".to_string(), date));
            }
            
            if let Some(duration) = current_duration {
                song.duration = Some(std::time::Duration::from_secs_f64(duration));
            }
            
            songs.push(song);
        }
        
        debug!("Found {} songs for artist '{}'", songs.len(), artist_name);
        Ok(songs)
    }
    
    /// Build the library by mapping all songs for all album artists
    pub fn build_library_from_artists(&self) -> Result<(), LibraryError> {
        info!("Building library by mapping all songs for all album artists");
        let start_time = Instant::now();
        
        // Clear existing data
        *self.library_loaded.lock().unwrap() = false;
        
        // Reset loading progress to 0
        if let Ok(mut progress) = self.loading_progress.lock() {
            *progress = 0.0;
        }
        
        {
            if let Ok(mut albums) = self.albums.write() {
                albums.clear();
            } else {
                error!("Failed to acquire write lock on albums");
                return Err(LibraryError::InternalError("Failed to acquire lock".to_string()));
            }
            
            if let Ok(mut artists) = self.artists.write() {
                artists.clear();
            } else {
                error!("Failed to acquire write lock on artists");
                return Err(LibraryError::InternalError("Failed to acquire lock".to_string()));
            }
            
            if let Ok(mut album_artists) = self.album_artists.write() {
                album_artists.clear();
            } else {
                error!("Failed to acquire write lock on album_artists");
                return Err(LibraryError::InternalError("Failed to acquire lock".to_string()));
            }
        }
        
        // First, get a list of all album artists in the database
        let artist_list = self.get_all_album_artists()?;
        let total_artists = artist_list.len();
        info!("Found {} album artists", total_artists);
        
        // Track metrics
        let mut total_albums = 0;
        let mut total_songs = 0;
        let mut processed_artists = 0;
        
        // Maps to build album and artist data
        let mut artist_albums: HashMap<String, HashSet<String>> = HashMap::new();
        let mut artist_track_counts: HashMap<String, usize> = HashMap::new();
        let mut album_tracks: HashMap<String, Vec<Track>> = HashMap::new(); // Changed from Vec<String> to Vec<Track>
        let mut album_metadata: HashMap<String, (Vec<String>, Option<i32>)> = HashMap::new(); // (artists, year)
        let mut album_first_file: HashMap<String, String> = HashMap::new(); // Store first file path for each album
        
        // Process each artist
        for artist_name in &artist_list {
            debug!("Processing artist: {}", artist_name);
            let artist_start = Instant::now();
            
            match self.fetch_all_songs_for_artist(artist_name) {
                Ok(songs) => {
                    debug!("Found {} songs for artist '{}'", songs.len(), artist_name);
                    total_songs += songs.len();
                    
                    // Extract albums from songs
                    let mut artist_album_set = HashSet::new();
                    let mut artist_songs_count = 0;
                    
                    for song in &songs {
                        // Extract album name
                        if let Some(album_name) = song.tags.iter()
                            .find(|(tag, _)| tag == "Album")
                            .map(|(_, value)| value.to_string()) {
                                
                            // Add album to artist's collection
                            artist_album_set.insert(album_name.clone());
                            
                            // Extract track title
                            let track_title = song.title.as_deref().unwrap_or("Unknown").to_string();
                            
                            // Extract track number and disc number
                            let track_num = song.tags.iter()
                                .find(|(tag, _)| tag == "Track")
                                .and_then(|(_, track_str)| {
                                    // Track number might be in format "5" or "5/12"
                                    track_str.split('/').next().and_then(|n| n.parse::<u16>().ok())
                                }).unwrap_or(0);
                                
                            // Use "1" as default disc number if not specified
                            let disc_num = song.tags.iter()
                                .find(|(tag, _)| tag == "Disc")
                                .map(|(_, disc_str)| disc_str.to_string())
                                .unwrap_or_else(|| "1".to_string());
                                
                            // Extract track artist (might be different from album artist)
                            let track_artist = song.tags.iter()
                                .find(|(tag, _)| tag == "Artist")
                                .map(|(_, artist_str)| artist_str.to_string());
                                
                            // Get album artist for comparison
                            let album_artist = album_metadata.entry(album_name.clone())
                                .or_insert_with(|| (Vec::new(), None))
                                .0.join(", ");
                                
                            // Create track with artist only if different from album artist
                            let track = if let Some(artist) = track_artist {
                                if album_artist.is_empty() || artist != album_artist {
                                    Track::with_artist(disc_num, track_num, track_title, artist, Some(&album_artist))
                                } else {
                                    Track::new(disc_num, track_num, track_title)
                                }
                            } else {
                                Track::new(disc_num, track_num, track_title)
                            };
                            
                            // Add track to album
                            album_tracks.entry(album_name.clone())
                                .or_insert_with(Vec::new)
                                .push(track);
                            
                            // Store the first file path for the album if not already stored
                            album_first_file.entry(album_name.clone())
                                .or_insert_with(|| song.file.clone());
                            
                            // Extract year if available
                            let year = song.tags.iter()
                                .find(|(tag, _)| tag == "Date")
                                .and_then(|(_, date_str)| {
                                    let year_part = date_str.split('-').next().unwrap_or(date_str);
                                    year_part.parse::<i32>().ok()
                                });
                            
                            // Store album metadata (artists, year)
                            album_metadata.entry(album_name.clone())
                                .or_insert_with(|| (Vec::new(), year))
                                .0.push(artist_name.clone());

                            // Ensure we don't have duplicates in the artists list
                            if let Some((artists, _)) = album_metadata.get_mut(&album_name) {
                                artists.sort();
                                artists.dedup();
                            }
                            
                            artist_songs_count += 1;
                        }
                    }
                    
                    // Store artist's albums and track count
                    artist_albums.insert(artist_name.clone(), artist_album_set.clone());
                    artist_track_counts.insert(artist_name.clone(), artist_songs_count);
                    
                    total_albums += artist_album_set.len();
                    let artist_time = artist_start.elapsed();
                    debug!("Processed artist '{}' with {} albums and {} songs in {:.2?}", 
                          artist_name, artist_album_set.len(), artist_songs_count, artist_time);
                    
                    processed_artists += 1;
                    
                    // Update progress based on number of artists processed
                    if let Ok(mut progress) = self.loading_progress.lock() {
                        *progress = if total_artists > 0 {
                            processed_artists as f32 / total_artists as f32
                        } else {
                            0.0
                        };
                    }
                },
                Err(e) => {
                    warn!("Failed to fetch songs for artist '{}': {}", artist_name, e);
                    
                    // Count as processed even if it failed
                    processed_artists += 1;
                    
                    // Update progress
                    if let Ok(mut progress) = self.loading_progress.lock() {
                        *progress = if total_artists > 0 {
                            processed_artists as f32 / total_artists as f32
                        } else {
                            0.0
                        };
                    }
                }
            }
        }
        
        // Build album objects
        {
            if let Ok(mut albums) = self.albums.write() {
                for (album_name, (album_artists, year)) in &album_metadata {
                    let tracks = album_tracks.get(album_name).cloned().unwrap_or_default();
                    let first_file = album_first_file.get(album_name).cloned();
                    
                    // Create a unique ID for the album using a 64-bit hash
                    // Combine all artist names and the album name to create a unique ID
                    let mut hasher = DefaultHasher::new();
                    album_name.hash(&mut hasher);
                    for artist in album_artists {
                        artist.hash(&mut hasher);
                    }
                    let album_id = hasher.finish();
                    
                    albums.insert(album_name.clone(), Album {
                        id: album_id,
                        name: album_name.clone(),
                        artists: Arc::new(Mutex::new(album_artists.clone())),
                        year: *year,
                        tracks: Arc::new(Mutex::new(tracks)),
                        cover_art: None,
                        uri: first_file,
                    });
                }
            } else {
                error!("Failed to acquire write lock on albums");
                return Err(LibraryError::InternalError("Failed to acquire lock".to_string()));
            }
        }
        
        // Build artist objects
        {
            if let Ok(mut artists) = self.artists.write() {
                for (artist_name, albums) in &artist_albums {
                    let track_count = artist_track_counts.get(artist_name).cloned().unwrap_or(0);
                    
                    // Create a unique ID for the artist using a hash
                    let mut hasher = DefaultHasher::new();
                    artist_name.hash(&mut hasher);
                    let artist_id = hasher.finish();
                    
                    artists.insert(artist_name.clone(), Artist {
                        id: artist_id,
                        name: artist_name.clone(),
                        albums: albums.clone(),
                        track_count,
                        metadata: None,
                    });
                }
            } else {
                error!("Failed to acquire write lock on artists");
                return Err(LibraryError::InternalError("Failed to acquire lock".to_string()));
            }
        }
        
        // Update artist metadata using the LibraryInterface method
        info!("Starting background metadata update for artists");
        self.update_artist_metadata();
        
        // Build album to artist relationships using AlbumArtists
        {
            // Get read locks for the albums and artists HashMaps
            if let (Ok(albums_guard), Ok(artists_guard), Ok(mut album_artists_guard)) = 
                (self.albums.read(), self.artists.read(), self.album_artists.write()) {
                
                // Use the new build_from_hashmaps method which directly accepts the HashMaps
                *album_artists_guard = AlbumArtists::build_from_hashmaps(&albums_guard, &artists_guard);
                
                debug!("Built album-artist relationships with {} mappings", album_artists_guard.count());
            } else {
                error!("Failed to acquire locks for building album-artist relationships");
                return Err(LibraryError::InternalError("Failed to acquire locks".to_string()));
            }
        }
        
        // Set progress to 1.0 when complete
        if let Ok(mut progress) = self.loading_progress.lock() {
            *progress = 1.0;
        }
        
        // Mark library as loaded
        *self.library_loaded.lock().unwrap() = true;
        
        let total_time = start_time.elapsed();
        info!("Library built in {:.2?}: {} artists, {} albums, {} songs", 
            total_time, artist_list.len(), total_albums, total_songs);
        
        Ok(())
    }
    
    /// Get a list of all album artists in the database
    fn get_all_album_artists(&self) -> Result<Vec<String>, LibraryError> {
        debug!("Fetching list of all album artists");
        
        // Connect to MPD server
        let addr = format!("{}:{}", self.hostname, self.port);
        let mut stream = std::net::TcpStream::connect(&addr)
            .map_err(|e| LibraryError::ConnectionError(format!("Failed to connect to MPD: {}", e)))?;
        
        // Send command to list all album artists
        if let Err(e) = stream.write_all(b"list albumartist\n") {
            return Err(LibraryError::ConnectionError(format!("Failed to send command: {}", e)));
        }
        
        // Read response
        let mut reader = std::io::BufReader::new(stream);
        let mut line = String::new();
        let mut artists = Vec::new();
        
        while let Ok(bytes) = reader.read_line(&mut line) {
            if bytes == 0 {
                break;
            }
            
            let line_trimmed = line.trim();
            if line_trimmed == "OK" {
                break;
            }
            
            if line_trimmed.starts_with("AlbumArtist: ") {
                let artist_name = line_trimmed[13..].to_string();
                if !artist_name.is_empty() {
                    artists.push(artist_name);
                }
            }
            
            line.clear();
        }
        
        debug!("Found {} album artists", artists.len());
        Ok(artists)
    }
}

impl LibraryInterface for MPDLibrary {
    fn new() -> Self {
        debug!("Creating new MPDLibrary with default connection");
        Self::with_connection("localhost", 6600)
    }
    
    fn is_loaded(&self) -> bool {
        if let Ok(loaded) = self.library_loaded.lock() {
            *loaded
        } else {
            false
        }
    }
    
    fn refresh_library(&self) -> Result<(), LibraryError> {
        debug!("Refreshing MPD library data");
        let start_time = Instant::now();
        
        // Use build_library_from_artists method instead of db_from_listallinfo
        if let Err(e) = self.build_library_from_artists() {
            error!("Error loading library data: {}", e);
            return Err(e);
        }
        
        // Calculate memory usage
        let mut memory_usage = MemoryUsage::new();
        
        // Calculate memory used by artists
        if let Ok(artists) = self.artists.read() {
            memory_usage.artist_count = artists.len();
            
            // Base size of HashMap
            memory_usage.overhead_memory += mem::size_of::<HashMap<String, Artist>>();
            
            // Calculate size of each artist
            for artist in artists.values() {
                memory_usage.artists_memory += MemoryUsage::calculate_artist_memory(artist);
                memory_usage.track_count += artist.track_count;
            }
            
            // Add overhead for HashMap capacity (rough estimate)
            memory_usage.overhead_memory += artists.capacity() * mem::size_of::<(String, Artist)>();
        }
        
        // Calculate memory used by albums
        if let Ok(albums) = self.albums.read() {
            memory_usage.album_count = albums.len();
            
            // Base size of HashMap
            memory_usage.overhead_memory += mem::size_of::<HashMap<String, Album>>();
            
            // Calculate size of each album including tracks
            for album in albums.values() {
                memory_usage.albums_memory += MemoryUsage::calculate_album_memory(album);
                memory_usage.tracks_memory += MemoryUsage::calculate_tracks_memory(&album.tracks);
            }
            
            // Add overhead for HashMap capacity (rough estimate)
            memory_usage.overhead_memory += albums.capacity() * mem::size_of::<(String, Album)>();
        }
        
        // Add overhead for album_artists
        if let Ok(album_artists) = self.album_artists.read() {
            memory_usage.album_artists_count = album_artists.len();
            memory_usage.overhead_memory += mem::size_of::<AlbumArtists>();
        }
        
        // Add overhead for Arc, RwLock, Mutex
        memory_usage.overhead_memory += 4 * mem::size_of::<Arc<RwLock<HashMap<String, Artist>>>>();
        
        // Log the memory statistics
        memory_usage.log_stats();
        
        let total_time = start_time.elapsed();
        
        // Summary of timing information
        info!("Library load complete in {:.2?}", total_time);
        
        Ok(())
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
        // Use the generic function from artistupdater for updating artists
        let library_arc = Arc::new(self.clone());
        crate::helpers::artistupdater::update_library_artists_metadata_in_background(library_arc);
    }
}
