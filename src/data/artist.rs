use std::collections::HashSet;
use serde::{Serialize, Deserialize};

/// Represents an Artist in the music database
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Artist {
    /// Artist name
    pub name: String,
    /// List of albums by this artist
    pub albums: HashSet<String>,
    /// Number of tracks by this artist
    pub track_count: usize,
}