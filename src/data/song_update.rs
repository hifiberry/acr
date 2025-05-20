use serde::{Deserialize, Serialize};

/// Represents an update to the information of the currently playing song.
/// `title` and `artist` are mandatory for identifying the song.
/// Other fields are optional; only non-None fields will be applied to the song.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SongInformationUpdate {
    pub title: String,
    pub artist: String, // Assuming primary artist for identification

    #[serde(skip_serializing_if = "Option::is_none")]
    pub album: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub album_artist: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub track_number: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_tracks: Option<i32>,
    // duration: Option<f64>, // Duration is usually static, less likely to be updated this way
    #[serde(skip_serializing_if = "Option::is_none")]
    pub genre: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub year: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cover_art_url: Option<String>,
    // stream_url: Option<String>, // Stream URL is usually static
    // source: Option<String>, // Source is usually static
    #[serde(skip_serializing_if = "Option::is_none")]
    pub liked: Option<bool>,
    // metadata: HashMap<String, serde_json::Value>, // For simplicity, not including generic metadata updates for now
}
