use serde::{Serialize, Deserialize, Serializer, Deserializer};
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex};
use crate::data::track::Track;

/// Represents an Album in the music database
#[derive(Debug, Clone)]
pub struct Album {
    /// Unique identifier for the album (64-bit hash)
    pub id: u64,
    /// Album name
    pub name: String,
    /// List of artists for this album
    pub artists: Arc<Mutex<Vec<String>>>,
    /// Year of album release (if available)
    pub year: Option<i32>,
    /// List of tracks on this album
    pub tracks: Arc<Mutex<Vec<Track>>>,
    /// Cover art path (if available)
    pub cover_art: Option<String>,
    /// URI of the first song file in the album (useful for retrieving cover art)
    pub uri: Option<String>,
}

// Custom serialization implementation for Album
impl Serialize for Album {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("Album", 7)?;
        
        state.serialize_field("id", &self.id)?;
        state.serialize_field("name", &self.name)?;
        
        // Get lock on artists and serialize directly as Vec<String>
        if let Ok(artists) = self.artists.lock() {
            state.serialize_field("artists", &*artists)?;
        } else {
            // If we can't get the lock, serialize an empty vector
            state.serialize_field("artists", &Vec::<String>::new())?;
        }
        
        state.serialize_field("year", &self.year)?;
        
        // Get lock on tracks and serialize directly as Vec<Track>
        if let Ok(tracks) = self.tracks.lock() {
            state.serialize_field("tracks", &*tracks)?;
        } else {
            // If we can't get the lock, serialize an empty vector
            state.serialize_field("tracks", &Vec::<Track>::new())?;
        }
        
        state.serialize_field("cover_art", &self.cover_art)?;
        state.serialize_field("uri", &self.uri)?;
        state.end()
    }
}

// Custom deserialization implementation for Album
impl<'de> Deserialize<'de> for Album {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        // Use a helper struct for deserialization
        #[derive(Deserialize)]
        struct AlbumHelper {
            id: u64,
            name: String,
            #[serde(default)]
            artists: Vec<String>,
            // For backward compatibility, also accept the old 'artist' field
            #[serde(default)]
            artist: Option<String>,
            year: Option<i32>,
            tracks: Vec<Track>,
            #[serde(skip_serializing_if = "Option::is_none")]
            cover_art: Option<String>,
            #[serde(skip_serializing_if = "Option::is_none")]
            uri: Option<String>,
        }
        
        // Deserialize to the helper struct first
        let helper = AlbumHelper::deserialize(deserializer)?;
        
        // Convert old artist field to artists if needed
        let mut artists = helper.artists;
        if artists.is_empty() && helper.artist.is_some() {
            // Split the old artist field by commas and add each artist
            for artist in helper.artist.unwrap().split(',').map(|s| s.trim().to_string()) {
                if !artist.is_empty() {
                    artists.push(artist);
                }
            }
        }
        
        // Convert helper to actual Album
        Ok(Album {
            id: helper.id,
            name: helper.name,
            artists: Arc::new(Mutex::new(artists)),
            year: helper.year,
            tracks: Arc::new(Mutex::new(helper.tracks)),
            cover_art: helper.cover_art,
            uri: helper.uri,
        })
    }
}

// Implement Hash trait to ensure the id is used as the hash
impl Hash for Album {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

// Implement PartialEq to compare albums using their id
impl PartialEq for Album {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

// Implement Eq to make Album fully comparable using its id
impl Eq for Album {}