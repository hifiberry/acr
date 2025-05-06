use std::collections::HashMap;
use std::time::Instant;
use std::io::{Write, BufReader, BufRead};
use std::net::TcpStream;
use log::{debug, info, error};
use crate::data::{LibraryError, Track};
use super::library::MPDLibrary;

/// MPD library loader that can load a library from MPD
pub struct MPDLibraryLoader {
    /// MPD server hostname
    hostname: String,
    
    /// MPD server port
    port: u16,
}

impl MPDLibraryLoader {
    /// Create a new MPD library loader with specific connection details
    pub fn new(hostname: &str, port: u16) -> Self {
        debug!("Creating new MPDLibraryLoader with connection {}:{}", hostname, port);
        
        MPDLibraryLoader {
            hostname: hostname.to_string(),
            port,
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
    /// This extracts track information from a song including track name, number, disc, and artist
    /// and creates a properly structured Track object
    fn track_from_mpd_song(song: &mpd::Song, album_artist: Option<&str>) -> crate::data::Track {
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
            
        // Extract artist
        let artist = song.tags.iter()
            .find(|(tag, _)| tag == "Artist")
            .map(|(_, value)| value.clone());
            
        // Create Track object - if artist is present, use with_artist method,
        // otherwise use the basic constructor
        if let Some(artist) = artist {
            Track::with_artist(disc_number, track_number, track_name.to_string(), artist, album_artist)
        } else {
            Track::new(disc_number, track_number, track_name.to_string())
        }
    }
    
    /// Create an Album object from an MPD song
    /// 
    /// This extracts album information from a song including album name, artist, year
    /// and creates a properly structured Album object
    fn album_from_mpd_song(song: &mpd::Song) -> crate::data::Album {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        use std::sync::{Arc, Mutex};
        use crate::data::{Album, Track};
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
        
        // Extract year from date if available
        let year = song.tags.iter()
            .find(|(tag, _)| tag == "Date")
            .and_then(|(_, date_str)| {
                let year_part = date_str.split('-').next().unwrap_or(date_str);
                year_part.parse::<i32>().ok()
            });
            
        // Generate a unique ID for the album based on the album key
        let album_key = Self::album_key(song);
        let mut hasher = DefaultHasher::new();
        album_key.hash(&mut hasher);
        let album_id = hasher.finish();
        
        // Create an empty track list - typically you'd populate this later
        let tracks = Arc::new(Mutex::new(Vec::<Track>::new()));
        
        // Create artists list by splitting the album artist string using musicbrainz helper
        let artists = match musicbrainz::split_artist_names(&album_artist, false) {
            Some(split_artists) => Arc::new(Mutex::new(split_artists)),
            None => Arc::new(Mutex::new(vec![album_artist]))
        };

        info!("Album ID: {}, Name: {}, Artists: {:?}", album_id, album_name, artists.lock().unwrap());
        
        // Use the song file as the URI
        let uri = Some(song.file.clone());
        
        // Create album object
        Album {
            id: album_id,
            name: album_name.to_string(),
            artists,
            artists_flat: None,
            year,
            tracks,
            cover_art: None,
            uri,
        }
    }
    
    /// Load all album artists from the MPD server
    fn load_albumartists(&self) -> Result<Vec<String>, LibraryError> {
        debug!("Loading album artists from MPD server at {}:{}", self.hostname, self.port);
        let start_time = Instant::now();
        
        // Connect to the MPD server
        let addr = format!("{}:{}", self.hostname, self.port);
        let mut stream = TcpStream::connect(&addr)
            .map_err(|e| LibraryError::ConnectionError(format!("Failed to connect to MPD: {}", e)))?;
        
        // Send the "list albumartist" command
        if let Err(e) = stream.write_all(b"list albumartist\n") {
            return Err(LibraryError::ConnectionError(format!("Failed to send command: {}", e)));
        }
        
        // Read the response from MPD
        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        let mut albumartists = Vec::new();
        
        while let Ok(bytes) = reader.read_line(&mut line) {
            if bytes == 0 {
                break; // End of stream
            }
            
            let line_trimmed = line.trim();
            if line_trimmed == "OK" {
                // End of response
                break;
            }
            
            // Parse album artist entries
            if line_trimmed.starts_with("AlbumArtist: ") {
                let artist_name = line_trimmed[13..].to_string();
                if !artist_name.is_empty() {
                    albumartists.push(artist_name);
                }
            }
            
            line.clear();
        }
        
        let elapsed = start_time.elapsed();
        debug!("Loaded {} album artists in {:?}", albumartists.len(), elapsed);
        
        Ok(albumartists)
    }
    
    /// Load albums from MPD
    pub fn load_albums_from_mpd(&self) -> Result<Vec<crate::data::Album>, LibraryError> {
        info!("Loading MPD library from {}:{}", self.hostname, self.port);
        let start_time = Instant::now();
        
        // Step 1: Load all album artists
        let albumartists = self.load_albumartists()?;
        info!("Found {} album artists in MPD database", albumartists.len());

        // Step 2: Load all songs for each album artist
        let mut all_songs = Vec::new();
        for artist in &albumartists {
            debug!("Loading songs for album artist: {}", artist);
            
            // Fetch all songs for this artist
            let songs = self.fetch_all_songs_for_artist(artist)?;
            debug!("Found {} songs for album artist '{}'", songs.len(), artist);
            all_songs.extend(songs);
        }

        info!("Loaded {} songs in total", all_songs.len());

        // Step 3: Create album objects from songs
        // use a HashMap with album ID as key to avoid duplicates
        // This will also help in tracking the number of unique albums
        // and their associated tracks
        let mut albums_map: HashMap<String, crate::data::Album> = std::collections::HashMap::new();
        for song in &all_songs {
            // Create a unique key for the album based on song metadata
            let album_key = Self::album_key(song);

            // check if the album already exists in the map
            if ! albums_map.contains_key(&album_key) {
                // Create an album object from the song
                let album = Self::album_from_mpd_song(song);                
                // Insert into the map using the album ID as key
                albums_map.insert(album_key.clone(), album);
            }

            // create a track object from the song
            let track = Self::track_from_mpd_song(song, None);

            // Add the track to the album's track list, but only if the track is not already present
            if let Some(album) = albums_map.get_mut(&album_key) {
                // Check if the track is already present in the album's track list
                let mut tracks = album.tracks.lock().unwrap();
                if !tracks.iter().any(|t| t.name == track.name && t.disc_number == track.disc_number) {
                    tracks.push(track);
                }
            } else {
                error!("Album not found in map for key: {}", album_key);
            }
        }
        info!("Created {} unique albums from songs", albums_map.len());
        
        // Move albums from HashMap to vector without copying
        let mut albums = Vec::with_capacity(albums_map.len());
        for (_, album) in albums_map.drain() {
            albums.push(album);
        }
        
        let elapsed = start_time.elapsed();
        info!("Loaded {} albums in {:?}", albums.len(), elapsed);
        
        Ok(albums)
    }
    
    /// Fetch all songs for a specific artist
    pub fn fetch_all_songs_for_artist(&self, artist_name: &str) -> Result<Vec<mpd::Song>, LibraryError> {
        debug!("Fetching all songs for artist: {}", artist_name);
        
        // Connect to MPD server
        let addr = format!("{}:{}", self.hostname, self.port);
        let mut stream = TcpStream::connect(&addr)
            .map_err(|e| LibraryError::ConnectionError(format!("Failed to connect to MPD: {}", e)))?;
        
        // Create find command for this artist
        let cmd = format!("find artist \"{}\"\n", artist_name.replace("\"", "\\\""));
        
        if let Err(e) = stream.write_all(cmd.as_bytes()) {
            return Err(LibraryError::ConnectionError(format!("Failed to send command: {}", e)));
        }
        
        // Read response
        let mut reader = std::io::BufReader::new(stream);
        let mut line = String::new();
        let mut songs = Vec::new();
        
        // Current song being processed
        let mut current_file = None;
        let mut current_title = None;
        let mut current_artist = None;
        let mut current_album = None;
        let mut current_album_artist = None;
        let mut current_track = None;
        let mut current_date = None;
        let mut current_duration = None;
        
        // Process response line by line
        while let Ok(bytes) = reader.read_line(&mut line) {
            if bytes == 0 {
                break; // End of stream
            }
            
            let line_trimmed = line.trim();
            if line_trimmed == "OK" {
                // End of response
                break;
            }
            
            // Process each line based on field
            if line_trimmed.starts_with("file: ") {
                // New song entry, save previous if exists
                if let Some(file) = current_file.take() {
                    // Create a song object from gathered data
                    let mut song = mpd::Song::default();
                    song.file = file;
                    song.title = current_title;
                    
                    // Add tags
                    if let Some(artist) = current_artist {
                        song.tags.push(("Artist".to_string(), artist));
                    }
                    
                    if let Some(album) = current_album {
                        song.tags.push(("Album".to_string(), album));
                    }
                    
                    if let Some(album_artist) = current_album_artist {
                        song.tags.push(("AlbumArtist".to_string(), album_artist));
                    }
                    
                    if let Some(track) = current_track {
                        song.tags.push(("Track".to_string(), track));
                    }
                    
                    if let Some(date) = current_date {
                        song.tags.push(("Date".to_string(), date));
                    }
                    
                    if let Some(duration) = current_duration {
                        song.duration = Some(std::time::Duration::from_secs_f64(duration));
                    }
                    
                    songs.push(song);
                }
                
                // Start new song
                current_file = Some(line_trimmed[6..].to_string());
                current_title = None;
                current_artist = None;
                current_album = None;
                current_album_artist = None;
                current_track = None;
                current_date = None;
                current_duration = None;
            } else if line_trimmed.starts_with("Title: ") {
                current_title = Some(line_trimmed[7..].to_string());
            } else if line_trimmed.starts_with("Artist: ") {
                current_artist = Some(line_trimmed[8..].to_string());
            } else if line_trimmed.starts_with("Album: ") {
                current_album = Some(line_trimmed[7..].to_string());
            } else if line_trimmed.starts_with("AlbumArtist: ") {
                current_album_artist = Some(line_trimmed[13..].to_string());
            } else if line_trimmed.starts_with("Track: ") {
                current_track = Some(line_trimmed[7..].to_string());
            } else if line_trimmed.starts_with("Date: ") {
                current_date = Some(line_trimmed[6..].to_string());
            } else if line_trimmed.starts_with("duration: ") {
                if let Ok(dur) = line_trimmed[10..].parse::<f64>() {
                    current_duration = Some(dur);
                }
            }
            
            line.clear();
        }
        
        // Process the last song if there's one in progress
        if let Some(file) = current_file.take() {
            // Create a song object from gathered data
            let mut song = mpd::Song::default();
            song.file = file;
            song.title = current_title;
            
            // Add tags
            if let Some(artist) = current_artist {
                song.tags.push(("Artist".to_string(), artist));
            }
            
            if let Some(album) = current_album {
                song.tags.push(("Album".to_string(), album));
            }
            
            if let Some(album_artist) = current_album_artist {
                song.tags.push(("AlbumArtist".to_string(), album_artist));
            }
            
            if let Some(track) = current_track {
                song.tags.push(("Track".to_string(), track));
            }
            
            if let Some(date) = current_date {
                song.tags.push(("Date".to_string(), date));
            }
            
            if let Some(duration) = current_duration {
                song.duration = Some(std::time::Duration::from_secs_f64(duration));
            }
            
            songs.push(song);
        }
        
        debug!("Found {} songs for artist '{}'", songs.len(), artist_name);
        Ok(songs)
    }
    
}