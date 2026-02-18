use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use parking_lot::{Mutex, RwLock};
use std::time::Instant;
use log::{debug, info, warn, error};
use chrono::Datelike;
use crate::data::{Album, Artist, AlbumArtists, LibraryInterface, LibraryError};
use crate::players::mpd::mpd::{MPDPlayerController, mpd_image_url};
use crate::helpers::url_encoding;
use crate::helpers::lyrics::LyricsProvider;

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
    
    /// Flag to control metadata enhancement
    enhance_metadata: bool,
    
    /// Reference to the MPDPlayerController that owns this library
    controller: Arc<MPDPlayerController>,
}

impl MPDLibrary {
    /// Create a new MPD library interface with specific connection details
    pub fn with_connection(hostname: &str, port: u16, controller: Arc<MPDPlayerController>) -> Self {
        debug!("Creating new MPDLibrary with connection {}:{}", hostname, port);
        
        // Get the enhance_metadata setting from the controller, if available
        let enhance_metadata = controller.get_enhance_metadata().unwrap_or(true);
        
        MPDLibrary {
            hostname: hostname.to_string(),
            port,
            albums: Arc::new(RwLock::new(HashMap::new())),
            artists: Arc::new(RwLock::new(HashMap::new())),
            album_artists: Arc::new(RwLock::new(AlbumArtists::new())),
            library_loaded: Arc::new(Mutex::new(false)),
            loading_progress: Arc::new(Mutex::new(0.0)),
            artist_separators: Arc::new(Mutex::new(None)),
            enhance_metadata,
            controller,
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
                // Use the mpd_image_url function from MPD controller
                let image_url = format!("{}/album:{}",
                    mpd_image_url(), album_id);
                album.cover_art = Some(image_url);
            }
        }
    }

    /// Populate calculated fields in artist objects
    /// 
    /// This adds derived fields like image URLs for artists that don't have them yet
    /// These calculated fields are not stored, but only calculated on demand
    pub fn populate_calculated_artist_fields(&self, artist: &mut Artist) {
        // Initialize metadata if not present
        if artist.metadata.is_none() {
            artist.metadata = Some(crate::data::ArtistMeta::new());
        }
        
        // Add artist image URL if not present in thumb_url
        if let Some(ref mut metadata) = artist.metadata {
            if metadata.thumb_url.is_empty() {
                // Use the coverart API endpoint for artist images
                let encoded_name = crate::helpers::url_encoding::encode_url_safe(&artist.name);
                let api_url = format!("{}/coverart/artist/{}/image", crate::constants::API_PREFIX, encoded_name);
                metadata.thumb_url = vec![api_url];
            }
        }
        
        // Note: Metadata updates (biography, MBIDs, genres) are only done during library loading
        // to avoid expensive operations on every API access. All metadata should be populated
        // during the initial library load and background update processes.
    }

    /// Get the current library loading progress (0.0 to 1.0)
    pub fn get_loading_progress(&self) -> f32 {
        let progress = self.loading_progress.lock();
        *progress
    }
    
    /// Set custom artist separators for use in library operations
    pub fn set_artist_separators(&mut self, separators: Vec<String>) {
        debug!("Setting custom artist separators in MPDLibrary: {:?}", separators);
        {
            let mut sep_guard = self.artist_separators.lock();
            *sep_guard = Some(separators);
        }
    }
    
    /// Get custom artist separators for artist name splitting
    pub fn get_artist_separators(&self) -> Option<Vec<String>> {
        // Return the stored separators if available
        let sep_guard = self.artist_separators.lock();
        sep_guard.clone()
    }
    
    /// Check if cover art extraction from music files is enabled
    fn is_extract_coverart_enabled(&self) -> bool {
        self.controller.get_extract_coverart().unwrap_or(true)
    }
    
    /// Create a URL-safe base64 encoded image URL for a file path
    /// This shortens very long URL-encoded file paths to a URL-safe base64 encoded string
    pub fn create_encoded_image_url(&self, file_path: &str) -> String {
        let encoded_path = url_encoding::encode_url_safe(file_path);
        debug!("Created URL-safe base64 encoded path '{}' for: {}", encoded_path, file_path);
        format!("{}/{}", mpd_image_url(), encoded_path)
    }
    
    /// Retrieve album cover art for a specific URI using MPD's albumart command
    /// 
    /// Returns a tuple of (binary data, mime-type) of the cover art if found, None otherwise
    pub fn cover_art(&self, uri: &str) -> Option<(Vec<u8>, String)> {
        use std::io::{Read, BufRead, BufReader, Write};
        use std::net::TcpStream;
        debug!("Retrieving cover art for URI: {}", uri);
        
        // Connect to MPD server
        let stream = match TcpStream::connect(format!("{}:{}", self.hostname, self.port)) {
            Ok(s) => s,
            Err(e) => {
                error!("Failed to connect to MPD server: {}", e);
                return None;
            }
        };
        
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut writer = stream;
        
        // Read the welcome message
        let mut welcome = String::new();
        if reader.read_line(&mut welcome).is_err() {
            error!("Failed to read welcome message from MPD");
            return None;
        }
        
        if !welcome.starts_with("OK") {
            error!("Unexpected welcome message from MPD: {}", welcome);
            return None;
        }
        
        // Send albumart command with URI and offset 0
        let cmd = format!("albumart \"{}\" 0\n", uri);
        if writer.write_all(cmd.as_bytes()).is_err() {
            error!("Failed to send albumart command to MPD");
            return None;
        }
        
        // Read the size response
        let mut size_line = String::new();
        if reader.read_line(&mut size_line).is_err() {
            error!("Failed to read size response from MPD");
            return None;
        }
        
        // Check if we got an ACK error response instead of a size line
        if size_line.starts_with("ACK") {
            // This is an error from MPD (likely "No file exists" or similar)
            debug!("MPD returned error response: {}", size_line.trim());
            return None;
        }
        
        // Parse the size
        let size: usize = match size_line.strip_prefix("size: ") {
            Some(size_str) => match size_str.trim().parse() {
                Ok(size) => size,
                Err(e) => {
                    error!("Failed to parse cover art size: {}", e);
                    return None;
                }
            },
            None => {
                error!("Unexpected size response format: {}", size_line);
                return None;
            }
        };
        
        // Read the binary size line
        let mut binary_line = String::new();
        if reader.read_line(&mut binary_line).is_err() {
            error!("Failed to read binary size response from MPD");
            return None;
        }
        
        // Parse the binary chunk size
        let chunk_size: usize = match binary_line.strip_prefix("binary: ") {
            Some(size_str) => match size_str.trim().parse() {
                Ok(size) => size,
                Err(e) => {
                    error!("Failed to parse binary chunk size: {}", e);
                    return None;
                }
            },
            None => {
                error!("Unexpected binary size response format: {}", binary_line);
                return None;
            }
        };
        
        // Read the binary data
        let mut buffer = vec![0u8; chunk_size];
        match reader.read_exact(&mut buffer) {
            Ok(_) => {},
            Err(e) => {
                error!("Failed to read binary data from MPD: {}", e);
                return None;
            }
        }

        // Read the OK line (there might be an empty line before it)
        let mut ok_line = String::new();
        if reader.read_line(&mut ok_line).is_err() {
            error!("Failed to read OK line from MPD");
            return None;
        }
        
        // Handle empty line by reading another line if needed
        if ok_line.trim().is_empty() {
            ok_line.clear();
            if reader.read_line(&mut ok_line).is_err() {
                error!("Failed to read OK line after empty line from MPD");
                return None;
            }
        }
        
        if !ok_line.trim().eq("OK") {
            error!("Unexpected response after binary data: {}", ok_line);
            return None;
        }
        
        // If this is the complete image, we're done
        let mut full_data = buffer;
        
        // For larger images, we need to fetch multiple chunks
        if chunk_size < size {
            let mut offset = chunk_size;
            
            while offset < size {
                // Send command to get next chunk
                let cmd = format!("albumart \"{}\" {}\n", uri, offset);
                if writer.write_all(cmd.as_bytes()).is_err() {
                    error!("Failed to send albumart command for chunk at offset {}", offset);
                    return None;
                }
                
                // Read size line (we already know the full size)
                let mut size_line = String::new();
                if reader.read_line(&mut size_line).is_err() {
                    error!("Failed to read size response for chunk at offset {}", offset);
                    return None;
                }
                
                // Check for ACK errors in subsequent chunks
                if size_line.starts_with("ACK") {
                    error!("MPD returned error during chunk request at offset {}: {}", offset, size_line.trim());
                    return None;
                }
                
                // Read binary size line
                let mut binary_line = String::new();
                if reader.read_line(&mut binary_line).is_err() {
                    error!("Failed to read binary size for chunk at offset {}", offset);
                    return None;
                }
                
                // Parse the binary chunk size
                let chunk_size: usize = match binary_line.strip_prefix("binary: ") {
                    Some(size_str) => match size_str.trim().parse() {
                        Ok(size) => size,
                        Err(e) => {
                            error!("Failed to parse binary chunk size at offset {}: {}", offset, e);
                            return None;
                        }
                    },
                    None => {
                        error!("Unexpected binary size format at offset {}: {}", offset, binary_line);
                        return None;
                    }
                };
                
                // Read the binary data for this chunk
                let mut buffer = vec![0u8; chunk_size];
                match reader.read_exact(&mut buffer) {
                    Ok(_) => {},
                    Err(e) => {
                        error!("Failed to read binary data at offset {}: {}", offset, e);
                        return None;
                    }
                }
                
                // Append to our full data
                full_data.extend_from_slice(&buffer);
                
                // Read the OK line (there might be an empty line before it)
                let mut ok_line = String::new();
                if reader.read_line(&mut ok_line).is_err() {
                    error!("Failed to read OK line at offset {}", offset);
                    return None;
                }
                
                // Handle empty line by reading another line if needed
                if ok_line.trim().is_empty() {
                    ok_line.clear();
                    if reader.read_line(&mut ok_line).is_err() {
                        error!("Failed to read OK line after empty line at offset {}", offset);
                        return None;
                    }
                }
                
                if !ok_line.trim().eq("OK") {
                    error!("Unexpected response after binary data at offset {}: {}", offset, ok_line);
                    return None;
                }
                
                // Update offset for next chunk
                offset += chunk_size;
            }
        }
        
        // Detect MIME type based on image data
        let mime_type = Self::detect_mime_type(&full_data);
        
        debug!("Successfully retrieved cover art of size {} bytes with MIME type {}", full_data.len(), mime_type);
        Some((full_data, mime_type))
    }

    /// Detect the MIME type of an image from its binary data
    fn detect_mime_type(data: &[u8]) -> String {
        // Check for magic numbers in header bytes to identify image format
        if data.len() < 4 {
            return "application/octet-stream".to_string();
        }
        
        // JPEG starts with FF D8 FF
        if data.starts_with(&[0xFF, 0xD8, 0xFF]) {
            return "image/jpeg".to_string();
        }
        
        // PNG starts with 89 50 4E 47 0D 0A 1A 0A
        if data.starts_with(&[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]) {
            return "image/png".to_string();
        }
        
        // GIF starts with "GIF87a" or "GIF89a"
        if data.starts_with(b"GIF87a") || data.starts_with(b"GIF89a") {
            return "image/gif".to_string();
        }
        
        // WebP starts with "RIFF" followed by 4 bytes, followed by "WEBP"
        if data.len() >= 12 && 
           data.starts_with(b"RIFF") && 
           data[8..12].starts_with(b"WEBP") {
            return "image/webp".to_string();
        }
        
        // BMP starts with "BM"
        if data.starts_with(b"BM") {
            return "image/bmp".to_string();
        }
        
        // Default to binary if we can't identify it
        "application/octet-stream".to_string()
    }

    /// Get cover art for a specific track URL
    /// 
    /// This function fetches the cover art for a given track URL and optionally caches it.
    /// 
    /// # Arguments
    /// * `track_url` - The URL/path of the track to get cover art for
    /// * `cache_path` - Optional path to store the image in the cache
    /// 
    /// # Returns
    /// A tuple of (binary data, mime-type) of the cover art if found, None otherwise
    pub fn get_track_cover(&self, track_url: &str, cache_path: Option<&str>) -> Option<(Vec<u8>, String)> {
        // Check if we should look in the cache first
        if let Some(path) = cache_path {
            // Check if the track has a cover in the cache
            if let Ok((image_data, mime_type)) = crate::helpers::imagecache::get_image_with_mime_type(path) {
                debug!("Found cached cover art for track at {}", path);
                return Some((image_data, mime_type));
            }
        }
        
        debug!("Retrieving cover art for track URL: {}", track_url);
        
        // Use the existing cover_art function to get the image data
        let image_result = self.cover_art(track_url);
        
        // If we got an image and have a cache path, store it in the imagecache
        if let (Some((image_data, mime_type)), Some(path)) = (&image_result, cache_path) {
            // Store the image in the cache using store_image_from_data
            if let Err(e) = crate::helpers::imagecache::store_image_from_data(path, image_data.clone(), mime_type.clone()) {
                warn!("Failed to cache track cover art at '{}': {}", path, e);
            } else {
                debug!("Stored track cover art in cache at {}", path);
            }
        }
        
        image_result
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
            {
                let album_artists = album.artists.lock();
                for artist_name in album_artists.iter() {
                    artist_names.insert(artist_name.clone());

                    // Store album-artist relationship for later
                    album_artist_relations.push((album.id.clone(), artist_name.clone()));
                }
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
                    // Create new metadata for album artists
                    let metadata = crate::data::ArtistMeta::new();
                    artist_with_metadata.metadata = Some(metadata);
                },
                Err(e) => {
                    warn!("Error loading cached metadata for artist {}: {}", artist_name, e);
                    // Create new metadata as fallback
                    let metadata = crate::data::ArtistMeta::new();
                    artist_with_metadata.metadata = Some(metadata);
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
                
                // No longer adding album names to artist.albums since we removed that attribute
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
        {
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
            {
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
        let artists = self.artists.read();
        if let Some(mut artist) = artists.get(name).cloned() {
            self.populate_calculated_artist_fields(&mut artist);
            Some(artist)
        } else {
            None
        }
    }    /// Get album cover art using the album's identifier
    /// 
    /// This function implements a multi-step approach to retrieve album cover art:
    /// 1. Check if the cover art is already in the image cache
    /// 2. Check if MPD delivers cover art via albumart command
    /// 3. If it doesn't, locate the directory of the album and look for local files
    /// 4. Attempt to extract cover art from music files in that directory
    /// 5. Try to save the extracted art as cover.jpg in the album directory
    /// 6. Store it in the imagecache for future requests
    /// 
    /// Returns a tuple of (binary data, mime-type) of the cover art if found, None otherwise
    pub fn get_album_cover(&self, id: &crate::data::Identifier) -> Option<(Vec<u8>, String)> {
        // First, look up the album by its ID
        let album = self.get_album_by_id(id)?;
        debug!("Found album with ID {}: {}", id, album.name);

        // Get artist name to create a better identifier
        let artist_name = {
            let artists = album.artists.lock();
            if !artists.is_empty() { artists[0].clone() } else { "Unknown Artist".to_string() }
        };

        // Get album year if available from release_date
        let year = album.release_date.map(|date| date.year());
        
        // Step 1: Try to get the cover art from the image cache first
        if let Ok((data, mime_type)) = crate::helpers::imagecache::get_album_cover(&artist_name, &album.name, year) {
            debug!("Found album cover in image cache for {}", album.name);
            return Some((data, mime_type));
        }

        // Get the URI of the first song in the album
        let uri = {
            let tracks = album.tracks.lock();
            if let Some(first_track) = tracks.first() {
                first_track.uri.clone()
            } else {
                return None;
            }
        };

        if uri.is_none() {
            warn!("No URI found for album {}, probably empty", album.name);
            return None;
        }

        let uri = uri.as_deref().unwrap();
        debug!("Attempting to retrieve cover art for album {} using URI: {}", album.name, uri);
        
        // Step 2: Try MPD's albumart command
        if let Some((data, mime_type)) = self.cover_art(uri) {
            debug!("Successfully retrieved cover art from MPD for album: {}", album.name);
            
            // Store the cover art in the imagecache with artist and album name
            let _ = crate::helpers::imagecache::store_album_cover(
                &artist_name, 
                &album.name, 
                year, 
                data.clone(), 
                mime_type.clone()
            );
            
            return Some((data, mime_type));
        }
        
        debug!("MPD did not provide cover art, checking if extraction from audio files is enabled");
        
        // Check if cover art extraction from music files is enabled
        if !self.is_extract_coverart_enabled() {
            debug!("Cover art extraction from music files is disabled, skipping file extraction");
            
            // Fall back to the original track cover method
            debug!("Falling back to get_track_cover for album {}", album.name);
            
            // Try to get track cover
            if let Some((data, mime_type)) = self.get_track_cover(uri, None) {
                // Store in image cache with artist and album info for future requests
                let _ = crate::helpers::imagecache::store_album_cover(
                    &artist_name,
                    &album.name,
                    year,
                    data.clone(),
                    mime_type.clone()
                );
                
                return Some((data, mime_type));
            }
            
            return None;
        }
        
        debug!("Cover art extraction is enabled, attempting to extract from audio files");
        
        // Step 3: Find the directory of the album
        let album_dir = self.get_album_directory(uri);
        if let Some(dir_path) = album_dir {
            // Step 4: Try to extract cover art from music files in the directory
            if let Some((data, mime_type)) = self.extract_cover_from_music_files(&dir_path) {
                debug!("Successfully extracted cover art from music files for album: {}", album.name);
                
                // Step 5: Try to save as cover.jpg in album directory
                self.save_cover_to_album_dir(&dir_path, &data);
                
                // Step 6: Store in imagecache for future requests
                let _ = crate::helpers::imagecache::store_album_cover(
                    &artist_name,
                    &album.name,
                    year,
                    data.clone(),
                    mime_type.clone()
                );
                
                return Some((data, mime_type));
            }
        }
        
        // Fall back to the original track cover method in case all else fails
        debug!("Falling back to get_track_cover for album {}", album.name);
        
        // Try to get track cover
        if let Some((data, mime_type)) = self.get_track_cover(uri, None) {
            // Store in image cache with artist and album info for future requests
            let _ = crate::helpers::imagecache::store_album_cover(
                &artist_name,
                &album.name,
                year,
                data.clone(),
                mime_type.clone()
            );
            
            Some((data, mime_type))
        } else {
            None
        }
    }
    
    /// Get artist cover art using the artist store
    /// 
    /// # Arguments
    /// * `artist_name` - The name of the artist
    /// 
    /// # Returns
    /// Option containing (image data, mime type) if found
    pub fn get_artist_cover(&self, artist_name: &str) -> Option<(Vec<u8>, String)> {
        debug!("Getting artist cover for: {}", artist_name);
        
        // Use the artist store to get the cached image path
        if let Some(cache_path) = crate::helpers::artist_store::get_artist_cached_image(artist_name) {
            debug!("Found cached artist image at: {}", cache_path);
            
            // Read the image data from the cache file
            if let Ok(image_data) = std::fs::read(&cache_path) {
                // Determine MIME type based on file extension
                let mime_type = if cache_path.ends_with(".jpg") || cache_path.ends_with(".jpeg") {
                    "image/jpeg".to_string()
                } else if cache_path.ends_with(".png") {
                    "image/png".to_string()
                } else if cache_path.ends_with(".webp") {
                    "image/webp".to_string()
                } else {
                    "image/jpeg".to_string() // Default to JPEG
                };
                
                debug!("Successfully loaded artist image for {}: {} bytes, MIME: {}", 
                       artist_name, image_data.len(), mime_type);
                return Some((image_data, mime_type));
            } else {
                warn!("Failed to read cached artist image from: {}", cache_path);
            }
        }
        
        // If no cached image found, try to download one
        if let Some(cache_path) = crate::helpers::artist_store::get_or_download_artist_image(artist_name) {
            debug!("Downloaded new artist image at: {}", cache_path);
            
            // Read the newly downloaded image
            if let Ok(image_data) = std::fs::read(&cache_path) {
                let mime_type = if cache_path.ends_with(".jpg") || cache_path.ends_with(".jpeg") {
                    "image/jpeg".to_string()
                } else if cache_path.ends_with(".png") {
                    "image/png".to_string()
                } else if cache_path.ends_with(".webp") {
                    "image/webp".to_string()
                } else {
                    "image/jpeg".to_string()
                };
                
                debug!("Successfully loaded downloaded artist image for {}: {} bytes, MIME: {}", 
                       artist_name, image_data.len(), mime_type);
                return Some((image_data, mime_type));
            } else {
                warn!("Failed to read downloaded artist image from: {}", cache_path);
            }
        }
        
        debug!("No artist cover found for: {}", artist_name);
        None
    }
    
    /// Extract the album directory from a track URI
    fn get_album_directory(&self, uri: &str) -> Option<String> {
        debug!("Extracting album directory from URI: {}", uri);
        // MPD URIs are typically relative paths to the music directory
        // We need to determine the music directory structure
        
        // Remove the filename from the URI to get the directory
        let uri_path = std::path::Path::new(uri);
        if let Some(parent) = uri_path.parent() {
            let parent_str = parent.to_string_lossy().to_string();
            if parent_str.is_empty() {
                debug!("Parent directory is empty, returning None");
                return None;
            }
            
            debug!("Extracted album directory: {}", parent_str);
            // Return the parent directory path
            return Some(parent_str);
        }
        
        debug!("No parent directory found, returning None");
        None
    }
    
    /// Extract cover art from music files in a directory
    fn extract_cover_from_music_files(&self, dir_path: &str) -> Option<(Vec<u8>, String)> {
        debug!("Extracting cover art from music files in directory: {}", dir_path);
        
        // Get the music directory from configuration or /etc/mpd.conf
        let mut base_paths = Vec::new();
        
        if let Some(music_dir) = self.controller.get_effective_music_directory() {
            debug!("Using configured music directory: {}", music_dir);
            base_paths.push(music_dir);
        }
        
        // Add fallback paths in case the configured path doesn't work
        base_paths.extend([
            "/var/lib/mpd/music".to_string(),   // Common default
            "/music".to_string(),               // Common mount point  
            "/home/mpd/music".to_string(),      // Another common path
            "/srv/music".to_string(),           // Another common path
            "".to_string(),                     // Use relative path as-is
        ]);
        
        for base_path in base_paths {
            let full_path = if base_path.is_empty() {
                dir_path.to_string()
            } else {
                format!("{}/{}", base_path, dir_path)
            };
            
            debug!("Trying extraction path: {}", full_path);
            
            // Check if this path exists before trying to extract
            if std::path::Path::new(&full_path).exists() {
                debug!("Path exists, attempting cover art extraction: {}", full_path);
                // Use the coverart helper to extract cover art from the directory
                let result = crate::helpers::local_coverart::extract_cover_from_music_files(&full_path);
                if result.is_some() {
                    debug!("Successfully extracted cover art from: {}", full_path);
                    return result;
                }
            } else {
                debug!("Path does not exist: {}", full_path);
            }
        }
        
        debug!("No valid path found for cover extraction");
        None
    }
    
    /// Save cover art to the album directory as cover.jpg
    fn save_cover_to_album_dir(&self, dir_path: &str, data: &[u8]) -> bool {
        // Get the music directory from configuration or /etc/mpd.conf
        let mut base_paths = Vec::new();
        
        if let Some(music_dir) = self.controller.get_effective_music_directory() {
            debug!("Using configured music directory for saving cover: {}", music_dir);
            base_paths.push(music_dir);
        }
        
        // Add fallback paths in case the configured path doesn't work
        base_paths.extend([
            "/var/lib/mpd/music".to_string(),   // Common default
            "/music".to_string(),               // Common mount point  
            "/home/mpd/music".to_string(),      // Another common path
            "/srv/music".to_string(),           // Another common path
            "".to_string(),                     // Use relative path as-is
        ]);
        
        for base_path in base_paths {
            let full_path = if base_path.is_empty() {
                dir_path.to_string()
            } else {
                format!("{}/{}", base_path, dir_path)
            };
            
            // Check if the parent directory exists before trying to save
            if std::path::Path::new(&full_path).exists() {
                debug!("Trying to save cover art to: {}", full_path);
                if crate::helpers::local_coverart::save_cover_to_dir(&full_path, data) {
                    return true;
                }
            }
        }
        
        false
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
        let loaded = self.library_loaded.lock();
        *loaded
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
                *self.library_loaded.lock() = false;
                
                // Reset loading progress to 0
                {
                    let mut progress = self.loading_progress.lock();
                    *progress = 0.0;
                }

                // Update albums collection
                {
                    let mut self_albums = self.albums.write();
                    self_albums.clear();

                    // Add each album to the collection with name as key
                    for mut album in albums {
                        self.populate_calculated_album_fields(&mut album);
                        self_albums.insert(album.name.clone(), album);
                    }

                    debug!("Updated library with {} albums", self_albums.len());
                }

                // Create artists and update album-artist relationships
                if let Err(e) = self.create_artists() {
                    error!("Error creating artists: {}", e);
                }

                // Mark as loaded and update progress
                *self.library_loaded.lock() = true;
                {
                    let mut progress = self.loading_progress.lock();
                    *progress = 1.0;
                }
                
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
                error!("Error loading MPD library: {}", e);
                Err(e)
            }
        };
        
        // Send an update_database notification of 100% before exiting, even in case of errors
        self.controller.notify_database_update(None, None, None, Some(100.0));
        
        result
    }
    
    fn get_albums(&self) -> Vec<Album> {
        let albums = self.albums.read();
        albums.values().cloned().map(|mut album| {
            self.populate_calculated_album_fields(&mut album);
            album
        }).collect()
    }
    
    fn get_artists(&self) -> Vec<Artist> {
        let artists = self.artists.read();
        artists.values().cloned().map(|mut artist| {
            self.populate_calculated_artist_fields(&mut artist);
            artist
        }).collect()
    }
    
    fn get_album_by_artist_and_name(&self, artist: &str, album: &str) -> Option<Album> {
        self.get_album_by_artist_and_name(artist, album)
    }
    
    fn get_artist_by_name(&self, name: &str) -> Option<Artist> {
        self.get_artist_by_name(name)
    }
    
    fn update_artist_metadata(&self) {
        if self.enhance_metadata {
            info!("Starting background metadata update for MPDLibrary artists");
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
        
        // First check if the identifier is a URL-safe base64 encoded string that needs to be decoded
        if url_encoding::is_url_safe_base64(&identifier) {
            debug!("Detected URL-safe base64 encoded identifier: {}", identifier);
            
            if let Some(original_path) = url_encoding::decode_url_safe(&identifier) {
                debug!("Decoded base64 '{}' to path: {}", identifier, original_path);
                // Use the decoded path as the identifier
                return self.get_image(original_path);
            } else {
                warn!("Failed to decode base64 identifier: {}", identifier);
                return None;
            }
        }
        
        // Check if the identifier starts with "album:"
        if let Some(album_id_str) = identifier.strip_prefix("album:") {
            debug!("Detected album identifier: {}", album_id_str);
            
            // Parse the album ID as a numeric ID (MPD only supports numeric IDs)
            match album_id_str.parse::<u64>() {
                Ok(album_id_num) => {
                    let album_id = crate::data::Identifier::Numeric(album_id_num);
                    debug!("Parsed album ID: {}", album_id);
                    
                    // Use get_album_cover to retrieve the image
                    return self.get_album_cover(&album_id);
                },
                Err(e) => {
                    warn!("Failed to parse album ID '{}' as a number: {}", album_id_str, e);
                    return None;
                }
            }
        }
        
        // Check if the identifier starts with "artist:"
        if let Some(artist_name) = identifier.strip_prefix("artist:") {
            debug!("Detected artist identifier: {}", artist_name);
            
            // Use get_artist_cover to retrieve the image
            return self.get_artist_cover(artist_name);
        }
        
        // If we've reached here, the identifier format wasn't recognized
        // As a fallback, assume the identifier is a track URL
        debug!("Treating identifier as track URL: {}", identifier);
        
        // Use get_track_cover with the identifier as a URL
        let result = self.get_track_cover(&identifier, None);
        
        if result.is_some() {
            debug!("Successfully retrieved image for track URL: {}", identifier);
        } else {
            debug!("No image found for track URL: {}", identifier);
        }
        
        result
    }

    fn force_update(&self) -> bool {
        use std::io::{Write, BufRead, BufReader};
        use std::net::TcpStream;
        
        debug!("Sending update command to MPD server at {}:{}", self.hostname, self.port);
        
        // Connect to MPD server
        match TcpStream::connect(format!("{}:{}", self.hostname, self.port)) {
            Ok(stream) => {
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut writer = stream;
                
                // Read the welcome message
                let mut welcome = String::new();
                if reader.read_line(&mut welcome).is_err() {
                    error!("Failed to read welcome message from MPD");
                    return false;
                }
                
                if !welcome.starts_with("OK") {
                    error!("Unexpected welcome message from MPD: {}", welcome);
                    return false;
                }
                
                // Send update command to rescan the library
                match writer.write_all(b"update\n") {
                    Ok(_) => {
                        // Read the response
                        let mut response = String::new();
                        if reader.read_line(&mut response).is_err() {
                            error!("Failed to read response from MPD");
                            return false;
                        }
                        
                        // Check if the response contains the update ID
                        if response.starts_with("updating_db:") {
                            debug!("MPD update command accepted: {}", response.trim());
                            return true;
                        } else if response == "OK\n" {
                            // Some MPD servers might just respond with OK
                            debug!("MPD update command accepted with OK response");
                            return true;
                        } else {
                            error!("Unexpected response from MPD update command: {}", response.trim());
                            return false;
                        }
                    },
                    Err(e) => {
                        error!("Failed to send update command to MPD: {}", e);
                        return false;
                    }
                }
            },
            Err(e) => {
                error!("Failed to connect to MPD server: {}", e);
                return false;
            }
        }
    }

    fn get_meta_keys(&self) -> Vec<String> {
        vec![
            "memory_usage".to_string(),
            "album_count".to_string(),
            "artist_count".to_string(),
            "track_count".to_string(),
            "hostname".to_string(),
            "port".to_string(),
            "library_loaded".to_string(),
            "loading_progress".to_string(),
            "enhance_metadata".to_string(),
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
                    "name": "MPDLibrary",
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
                let albums = self.albums.read();
                Some(albums.len().to_string())
            },
            "artist_count" => {
                let artists = self.artists.read();
                Some(artists.len().to_string())
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
            "hostname" => Some(self.hostname.clone()),
            "port" => Some(self.port.to_string()),
            "library_loaded" => {
                let loaded = self.library_loaded.lock();
                Some(loaded.to_string())
            },
            "loading_progress" => {
                let progress = self.loading_progress.lock();
                Some(progress.to_string())
            },
            "enhance_metadata" => Some(self.enhance_metadata.to_string()),
            _ => None,
        }
    }
    
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl MPDLibrary {
    /// Get the effective music directory from the controller
    pub fn get_music_directory(&self) -> Option<String> {
        self.controller.get_effective_music_directory()
    }
    
    /// Get lyrics for a song by its file path/URL
    /// 
    /// This method looks for .lrc files alongside the music files in the MPD music directory.
    /// The LRC file should have the same name as the music file but with .lrc extension.
    pub fn get_lyrics_by_url(&self, file_path: &str) -> crate::helpers::lyrics::LyricsResult<crate::helpers::lyrics::LyricsContent> {
        // Get the music directory from the controller
        let music_directory = self.controller.get_effective_music_directory()
            .unwrap_or_else(|| "/var/lib/mpd/music".to_string());
        
        // Create an MPD lyrics provider
        let provider = crate::helpers::lyrics::MPDLyricsProvider::new(music_directory);
        
        // Use the provider to get lyrics
        provider.get_lyrics_by_url(file_path)
    }
    
    /// Get lyrics for a song by its ID in the MPD database
    /// 
    /// This method retrieves the song information from MPD and then looks for lyrics.
    /// Currently simplified - for full implementation, we'd need to match queue songs with IDs.
    pub fn get_lyrics_by_id(&self, song_id: &str) -> crate::helpers::lyrics::LyricsResult<crate::helpers::lyrics::LyricsContent> {
        // For now, return NotFound as the implementation would be complex
        // In a full implementation, we'd need to query the current queue and match song IDs
        log::debug!("Lyrics lookup by song ID not yet fully implemented for MPD: {}", song_id);
        Err(crate::helpers::lyrics::LyricsError::NotFound)
    }
    
    /// Get lyrics for a song by metadata (artist, title, etc.)
    /// 
    /// This is a convenience method that attempts to find the song in the library
    /// and then get lyrics for it. Currently not implemented as it would require
    /// searching through all tracks to find matching metadata.
    pub fn get_lyrics_by_metadata(&self, lookup: &crate::helpers::lyrics::LyricsLookup) -> crate::helpers::lyrics::LyricsResult<crate::helpers::lyrics::LyricsContent> {
        // For now, return NotFound as we'd need to implement track search by metadata
        log::debug!("Lyrics lookup by metadata not yet implemented for MPD: {} - {}", lookup.artist, lookup.title);
        Err(crate::helpers::lyrics::LyricsError::NotFound)
    }
}
