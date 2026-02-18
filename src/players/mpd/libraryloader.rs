use std::collections::HashMap;
use std::time::Instant;
use std::sync::Arc;
use log::{debug, info, error, warn};
use chrono::NaiveDate;
use crate::data::LibraryError;
use crate::players::mpd::mpd::MPDPlayerController;
use crate::helpers::backgroundjobs::{register_job, update_job, complete_job};

/// Number of songs to process before updating progress
const PROGRESS_UPDATE_FREQUENCY: usize = 100;

/// MPD library loader that can load a library from MPD
pub struct MPDLibraryLoader {
    /// MPD server hostname
    hostname: String,
    
    /// MPD server port
    port: u16,
    
    /// Reference to the MPDPlayerController that owns this library
    controller: Arc<MPDPlayerController>,
}

impl MPDLibraryLoader {
    /// Create a new MPD library loader with specific connection details
    pub fn new(hostname: &str, port: u16, controller: Arc<MPDPlayerController>) -> Self {
        debug!("Creating new MPDLibraryLoader with connection {}:{}", hostname, port);
        
        MPDLibraryLoader {
            hostname: hostname.to_string(),
            port,
            controller,
        }
    }

    /// Create a unique key for an album based on song metadata
    /// 
    /// This combines album name, album artist, and date to create a consistent key
    /// that identifies unique albums even if they have the same name
    fn album_key(song: &mpd::Song) -> String {
        // Extract album name (default to "Unknown Album" if not present)
        let album = song.tags.iter()
            .find(|(tag, _)| tag == "Album")
            .map(|(_, value)| value.as_str())
            .unwrap_or("Unknown Album");
            
        // Extract album artist (default to artist or "Unknown Artist" if not present)
        let album_artist = if let Some((_, value)) = song.tags.iter()
            .find(|(tag, _)| tag == "AlbumArtist") {
            value.as_str()
        } else if let Some((_, value)) = song.tags.iter()
            .find(|(tag, _)| tag == "Artist") {
            value.as_str()
        } else {
            "Unknown Artist"
        };
            
        // Extract date (default to empty string if not present)
        let date = song.tags.iter()
            .find(|(tag, _)| tag == "Date")
            .map(|(_, value)| value.as_str())
            .unwrap_or("");
            
        // Combine the three parts with | separator
        format!("{}|{}|{}", album, album_artist, date)
    }

    /// Create a Track object from an MPD song
    /// 
    /// This extracts track information from a song including track name, number, disc, artist, and uri
    /// and creates a properly structured Track object
    fn track_from_mpd_song(song: &mpd::Song) -> crate::data::Track {
        use crate::data::Track;
        
        // Extract track title (default to filename if not present)
        let track_name = song.title.as_ref()
            .map(|title| title.as_str())
            .unwrap_or_else(|| {
                // Fall back to filename if title is missing
                song.file.split('/').last().unwrap_or("Unknown Track")
            });
            
        // Extract track number (default to 0 if not present)
        let track_number = song.tags.iter()
            .find(|(tag, _)| tag == "Track")
            .and_then(|(_, value)| {
                // Handle track numbers in format "1" or "1/10"
                value.split('/').next().and_then(|num| num.parse::<u16>().ok())
            })
            .unwrap_or(0);
            
        // Extract disc number (default to "1" if not present)
        let disc_number = song.tags.iter()
            .find(|(tag, _)| tag == "Disc")
            .map(|(_, value)| value.as_str())
            .unwrap_or("1").to_string();
            
        // First check song.artist, then fall back to tags if not present
        let track_artist = if let Some(artist) = &song.artist {
            Some(artist.clone())
        } else {
            song.tags.iter()
                .find(|(tag, _)| tag == "Artist")
                .map(|(_, value)| value.clone())
        };

        // Extract album artist from tags as well, don't use artist from song
        let album_artist: Option<String> = song.tags.iter()
            .find(|(tag, _)| tag == "AlbumArtist")
            .map(|(_, value)| value.clone());
        
        // Get the file URI from the song
        let uri = song.file.clone();
        
        // Create Track object with appropriate fields
        let track = if let Some(artist) = track_artist {
            // Convert Option<String> to Option<&str> by mapping with as_str() or using as_deref()
            let album_artist_ref = album_artist.as_deref();
            Track::with_artist(
                Some(disc_number), 
                Some(track_number), 
                track_name.to_string(), 
                artist, 
                album_artist_ref
            )
        } else {
            Track::new(Some(disc_number), Some(track_number), track_name.to_string())
        };
        
        // Add URI to the track and return it
        track.with_uri(uri)
    }
    
