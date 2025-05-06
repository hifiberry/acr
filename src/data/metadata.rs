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
    
    /// Artist biography text
    #[serde(skip_serializing_if = "Option::is_none")]
    pub biography: Option<String>,
    
    /// Musical genres associated with this artist
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub genres: Vec<String>,
}

impl ArtistMeta {
    /// Create a new empty ArtistMeta
    pub fn new() -> Self {
        Self {
            mbid: Vec::new(),
            thumb_url: Vec::new(),
            banner_url: Vec::new(),
            biography: None,
            genres: Vec::new(),
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
    
    /// Add a genre if it doesn't already exist
    pub fn add_genre(&mut self, genre: String) {
        if !self.genres.contains(&genre) {
            self.genres.push(genre);
        }
    }
    
    /// Check if this metadata contains any actual data
    pub fn is_empty(&self) -> bool {
        self.mbid.is_empty() && 
        self.thumb_url.is_empty() && 
        self.banner_url.is_empty() && 
        self.biography.is_none() &&
        self.genres.is_empty()
    }
    
    /// Clear all metadata
    pub fn clear(&mut self) {
        self.mbid.clear();
        self.thumb_url.clear();
        self.banner_url.clear();
        self.biography = None;
        self.genres.clear();
    }
}