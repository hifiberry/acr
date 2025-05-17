// Data structures for AudioControl3

pub mod album;
pub mod album_artists;
pub mod artist;
pub mod capabilities;
pub mod loop_mode;
pub mod player;
pub mod player_command;
pub mod player_event;
pub mod player_update;
pub mod serializable;
pub mod song;
pub mod stream_details;
pub mod library;
pub mod track;
pub mod metadata;

use std::fmt;
use serde::{Serialize, Deserialize, Serializer, Deserializer};
use std::hash::{Hash, Hasher};

/// Represents an identifier that can be either numeric or string-based
#[derive(Debug, Clone, Eq)]
pub enum Identifier {
    /// Numeric identifier (e.g., internal hash)
    Numeric(u64),
    /// String identifier (e.g., external ID like MusicBrainz ID)
    String(String),
}

impl Identifier {
    /// Get the numeric value, if available
    pub fn numeric(&self) -> Option<u64> {
        match self {
            Identifier::Numeric(n) => Some(*n),
            _ => None,
        }
    }

    /// Get the string value, if available
    pub fn string(&self) -> Option<&str> {
        match self {
            Identifier::String(s) => Some(s),
            _ => None,
        }
    }

    /// Convert to string representation
    pub fn to_string(&self) -> String {
        match self {
            Identifier::Numeric(n) => n.to_string(),
            Identifier::String(s) => s.clone(),
        }
    }
}

// Make Identifier hashable, hashing based on content
impl Hash for Identifier {
    fn hash<H: Hasher>(&self, state: &mut H) {
        match self {
            Identifier::Numeric(n) => {
                0u8.hash(state); // Tag for numeric
                n.hash(state);
            },
            Identifier::String(s) => {
                1u8.hash(state); // Tag for string
                s.hash(state);
            },
        }
    }
}

// Make Identifier comparable
impl PartialEq for Identifier {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Identifier::Numeric(a), Identifier::Numeric(b)) => a == b,
            (Identifier::String(a), Identifier::String(b)) => a == b,
            // Different types are not equal
            _ => false,
        }
    }
}

// Implement Display for better debugging
impl fmt::Display for Identifier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Identifier::Numeric(n) => write!(f, "{}", n),
            Identifier::String(s) => write!(f, "{}", s),
        }
    }
}

// Implement serialization for Identifier
impl Serialize for Identifier {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Identifier::Numeric(n) => serializer.serialize_str(&n.to_string()),
            Identifier::String(s) => serializer.serialize_str(s),
        }
    }
}

// Implement deserialization for Identifier
impl<'de> Deserialize<'de> for Identifier {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        
        // Try to parse as u64 first
        if let Ok(num) = s.parse::<u64>() {
            Ok(Identifier::Numeric(num))
        } else {
            Ok(Identifier::String(s))
        }
    }
}

// Re-export types from child modules
pub use album::*;
pub use album_artists::*;
pub use artist::*;
pub use capabilities::*;
pub use loop_mode::*;
pub use player::*;
pub use player_command::*;
pub use player_event::*;
pub use player_update::*;
pub use serializable::*;
pub use song::*;
pub use stream_details::*;
pub use library::*;
pub use track::*;
pub use metadata::*;