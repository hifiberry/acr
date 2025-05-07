use std::collections::HashSet;
use std::hash::{Hash, Hasher};
use serde::{Serialize, Deserialize};
use crate::data::metadata::ArtistMeta;

/// Represents an Artist in the music database
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Artist {
    /// Unique identifier for the artist (64-bit hash)
    pub id: u64,
    /// Artist name
    pub name: String,
    /// Is not a single, but multiple artists (e.g. "Artist1, Artist2")
    pub is_multi: bool,
    /// List of albums by this artist
    pub albums: HashSet<String>,
    /// Number of tracks by this artist
    pub track_count: usize,
    /// Additional metadata for the artist (MusicBrainz ID, images, etc)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<ArtistMeta>,
}

impl Artist {
    /// Add a MusicBrainz ID to the artist
    pub fn add_mbid(&mut self, mbid: String) {
        if let Some(meta) = &mut self.metadata {
            meta.add_mbid(mbid);
        } else {
            // Create a new ArtistMeta and add the MBID
            let mut meta = ArtistMeta::new();
            meta.add_mbid(mbid);
            self.metadata = Some(meta);
        }
    }
    
    /// Add a thumbnail URL to the artist
    pub fn add_thumb_url(&mut self, url: String) {
        if let Some(meta) = &mut self.metadata {
            meta.add_thumb_url(url);
        } else {
            let mut meta = ArtistMeta::new();
            meta.add_thumb_url(url);
            self.metadata = Some(meta);
        }
    }
    
    /// Add a banner URL to the artist
    pub fn add_banner_url(&mut self, url: String) {
        if let Some(meta) = &mut self.metadata {
            meta.add_banner_url(url);
        } else {
            let mut meta = ArtistMeta::new();
            meta.add_banner_url(url);
            self.metadata = Some(meta);
        }
    }
    
    /// Check if this is a multi-artist entry (contains comma in the name)
    pub fn is_multi(&self) -> bool {
        self.name.contains(',')
    }
    
    /// Clear all metadata for this artist
    pub fn clear_metadata(&mut self) {
        self.metadata = None;
    }
    
    /// Ensure that the artist has metadata, creating it if needed
    pub fn ensure_metadata(&mut self) {
        if self.metadata.is_none() {
            self.metadata = Some(ArtistMeta::new());
        }
    }
}

// Implement Hash trait to ensure the id is used as the hash
impl Hash for Artist {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

// Implement PartialEq to compare artists using their id
impl PartialEq for Artist {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

// Implement Eq to make Artist fully comparable using its id
impl Eq for Artist {}