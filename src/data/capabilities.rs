use serde::{Serialize, Deserialize, Serializer, Deserializer};
use strum_macros::{Display, EnumString, AsRefStr};
use enumflags2::{bitflags, BitFlags};
use std::fmt;

/// Enum representing the capabilities of a player
#[bitflags]
#[repr(u32)]  // Explicitly specify representation type for BitFlags
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Display, EnumString, AsRefStr)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum PlayerCapability {
    /// Can play media
    Play = 0x0001,
    /// Can pause playback
    Pause = 0x0002,
    /// Can toggle between play and pause
    PlayPause = 0x0004,
    /// Can stop playback
    Stop = 0x0008,
    /// Can skip to next track
    Next = 0x0010,
    /// Can skip to previous track
    Previous = 0x0020,
    /// Can seek within a track
    Seek = 0x0040,
    /// Can report playback position
    Position = 0x0080,
    /// Can report track duration/length
    Length = 0x0100,
    /// Can control volume
    Volume = 0x0200,
    /// Can mute/unmute
    Mute = 0x0400,
    /// Can toggle shuffle mode
    Shuffle = 0x0800,
    /// Can set loop mode
    Loop = 0x1000,
    /// Can manage playlists
    Playlists = 0x2000,
    /// Can manage queue
    Queue = 0x4000,
    /// Can provide metadata
    Metadata = 0x8000,
    /// Can provide album art
    AlbumArt = 0x10000,
    /// Can search for tracks
    Search = 0x20000,
    /// Can browse media library
    Browse = 0x40000,
    /// Can manage favorites
    Favorites = 0x80000,
    /// Can update internal database
    DatabaseUpdate = 0x100000,
    /// Can be killed (terminated forcefully)
    Killable = 0x200000,
    /// Player controller supports receiving updates (song change, position, etc.)
    ReceivesUpdates = 0x400000,
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
            Self::ReceivesUpdates => "receives_updates",
        }
    }

    /// Create a BitFlags with all capabilities
    pub fn all_flags() -> BitFlags<PlayerCapability> {
        BitFlags::from_flag(Self::Play) |
        BitFlags::from_flag(Self::Pause) |
        BitFlags::from_flag(Self::PlayPause) |
        BitFlags::from_flag(Self::Stop) |
        BitFlags::from_flag(Self::Next) |
        BitFlags::from_flag(Self::Previous) |
        BitFlags::from_flag(Self::Seek) |
        BitFlags::from_flag(Self::Position) |
        BitFlags::from_flag(Self::Length) |
        BitFlags::from_flag(Self::Volume) |
        BitFlags::from_flag(Self::Mute) |
        BitFlags::from_flag(Self::Shuffle) |
        BitFlags::from_flag(Self::Loop) |
        BitFlags::from_flag(Self::Playlists) |
        BitFlags::from_flag(Self::Queue) |
        BitFlags::from_flag(Self::Metadata) |
        BitFlags::from_flag(Self::AlbumArt) |
        BitFlags::from_flag(Self::Search) |
        BitFlags::from_flag(Self::Browse) |
        BitFlags::from_flag(Self::Favorites) |
        BitFlags::from_flag(Self::DatabaseUpdate) |
        BitFlags::from_flag(Self::Killable) |
        BitFlags::from_flag(Self::ReceivesUpdates)
    }

    /// Convert a Vec of capabilities to BitFlags
    pub fn vec_to_flags(capabilities: &[PlayerCapability]) -> BitFlags<PlayerCapability> {
        let mut flags = BitFlags::empty();
        for cap in capabilities {
            flags |= BitFlags::from_flag(*cap);
        }
        flags
    }

    /// Convert BitFlags to a Vec of capabilities
    pub fn flags_to_vec(flags: BitFlags<PlayerCapability>) -> Vec<PlayerCapability> {
        flags.iter().collect()
    }
}