    /// Create an Album object from an MPD song
    /// 
    /// This extracts album information from a song including album name, artist, release date
    /// and creates a properly structured Album object
    fn album_from_mpd_song(song: &mpd::Song, custom_separators: Option<&[String]>) -> crate::data::Album {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        use std::sync::Arc;
        use parking_lot::Mutex;
        use crate::data::{Album, Track, Identifier};
        use crate::helpers::musicbrainz;
        
        // Extract album name (default to "Unknown Album" if not present)
        let album_name = song.tags.iter()
            .find(|(tag, _)| tag == "Album")
            .map(|(_, value)| value.as_str())
            .unwrap_or("Unknown Album");
            
        // Extract album artist (default to artist or "Unknown Artist" if not present)
        let album_artist = if let Some((_, value)) = song.tags.iter()
            .find(|(tag, _)| tag == "AlbumArtist") {
            value.clone()
        } else if let Some((_, value)) = song.tags.iter()
            .find(|(tag, _)| tag == "Artist") {
            value.clone()
        } else {
            "Unknown Artist".to_string()
        };
        
        // Extract date from tags and convert to NaiveDate
        let release_date = song.tags.iter()
            .find(|(tag, _)| tag == "Date")
            .and_then(|(_, date_str)| {
                // Try to parse the date string in various formats
                Self::parse_release_date(date_str)
            });
        
        // Generate a unique ID for the album based on the album key
        let album_key = Self::album_key(song);
        let mut hasher = DefaultHasher::new();
        album_key.hash(&mut hasher);
        let album_id = hasher.finish();
        
        // Create an empty track list - typically you'd populate this later
        let tracks = Arc::new(Mutex::new(Vec::<Track>::new()));
        
        // Create artists list by splitting the album artist string using musicbrainz helper with custom separators
        let artists = match musicbrainz::split_artist_names(&album_artist, false, custom_separators) {
            Some(split_artists) => Arc::new(Mutex::new(split_artists)),
            None => Arc::new(Mutex::new(vec![album_artist]))
        };

        debug!("Album ID: {}, Name: {}, Artists: {:?}", album_id, album_name, artists.lock());

        
        // Create album object with new Identifier enum
        Album {
            id: Identifier::Numeric(album_id),
            name: album_name.to_string(),
            artists,
            artists_flat: None,
            release_date,
            tracks,
            cover_art: None,
            uri: None
        }
    }
    
    /// Parse a date string into a NaiveDate
    /// 
    /// Attempts to parse various date formats including:
    /// - Full ISO dates (YYYY-MM-DD)
    /// - Partial dates (YYYY-MM)
    /// - Year only (YYYY)
    /// 
    /// If only the year is known, it will use January 1st of that year
    fn parse_release_date(date_str: &str) -> Option<NaiveDate> {
        // Try full ISO date format (YYYY-MM-DD)
        if let Ok(date) = NaiveDate::parse_from_str(date_str, "%Y-%m-%d") {
            return Some(date);
        }
        
        // Try year-month format (YYYY-MM)
        if let Ok(date) = NaiveDate::parse_from_str(&format!("{}-01", date_str), "%Y-%m-%d") {
            return Some(date);
        }
        
        // Try to extract just the year part (YYYY)
        let year_part = date_str.split('-').next().unwrap_or(date_str);
        if let Ok(year) = year_part.parse::<i32>() {
            // Use January 1st for the date when only the year is known
            if let Some(date) = NaiveDate::from_ymd_opt(year, 1, 1) {
                return Some(date);
            } else {
                warn!("Invalid year in date string: {}", date_str);
            }
        }
        
        // Could not parse the date string
        debug!("Could not parse release date from: {}", date_str);
        None
    }
    
    /// Load all album artists from the MPD server
    fn load_artists(&self) -> Result<Vec<String>, LibraryError> {
        debug!("Loading album artists from MPD server at {}:{}", self.hostname, self.port);
        let start_time = Instant::now();
        
        // Create a fresh MPD client using the MPD crate
        let conn_string = format!("{}:{}", self.hostname, self.port);
        let mut client = mpd::Client::connect(&conn_string)
            .map_err(|e| LibraryError::ConnectionError(format!("Failed to connect to MPD: {}", e)))?;
        
        // Use the list command to get all artists
        // Convert the string to Cow<str> using .into() as required by the MPD crate
        let artists = client.list(&mpd::Term::Tag("Artist".into()), &mpd::Query::new())
            .map_err(|e| LibraryError::ConnectionError(format!("Failed to list artists from MPD: {}", e)))?;

        info!("MPD list command returned {} artists", artists.len());
        
        // Collect all artist names
        let mut albumartists = Vec::with_capacity(artists.len());
        for artist in artists {
            albumartists.push(artist);
        }
        
        let elapsed = start_time.elapsed();
        info!("Loaded {} album artists in {:?}", albumartists.len(), elapsed);
        
        Ok(albumartists)
    }
    
