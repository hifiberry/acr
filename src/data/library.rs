use std::error::Error;
use crate::data::album::Album;
use crate::data::artist::Artist;
use crate::data::Identifier;

//
// Library Error Definition
//

/// Generic error type for library operations
#[derive(Debug)]
pub enum LibraryError {
    /// Connection error
    ConnectionError(String),
    /// Query error
    QueryError(String),
    /// Internal library error
    InternalError(String),
    /// Data format error
    FormatError(String),
}

impl std::fmt::Display for LibraryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LibraryError::ConnectionError(msg) => write!(f, "Connection error: {}", msg),
            LibraryError::QueryError(msg) => write!(f, "Query error: {}", msg),
            LibraryError::InternalError(msg) => write!(f, "Internal error: {}", msg),
            LibraryError::FormatError(msg) => write!(f, "Format error: {}", msg),
        }
    }
}

impl Error for LibraryError {}

//
// Library Interface Definition
//

/// Common trait for music library interfaces
pub trait LibraryInterface {
    /// Create a new library instance with default connection parameters
    fn new() -> Self where Self: Sized;
    
    /// Check if the library data is loaded
    fn is_loaded(&self) -> bool;
    
    /// Refresh the library by loading all albums and artists into memory
    fn refresh_library(&self) -> Result<(), LibraryError>;
    
    /// Get all albums
    fn get_albums(&self) -> Vec<Album>;
    
    /// Get all artists
    fn get_artists(&self) -> Vec<Artist>;
    
    /// Get album by artist and album name
    fn get_album_by_artist_and_name(&self, artist: &str, album: &str) -> Option<Album>;
    
    /// Get album by ID
    fn get_album_by_id(&self, id: &Identifier) -> Option<Album>;
    
    /// Get artist by name
    fn get_artist_by_name(&self, name: &str) -> Option<Artist>;
    
    /// Get albums by artist ID
    fn get_albums_by_artist_id(&self, artist_id: &Identifier) -> Vec<Album>;
    
    /// Get an image by identifier
    /// 
    /// This is a generic image retrieval method that will be implemented 
    /// by a dedicated image interface in the future.
    fn get_image(&self, _identifier: String) -> Option<String> {
        None // Default implementation returns None
    }
    
    /// Update artist metadata in background
    /// 
    /// This method should update the metadata for all artists in the library using
    /// background worker thread. The default implementation does nothing.
    fn update_artist_metadata(&self) {
        // Default empty implementation
    }
}