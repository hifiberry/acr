use serde::{Serialize, Deserialize};

/// Metadata for Artists including external IDs and image URLs
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ArtistMeta {
    /// MusicBrainz ID for the artist
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub mbid: Vec<String>,
    
    /// Thumbnail image URL or filename
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub thumb_url: Vec<String>,
    
    /// Banner/background image URL or filename
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub banner_url: Vec<String>,
}

impl ArtistMeta {
    /// Create a new empty ArtistMeta
    pub fn new() -> Self {
        Self::default()
    }
    
    /// Create ArtistMeta with a MusicBrainz ID
    pub fn with_mbid(mbid: String) -> Self {
        Self {
            mbid: vec![mbid],
            ..Self::default()
        }
    }
    
    /// Add a MusicBrainz ID
    pub fn add_mbid(&mut self, mbid: String) {
        if !self.mbid.contains(&mbid) {
            self.mbid.push(mbid);
        }
    }
    
    /// Add a thumbnail URL or filename
    pub fn add_thumb_url(&mut self, url: String) {
        if !self.thumb_url.contains(&url) {
            self.thumb_url.push(url);
        }
    }
    
    /// Add a banner URL or filename
    pub fn add_banner_url(&mut self, url: String) {
        if !self.banner_url.contains(&url) {
            self.banner_url.push(url);
        }
    }
    
    /// Check if this metadata contains any actual data
    pub fn is_empty(&self) -> bool {
        self.mbid.is_empty() && self.thumb_url.is_empty() && self.banner_url.is_empty()
    }
    
    /// Clear all metadata
    pub fn clear(&mut self) {
        self.mbid.clear();
        self.thumb_url.clear();
        self.banner_url.clear();
    }
}