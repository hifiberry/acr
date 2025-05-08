use std::hash::{Hash, Hasher};
use std::collections::HashSet;
use serde::{Serialize, Deserialize, Serializer, Deserializer};
use crate::data::{Identifier, metadata::ArtistMeta};

/// Represents an Artist in the music database
#[derive(Debug, Clone)]
pub struct Artist {
    /// Unique identifier for the artist (can be numeric or string)
    pub id: Identifier,
    /// Artist name
    pub name: String,
    /// Is not a single, but multiple artists (e.g. "Artist1, Artist2")
    pub is_multi: bool,
    /// Additional metadata for the artist (MusicBrainz ID, images, etc)
    pub metadata: Option<ArtistMeta>,
}

// Custom serialization implementation for Artist to represent id as string
impl Serialize for Artist {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("Artist", 4)?;
        
        // Serialize id
        state.serialize_field("id", &self.id)?;
        state.serialize_field("name", &self.name)?;
        state.serialize_field("is_multi", &self.is_multi)?;
        state.serialize_field("metadata", &self.metadata)?;
        
        state.end()
    }
}

// Custom deserialization implementation for Artist
impl<'de> Deserialize<'de> for Artist {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        // Use a helper struct for deserialization
        #[derive(Deserialize)]
        struct ArtistHelper {
            id: Identifier,
            name: String,
            is_multi: bool,
            #[serde(default)]
            albums: HashSet<String>,  // Keep for backward compatibility
            #[serde(default)]
            track_count: usize,       // Keep for backward compatibility
            #[serde(default)]
            metadata: Option<ArtistMeta>,
        }

        // Deserialize the helper struct first
        let helper = ArtistHelper::deserialize(deserializer)?;
        
        // Convert helper to actual Artist
        Ok(Artist {
            id: helper.id,
            name: helper.name,
            is_multi: helper.is_multi,
            metadata: helper.metadata,
        })
    }
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