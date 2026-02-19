use std::collections::{HashMap, HashSet};
use serde::{Serialize, Deserialize};
use crate::data::{Album, Artist, Identifier};

/// Represents a many-to-many mapping between albums and artists
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlbumArtists {
    /// Maps album IDs to vectors of artist IDs
    album_to_artists: HashMap<Identifier, Vec<Identifier>>,
    
    /// Maps artist IDs to sets of album IDs
    artist_to_albums: HashMap<Identifier, HashSet<Identifier>>,
}

impl Default for AlbumArtists {
    fn default() -> Self {
        Self::new()
    }
}

impl AlbumArtists {
    /// Create a new empty AlbumArtists mapping
    pub fn new() -> Self {
        AlbumArtists {
            album_to_artists: HashMap::new(),
            artist_to_albums: HashMap::new(),
        }
    }
    
    /// Add a mapping between an album and an artist
    pub fn add_mapping(&mut self, album_id: Identifier, artist_id: Identifier) {
        // Add artist to album's vector
        self.album_to_artists
            .entry(album_id.clone())
            .or_default()
            .push(artist_id.clone());
            
        // Add album to artist's set
        self.artist_to_albums
            .entry(artist_id)
            .or_default()
            .insert(album_id);
    }
    
    // For backward compatibility with code that still uses u64
    pub fn add_mapping_u64(&mut self, album_id: u64, artist_id: u64) {
        self.add_mapping(Identifier::Numeric(album_id), Identifier::Numeric(artist_id));
    }
    
    /// Remove a mapping between an album and an artist
    pub fn remove_mapping(&mut self, album_id: &Identifier, artist_id: &Identifier) {
        // Remove artist from album's vector
        if let Some(artists) = self.album_to_artists.get_mut(album_id) {
            if let Some(pos) = artists.iter().position(|id| id == artist_id) {
                artists.remove(pos);
            }
            if artists.is_empty() {
                self.album_to_artists.remove(album_id);
            }
        }
        
        // Remove album from artist's set
        if let Some(albums) = self.artist_to_albums.get_mut(artist_id) {
            albums.remove(album_id);
            if albums.is_empty() {
                self.artist_to_albums.remove(artist_id);
            }
        }
    }
    
    // For backward compatibility with code that still uses u64
    pub fn remove_mapping_u64(&mut self, album_id: &u64, artist_id: &u64) {
        self.remove_mapping(&Identifier::Numeric(*album_id), &Identifier::Numeric(*artist_id));
    }
    
    /// Get all artist IDs associated with an album
    pub fn get_artists_for_album(&self, album_id: &Identifier) -> Vec<Identifier> {
        self.album_to_artists
            .get(album_id)
            .cloned()
            .unwrap_or_else(Vec::new)
    }
    
    // For backward compatibility with code that still uses u64
    pub fn get_artists_for_album_u64(&self, album_id: &u64) -> Vec<u64> {
        self.get_artists_for_album(&Identifier::Numeric(*album_id))
            .into_iter()
            .filter_map(|id| id.numeric())
            .collect()
    }
    
    /// Get all album IDs associated with an artist
    pub fn get_albums_for_artist(&self, artist_id: &Identifier) -> HashSet<Identifier> {
        self.artist_to_albums
            .get(artist_id)
            .cloned()
            .unwrap_or_else(HashSet::new)
    }
    
    // For backward compatibility with code that still uses u64
    pub fn get_albums_for_artist_u64(&self, artist_id: &u64) -> HashSet<u64> {
        self.get_albums_for_artist(&Identifier::Numeric(*artist_id))
            .into_iter()
            .filter_map(|id| id.numeric())
            .collect()
    }
    
    /// Check if an album-artist association exists
    pub fn has_mapping(&self, album_id: &Identifier, artist_id: &Identifier) -> bool {
        self.album_to_artists
            .get(album_id)
            .is_some_and(|artists| artists.contains(artist_id))
    }
    
    // For backward compatibility with code that still uses u64
    pub fn has_mapping_u64(&self, album_id: &u64, artist_id: &u64) -> bool {
        self.has_mapping(&Identifier::Numeric(*album_id), &Identifier::Numeric(*artist_id))
    }
    
    /// Build album-artist mappings from HashMap collections of Album and Artist
    pub fn build_from_hashmaps(albums: &HashMap<String, Album>, artists: &HashMap<String, Artist>) -> Self {
        let mut mapping = Self::new();
        
        // Process all albums and their artists
        for album in albums.values() {
            // Get the artists for this album
            let artist_names = album.artists.lock();
            // Process each artist name in the vector
            for artist_name in artist_names.iter() {
                if let Some(artist) = artists.get(artist_name) {
                    mapping.add_mapping(album.id.clone(), artist.id.clone());
                }
            }
        }
        
        mapping
    }

    /// Build album-artist mappings from existing Album and Artist collections
    pub fn build_from_collections(albums: &[Album], artists: &[Artist]) -> Self {
        let mut mapping = Self::new();
        
        // Create a lookup map for artist names to IDs
        let mut artist_name_to_id = HashMap::new();
        for artist in artists {
            artist_name_to_id.insert(&artist.name, artist.id.clone());
        }
        
        // Process all albums and their artists
        for album in albums {
            // Get the artists for this album
            let artist_names = album.artists.lock();
            // Process each artist name in the vector
            for name in artist_names.iter() {
                if let Some(artist_id) = artist_name_to_id.get(name) {
                    mapping.add_mapping(album.id.clone(), artist_id.clone());
                }
            }
        }
        
        mapping
    }
    
    /// Get total number of album-artist mappings
    pub fn count(&self) -> usize {
        self.album_to_artists
            .values()
            .fold(0, |acc, artists| acc + artists.len())
    }
    
    /// Get the memory usage of this mapping
    pub fn memory_usage(&self) -> usize {
        // Base size of the struct
        let base_size = std::mem::size_of::<Self>();
        
        // Size of album_to_artists HashMap
        let album_map_size = std::mem::size_of::<HashMap<Identifier, Vec<Identifier>>>();
        let album_entries_size = self.album_to_artists.len() * std::mem::size_of::<(Identifier, Vec<Identifier>)>();
        let album_vecs_size = self.album_to_artists
            .values()
            .fold(0, |acc, vec| acc + std::mem::size_of::<Vec<Identifier>>() + vec.len() * std::mem::size_of::<Identifier>());
        
        // Size of artist_to_albums HashMap
        let artist_map_size = std::mem::size_of::<HashMap<Identifier, HashSet<Identifier>>>();
        let artist_entries_size = self.artist_to_albums.len() * std::mem::size_of::<(Identifier, HashSet<Identifier>)>();
        let artist_sets_size = self.artist_to_albums
            .values()
            .fold(0, |acc, set| acc + std::mem::size_of::<HashSet<Identifier>>() + set.len() * std::mem::size_of::<Identifier>());
        
        base_size + album_map_size + album_entries_size + album_vecs_size + 
            artist_map_size + artist_entries_size + artist_sets_size
    }
    
    /// Clear all album-artist mappings
    pub fn clear(&mut self) {
        self.album_to_artists.clear();
        self.artist_to_albums.clear();
    }
    
    /// Get total number of mappings (alias for count method)
    pub fn len(&self) -> usize {
        self.count()
    }
}