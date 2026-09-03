/// Class representing metadata for a song/track
use std::collections::HashMap;
use std::fmt; // Added for Display
use serde::{Serialize, Deserialize};

/// Metadata key recording where `cover_art_url` came from, when it came from a
/// source a later lookup is allowed to replace. Absent means the cover art is
/// the song's own artwork and must be left alone.
pub const COVER_ART_SOURCE: &str = "cover_art_source";

/// Value for [`COVER_ART_SOURCE`] marking cover art that is only the radio
/// station's logo rather than artwork for the track being played.
pub const COVER_ART_SOURCE_STATION_LOGO: &str = "station_logo";

/// Value for [`COVER_ART_SOURCE`] marking cover art that Last.fm supplied for
/// the track's album. It is the track's own artwork, so it is not replaceable.
pub const COVER_ART_SOURCE_LASTFM: &str = "lastfm";

/// Value for [`COVER_ART_SOURCE`] marking cover art that reached the song
/// through an enrichment lookup which did not name its own source. It is a
/// real answer for the track rather than a placeholder, so it is not
/// replaceable; the point of recording it is that the marker of the
/// placeholder it replaced must never be left standing over it.
pub const COVER_ART_SOURCE_ENRICHMENT: &str = "enrichment";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Song {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artist: Option<String>,
    
    #[serde(skip_serializing_if = "Option::is_none")]
    pub album: Option<String>,
    
    #[serde(skip_serializing_if = "Option::is_none")]
    pub album_artist: Option<String>,
    
    #[serde(skip_serializing_if = "Option::is_none")]
    pub track_number: Option<i32>,
    
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_tracks: Option<i32>,
    
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration: Option<f64>, // in seconds
    
    #[serde(skip_serializing_if = "Option::is_none")]
    pub genre: Option<String>,
    
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub genres: Vec<String>,
    
    #[serde(skip_serializing_if = "Option::is_none")]
    pub year: Option<i32>,
    
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cover_art_url: Option<String>,
    
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream_url: Option<String>,
    
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>, // e.g., "spotify", "local", "radio"

    #[serde(skip_serializing_if = "Option::is_none")]
    pub liked: Option<bool>, // Indicates if the song is liked or favorited
    
    #[serde(skip_serializing_if = "Option::is_none")]
    pub composer: Option<String>,
    
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    pub metadata: HashMap<String, serde_json::Value>,
}

// The to_json method is now provided by the Serializable trait
// which is automatically implemented for all types that implement Serialize

impl Song {
    /// Whether the cover art currently on this song may be replaced by a better
    /// lookup. True when there is none, and when what is there is only a
    /// placeholder such as a radio station's logo — see [`COVER_ART_SOURCE`].
    /// Artwork belonging to the song itself is never replaceable.
    pub fn cover_art_is_replaceable(&self) -> bool {
        if self.cover_art_url.is_none() {
            return true;
        }
        self.metadata
            .get(COVER_ART_SOURCE)
            .and_then(|value| value.as_str())
            .is_some_and(|source| source == COVER_ART_SOURCE_STATION_LOGO)
    }
}

impl PartialEq for Song {
    fn eq(&self, other: &Self) -> bool {
        // Compare only title, artist and album for equality
        self.title == other.title &&
        self.artist == other.artist &&
        self.album == other.album
    }
}

impl fmt::Display for Song {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut display_str = self.title.as_deref().unwrap_or("Unknown Title").to_string();
        if let Some(artist_name) = &self.artist {
            if !artist_name.is_empty() {
                display_str.push_str(" by ");
                display_str.push_str(artist_name);
            }
        }
        if let Some(album_name) = &self.album {
            display_str.push_str(&format!(" (Album: {})", album_name));
        }
        write!(f, "{}", display_str)
    }
}