use serde::{Serialize, Deserialize};

/// Result of order detection
#[derive(Debug, PartialEq, Eq, Hash, Clone, Serialize, Deserialize)]
pub enum OrderResult {
    /// First part is artist, second part is song
    ArtistSong,
    /// First part is song, second part is artist
    SongArtist,
    /// No combination found in MusicBrainz
    Unknown,
    /// Both combinations found, cannot determine
    Undecided,
}
