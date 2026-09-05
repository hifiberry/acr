use std::collections::HashMap;
use std::error::Error;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use parking_lot::RwLock;
use acr_types::enrichment::{merge_genres, Applied, EnrichmentBatch};
use acr_types::ArtistMeta;
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
// Library Version Counter
//

/// The change counter behind the library ETags.
///
/// Lives here, not in `acr-types`: the metadata crate never touches it — it
/// hands back an `Option<String>` token in `EnrichmentBatch`/`Applied`, and
/// the bump happens on the player side (see `MPDLibrary::apply`). Nothing
/// outside this package constructs or reads one.
///
/// Cloning shares the counter: the library keeps one handle and hands clones to
/// the background updaters that mutate it, so a bump from any of them is visible
/// to the endpoints that serve the validator.
#[derive(Debug, Clone)]
pub struct LibraryVersion {
    /// Distinguishes this counter from every other one that has ever existed.
    ///
    /// Without it the counter resets to 0 on restart and climbs back through
    /// values it already issued, so a client could be told 304 for content that
    /// changed. This is a random half, defending across restarts, plus a
    /// process-monotonic sequence appended below, defending within a process:
    /// two independently constructed counters in one process cannot share a
    /// nonce even in the (already astronomically unlikely) case that the
    /// random halves collide.
    nonce: String,
    counter: Arc<AtomicU64>,
}

impl Default for LibraryVersion {
    fn default() -> Self {
        Self::new()
    }
}

/// Process-wide sequence handed out to each `LibraryVersion::new()` call, so
/// two counters constructed in the same process are guaranteed a distinct
/// nonce regardless of what `rand::random` returns. This only has to make
/// each call to `new()` observe a value no other call observes - it is not
/// read or written anywhere near the request-serving hot path that the
/// version counter itself is on - so ordering weaker than the counter's own
/// `SeqCst` is fine here.
static NONCE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

impl LibraryVersion {
    pub fn new() -> Self {
        let sequence = NONCE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        Self {
            nonce: format!("{:08x}-{:x}", rand::random::<u32>(), sequence),
            counter: Arc::new(AtomicU64::new(0)),
        }
    }

    /// The current value.
    pub fn get(&self) -> u64 {
        self.counter.load(Ordering::SeqCst)
    }

    /// Record that the library changed.
    ///
    /// Called as close to the mutation as the code allows. A mutation that does
    /// not bump serves stale data to every client holding a cached list, and no
    /// test can detect that, so proximity to the write is the only guard.
    pub fn bump(&self) {
        self.counter.fetch_add(1, Ordering::SeqCst);
    }

    /// The opaque validator for the library's current contents.
    ///
    /// Compare for equality only. It is not ordered and carries no arithmetic
    /// meaning; a caller that stripped the nonce to compare counters would
    /// reintroduce the stale-serve this design removes.
    pub fn token(&self) -> String {
        format!("{}-{}", self.nonce, self.get())
    }
}

//
// Enrichment merge
//

