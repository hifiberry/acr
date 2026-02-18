use std::mem;
use std::sync::Arc;
use parking_lot::Mutex;
use log::info;
use crate::data::{Album, Artist, Track};

/// Memory usage tracker to estimate memory used by library components
pub struct MemoryUsage {
    /// Total memory used by all artists (bytes)
    pub artists_memory: usize,
    /// Total memory used by all albums (bytes)
    pub albums_memory: usize,
    /// Total memory used by all tracks/songs (bytes)
    pub tracks_memory: usize,
    /// Count of artists
    pub artist_count: usize,
    /// Count of albums
    pub album_count: usize,
    /// Count of tracks
    pub track_count: usize,
    /// Count of album-artist mappings
    pub album_artists_count: usize,
    /// Other memory overhead (hashmaps, etc.)
    pub overhead_memory: usize,
}

impl MemoryUsage {
    /// Create a new empty memory usage tracker
    pub fn new() -> Self {
        MemoryUsage {
            artists_memory: 0,
            albums_memory: 0,
            tracks_memory: 0,
            artist_count: 0,
            album_count: 0,
            track_count: 0,
            album_artists_count: 0,
            overhead_memory: 0,
        }
    }
    
    /// Get total memory usage in bytes
    pub fn total(&self) -> usize {
        self.artists_memory + self.albums_memory + self.tracks_memory + self.overhead_memory
    }
    
    /// Format memory size in human-readable format
    pub fn format_size(size: usize) -> String {
        if size < 1024 {
            format!("{} bytes", size)
        } else if size < 1024 * 1024 {
            format!("{:.2} KB", size as f64 / 1024.0)
        } else if size < 1024 * 1024 * 1024 {
            format!("{:.2} MB", size as f64 / (1024.0 * 1024.0))
        } else {
            format!("{:.2} GB", size as f64 / (1024.0 * 1024.0 * 1024.0))
        }
    }
    
    /// Log memory usage statistics
    pub fn log_stats(&self) {
        info!("Memory usage statistics:");
        info!("  - Artists: {} entries using {}", 
              self.artist_count, Self::format_size(self.artists_memory));
        info!("  - Albums:  {} entries using {}", 
              self.album_count, Self::format_size(self.albums_memory));
        info!("  - Tracks:  {} entries using {}", 
              self.track_count, Self::format_size(self.tracks_memory));
        info!("  - Overhead: {}", Self::format_size(self.overhead_memory));
        info!("  - Total:    {}", Self::format_size(self.total()));
        
        if self.artist_count > 0 {
            info!("  - Average per artist: {}", 
                Self::format_size(self.artists_memory / self.artist_count));
        }
        
        if self.album_count > 0 {
            info!("  - Average per album: {}", 
                Self::format_size(self.albums_memory / self.album_count));
        }
        
        if self.track_count > 0 {
            info!("  - Average per track: {}", 
                Self::format_size(self.tracks_memory / self.track_count));
        }
    }
    
    /// Estimate the memory used by a string
    pub fn string_size(s: &Option<String>) -> usize {
        match s {
            Some(string) => {
                // String struct (3 words) + capacity on heap
                mem::size_of::<String>() + string.capacity()
            },
            None => 0
        }
    }
    
    /// Estimate the memory used by a vector of strings
    pub fn string_vec_size(strings: &[String]) -> usize {
        // Base size of the Vec
        let base_size = mem::size_of::<Vec<String>>();
        
        // Size of each string in the vector
        let strings_size = strings.iter().fold(0, |acc, s| {
            acc + mem::size_of::<String>() + s.capacity()
        });
        
        // We can't get the capacity from a slice, so we'll just add a small overhead
        // based on the length of the slice as an estimate
        let capacity_overhead = if !strings.is_empty() {
            // Assume a typical Vec reserves a bit more than its length
            (strings.len() / 4) * mem::size_of::<String>()
        } else {
            0
        };
        
        base_size + strings_size + capacity_overhead
    }
    
