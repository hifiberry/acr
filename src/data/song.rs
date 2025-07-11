/// Class representing metadata for a song/track
use std::collections::HashMap;
use std::fmt; // Added for Display
use serde::{Serialize, Deserialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Song {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artist: Option<String>,
    
    #[serde(skip_serializing_if = "Option::is_none")]
    pub album: Option<String>,
    
    #[serde(skip_serializing_if = "Option::is_none")]
    pub album_artist: Option<String>,
    
    #[serde(skip_serializing_if = "Option::is_none")]
    pub track_number: Option<i32>,
    
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_tracks: Option<i32>,
    
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration: Option<f64>, // in seconds
    
    #[serde(skip_serializing_if = "Option::is_none")]
    pub genre: Option<String>,
    
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub genres: Vec<String>,
    
    #[serde(skip_serializing_if = "Option::is_none")]
    pub year: Option<i32>,
    
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cover_art_url: Option<String>,
    
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream_url: Option<String>,
    
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>, // e.g., "spotify", "local", "radio"

    #[serde(skip_serializing_if = "Option::is_none")]
    pub liked: Option<bool>, // Indicates if the song is liked or favorited
    
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    pub metadata: HashMap<String, serde_json::Value>,
}

// The to_json method is now provided by the Serializable trait
// which is automatically implemented for all types that implement Serialize

impl PartialEq for Song {
    fn eq(&self, other: &Self) -> bool {
        // Compare only title, artist and album for equality
        self.title == other.title &&
        self.artist == other.artist &&
        self.album == other.album
    }
}

impl fmt::Display for Song {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut display_str = self.title.as_deref().unwrap_or("Unknown Title").to_string();
        if let Some(artist_name) = &self.artist {
            if !artist_name.is_empty() {
                display_str.push_str(" by ");
                display_str.push_str(artist_name);
            }
        }
        if let Some(album_name) = &self.album {
            display_str.push_str(&format!(" (Album: {})", album_name));
        }
        write!(f, "{}", display_str)
    }
}