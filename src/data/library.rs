use std::error::Error;
use crate::data::album::Album;
use crate::data::artist::Artist;
use crate::data::Identifier;

//
// Library Error Definition
//

/// Generic error type for library operations
#[derive(Debug)]
pub enum LibraryError {
    /// Connection error
    ConnectionError(String),
    /// Query error
    QueryError(String),
    /// Internal library error
    InternalError(String),
    /// Data format error
    FormatError(String),
}

impl std::fmt::Display for LibraryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LibraryError::ConnectionError(msg) => write!(f, "Connection error: {}", msg),
            LibraryError::QueryError(msg) => write!(f, "Query error: {}", msg),
            LibraryError::InternalError(msg) => write!(f, "Internal error: {}", msg),
            LibraryError::FormatError(msg) => write!(f, "Format error: {}", msg),
        }
    }
}

impl Error for LibraryError {}

//
// Library Interface Definition
//

/// How well an artist name matched during a fuzzy search
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtistMatchType {
    /// Exact case-sensitive match
    Exact,
    /// Case-insensitive match (different casing only)
    CaseInsensitive,
    /// Fuzzy/similarity match (typos or slight differences)
    Fuzzy,
}

/// Result of a fuzzy artist search, including the match quality
#[derive(Debug, Clone)]
pub struct ArtistMatch {
    pub artist: Artist,
    pub match_type: ArtistMatchType,
    /// Similarity score 0.0–1.0; always 1.0 for Exact/CaseInsensitive
    pub score: f64,
}

/// Common trait for music library interfaces
pub trait LibraryInterface {
    /// Create a new library instance with default connection parameters
    fn new() -> Self where Self: Sized;
    
    /// Check if the library data is loaded
    fn is_loaded(&self) -> bool;
    
    /// Refresh the library by loading all albums and artists into memory
    fn refresh_library(&self) -> Result<(), LibraryError>;
    
    /// Get all albums
    fn get_albums(&self) -> Vec<Album>;
    
    /// Get all artists
    fn get_artists(&self) -> Vec<Artist>;
    
    /// Get album by artist and album name
    fn get_album_by_artist_and_name(&self, artist: &str, album: &str) -> Option<Album>;
    
    /// Get album by ID
    fn get_album_by_id(&self, id: &Identifier) -> Option<Album>;
    
    /// Get artist by name
    fn get_artist_by_name(&self, name: &str) -> Option<Artist>;

    /// Find artist with fuzzy matching.
    ///
    /// Tries in order:
    /// 1. Exact case-sensitive match → `ArtistMatchType::Exact`
    /// 2. Case-insensitive match     → `ArtistMatchType::CaseInsensitive`
    /// 3. Jaro-Winkler similarity ≥ 0.85 (best score wins) → `ArtistMatchType::Fuzzy`
    ///
    /// Default behaviour (no `fuzzy`) is unchanged – call `get_artist_by_name` instead.
    fn find_artist_fuzzy(&self, name: &str) -> Option<ArtistMatch> {
        let artists = self.get_artists();
        // Exact match
        if let Some(artist) = artists.iter().find(|a| a.name == name) {
            return Some(ArtistMatch { artist: artist.clone(), match_type: ArtistMatchType::Exact, score: 1.0 });
        }
        // Case-insensitive match
        let name_lower = name.to_lowercase();
        if let Some(artist) = artists.iter().find(|a| a.name.to_lowercase() == name_lower) {
            return Some(ArtistMatch { artist: artist.clone(), match_type: ArtistMatchType::CaseInsensitive, score: 1.0 });
        }
        // Fuzzy match (Jaro-Winkler)
        const THRESHOLD: f64 = 0.85;
        artists.iter()
            .map(|a| (strsim::jaro_winkler(&name_lower, &a.name.to_lowercase()), a))
            .filter(|(score, _)| *score >= THRESHOLD)
            .max_by(|(s1, _), (s2, _)| s1.partial_cmp(s2).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(score, artist)| ArtistMatch {
                artist: artist.clone(),
                match_type: ArtistMatchType::Fuzzy,
                score,
            })
    }
    
    /// Get albums by artist ID
    fn get_albums_by_artist_id(&self, artist_id: &Identifier) -> Vec<Album>;
    
    /// Force an update of the library data in the underlying system
    /// 
    /// This differs from refresh_library in that it asks the backend system
    /// to scan for new files or changes, rather than just refreshing our in-memory data.
    /// Returns true if the update was initiated successfully, false otherwise.
    fn force_update(&self) -> bool {
        // Default implementation does nothing and returns false
        false
    }
    
    /// Get all unique raw genres from album tags, sorted alphabetically (no cleanup applied)
    fn get_raw_album_genres(&self) -> Vec<String> {
        let mut seen = std::collections::HashSet::new();
        let mut genres: Vec<String> = self.get_albums()
            .into_iter()
            .flat_map(|a| a.genres)
            .filter(|g| seen.insert(g.clone()))
            .collect();
        genres.sort_unstable();
        genres
    }

    /// Get all unique raw genres from artist metadata, sorted alphabetically (no cleanup applied)
    fn get_raw_artist_genres(&self) -> Vec<String> {
        let mut seen = std::collections::HashSet::new();
        let mut genres: Vec<String> = self.get_artists()
            .into_iter()
            .filter_map(|a| a.metadata)
            .flat_map(|m| m.genres)
            .filter(|g| seen.insert(g.clone()))
            .collect();
        genres.sort_unstable();
        genres
    }

