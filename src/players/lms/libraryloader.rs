use std::collections::HashMap;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};
use log::{debug, error, info, warn};
use serde_json::Value;
use chrono::NaiveDate;
use crate::data::{Album, Track, Identifier, LibraryError};
use crate::players::lms::jsonrps::LmsRpcClient;
use crate::helpers::musicbrainz;

/// Number of items to fetch in a single request
const BATCH_SIZE: u32 = 1000;

/// LMS library loader that can load a library from LMS
pub struct LMSLibraryLoader {
    /// Client for communicating with the LMS server
    client: Arc<LmsRpcClient>,
}

impl LMSLibraryLoader {
    /// Create a new LMS library loader with a specific client
    pub fn new(client: Arc<LmsRpcClient>) -> Self {
        debug!("Creating new LMSLibraryLoader");
        
        LMSLibraryLoader {
            client,
        }
    }

    /// Create a unique key for an album based on album info
    /// 
    /// This combines album name, album artist, and year to create a consistent key
    /// that identifies unique albums even if they have the same name
    fn album_key(album: &serde_json::Value) -> String {
        // Extract album title (default to "Unknown Album" if not present)
        let title = album["title"].as_str()
            .unwrap_or("Unknown Album");
            
        // Extract album artist (default to "Unknown Artist" if not present)
        let album_artist = album["artist"].as_str()
            .unwrap_or("Unknown Artist");
            
        // Extract year (default to empty string if not present)
        let year = album["year"].as_u64()
            .map(|y| y.to_string())
            .unwrap_or_default();
            
        // Combine the three parts with | separator
        format!("{}|{}|{}", title, album_artist, year)
    }

    /// Create a Track object from an LMS track JSON object
    fn track_from_lms_json(track: &serde_json::Value) -> Option<Track> {        // extract track id
        let id = track["id"].as_u64();

        debug!("Track ID: {:?}", id);

        // Extract track title (default to "Unknown Track" if not present)
        let title = track["title"].as_str()
            .unwrap_or("Unknown Track");
        
        // Extract track number (default to 0 if not present)
        let track_number = track["tracknum"].as_u64()
            .unwrap_or(0) as u16;
        
        // Extract disc number (default to 1 if not present)
        let disc_number = track["disc"].as_u64()
            .map(|d| d.to_string())
            .unwrap_or_else(|| "1".to_string());
        
        // Extract artist
        let artist = track["artist"].as_str()
            .map(|s| s.to_string());
        
        // Extract album artist (album.artist property)
        let album_artist = track["albumartist"].as_str()
            .or_else(|| track["album_artist"].as_str())
            .map(|s| s.to_string());
        
        // Get the file URI
        let uri = track["url"].as_str()
            .or_else(|| track["file"].as_str())
            .map(|s| s.to_string());
        
        if artist.is_none() && uri.is_none() {
            // Skip tracks without minimal information
            warn!("Skipping track '{}' with insufficient metadata", title);
            return None;
        }
        
        // Create Track object with appropriate fields
        let mut track_obj = if let Some(artist_name) = artist {
            let album_artist_ref = album_artist.as_deref();
            Track::with_artist(
                Some(disc_number), 
                Some(track_number), 
                title.to_string(), 
                artist_name, 
                album_artist_ref
            )
        } else {
            Track::new(Some(disc_number), Some(track_number), title.to_string())
        };
          // Add URI if available
        if let Some(uri_str) = uri {
            track_obj = track_obj.with_uri(uri_str);
        }
        
        // Add track ID if available
        if let Some(track_id) = id {
            track_obj = track_obj.with_id(Identifier::Numeric(track_id));
        }
        
        // Return the created track
        Some(track_obj)
    }
    
    /// Parse a date string into a NaiveDate
    /// 
    /// Attempts to parse a year value or various date formats
    fn parse_release_date(date_value: Option<&Value>) -> Option<NaiveDate> {
        // Handle the case where date is not present
        let date_str = match date_value {
            Some(Value::String(s)) => s,
            Some(Value::Number(n)) if n.is_u64() => return NaiveDate::from_ymd_opt(n.as_u64().unwrap() as i32, 1, 1),
            _ => return None,
        };

        // Try full ISO date format (YYYY-MM-DD)
        if let Ok(date) = NaiveDate::parse_from_str(date_str, "%Y-%m-%d") {
            return Some(date);
        }
        
        // Try year-month format (YYYY-MM)
        if let Ok(date) = NaiveDate::parse_from_str(&format!("{}-01", date_str), "%Y-%m-%d") {
            return Some(date);
        }
        
        // Try just the year
        if let Ok(year) = date_str.parse::<i32>() {
            return NaiveDate::from_ymd_opt(year, 1, 1);
        }
        
        debug!("Could not parse release date from: {:?}", date_str);
        None
    }
      /// Create an Album object from an LMS album JSON object
    fn album_from_lms_json(&self, album_json: &serde_json::Value, custom_separators: Option<&[String]>) -> Option<Album> {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        use std::sync::Arc;
        use parking_lot::Mutex;
        
        // Extract album title
        let title = album_json["album"].as_str();
        if title.is_none() || title.unwrap().trim().is_empty() {
            error!("Skipping album with missing title, might be a bug");
            return None;
        }
        let title = title.unwrap();
        
        // Extract album artist
        let album_artist = album_json["artist"].as_str();
        if album_artist.is_none() || album_artist.unwrap().trim().is_empty() {
            error!("Skipping album '{}' with missing artist, might be a bug", title);
            return None;
        }
        let album_artist = album_artist.unwrap();
        
        // Extract album ID
        let album_id = album_json["id"].as_u64()
            .or_else(|| {
                // If no ID is provided, generate one from the album key
                let key = Self::album_key(album_json);
                let mut hasher = DefaultHasher::new();
                key.hash(&mut hasher);
                Some(hasher.finish())
            })
            .unwrap_or(0);
          // Extract release date
        let release_date = Self::parse_release_date(album_json.get("year"));
        
        // Create empty tracks list to be populated later
        let tracks = Arc::new(Mutex::new(Vec::<Track>::new()));
        
        // Create artists list by splitting the album artist string
        let artists = match musicbrainz::split_artist_names(album_artist, false, custom_separators) {
            Some(split_artists) => Arc::new(Mutex::new(split_artists)),
            None => Arc::new(Mutex::new(vec![album_artist.to_string()]))
        };
          debug!("Created album: {} (ID: {}) by {:?}", 
               title, album_id, artists.lock());
        
        // Create and return the Album object
        Some(Album {
            id: Identifier::Numeric(album_id),
            name: title.to_string(),
            artists,
            artists_flat: None,
            release_date,
            tracks,
            cover_art: None,
            uri: None // LMS doesn't provide album URIs
        })
    }

