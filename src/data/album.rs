use serde::{Serialize, Deserialize, Serializer, Deserializer};
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use parking_lot::Mutex;
use crate::data::{Identifier, track::Track};

/// Represents an Album in the music database
#[derive(Debug, Clone)]
pub struct Album {
    /// Unique identifier for the album (can be numeric or string)
    pub id: Identifier,
    /// Album name
    pub name: String,
    /// List of artists for this album
    pub artists: Arc<Mutex<Vec<String>>>,
    // Artists in a single string (might not be populated)
    pub artists_flat: Option<String>,
    /// Release date of the album (optional)
    pub release_date: Option<chrono::NaiveDate>,
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
        
        // Serialize id using Identifier's serialization
        state.serialize_field("id", &self.id)?;
        state.serialize_field("name", &self.name)?;
        
        // Get lock on artists and serialize directly as Vec<String>
        let artists = self.artists.lock();
        state.serialize_field("artists", &*artists)?;
        
        // Serialize release_date field
        state.serialize_field("release_date", &self.release_date)?;
        
        // Get lock on tracks and serialize directly as Vec<Track>
        let tracks = self.tracks.lock();
        state.serialize_field("tracks", &*tracks)?;
        
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
            id: Identifier,
            name: String,
            #[serde(default)]
            artists: Vec<String>,
            // For backward compatibility, also accept the old 'artist' field
            #[serde(default)]
            artist: Option<String>,
            #[serde(skip_serializing_if = "Option::is_none")]
            release_date: Option<chrono::NaiveDate>,
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
        if artists.is_empty() {
            if let Some(artist_str) = helper.artist {
                // Split the old artist field by commas and add each artist
                for artist in artist_str.split(',').map(|s| s.trim().to_string()) {
                    if !artist.is_empty() {
                        artists.push(artist);
                    }
                }
            }
        }
        
        // Convert helper to actual Album
        Ok(Album {
            id: helper.id,
            name: helper.name,
            artists: Arc::new(Mutex::new(artists)),
            artists_flat: None, // Initialize artists_flat as None
            release_date: helper.release_date,
            tracks: Arc::new(Mutex::new(helper.tracks)),
            cover_art: helper.cover_art,
            uri: helper.uri,
        })
    }
}

impl Album {
    /// Sort tracks by disc number and track number
    /// 
    /// This method sorts the album's track list first by disc number (if available)
    /// and then by track number within each disc. This ensures tracks are in the
    /// correct playing order.
    pub fn sort_tracks(&self) {
        let mut tracks = self.tracks.lock();
        tracks.sort_by(|a, b| {
            // First compare disc numbers (default to "1" if not present)
            let disc_a = a.disc_number.as_ref().cloned().unwrap_or_else(|| "1".to_string());
            let disc_b = b.disc_number.as_ref().cloned().unwrap_or_else(|| "1".to_string());

            // Try to parse disc numbers as integers
            let disc_num_a = disc_a.parse::<u32>().unwrap_or(1);
            let disc_num_b = disc_b.parse::<u32>().unwrap_or(1);

            // Compare discs first
            match disc_num_a.cmp(&disc_num_b) {
                std::cmp::Ordering::Equal => {
                    // If discs are the same, compare track numbers
                    let track_num_a = a.track_number.unwrap_or(0);
                    let track_num_b = b.track_number.unwrap_or(0);
                    track_num_a.cmp(&track_num_b)
                },
                other => other, // If discs are different, sort by disc
            }
        });
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