    /// Calculate memory used by an artist
    pub fn calculate_artist_memory(artist: &Artist) -> usize {
        // Base size of Artist struct
        let base_size = mem::size_of::<Artist>();
        
        // Size of artist id
        let id_size = mem::size_of::<u64>();
        
        // Size of artist name
        let name_size = artist.name.capacity();
        
        // Size of is_multi boolean
        let is_multi_size = mem::size_of::<bool>();
        
        // Size of metadata if present
        let metadata_size = match &artist.metadata {
            Some(meta) => {
                // Estimate size of metadata - this could be improved with more detailed calculation
                let mbid_size = meta.mbid.iter().fold(0, |acc, id| acc + id.capacity());
                
                // These are Vec<String>, not Option<String>, so calculate directly
                let thumb_url_size = meta.thumb_url.iter().fold(0, |acc, url| acc + url.capacity());
                let banner_url_size = meta.banner_url.iter().fold(0, |acc, url| acc + url.capacity());
                
                // Add potential biography size
                let biography_size = meta.biography.as_ref().map_or(0, |bio| bio.capacity());
                
                // Add genres size
                let genres_size = meta.genres.iter().fold(0, |acc, genre| acc + genre.capacity());
                
                mbid_size + thumb_url_size + banner_url_size + biography_size + genres_size + 
                    mem::size_of::<crate::data::metadata::ArtistMeta>()
            },
            None => 0
        };
        
        base_size + id_size + name_size + is_multi_size + metadata_size
    }
    
    /// Calculate memory used by an album
    pub fn calculate_album_memory(album: &Album) -> usize {
        // Base size of Album struct
        let base_size = mem::size_of::<Album>();
        
        // Size of album id
        let id_size = mem::size_of::<u64>();
        
        // Size of album name
        let name_size = album.name.capacity();
        
        // Size of artists (Arc<Mutex<Vec<String>>>)
        let artists_size = mem::size_of::<Arc<Mutex<Vec<String>>>>();
        
        // Access the artists to calculate their memory usage
        let artists_content_size = Self::string_vec_size(&album.artists.lock());
        
        // Size of artists_flat if present
        let artists_flat_size = match &album.artists_flat {
            Some(flat) => mem::size_of::<String>() + flat.capacity(),
            None => 0
        };
        
        // Size of release_date (NaiveDate)
        let release_date_size = if album.release_date.is_some() { mem::size_of::<chrono::NaiveDate>() } else { 0 };
        
        // Size of cover art URL if present
        let cover_art_size = Self::string_size(&album.cover_art);
        
        // Size of URI if present
        let uri_size = Self::string_size(&album.uri);
        
        // The size of the tracks is calculated separately with calculate_tracks_memory
        
        base_size + id_size + name_size + artists_size + artists_content_size + 
            artists_flat_size + release_date_size + cover_art_size + uri_size
    }
    
    /// Calculate memory used by tracks
    pub fn calculate_tracks_memory(tracks: &Arc<Mutex<Vec<Track>>>) -> usize {
        let mut size = 0;

        // Add base size of Arc and Mutex
        size += std::mem::size_of::<Arc<Mutex<Vec<Track>>>>();

        // Access the tracks to calculate their memory usage
        let tracks_guard = tracks.lock();
        // Add base size of Vec
        size += std::mem::size_of::<Vec<Track>>();

        // Add capacity overhead
        size += tracks_guard.capacity() * std::mem::size_of::<Track>();

        // Add size of each track's data
        for track in tracks_guard.iter() {
            // Optional String data for disc_number
            if let Some(disc_number) = &track.disc_number {
                size += mem::size_of::<String>() + disc_number.capacity();
            }

            // Optional track_number (u16)
            if track.track_number.is_some() {
                size += std::mem::size_of::<u16>();
            }

            // String data for name
            size += track.name.capacity();

            // Optional artist string data
            if let Some(artist) = &track.artist {
                size += mem::size_of::<String>() + artist.capacity();
            }

            // Optional URI string data
            if let Some(uri) = &track.uri {
                size += mem::size_of::<String>() + uri.capacity();
            }
        }
        
        size
    }
}