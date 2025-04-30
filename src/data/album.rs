use serde::{Serialize, Deserialize};

/// Represents an Album in the music database
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Album {
    /// Album name
    pub name: String,
    /// Artist name (typically album artist)
    pub artist: Option<String>,
    /// Year of album release (if available)
    pub year: Option<i32>,
    /// List of tracks on this album
    pub tracks: Vec<String>,
    /// Cover art path (if available)
    pub cover_art: Option<String>,
}