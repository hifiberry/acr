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
    
    /// Force an update of the library data in the underlying system
    /// 
    /// This differs from refresh_library in that it asks the backend system
    /// to scan for new files or changes, rather than just refreshing our in-memory data.
    /// Returns true if the update was initiated successfully, false otherwise.
    fn force_update(&self) -> bool {
        // Default implementation does nothing and returns false
        false
    }
    
    /// Allow downcasting to concrete types
    fn as_any(&self) -> &dyn std::any::Any;
    
    /// Get an image by identifier
    /// the identifier has no specific format, it can be used differently 
    /// depending on the library implementation
    /// returns a tuple of (image data, mime type)
    fn get_image(&self, identifier: String) -> Option<(Vec<u8>, String)>;
    
    /// Update artist metadata in background
    /// 
    /// This method should update the metadata for all artists in the library using
    /// background worker thread. The default implementation does nothing.
    fn update_artist_metadata(&self);

    /// Get a list of meta keys for the library
    /// 
    /// This method should return a list of meta keys that are available in the 
    /// library.
    /// The default implementation returns an empty vector.    
    fn get_meta_keys(&self) -> Vec<String> {
        vec![]
    }

    /// Get a specific metadata value as string
    /// 
    /// This method should return a specific metadata value for a given key.
    /// The default implementation returns None.
    fn get_metadata_value(&self, _key: &str) -> Option<String> {
        None
    }
    
    /// Get all metadata as a HashMap with JSON values
    /// 
    /// This method should return all metadata for the library as a HashMap with
    /// JSON values. The default implementation returns an empty HashMap.
    fn get_metadata(&self) -> Option<std::collections::HashMap<String, serde_json::Value>> {
        // Convert string metadata to JSON values
        let mut result = std::collections::HashMap::new();
        
        // Add each meta key to the result
        for key in self.get_meta_keys() {
            if let Some(value) = self.get_metadata_value(&key) {
                // Try to parse as JSON, fall back to string value
                match serde_json::from_str(&value) {
                    Ok(json_value) => {
                        result.insert(key, json_value);
                    },
                    Err(_) => {
                        // Use string value
                        result.insert(key, serde_json::Value::String(value));
                    }
                }
            }
        }
        
        if result.is_empty() {
            None
        } else {
            Some(result)
        }
    }
}