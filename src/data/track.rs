use serde::{Serialize, Deserialize};

/// Represents a Track in an album
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Track {
    /// Disc number (as a string to support formats like "1/2")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disc_number: Option<String>,
    /// Track number
    #[serde(skip_serializing_if = "Option::is_none")]
    pub track_number: Option<u16>,
    /// Track name
    pub name: String,
    /// Track artist (only stored if different from album artist)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artist: Option<String>,
    /// URI/filename of the track (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uri: Option<String>,
}

impl Track {
    /// Create a new Track
    pub fn new(disc_number: Option<String>, track_number: Option<u16>, name: String) -> Self {
        Self {
            disc_number,
            track_number,
            name,
            artist: None,
            uri: None,
        }
    }
    
    /// Create a new Track with just the name (convenience method)
    pub fn with_name(name: String) -> Self {
        Self {
            disc_number: None,
            track_number: None,
            name,
            artist: None,
            uri: None,
        }
    }
    
    /// Create a new Track with an artist
    pub fn with_artist(disc_number: Option<String>, track_number: Option<u16>, name: String, artist: String, album_artist: Option<&str>) -> Self {
        // Only store artist if it differs from the album artist
        let track_artist = if let Some(album_artist) = album_artist {
            if artist != album_artist {
                Some(artist)
            } else {
                None
            }
        } else {
            Some(artist)
        };
        
        Self {
            disc_number,
            track_number,
            name,
            artist: track_artist,
            uri: None,
        }
    }
    
    /// Set the URI/filename for this track
    pub fn with_uri(mut self, uri: String) -> Self {
        self.uri = Some(uri);
        self
    }
}