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
    /// List of albums by this artist
    pub albums: HashSet<String>,
    /// Number of tracks by this artist
    pub track_count: usize,
    /// Additional metadata for the artist (MusicBrainz ID, images, etc)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<ArtistMeta>,
}

impl Artist {
    /// Get the artist's MusicBrainz ID if available
    pub fn mbid(&self) -> Option<String> {
        self.metadata.as_ref().and_then(|m| m.mbid.clone())
    }
    
    /// Get the artist's thumbnail URL if available
    pub fn thumb_url(&self) -> Option<String> {
        self.metadata.as_ref().and_then(|m| m.thumb_url.clone())
    }
    
    /// Get the artist's banner URL if available
    pub fn banner_url(&self) -> Option<String> {
        self.metadata.as_ref().and_then(|m| m.banner_url.clone())
    }
    
    /// Set the artist's MusicBrainz ID
    pub fn set_mbid(&mut self, mbid: String) {
        if let Some(meta) = &mut self.metadata {
            meta.set_mbid(mbid);
        } else {
            self.metadata = Some(ArtistMeta::with_mbid(mbid));
        }
    }
    
    /// Set the artist's thumbnail URL
    pub fn set_thumb_url(&mut self, url: String) {
        if let Some(meta) = &mut self.metadata {
            meta.set_thumb_url(url);
        } else {
            let mut meta = ArtistMeta::new();
            meta.set_thumb_url(url);
            self.metadata = Some(meta);
        }
    }
    
    /// Set the artist's banner URL
    pub fn set_banner_url(&mut self, url: String) {
        if let Some(meta) = &mut self.metadata {
            meta.set_banner_url(url);
        } else {
            let mut meta = ArtistMeta::new();
            meta.set_banner_url(url);
            self.metadata = Some(meta);
        }
    }
    
    /// Clear all metadata for this artist
    pub fn clear_metadata(&mut self) {
        self.metadata = None;
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