/// Merge one enrichment batch into a library's album and artist maps.
///
/// Every backend that implements `EnrichmentSink` calls this. The rules a
/// client can observe — what replaces what, what counts as a change, how an
/// album in the batch is found in a map keyed by name — live here once, so two
/// backends cannot drift into merging the same batch differently.
///
/// The caller keeps two decisions this function cannot make: the staleness
/// check against its own library version (only it knows whether it has one),
/// and what to do with the `bool` returned here, which says whether anything a
/// client can observe changed. A backend that tracks a version bumps it exactly
/// once when that is true, and never when it is false: a mutation that does not
/// bump serves stale lists, and a bump without a mutation invalidates every
/// client's cache for nothing.
///
/// Merging happens in place, under one write lock per map, which is what makes
/// a batch idempotent within itself: a repeated entry is compared against what
/// the earlier one already wrote, so it merges once and is counted once.
pub fn apply_batch(
    albums: &RwLock<HashMap<String, Album>>,
    artists: &RwLock<HashMap<String, Artist>>,
    batch: &EnrichmentBatch,
) -> (Applied, bool) {
    let mut applied = Applied::default();
    let mut changed = false;

    if !batch.albums.is_empty() {
        let mut albums = albums.write();
        // The map is keyed by album name, but a batch carries ids: a name is
        // not unique across artists and the metadata side never sees the key.
        // This is the lookup `albumupdater` did by id before the seam.
        //
        // Matching by id still has to happen without an O(n) scan per batch
        // entry: on a 10 000-album library, `values_mut().find(...)` per
        // incoming album turns a 50-entry batch into 500 000 allocating
        // comparisons under this write lock, ~200 times over a full sweep.
        // Building the id -> name index once per batch keeps the per-entry
        // cost at a hash lookup.
        let id_to_name: HashMap<String, String> = albums
            .iter()
            .map(|(name, album)| (album.id.to_string(), name.clone()))
            .collect();

        for incoming in &batch.albums {
            let Some(name) = id_to_name.get(&incoming.id) else {
                continue;
            };
            if let Some(album) = albums.get_mut(name) {
                if merge_genres(&mut album.genres, &incoming.genres) {
                    applied.albums += 1;
                    changed = true;
                }
            }
        }
    }

    if !batch.artists.is_empty() {
        let mut artists = artists.write();
        for incoming in &batch.artists {
            let Some(artist) = artists.get_mut(&incoming.name) else {
                continue;
            };
            // A summary that says several artists share this name and carries
            // nothing else is one whose metadata the lookup cleared: nothing
            // it found describes a single artist, so there is nothing to say
            // about one. Such an entry keeps no metadata at all rather than an
            // empty one, which is the `"metadata": null` the artist routes
            // have always served for it.
            if incoming.is_multi
                && incoming.mbid.is_empty()
                && incoming.genres.is_empty()
                && incoming.thumb_url.is_empty()
            {
                if artist.metadata.is_some() || !artist.is_multi {
                    applied.artists += 1;
                    changed = true;
                }
                artist.metadata = None;
                artist.is_multi = true;
                continue;
            }

            // `Artist`'s own `PartialEq` compares ids only — that is identity,
            // not content — so the change check names the fields a summary
            // carries. `metadata` appearing at all is one of them: a client
            // sees `null` become `{}`.
            let was_multi = artist.is_multi;
            let had_metadata = artist.metadata.is_some();
            let meta = artist.metadata.get_or_insert_with(ArtistMeta::new);
            let mbid_changed = meta.mbid != incoming.mbid;
            let genres_changed = meta.genres != incoming.genres;
            let thumbs_changed = meta.thumb_url != incoming.thumb_url;
            meta.mbid = incoming.mbid.clone();
            meta.genres = incoming.genres.clone();
            // Stored as given, empty included: the metadata side writes a
            // thumbnail URL only when a lookup found an image, so an empty
            // list is the answer "there is none" and the artist list route
            // serves it as such.
            meta.thumb_url = incoming.thumb_url.clone();
            artist.is_multi = incoming.is_multi;

            if !had_metadata
                || mbid_changed
                || genres_changed
                || thumbs_changed
                || was_multi != incoming.is_multi
            {
                applied.artists += 1;
                changed = true;
            }
        }
    }

    (applied, changed)
}

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

    /// Number of albums attributed to an artist.
    ///
    /// The default materialises the artist's albums and counts them. Backends
    /// that keep an album-artist index should override this: callers such as
    /// the artist list endpoint ask once per artist, so a default that walks
    /// the library each time makes that endpoint quadratic.
    fn album_count_for_artist(&self, artist_id: &Identifier) -> usize {
        self.get_albums_by_artist_id(artist_id).len()
    }
    
    /// Force an update of the library data in the underlying system
    ///
    /// This differs from refresh_library in that it asks the backend system
    /// to scan for new files or changes, rather than just refreshing our in-memory data.
    /// Returns true if the update was initiated successfully, false otherwise.
    fn force_update(&self) -> bool {
        // Default implementation does nothing and returns false
        false
    }

    /// Whether this library supports deleting albums and tracks from disk.
    /// Default is false; only backends with direct filesystem access should override.
    fn supports_delete(&self) -> bool {
        false
    }

    /// Delete an album and all its tracks from the underlying filesystem.
    /// A library refresh is triggered automatically on success.
    /// Returns Err if not supported or if deletion fails.
    fn delete_album(&self, album_id: &Identifier) -> Result<(), LibraryError> {
        let _ = album_id;
        Err(LibraryError::InternalError("Delete not supported by this library".to_string()))
    }

    /// Delete a single track by its URI (relative path like `Artist/Album/01.flac`).
    /// A library refresh is triggered automatically on success.
    /// Returns Err if not supported or if deletion fails.
    fn delete_track(&self, track_uri: &str) -> Result<(), LibraryError> {
        let _ = track_uri;
        Err(LibraryError::InternalError("Delete not supported by this library".to_string()))
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

    /// Get albums filtered by category (case-insensitive, explicit mappings only).
    /// Checks both album-level genre tags and artist metadata genres.
    fn get_albums_by_category(&self, category: &str) -> Vec<Album> {
        let cat_lower = category.to_lowercase();

        // Build a set of artist names whose metadata genres include this category
        let artist_matches: std::collections::HashSet<String> = self.get_artists()
            .into_iter()
            .filter(|a| {
                a.metadata.as_ref()
                    .map(|m| {
                        let cats = crate::helpers::genre_cleanup::map_to_categories_global(m.genres.clone());
                        cats.iter().any(|c| c.to_lowercase() == cat_lower)
                    })
                    .unwrap_or(false)
            })
            .map(|a| a.name.to_lowercase())
            .collect();

        self.get_albums()
            .into_iter()
            .filter(|a| {
                // Check album-level genre tags first
                let cats = crate::helpers::genre_cleanup::map_to_categories_global(a.genres.clone());
                if cats.iter().any(|c| c.to_lowercase() == cat_lower) {
                    return true;
                }
                // Fall back to artist metadata genres
                let artists = a.artists.lock();
                artists.iter().any(|name| artist_matches.contains(&name.to_lowercase()))
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
    
    /// An opaque token that changes whenever this library's contents change.
    ///
    /// `None` means the backend does not track changes. Callers must then emit
    /// no validator: claiming one a backend cannot honour is worse than
    /// omitting it.
    fn library_version(&self) -> Option<String> {
        None
    }

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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use parking_lot::Mutex;

    /// A library that knows nothing except which albums belong to one artist.
    /// It exists to pin down the *default* `album_count_for_artist`, which is
    /// what every backend that does not keep an album-artist index inherits.
    struct CountingLibrary {
        albums: Vec<Album>,
        calls: Mutex<usize>,
    }

    fn album(id: u64) -> Album {
        Album {
            id: Identifier::Numeric(id),
            name: format!("Album {}", id),
            artists: Arc::new(Mutex::new(Vec::new())),
            artists_flat: None,
            release_date: None,
            tracks: Arc::new(Mutex::new(Vec::new())),
            cover_art: None,
            uri: None,
            genres: Vec::new(),
        }
    }

    impl LibraryInterface for CountingLibrary {
        fn new() -> Self {
            CountingLibrary { albums: Vec::new(), calls: Mutex::new(0) }
        }
        fn is_loaded(&self) -> bool { true }
        fn refresh_library(&self) -> Result<(), LibraryError> { Ok(()) }
        fn get_albums(&self) -> Vec<Album> { self.albums.clone() }
        fn get_artists(&self) -> Vec<Artist> { Vec::new() }
        fn get_album_by_artist_and_name(&self, _artist: &str, _album: &str) -> Option<Album> { None }
        fn get_album_by_id(&self, _id: &Identifier) -> Option<Album> { None }
        fn get_artist_by_name(&self, _name: &str) -> Option<Artist> { None }
        fn get_albums_by_artist_id(&self, _artist_id: &Identifier) -> Vec<Album> {
            *self.calls.lock() += 1;
            self.albums.clone()
        }
        fn as_any(&self) -> &dyn std::any::Any { self }
        fn get_image(&self, _identifier: String) -> Option<(Vec<u8>, String)> { None }
    }

    /// A backend that does not override the method must keep the answer it gave
    /// before the method existed: the length of the album list for that artist.
    #[test]
    fn default_album_count_for_artist_falls_back_to_listing_the_albums() {
        let library = CountingLibrary {
            albums: vec![album(1), album(2), album(3)],
            calls: Mutex::new(0),
        };

        assert_eq!(library.album_count_for_artist(&Identifier::Numeric(7)), 3);
        assert_eq!(*library.calls.lock(), 1, "default should delegate exactly once");
    }

    #[test]
    fn default_album_count_for_artist_is_zero_without_albums() {
        let library = CountingLibrary { albums: Vec::new(), calls: Mutex::new(0) };

        assert_eq!(library.album_count_for_artist(&Identifier::Numeric(7)), 0);
    }

    fn maps(
        albums: Vec<Album>,
        artists: Vec<Artist>,
    ) -> (RwLock<HashMap<String, Album>>, RwLock<HashMap<String, Artist>>) {
        (
            RwLock::new(albums.into_iter().map(|a| (a.name.clone(), a)).collect()),
            RwLock::new(artists.into_iter().map(|a| (a.name.clone(), a)).collect()),
        )
    }

    fn artist(name: &str) -> Artist {
        Artist {
            id: Identifier::String(name.to_string()),
            name: name.to_string(),
            is_multi: false,
            metadata: None,
        }
    }

    /// A batch that repeats an entry must merge it once. The merge is by
    /// comparison against the map, and the map is written in place, so the
    /// second copy is compared against what the first one wrote.
    #[test]
    fn a_repeated_entry_in_one_batch_is_applied_once() {
        let (albums, artists) = maps(vec![album(1)], vec![]);
        let genres = acr_types::enrichment::AlbumGenres {
            id: "1".to_string(),
            genres: vec!["rock".to_string()],
        };
        let batch = EnrichmentBatch {
            library_version: None,
            artists: vec![],
            albums: vec![genres.clone(), genres],
        };

        let (applied, changed) = apply_batch(&albums, &artists, &batch);

        assert_eq!(applied.albums, 1, "the same genres twice is one change");
        assert!(changed);
        assert_eq!(albums.read()["Album 1"].genres, vec!["rock"]);
    }

    /// Nothing to merge must not report a change: a caller that tracks a
    /// version bumps on this bool, and a bump invalidates every client's cache.
    #[test]
    fn a_batch_that_changes_nothing_reports_no_change() {
        let mut existing = album(1);
        existing.genres = vec!["rock".to_string()];
        let (albums, artists) = maps(vec![existing], vec![]);

        let (applied, changed) = apply_batch(
            &albums,
            &artists,
            &EnrichmentBatch {
                library_version: None,
                artists: vec![],
                albums: vec![acr_types::enrichment::AlbumGenres {
                    id: "1".to_string(),
                    genres: vec!["rock".to_string()],
                }],
            },
        );

        assert_eq!(applied.albums, 0);
        assert!(!changed);
    }

    /// An id no longer in the library is skipped rather than inserted: the
    /// batch was computed against a list this library may since have reloaded.
    #[test]
    fn an_entry_for_something_the_library_does_not_have_is_ignored() {
        let (albums, artists) = maps(vec![album(1)], vec![artist("Bowie")]);

        let (applied, changed) = apply_batch(
            &albums,
            &artists,
            &EnrichmentBatch {
                library_version: None,
                artists: vec![acr_types::enrichment::ArtistSummary {
                    name: "Someone Else".to_string(),
                    mbid: vec!["x".to_string()],
                    ..Default::default()
                }],
                albums: vec![acr_types::enrichment::AlbumGenres {
                    id: "77".to_string(),
                    genres: vec!["rock".to_string()],
                }],
            },
        );

        assert_eq!((applied.albums, applied.artists), (0, 0));
        assert!(!changed);
        assert_eq!(artists.read().len(), 1, "nothing may be inserted");
    }

    /// `metadata` going from absent to present is visible in the JSON a client
    /// reads, so it counts as a change even when every field in it is empty.
    #[test]
    fn giving_an_artist_its_first_metadata_is_a_change() {
        let (albums, artists) = maps(vec![], vec![artist("Bowie")]);

        let (applied, changed) = apply_batch(
            &albums,
            &artists,
            &EnrichmentBatch {
                library_version: None,
                artists: vec![acr_types::enrichment::ArtistSummary {
                    name: "Bowie".to_string(),
                    ..Default::default()
                }],
                albums: vec![],
            },
        );

        assert_eq!(applied.artists, 1);
        assert!(changed);
        assert!(artists.read()["Bowie"].metadata.is_some());
    }

    /// A name that covers several artists keeps no metadata at all. The
    /// lookup clears it — nothing it found describes a single artist — and the
    /// artist routes serve that as `"metadata": null`, which is what a client
    /// reads to tell "several artists" from "one artist, nothing known".
    #[test]
    fn a_cleared_multi_artist_keeps_no_metadata() {
        let mut existing = artist("Simon & Garfunkel");
        existing.metadata = Some(ArtistMeta::new());
        let (albums, artists) = maps(vec![], vec![existing]);

        let (applied, changed) = apply_batch(
            &albums,
            &artists,
            &EnrichmentBatch {
                library_version: None,
                artists: vec![acr_types::enrichment::ArtistSummary {
                    name: "Simon & Garfunkel".to_string(),
                    is_multi: true,
                    ..Default::default()
                }],
                albums: vec![],
            },
        );

        assert_eq!(applied.artists, 1);
        assert!(changed);
        let artists = artists.read();
        assert!(artists["Simon & Garfunkel"].is_multi);
        assert!(
            artists["Simon & Garfunkel"].metadata.is_none(),
            "a cleared multi-artist serves null, not an empty object"
        );
    }

    /// Applying the same cleared multi-artist twice must not look like a
    /// second change: on MPD that would bump the version and invalidate every
    /// client's cached list for nothing.
    #[test]
    fn clearing_an_already_cleared_multi_artist_is_not_a_change() {
        let mut existing = artist("Simon & Garfunkel");
        existing.is_multi = true;
        let (albums, artists) = maps(vec![], vec![existing]);

        let (applied, changed) = apply_batch(
            &albums,
            &artists,
            &EnrichmentBatch {
                library_version: None,
                artists: vec![acr_types::enrichment::ArtistSummary {
                    name: "Simon & Garfunkel".to_string(),
                    is_multi: true,
                    ..Default::default()
                }],
                albums: vec![],
            },
        );

        assert_eq!(applied.artists, 0);
        assert!(!changed);
    }

    /// A multi-artist a lookup did find something for keeps what it found:
    /// only the empty case means "cleared".
    #[test]
    fn a_multi_artist_with_something_to_say_keeps_its_metadata() {
        let (albums, artists) = maps(vec![], vec![artist("Simon & Garfunkel")]);

        apply_batch(
            &albums,
            &artists,
            &EnrichmentBatch {
                library_version: None,
                artists: vec![acr_types::enrichment::ArtistSummary {
                    name: "Simon & Garfunkel".to_string(),
                    is_multi: true,
                    genres: vec!["folk".to_string()],
                    ..Default::default()
                }],
                albums: vec![],
            },
        );

        let artists = artists.read();
        let a = &artists["Simon & Garfunkel"];
        assert!(a.is_multi);
        assert_eq!(a.metadata.as_ref().unwrap().genres, vec!["folk"]);
    }

    /// A provider's own URL is stored exactly as it arrives: it is not the
    /// daemon's to rewrite, and the artist list route serves it verbatim.
    #[test]
    fn an_external_thumbnail_url_is_carried_unchanged() {
        let (albums, artists) = maps(vec![], vec![artist("Bowie")]);

        apply_batch(
            &albums,
            &artists,
            &EnrichmentBatch {
                library_version: None,
                artists: vec![acr_types::enrichment::ArtistSummary {
                    name: "Bowie".to_string(),
                    thumb_url: vec![
                        "/api/coverart/artist/YWJj/image".to_string(),
                        "https://example.com/artist.png".to_string(),
                    ],
                    ..Default::default()
                }],
                albums: vec![],
            },
        );

        let artists = artists.read();
        assert_eq!(
            artists["Bowie"].metadata.as_ref().unwrap().thumb_url,
            vec![
                "/api/coverart/artist/YWJj/image".to_string(),
                "https://example.com/artist.png".to_string(),
            ]
        );
    }

    /// An empty genre list never clears what a library already read from tags:
    /// the tags are better data than a lookup that found nothing.
    #[test]
    fn an_empty_genre_list_does_not_clear_existing_genres() {
        let mut existing = album(1);
        existing.genres = vec!["jazz".to_string()];
        let (albums, artists) = maps(vec![existing], vec![]);

        let (_, changed) = apply_batch(
            &albums,
            &artists,
            &EnrichmentBatch {
                library_version: None,
                artists: vec![],
                albums: vec![acr_types::enrichment::AlbumGenres {
                    id: "1".to_string(),
                    genres: vec![],
                }],
            },
        );

        assert!(!changed);
        assert_eq!(albums.read()["Album 1"].genres, vec!["jazz"]);
    }

    #[test]
    fn a_backend_that_does_not_opt_in_reports_no_version() {
        // The mock in this module does not override library_version.
        let lib = CountingLibrary::new();
        assert_eq!(lib.library_version(), None);
    }

    /// `apply_batch`'s album lookup must stay near-constant per entry: on a
    /// realistically large library, a per-entry `values_mut().find(...)`
    /// scan turns a handful of batch entries into hundreds of thousands of
    /// allocating string comparisons under the write lock. This runs the
    /// same shape a real sweep does (a large library, a batch that touches a
    /// scattered handful of it, repeated many times) and bounds the total
    /// time generously: an id-indexed lookup finishes this in well under a
    /// second even in an unoptimised test build, while a reintroduced
    /// per-entry scan does not.
    ///
    /// The bound is wall-clock, so the margins are worth recording. On an idle
    /// machine this finishes in about 0.4 s, and with every core saturated in
    /// about 0.6 s, against the 2 s bound; a reintroduced per-entry scan takes
    /// about 4 s. If it ever fails on a loaded build host, raising the bound is
    /// the wrong fix -- 2 s is only half the regression it exists to catch, so
    /// a higher bound stops catching it. Replace it with something
    /// scale-invariant instead: the ratio between two library sizes, or a
    /// direct count of comparisons, neither of which depends on how busy the
    /// machine is.
    #[test]
    fn apply_batch_stays_fast_against_a_large_library() {
        const ALBUM_COUNT: u64 = 20_000;
        const BATCH_SIZE: u64 = 50;
        const SWEEPS: u64 = 50;

        let all_albums: Vec<Album> = (0..ALBUM_COUNT).map(album).collect();
        let (albums, artists) = maps(all_albums, vec![]);

        let start = std::time::Instant::now();
        for sweep in 0..SWEEPS {
            let batch = EnrichmentBatch {
                library_version: None,
                artists: vec![],
                albums: (0..BATCH_SIZE)
                    .map(|i| {
                        // Spread the touched ids across the whole library
                        // rather than clustering them, so an id-based index
                        // is the only thing that can keep this fast.
                        let id = (sweep * 977 + i * 4001) % ALBUM_COUNT;
                        acr_types::enrichment::AlbumGenres {
                            id: id.to_string(),
                            genres: vec![format!("genre-{sweep}-{i}")],
                        }
                    })
                    .collect(),
            };
            apply_batch(&albums, &artists, &batch);
        }
        let elapsed = start.elapsed();

        assert!(
            elapsed < std::time::Duration::from_secs(2),
            "apply_batch took {elapsed:?} for {SWEEPS} batches of {BATCH_SIZE} \
             against a {ALBUM_COUNT}-album library; a per-entry linear scan \
             is the likely cause if this regresses",
        );
    }

    #[test]
    fn a_new_version_starts_at_zero() {
        assert_eq!(LibraryVersion::new().get(), 0);
    }

    #[test]
    fn bumping_increases_the_version() {
        let v = LibraryVersion::new();
        v.bump();
        assert_eq!(v.get(), 1);
        v.bump();
        assert_eq!(v.get(), 2);
    }

    #[test]
    fn clones_share_one_counter() {
        // The updaters hold clones; a bump through one must be visible through
        // the library's own handle, or the ETag would not move.
        let v = LibraryVersion::new();
        let handed_to_an_updater = v.clone();
        handed_to_an_updater.bump();
        assert_eq!(v.get(), 1);
    }

    #[test]
    fn the_token_combines_the_nonce_and_the_counter() {
        let v = LibraryVersion::new();
        let first = v.token();
        v.bump();
        let second = v.token();

        assert_ne!(first, second, "a bump must change the token");
        let (nonce_a, count_a) = first.rsplit_once('-').unwrap();
        let (nonce_b, count_b) = second.rsplit_once('-').unwrap();
        assert_eq!(nonce_a, nonce_b, "the nonce is stable within one counter");
        assert_eq!(count_a, "0");
        assert_eq!(count_b, "1");
    }

    #[test]
    fn two_counters_never_share_a_nonce() {
        // This is the property that makes a restart safe: a fresh counter must
        // not reissue a token an earlier one already handed out.
        let a = LibraryVersion::new();
        let b = LibraryVersion::new();
        assert_ne!(a.token(), b.token(), "a new counter must not reuse a token");
    }

    #[test]
    fn a_clone_reports_the_same_token() {
        let v = LibraryVersion::new();
        let handed_out = v.clone();
        handed_out.bump();
        assert_eq!(v.token(), handed_out.token());
    }
}
