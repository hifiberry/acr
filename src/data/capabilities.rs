use serde::{Serialize, Deserialize};
use strum_macros::{Display, EnumString, AsRefStr};

/// Enum representing the capabilities of a player
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Display, EnumString, AsRefStr)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum PlayerCapability {
    /// Can play media
    Play,
    /// Can pause playback
    Pause,
    /// Can toggle between play and pause
    PlayPause,
    /// Can stop playback
    Stop,
    /// Can skip to next track
    Next,
    /// Can skip to previous track
    Previous,
    /// Can seek within a track
    Seek,
    /// Can report playback position
    Position,
    /// Can report track duration/length
    Length,
    /// Can control volume
    Volume,
    /// Can mute/unmute
    Mute,
    /// Can toggle shuffle mode
    Shuffle,
    /// Can set loop mode
    Loop,
    /// Can manage playlists
    Playlists,
    /// Can manage queue
    Queue,
    /// Can provide metadata
    Metadata,
    /// Can provide album art
    AlbumArt,
    /// Can search for tracks
    Search,
    /// Can browse media library
    Browse,
    /// Can manage favorites
    Favorites,
    /// Can update internal database
    DatabaseUpdate,
    /// Can be killed (terminated forcefully)
    Killable,
}

impl PlayerCapability {
    /// Get the string representation of the capability
    pub fn as_str(&self) -> &str {
        match self {
            Self::Play => "play",
            Self::Pause => "pause",
            Self::PlayPause => "playpause",
            Self::Stop => "stop",
            Self::Next => "next",
            Self::Previous => "previous",
            Self::Seek => "seek",
            Self::Position => "position",
            Self::Length => "length",
            Self::Volume => "volume",
            Self::Mute => "mute",
            Self::Shuffle => "shuffle",
            Self::Loop => "loop",
            Self::Playlists => "playlists",
            Self::Queue => "queue",
            Self::Metadata => "metadata",
            Self::AlbumArt => "album_art",
            Self::Search => "search",
            Self::Browse => "browse",
            Self::Favorites => "favorites",
            Self::DatabaseUpdate => "db_update",
            Self::Killable => "killable",
        }
    }

    /// Get a list of all capabilities
    pub fn all() -> Vec<PlayerCapability> {
        vec![
            Self::Play,
            Self::Pause,
            Self::PlayPause,
            Self::Stop,
            Self::Next,
            Self::Previous,
            Self::Seek,
            Self::Position,
            Self::Length,
            Self::Volume,
            Self::Mute,
            Self::Shuffle,
            Self::Loop,
            Self::Playlists,
            Self::Queue,
            Self::Metadata,
            Self::AlbumArt,
            Self::Search,
            Self::Browse,
            Self::Favorites,
            Self::DatabaseUpdate,
            Self::Killable,
        ]
    }
}

impl From<PlayerCapability> for String {
    fn from(cap: PlayerCapability) -> Self {
        cap.as_str().to_string()
    }
}