    /// Load albums from MPD
    pub fn load_albums_from_mpd(&self, custom_separators: Option<Vec<String>>) -> Result<Vec<crate::data::Album>, LibraryError> {
        // Use separate job IDs for loading data and processing songs
        let load_job_id = "mpd_load_data".to_string();
        let process_job_id = "mpd_process_songs".to_string();
        
        // Register background job for data loading
        if let Err(e) = register_job(load_job_id.clone(), "MPD Load Data".to_string()) {
            warn!("Failed to register background job for MPD data loading: {}", e);
        }
        
        // progress indicator (f32 0.0..100.0)
        let mut progress: f32 = 0.0;
        self.controller.notify_database_update(Some("Starting MPD database import".to_string()), None, None, Some(progress)); 

        info!("Loading MPD library from {}:{}", self.hostname, self.port);
        let start_time = Instant::now();
        
        // Step 1: Load all artists
        let artists = match self.load_artists() {
            Ok(artists) => artists,
            Err(e) => {
                let _ = complete_job(&load_job_id);
                return Err(e);
            }
        };
        
        info!("Found {} artists in MPD database", artists.len());
        progress = 10.0; // Update progress to 10%
        
        // Update background job progress
        let _ = update_job(&load_job_id, Some("Loading artists".to_string()), None, Some(artists.len()));
        
        // Send database update event to show initial progress
        // Note: We no longer need to pass the source parameter
        self.controller.notify_database_update(Some("Loading artists".to_string()), None, None, Some(progress));

        info!("Sent notify - this is INFO level");
        warn!("This is WARN level for testing");
        error!("This is ERROR level for testing");
        
        debug!("Database loading progress: {:.1}%", progress);

        // Step 2: Load all songs for each album artist
        let mut all_songs = Vec::new();
        for (artist_index, artist) in artists.iter().enumerate() {
            // more verbose logging for "real" artists
            if artist.contains(",") {
                debug!("Loading songs for artist: {}", artist);
            } else {
                info!("Loading songs for artist: {}", artist);
            }
            
            // Update background job progress for artist processing
            let artist_progress = format!("Loading songs for artist {}/{}: {}", 
                artist_index + 1, artists.len(), artist);
            let _ = update_job(&load_job_id, Some(artist_progress), Some(artist_index + 1), Some(artists.len()));
            
            // Fetch all songs for this artist
            let songs = match self.fetch_all_songs_for_artist(artist) {
                Ok(songs) => songs,
                Err(e) => {
                    let _ = complete_job(&load_job_id);
                    return Err(e);
                }
            };
            debug!("Found {} songs for album artist '{}'", songs.len(), artist);
            all_songs.extend(songs);
        }
        progress = 20.0; // Update progress to 20%
        
        // Complete the data loading job
        if let Err(e) = complete_job(&load_job_id) {
            warn!("Failed to complete data loading job {}: {}", load_job_id, e);
        }
        
        // Register background job for song processing
        if let Err(e) = register_job(process_job_id.clone(), "MPD Process Songs".to_string()) {
            warn!("Failed to register background job for MPD song processing: {}", e);
        }
        
        // Update background job for song processing phase
        let _ = update_job(&process_job_id, Some("Processing songs".to_string()), None, Some(all_songs.len()));
        
        // Send database update event to show progress
        self.controller.notify_database_update(Some("Processing songs".to_string()), None, None, Some(progress));
        
        debug!("Database loading progress: {:.1}%", progress);

        info!("Loaded {} songs in total", all_songs.len());

        // Step 3: Create album objects from songs
        // use a HashMap with album ID as key to avoid duplicates
        // This will also help in tracking the number of unique albums
        // and their associated tracks
        let mut albums_map: HashMap<String, crate::data::Album> = std::collections::HashMap::new();
        let total_songs = all_songs.len();
        let songs_per_progress_point = (90.0 - 20.0) / (total_songs as f32);
        
        for (index, song) in all_songs.iter().enumerate() {
            // Create a unique key for the album based on song metadata
            let album_key = Self::album_key(song);

            // check if the album already exists in the map
            if !albums_map.contains_key(&album_key) {
                // Create an album object from the song, using custom separators if provided
                let album = Self::album_from_mpd_song(song, custom_separators.as_deref());
                // Insert into the map using the album ID as key
                albums_map.insert(album_key.clone(), album);
            }

            // create a track object from the song
            let track = Self::track_from_mpd_song(song);

            // Add the track to the album's track list, but only if the track is not already present
            if let Some(album) = albums_map.get_mut(&album_key) {
                // Check if the track is already present in the album's track list
                let mut tracks = album.tracks.lock();
                if !tracks.iter().any(|t| t.name == track.name && t.disc_number == track.disc_number) {
                    tracks.push(track);
                }
            } else {
                error!("Album not found in map for key: {}", album_key);
            }
            
            // Update progress every PROGRESS_UPDATE_FREQUENCY songs or on the last song
            if index % PROGRESS_UPDATE_FREQUENCY == 0 || index == total_songs - 1 {
                // Calculate progress (range 20-90%)
                progress = 20.0 + (index as f32 * songs_per_progress_point);
                progress = progress.min(90.0); // Cap at 90%
                
                debug!("Album processing progress: {:.1}% ({}/{} songs)", progress, index + 1, total_songs);
                
                // Get album and artist names for the current song
                let album_name = song.tags.iter()
                    .find(|(tag, _)| tag == "Album")
                    .map(|(_, value)| value.as_str())
                    .unwrap_or("Unknown Album").to_string();
                
                let artist_name = song.tags.iter()
                    .find(|(tag, _)| tag == "Artist")
                    .map(|(_, value)| value.as_str())
                    .unwrap_or("Unknown Artist").to_string();
                
                let song_name = song.title.as_ref()
                    .map(|s| s.as_str())
                    .unwrap_or("Unknown Song").to_string();
                
                // Update background job with current song processing
                let song_progress = format!("Processing song {}/{}: {} - {}", 
                    index + 1, total_songs, artist_name, song_name);
                let _ = update_job(&process_job_id, Some(song_progress), Some(index + 1), Some(total_songs));
                
                // Send database update event with current item details
                self.controller.notify_database_update(Some(artist_name), Some(album_name), Some(song_name), Some(progress));
                
                debug!("Database loading progress: {:.1}%", progress);
            }
        }
        
        info!("Created {} unique albums from songs", albums_map.len());
        
        // Move albums from HashMap to vector without copying
        let mut albums = Vec::with_capacity(albums_map.len());
        for (_, album) in albums_map.drain() {
            // Sort the tracks by disc and track number before adding to the result
            album.sort_tracks();
            albums.push(album);
        }
        
        // Final progress update (99%)
        progress = 99.0;
        
        // Update background job with final status
        let final_progress = format!("Library load complete: {} albums created", albums.len());
        let _ = update_job(&process_job_id, Some(final_progress), Some(albums.len()), Some(albums.len()));
        
        // Send the final database update event
        self.controller.notify_database_update(Some("Library load complete".to_string()), None, None, Some(progress));
        
        debug!("Database loading progress: {:.1}%", progress);
        
        let elapsed = start_time.elapsed();
        info!("Loaded {} albums in {:?}", albums.len(), elapsed);
        
        // Complete the song processing background job
        if let Err(e) = complete_job(&process_job_id) {
            warn!("Failed to complete song processing job {}: {}", process_job_id, e);
        }
        
        Ok(albums)
    }
    
    /// Fetch all songs for a specific artist
    pub fn fetch_all_songs_for_artist(&self, artist_name: &str) -> Result<Vec<mpd::Song>, LibraryError> {
        debug!("Fetching all songs for artist: {}", artist_name);
        
        // Create a new MPD client connection
        let conn_string = format!("{}:{}", self.hostname, self.port);
        let mut client = mpd::Client::connect(&conn_string)
            .map_err(|e| LibraryError::ConnectionError(format!("Failed to connect to MPD: {}", e)))?;
        
        // Use the MPD find command to get all songs by this artist
        // Create a query for artist = artist_name using a proper binding
        // to prevent the temporary value from being dropped
        let mut query_obj = mpd::Query::new();
        let query = query_obj.and(
            mpd::Term::Tag("Artist".into()),
            artist_name
        );
        
        // Pass None for Window parameter to satisfy the Into<Window> trait
        let songs = client.find(&query, None)
            .map_err(|e| LibraryError::ConnectionError(format!("Failed to find songs for artist '{}': {}", artist_name, e)))?;
            
        debug!("Found {} songs for artist '{}'", songs.len(), artist_name);
        Ok(songs)
    }
    
}