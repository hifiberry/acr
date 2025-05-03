use std::collections::HashSet;
use std::hash::{Hash, Hasher};
use serde::{Serialize, Deserialize};

/// Represents an Artist in the music database
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Artist {
    /// Unique identifier for the artist (64-bit hash)
    pub id: u64,
    /// Artist name
    pub name: String,
    /// List of albums by this artist
    pub albums: HashSet<String>,
    /// Number of tracks by this artist
    pub track_count: usize,
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