    /// Get all unique raw genres (albums + artist metadata combined), no cleanup applied
    fn get_raw_genres(&self) -> Vec<String> {
        let mut seen = std::collections::HashSet::new();
        let mut genres: Vec<String> = self.get_raw_album_genres()
            .into_iter()
            .chain(self.get_raw_artist_genres())
            .filter(|g| seen.insert(g.clone()))
            .collect();
        genres.sort_unstable();
        genres
    }

    /// Get all unique genres from album tags, sorted alphabetically
    fn get_album_genres(&self) -> Vec<String> {
        crate::helpers::genre_cleanup::clean_genres_global(self.get_raw_album_genres())
    }

    /// Get all unique genres from artist metadata, sorted alphabetically
    fn get_artist_genres(&self) -> Vec<String> {
        crate::helpers::genre_cleanup::clean_genres_global(self.get_raw_artist_genres())
    }

    /// Get all unique genres from albums and artist metadata combined, sorted alphabetically
    fn get_genres(&self) -> Vec<String> {
        crate::helpers::genre_cleanup::clean_genres_global(self.get_raw_genres())
    }

    /// Get albums filtered by genre (case-insensitive, cleanup applied to album genres before matching)
    fn get_albums_by_genre(&self, genre: &str) -> Vec<Album> {
        let genre_lower = genre.to_lowercase();
        self.get_albums()
            .into_iter()
            .filter(|a| {
                let cleaned = crate::helpers::genre_cleanup::clean_genres_global(a.genres.clone());
                cleaned.iter().any(|g| g.to_lowercase() == genre_lower)
            })
            .collect()
    }

    /// Get artists filtered by genre via their metadata (case-insensitive, cleanup applied)
    fn get_artists_by_genre(&self, genre: &str) -> Vec<Artist> {
        let genre_lower = genre.to_lowercase();
        self.get_artists()
            .into_iter()
            .filter(|a| {
                a.metadata.as_ref()
                    .map(|m| {
                        let cleaned = crate::helpers::genre_cleanup::clean_genres_global(m.genres.clone());
                        cleaned.iter().any(|g| g.to_lowercase() == genre_lower)
                    })
                    .unwrap_or(false)
            })
            .collect()
    }

    /// Get all unique categories (explicitly mapped genre labels) from albums and artist metadata
    ///
    /// Categories are only genres that have an explicit mapping configured.
    /// Genres without a mapping are excluded — use get_genres() for all cleaned genres.
    fn get_categories(&self) -> Vec<String> {
        crate::helpers::genre_cleanup::map_to_categories_global(self.get_raw_genres())
    }

    /// Get albums filtered by category (case-insensitive, explicit mappings only)
    fn get_albums_by_category(&self, category: &str) -> Vec<Album> {
        let cat_lower = category.to_lowercase();
        self.get_albums()
            .into_iter()
            .filter(|a| {
                let cats = crate::helpers::genre_cleanup::map_to_categories_global(a.genres.clone());
                cats.iter().any(|c| c.to_lowercase() == cat_lower)
            })
            .collect()
    }

    /// Get artists filtered by category via their metadata (case-insensitive, explicit mappings only)
    fn get_artists_by_category(&self, category: &str) -> Vec<Artist> {
        let cat_lower = category.to_lowercase();
        self.get_artists()
            .into_iter()
            .filter(|a| {
                a.metadata.as_ref()
                    .map(|m| {
                        let cats = crate::helpers::genre_cleanup::map_to_categories_global(m.genres.clone());
                        cats.iter().any(|c| c.to_lowercase() == cat_lower)
                    })
                    .unwrap_or(false)
            })
            .collect()
    }

    /// Allow downcasting to concrete types
    fn as_any(&self) -> &dyn std::any::Any;
    
    /// Get an image by identifier
    /// the identifier has no specific format, it can be used differently 
    /// depending on the library implementation
    /// returns a tuple of (image data, mime type)
    fn get_image(&self, identifier: String) -> Option<(Vec<u8>, String)>;
    
    /// Update artist metadata in background
    /// 
    /// This method should update the metadata for all artists in the library using
    /// background worker thread. The default implementation does nothing.
    fn update_artist_metadata(&self);

    /// Get a list of meta keys for the library
    /// 
    /// This method should return a list of meta keys that are available in the 
    /// library.
    /// The default implementation returns an empty vector.    
    fn get_meta_keys(&self) -> Vec<String> {
        vec![]
    }

    /// Get a specific metadata value as string
    /// 
    /// This method should return a specific metadata value for a given key.
    /// The default implementation returns None.
    fn get_metadata_value(&self, _key: &str) -> Option<String> {
        None
    }
    
    /// Get all metadata as a HashMap with JSON values
    /// 
    /// This method should return all metadata for the library as a HashMap with
    /// JSON values. The default implementation returns an empty HashMap.
    fn get_metadata(&self) -> Option<std::collections::HashMap<String, serde_json::Value>> {
        // Convert string metadata to JSON values
        let mut result = std::collections::HashMap::new();
        
        // Add each meta key to the result
        for key in self.get_meta_keys() {
            if let Some(value) = self.get_metadata_value(&key) {
                // Try to parse as JSON, fall back to string value
                match serde_json::from_str(&value) {
                    Ok(json_value) => {
                        result.insert(key, json_value);
                    },
                    Err(_) => {
                        // Use string value
                        result.insert(key, serde_json::Value::String(value));
                    }
                }
            }
        }
        
        if result.is_empty() {
            None
        } else {
            Some(result)
        }
    }
}