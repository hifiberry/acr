// MPD Library interface to work with the MPD music database
use mpd::{Client, Song as MPDSong, error::Error as MpdError};
use std::net::TcpStream;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, RwLock};
use log::{debug, info, warn, error};
use crate::data::{Album, Artist, LibraryInterface, LibraryError};

/// MPD library interface that provides access to albums and artists
pub struct MPDLibrary {
    /// MPD server hostname
    hostname: String,
    
    /// MPD server port
    port: u16,
    
    /// Cache of albums, key is album name
    albums: Arc<RwLock<HashMap<String, Album>>>,
    
    /// Cache of artists, key is artist name
    artists: Arc<RwLock<HashMap<String, Artist>>>,
    
    /// Flag indicating if library is loaded
    library_loaded: Arc<Mutex<bool>>,
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
            library_loaded: Arc::new(Mutex::new(false)),
        }
    }
    
    /// Get a fresh MPD client connection
    fn get_client(&self) -> Result<Client<TcpStream>, MpdError> {
        let addr = format!("{}:{}", self.hostname, self.port);
        Client::connect(&addr)
    }
    
    /// Convert MPD error to LibraryError
    fn convert_error(error: MpdError) -> LibraryError {
        match error {
            MpdError::Parse(_) => LibraryError::FormatError(error.to_string()),
            MpdError::Io(_) => LibraryError::ConnectionError(error.to_string()),
            _ => LibraryError::InternalError(format!("Unknown error: {}", error)),
        }
    }
    
    /// Process a single song to extract album and artist information
    fn process_song(&self,
                    song: &MPDSong,
                    artist_albums: &mut HashMap<String, HashSet<String>>,
                    artist_track_counts: &mut HashMap<String, usize>,
                    album_tracks: &mut HashMap<String, Vec<String>>) -> Result<(), LibraryError> {
        
        // Get the track name and album
        let track_name = song.title.as_deref().unwrap_or("Unknown").to_string();
        
        let album_name = song.tags.iter()
            .find(|(name, _)| name == "Album")
            .map(|(_, value)| value.to_string())
            .unwrap_or_else(|| "Unknown Album".to_string());
        
        // Get the album artist (prefer AlbumArtist tag, fallback to Artist)
        let album_artist = song.tags.iter()
            .find(|(name, _)| name == "AlbumArtist")
            .or_else(|| song.tags.iter().find(|(name, _)| name == "Artist"))
            .map(|(_, value)| value.to_string());
        
        // Get the track artist (different from album artist potentially)
        let track_artist = song.tags.iter()
            .find(|(name, _)| name == "Artist")
            .map(|(_, value)| value.to_string());
        
        // Extract year if available
        let year = song.tags.iter()
            .find(|(name, _)| name == "Date")
            .and_then(|(_, date_str)| {
                let year_part = date_str.split('-').next().unwrap_or(date_str);
                year_part.parse::<i32>().ok()
            });
        
        // Update album tracks
        if !track_name.is_empty() {
            album_tracks.entry(album_name.clone())
                .or_insert_with(Vec::new)
                .push(track_name);
        }
        
        // If we have album info, prepare to add it to our albums cache
        if let Ok(mut albums) = self.albums.write() {
            let album = albums.entry(album_name.clone()).or_insert_with(|| Album {
                name: album_name.clone(),
                artist: album_artist.clone(),
                year,
                tracks: Vec::new(),
                cover_art: None,
            });
            
            // Update year if we didn't have it before
            if album.year.is_none() && year.is_some() {
                album.year = year;
            }
            
            // Update album artist if we didn't have it before
            if album.artist.is_none() && album_artist.is_some() {
                album.artist = album_artist.clone();
            }
        } else {
            return Err(LibraryError::InternalError("Failed to acquire write lock on albums".to_string()));
        }
        
        // Update artist-to-album mapping and track counts
        if let Some(artist) = &track_artist {
            artist_albums.entry(artist.clone())
                .or_insert_with(HashSet::new)
                .insert(album_name.clone());
            
            *artist_track_counts.entry(artist.clone()).or_insert(0) += 1;
        }
        
        Ok(())
    }
    
    /// Finalize album and artist data after processing all songs
    fn finalize_albums_and_artists(&self,
                                 artist_albums: HashMap<String, HashSet<String>>,
                                 artist_track_counts: HashMap<String, usize>,
                                 album_tracks: HashMap<String, Vec<String>>) -> Result<(), LibraryError> {
        
        // Update album tracks
        if let Ok(mut albums) = self.albums.write() {
            for (album_name, tracks) in album_tracks {
                if let Some(album) = albums.get_mut(&album_name) {
                    album.tracks = tracks;
                }
            }
        } else {
            error!("Failed to acquire write lock for album tracks");
            return Err(LibraryError::InternalError("Failed to acquire write lock for album tracks".to_string()));
        }
        
        // Build artist objects
        if let Ok(mut artists) = self.artists.write() {
            for (artist_name, albums) in artist_albums {
                let track_count = artist_track_counts.get(&artist_name).cloned().unwrap_or(0);
                
                artists.insert(artist_name.clone(), Artist {
                    name: artist_name,
                    albums,
                    track_count,
                });
            }
        } else {
            error!("Failed to acquire write lock for artists");
            return Err(LibraryError::InternalError("Failed to acquire write lock for artists".to_string()));
        }
        
        Ok(())
    }
}

impl LibraryInterface for MPDLibrary {
    fn new() -> Self {
        debug!("Creating new MPDLibrary with default connection");
        Self::with_connection("localhost", 6600)
    }
    
    fn is_loaded(&self) -> bool {
        *self.library_loaded.lock().unwrap()
    }
    
    fn refresh_library(&self) -> Result<(), LibraryError> {
        debug!("Refreshing MPD library data");
        let mut client = self.get_client().map_err(Self::convert_error)?;
        
        *self.library_loaded.lock().unwrap() = false;
        
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
        }
        
        let songs = client.listall().map_err(Self::convert_error)?;
        info!("Found {} total entries in MPD database", songs.len());
        
        let mut artist_albums: HashMap<String, HashSet<String>> = HashMap::new();
        let mut artist_track_counts: HashMap<String, usize> = HashMap::new();
        
        let mut album_tracks: HashMap<String, Vec<String>> = HashMap::new();
        
        for song in songs {
            self.process_song(&song, &mut artist_albums, &mut artist_track_counts, &mut album_tracks)?;
        }
        
        self.finalize_albums_and_artists(artist_albums, artist_track_counts, album_tracks)?;
        
        *self.library_loaded.lock().unwrap() = true;
        
        info!("MPD library refresh complete: {} albums, {} artists", 
              self.albums.read().unwrap().len(),
              self.artists.read().unwrap().len());
        
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
        
        if let Some(artist) = self.get_artist(artist_name) {
            if let Ok(albums) = self.albums.read() {
                for album_name in &artist.albums {
                    if let Some(album) = albums.get(album_name) {
                        result.push(album.clone());
                    }
                }
            }
        }
        
        result
    }
    
    fn get_album_cover(&self, _album_name: &str) -> Option<String> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_create_library() {
        let lib = MPDLibrary::new();
        assert_eq!(lib.is_loaded(), false);
        assert!(lib.get_albums().is_empty());
        assert!(lib.get_artists().is_empty());
    }
}