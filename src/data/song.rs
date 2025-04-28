/// Class representing metadata for a song/track
use std::collections::HashMap;
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
    
    #[serde(skip_serializing_if = "Option::is_none")]
    pub year: Option<i32>,
    
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cover_art_url: Option<String>,
    
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream_url: Option<String>,
    
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>, // e.g., "spotify", "local", "radio"
    
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    pub metadata: HashMap<String, serde_json::Value>,
}

impl Song {
    /// Convert song metadata to JSON string
    ///
    /// Returns:
    ///     JSON string representation of the song metadata
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }
}