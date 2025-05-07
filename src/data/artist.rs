use std::hash::{Hash, Hasher};
use std::collections::HashSet;
use serde::{Serialize, Deserialize, Serializer, Deserializer};
use crate::data::metadata::ArtistMeta;

/// Represents an Artist in the music database
#[derive(Debug, Clone)]
pub struct Artist {
    /// Unique identifier for the artist (64-bit hash)
    pub id: u64,
    /// Artist name
    pub name: String,
    /// Is not a single, but multiple artists (e.g. "Artist1, Artist2")
    pub is_multi: bool,
    /// Number of tracks by this artist
    pub track_count: usize,
    /// Additional metadata for the artist (MusicBrainz ID, images, etc)
    pub metadata: Option<ArtistMeta>,
}

// Custom serialization implementation for Artist to represent u64 id as string
impl Serialize for Artist {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("Artist", 5)?;
        
        // Serialize id as string
        state.serialize_field("id", &self.id.to_string())?;
        state.serialize_field("name", &self.name)?;
        state.serialize_field("is_multi", &self.is_multi)?;
        state.serialize_field("track_count", &self.track_count)?;
        state.serialize_field("metadata", &self.metadata)?;
        
        state.end()
    }
}

// Custom deserialization implementation for Artist to handle string id
impl<'de> Deserialize<'de> for Artist {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        // Use a helper struct for deserialization
        #[derive(Deserialize)]
        struct ArtistHelper {
            #[serde(deserialize_with = "deserialize_id_from_string")]
            id: u64,
            name: String,
            is_multi: bool,
            #[serde(default)]
            albums: HashSet<String>,  // Keep for backward compatibility
            track_count: usize,
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
            track_count: helper.track_count,
            metadata: helper.metadata,
        })
    }
}

// Helper function to deserialize ID that could be a string or a number
fn deserialize_id_from_string<'de, D>(deserializer: D) -> Result<u64, D::Error>
where
    D: Deserializer<'de>,
{
    struct IdVisitor;

    impl<'de> serde::de::Visitor<'de> for IdVisitor {
        type Value = u64;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("a string or integer representing a u64")
        }

        fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            Ok(value)
        }

        fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            value.parse::<u64>().map_err(serde::de::Error::custom)
        }

        fn visit_string<E>(self, value: String) -> Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            value.parse::<u64>().map_err(serde::de::Error::custom)
        }
    }

    deserializer.deserialize_any(IdVisitor)
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