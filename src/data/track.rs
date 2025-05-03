use serde::{Serialize, Deserialize};

/// Represents a Track in an album
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Track {
    /// Disc number (as a string to support formats like "1/2")
    pub disc_number: String,
    /// Track number
    pub track_number: u16,
    /// Track name
    pub name: String,
    /// Track artist (only stored if different from album artist)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artist: Option<String>,
}

impl Track {
    /// Create a new Track
    pub fn new(disc_number: String, track_number: u16, name: String) -> Self {
        Self {
            disc_number,
            track_number,
            name,
            artist: None,
        }
    }
    
    /// Create a new Track with an artist
    pub fn with_artist(disc_number: String, track_number: u16, name: String, artist: String, album_artist: Option<&str>) -> Self {
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
        }
    }
}