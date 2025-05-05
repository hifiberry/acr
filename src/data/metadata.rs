use serde::{Serialize, Deserialize};

/// Metadata for Artists including external IDs and image URLs
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ArtistMeta {
    /// MusicBrainz ID for the artist
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mbid: Option<String>,
    
    /// Thumbnail image URL or filename
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thumb_url: Option<String>,
    
    /// Banner/background image URL or filename
    #[serde(skip_serializing_if = "Option::is_none")]
    pub banner_url: Option<String>,
}

impl ArtistMeta {
    /// Create a new empty ArtistMeta
    pub fn new() -> Self {
        Self::default()
    }
    
    /// Create ArtistMeta with a MusicBrainz ID
    pub fn with_mbid(mbid: String) -> Self {
        Self {
            mbid: Some(mbid),
            ..Self::default()
        }
    }
    
    /// Set the MusicBrainz ID
    pub fn set_mbid(&mut self, mbid: String) {
        self.mbid = Some(mbid);
    }
    
    /// Set the thumbnail URL or filename
    pub fn set_thumb_url(&mut self, url: String) {
        self.thumb_url = Some(url);
    }
    
    /// Set the banner URL or filename
    pub fn set_banner_url(&mut self, url: String) {
        self.banner_url = Some(url);
    }
    
    /// Check if this metadata contains any actual data
    pub fn is_empty(&self) -> bool {
        self.mbid.is_none() && self.thumb_url.is_none() && self.banner_url.is_none()
    }
    
    /// Clear all metadata
    pub fn clear(&mut self) {
        self.mbid = None;
        self.thumb_url = None;
        self.banner_url = None;
    }
}