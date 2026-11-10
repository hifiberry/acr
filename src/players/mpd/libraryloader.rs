use std::collections::HashMap;
use std::time::Instant;
use std::sync::Arc;
use log::{debug, info, warn};
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
        let track_name = song.title.as_deref()
            .unwrap_or_else(|| {
                // Fall back to filename if title is missing
                song.file.split('/').next_back().unwrap_or("Unknown Track")
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
        
        // Split the album-artist string on separators alone, and keep the
        // string itself.
        //
        // A separator split is wrong in both directions for a name that
        // contains one: "Emerson, Lake & Palmer" becomes three artists and
        // "Alpha and Beta" stays one. It used to be right immediately, at the
        // cost of a MusicBrainz round trip per album on a load that can cover
        // 200,000 songs; the correction now arrives with the enrichment sweep
        // instead (`data::library::apply_splits`), and `artists_flat` is what
        // it finds the album by -- splitting drops the separators, so the list
        // alone cannot be turned back into the name it came from.
        let artists = match crate::audiocontrol::resolver::split_album_artist(&album_artist, custom_separators) {
            Some(split_artists) => Arc::new(Mutex::new(split_artists)),
            None => Arc::new(Mutex::new(vec![album_artist.clone()]))
        };

        debug!("Album ID: {}, Name: {}, Artists: {:?}", album_id, album_name, artists.lock());

        // Extract genres from the Genre tag (MPD may report multiple Genre tags per song)
        let genres: Vec<String> = song.tags.iter()
            .filter(|(tag, _)| tag == "Genre")
            .map(|(_, value)| value.clone())
            .collect();

        // Create album object with new Identifier enum
        Album {
            id: Identifier::Numeric(album_id),
            name: album_name.to_string(),
            artists,
            artists_flat: Some(album_artist),
            release_date,
            tracks,
            cover_art: None,
            uri: None,
            genres,
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

        debug!("Database loading progress: {:.1}%", progress);

        // Step 2: Load each artist's songs and fold them into albums at once.
        //
        // The songs are not collected first. An `mpd::Song` owns every tag as
        // its own pair of strings, so a whole library of them is millions of
        // small allocations alive together, several times the size of what
        // is built from them -- and freeing them afterwards leaves the heap
        // fragmented at that high-water mark for the life of the process.
        // Converting one artist's songs before fetching the next keeps the
        // peak at the library itself plus one artist.
        let mut albums_map: HashMap<String, crate::data::Album> = HashMap::new();
        let mut total_songs = 0usize;
        let total_artists = artists.len();

        for (artist_index, artist) in artists.iter().enumerate() {
            // more verbose logging for "real" artists
            if artist.contains(",") {
                debug!("Loading songs for artist: {}", artist);
            } else {
                info!("Loading songs for artist: {}", artist);
            }

            // Fetch all songs for this artist
            let songs = match self.fetch_all_songs_for_artist(artist) {
                Ok(songs) => songs,
                Err(e) => {
                    let _ = complete_job(&load_job_id);
                    return Err(e);
                }
            };
            debug!("Found {} songs for album artist '{}'", songs.len(), artist);

            total_songs += songs.len();
            for song in &songs {
                add_song_to_albums(&mut albums_map, song, custom_separators.as_deref());
            }

            // Report every PROGRESS_UPDATE_FREQUENCY artists and on the last.
            if artist_index % PROGRESS_UPDATE_FREQUENCY == 0 || artist_index + 1 == total_artists {
                // Range 10-90%
                progress = (10.0 + 80.0 * (artist_index + 1) as f32 / total_artists as f32).min(90.0);

                let artist_progress = format!("Loading songs for artist {}/{}: {}",
                    artist_index + 1, total_artists, artist);
                let _ = update_job(&load_job_id, Some(artist_progress), Some(artist_index + 1), Some(total_artists));

                let last = songs.last();
                let album_name = last.and_then(|song| {
                    song.tags.iter().find(|(tag, _)| tag == "Album").map(|(_, value)| value.clone())
                });
                let song_name = last.and_then(|song| song.title.clone());
                self.controller.notify_database_update(Some(artist.clone()), album_name, song_name, Some(progress));

                debug!("Database loading progress: {:.1}%", progress);
            }
        }

        // Complete the data loading job
        if let Err(e) = complete_job(&load_job_id) {
            warn!("Failed to complete data loading job {}: {}", load_job_id, e);
        }

        // Register background job for the finishing pass
        if let Err(e) = register_job(process_job_id.clone(), "MPD Process Songs".to_string()) {
            warn!("Failed to register background job for MPD song processing: {}", e);
        }

        info!("Loaded {} songs in total", total_songs);

        info!("Created {} unique albums from songs", albums_map.len());
        
        // Move albums from HashMap to vector.
        //
        // An album with no genres from its file tags used to ask an injected
        // enricher for whatever the genre cache already held. Over HTTP that
        // question was answered `None` by contract -- it runs once per album
        // while the library loads and may not do network I/O -- so it has been
        // dead in every deployed build since the seam became HTTP, and asking
        // at all was the main daemon calling the metadata daemon. Genres for an
        // album that has none arrive in the enrichment batch, which is the
        // direction the seam allows.
        let mut albums = Vec::with_capacity(albums_map.len());
        for (_, album) in albums_map.drain() {
            // Sort the tracks by disc and track number before adding to the result
            album.sort_tracks();
            // The list was grown one push at a time and is final now; give
            // back the spare capacity that growth left on every album.
            album.tracks.lock().shrink_to_fit();
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
        let songs = client.find(query, None)
            .map_err(|e| LibraryError::ConnectionError(format!("Failed to find songs for artist '{}': {}", artist_name, e)))?;
            
        debug!("Found {} songs for artist '{}'", songs.len(), artist_name);
        Ok(songs)
    }
    
}
/// Fold one song into the albums built so far: create its album on first
/// sight, add its track unless that file is already there, and merge its
/// genres into the album.
fn add_song_to_albums(
    albums_map: &mut HashMap<String, crate::data::Album>,
    song: &mpd::Song,
    custom_separators: Option<&[String]>,
) {
    let album = albums_map
        .entry(MPDLibraryLoader::album_key(song))
        .or_insert_with(|| MPDLibraryLoader::album_from_mpd_song(song, custom_separators));

    let track = MPDLibraryLoader::track_from_mpd_song(song);
    {
        let mut tracks = album.tracks.lock();
        if !tracks.iter().any(|t| is_same_track(t, &track)) {
            tracks.push(track);
        }
    }

    for genre in song.tags.iter()
        .filter(|(tag, _)| tag == "Genre")
        .map(|(_, v)| v.as_str())
    {
        if !album.genres.iter().any(|g| g == genre) {
            album.genres.push(genre.to_string());
        }
    }
}

/// Whether two tracks loaded for one album are the same file.
///
/// A song with several artists comes back once per artist, so the loader sees
/// it repeatedly. The file is what identifies it: two tracks that merely share
/// a title ("Interlude", "Untitled") on one disc are different tracks, and
/// comparing by title dropped the second. Title and disc decide only when a
/// URI is missing.
fn is_same_track(a: &crate::data::Track, b: &crate::data::Track) -> bool {
    match (&a.uri, &b.uri) {
        (Some(a_uri), Some(b_uri)) => a_uri == b_uri,
        _ => a.name == b.name && a.disc_number == b.disc_number,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn song(file: &str, tags: &[(&str, &str)]) -> mpd::Song {
        mpd::Song {
            file: file.to_string(),
            tags: tags.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
            ..Default::default()
        }
    }

    /// A song with two artists comes back once per artist; a compilation
    /// spreads one album over several artists' batches. Folding songs in one
    /// at a time has to give the same albums as seeing them all together.
    #[test]
    fn songs_fold_into_albums_whatever_batch_they_arrive_in() {
        let tags = |title: &'static str, genre: &'static str| {
            vec![("Album", "Shared"), ("AlbumArtist", "Various"), ("Title", title), ("Genre", genre)]
        };
        let one = song("shared/01.flac", &tags("One", "Rock"));
        let two = song("shared/02.flac", &tags("Two", "Pop"));
        let other = song("other/01.flac", &[("Album", "Other"), ("Artist", "Solo"), ("Title", "One")]);

        let mut albums = HashMap::new();
        // First artist's batch, then a second batch repeating a song.
        for s in [&one, &other] {
            add_song_to_albums(&mut albums, s, None);
        }
        for s in [&one, &two] {
            add_song_to_albums(&mut albums, s, None);
        }

        assert_eq!(albums.len(), 2);
        let shared = albums.values().find(|a| a.name == "Shared").expect("the shared album");
        let uris: Vec<_> = shared.tracks.lock().iter().filter_map(|t| t.uri.clone()).collect();
        assert_eq!(uris, vec!["shared/01.flac", "shared/02.flac"]);
        assert_eq!(shared.genres, vec!["Rock", "Pop"]);
        let solo = albums.values().find(|a| a.name == "Other").expect("the other album");
        assert_eq!(solo.tracks.lock().len(), 1);
    }

    #[test]
    fn two_tracks_sharing_a_title_on_one_disc_are_both_kept() {
        let first = MPDLibraryLoader::track_from_mpd_song(&song("a/02 Interlude.flac", &[("Title", "Interlude")]));
        let second = MPDLibraryLoader::track_from_mpd_song(&song("a/07 Interlude.flac", &[("Title", "Interlude")]));

        assert!(!is_same_track(&first, &second));
    }

    #[test]
    fn the_same_file_seen_twice_is_one_track() {
        let first = MPDLibraryLoader::track_from_mpd_song(&song("a/01.flac", &[("Title", "One")]));
        let again = MPDLibraryLoader::track_from_mpd_song(&song("a/01.flac", &[("Title", "One")]));

        assert!(is_same_track(&first, &again));
    }

    #[test]
    fn release_dates_parse_at_every_precision_mpd_reports() {
        use chrono::Datelike;

        let full = MPDLibraryLoader::parse_release_date("2020-05-17").expect("full date");
        assert_eq!((full.year(), full.month(), full.day()), (2020, 5, 17));

        let year = MPDLibraryLoader::parse_release_date("1999").expect("year only");
        assert_eq!(year.year(), 1999);

        assert!(MPDLibraryLoader::parse_release_date("").is_none());
        assert!(MPDLibraryLoader::parse_release_date("soon").is_none());

        let month = MPDLibraryLoader::parse_release_date("2020-05").expect("year and month");
        assert_eq!((month.year(), month.month(), month.day()), (2020, 5, 1));

        // An impossible month or day still names a year.
        let fallback = MPDLibraryLoader::parse_release_date("2020-13-45").expect("year fallback");
        assert_eq!((fallback.year(), fallback.month(), fallback.day()), (2020, 1, 1));
    }
}