impl From<PlayerCapability> for String {
    fn from(cap: PlayerCapability) -> Self {
        cap.as_str().to_string()
    }
}

/// A set of player capabilities, implemented efficiently using bitflags
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlayerCapabilitySet {
    flags: BitFlags<PlayerCapability>,
}

impl PlayerCapabilitySet {
    /// Create a new empty capabilities set
    pub fn empty() -> Self {
        Self {
            flags: BitFlags::empty(),
        }
    }

    /// Add a capability to the set
    pub fn add_capability(&mut self, capability: PlayerCapability) {
        self.flags |= BitFlags::from_flag(capability);
    }

    /// Remove a capability from the set
    pub fn remove_capability(&mut self, capability: PlayerCapability) {
        self.flags &= !BitFlags::from_flag(capability);
    }

    /// Check if a specific capability is in the set
    pub fn has_capability(&self, capability: PlayerCapability) -> bool {
        self.flags.contains(capability)
    }
    
    /// Check if the set is empty (contains no capabilities)
    pub fn is_empty(&self) -> bool {
        self.flags.is_empty()
    }

    /// Create a set from a slice of capabilities
    pub fn from_slice(capabilities: &[PlayerCapability]) -> Self {
        let mut set = Self::empty();
        for capability in capabilities {
            set.add_capability(*capability);
        }
        set
    }

    /// Convert to a Vec of individual capabilities
    pub fn to_vec(&self) -> Vec<PlayerCapability> {
        self.flags.iter().collect()
    }

    /// Get the underlying BitFlags representation
    pub fn as_bitflags(&self) -> BitFlags<PlayerCapability> {
        self.flags
    }
}

impl Default for PlayerCapabilitySet {
    fn default() -> Self {
        Self::empty()
    }
}

// Implement From conversions to make it easier to work with
impl From<PlayerCapability> for PlayerCapabilitySet {
    fn from(capability: PlayerCapability) -> Self {
        let mut set = Self::empty();
        set.add_capability(capability);
        set
    }
}

impl From<Vec<PlayerCapability>> for PlayerCapabilitySet {
    fn from(capabilities: Vec<PlayerCapability>) -> Self {
        Self::from_slice(&capabilities)
    }
}

impl From<PlayerCapabilitySet> for Vec<PlayerCapability> {
    fn from(set: PlayerCapabilitySet) -> Self {
        set.to_vec()
    }
}

// Support for collecting capabilities into a set
impl FromIterator<PlayerCapability> for PlayerCapabilitySet {
    fn from_iter<T: IntoIterator<Item = PlayerCapability>>(iter: T) -> Self {
        let mut set = Self::empty();
        for capability in iter {
            set.add_capability(capability);
        }
        set
    }
}

/// Support for serializing PlayerCapabilitySet
impl Serialize for PlayerCapabilitySet {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        // Serialize as a vec of capabilities for human-readable formats
        self.to_vec().serialize(serializer)
    }
}

/// Support for deserializing PlayerCapabilitySet from various formats
impl<'de> Deserialize<'de> for PlayerCapabilitySet {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        // Deserialize from a vec of capabilities
        let capabilities = Vec::<PlayerCapability>::deserialize(deserializer)?;
        Ok(Self::from_slice(&capabilities))
    }
}

impl fmt::Display for PlayerCapabilitySet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let caps = self.to_vec();
        write!(f, "[")?;
        for (i, cap) in caps.iter().enumerate() {
            if i > 0 {
                write!(f, ", ")?;
            }
            write!(f, "{}", cap)?;
        }
        write!(f, "]")
    }
}

// Implement IntoIterator for &PlayerCapabilitySet to allow iteration over capabilities
impl IntoIterator for &PlayerCapabilitySet {
    type Item = PlayerCapability;
    type IntoIter = std::vec::IntoIter<PlayerCapability>;

    fn into_iter(self) -> Self::IntoIter {
        self.to_vec().into_iter()
    }
}