    /// Load all albums from LMS
    pub fn load_albums_from_lms(&self, custom_separators: Option<Vec<String>>) -> Result<Vec<Album>, LibraryError> {
        info!("Loading LMS library");
        let start_time = Instant::now();
        
        // Use a map to store albums by ID to avoid duplicates
        let mut albums_map: HashMap<u64, Album> = HashMap::new();
        let mut start = 0;

        let mut track_count: u32 = 0;
        
        loop {
            // Fetch all albums directly using the 'albums' command
            debug!("Fetching albums batch starting at index {}", start);
            let result = match self.client.database_request(
                "albums", 
                start, 
                BATCH_SIZE, 
                vec![("tags", "aylD")]  // Include year, artist, title, and duration
            ) {
                Ok(res) => res,
                Err(e) => return Err(LibraryError::ConnectionError(format!(
                    "Failed to fetch albums from LMS: {}", e)))
            };

            debug!("Fetched albums starting at index {}", start);

            // Extract albums from the response
            let albums_array = match result.get("albums_loop") {
                Some(Value::Array(arr)) => arr.clone(),
                _ => {
                    warn!("No albums_loop found in response or not an array");
                    break;
                }
            };
            
            if albums_array.is_empty() {
                debug!("No more albums to fetch");
                break;
            }
            
            debug!("Processing {} albums from current batch", albums_array.len());
            
            // Process albums
            for album_json in &albums_array { // Changed to borrow the array instead of moving it
                // Create album object from JSON
                if let Some(album) = self.album_from_lms_json(album_json, custom_separators.as_deref()) {
                    if let Identifier::Numeric(id) = album.id {
                        // Extract album ID string for track fetching
                        let album_id_str = id.to_string();
                        
                        // Now fetch tracks for this album
                        // Note: Since this is a non-async implementation, we can't use the async method
                        // So we'll implement the tracks fetching directly here
                        let mut track_start = 0;
                        let mut album_tracks = Vec::new();
                        
                        loop {
                            // Fetch batch of tracks for this album
                            let tracks_result = match self.client.database_request(
                                "tracks", 
                                track_start, 
                                BATCH_SIZE, 
                                vec![("tags", "acdltfi"), ("album_id", album_id_str.as_str())]
                            ) {
                                Ok(res) => res,
                                Err(e) => {
                                    warn!("Failed to fetch tracks for album ID '{}': {}", album_id_str, e);
                                    break;
                                }
                            };
                            
                            // Extract tracks from the response
                            let tracks_array = match tracks_result.get("titles_loop") {
                                Some(Value::Array(arr)) => arr,
                                _ => break // No more tracks or invalid response
                            };
                            
                            if tracks_array.is_empty() {
                                break; // No more tracks to fetch
                            }
                            
                            // Process each track
                            for track_json in tracks_array {
                                if let Some(track) = Self::track_from_lms_json(track_json) {
                                    album_tracks.push(track);
                                    track_count += 1;
                                }
                            }
                            
                            // Update start index for next batch
                            track_start += BATCH_SIZE;
                            
                            // If we received fewer than BATCH_SIZE tracks, we've reached the end
                            if tracks_array.len() < BATCH_SIZE as usize {
                                break;
                            }
                        }
                        
                        // Add the tracks to the album
                        {
                            let mut album_tracks_lock = album.tracks.lock();
                            *album_tracks_lock = album_tracks;
                        }
                        
                        // Add album to map
                        albums_map.insert(id, album);
                    }
                }
            }

            debug!("Tracks in DB: {}", track_count);
            
            // Update start index for next batch
            start += BATCH_SIZE;
            
            // If we received fewer than BATCH_SIZE albums, we've reached the end
            if albums_array.len() < BATCH_SIZE as usize {
                break;
            }
        }

        info!("Loaded {} albums with {} tracks", albums_map.len(), track_count);
        thread::sleep(Duration::from_secs(10));
            

        // Convert HashMap to Vec
        let mut albums = Vec::with_capacity(albums_map.len());
        for (_, album) in albums_map {
            // Sort the album tracks by disc and track number
            album.sort_tracks();
            albums.push(album);
        }
        
        let elapsed = start_time.elapsed();
        info!("Loaded {} albums in {:?}", albums.len(), elapsed);
        
        Ok(albums)
    }
}