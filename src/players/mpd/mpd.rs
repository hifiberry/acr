use crate::players::player_controller::{BasePlayerController, PlayerController};
use crate::data::{PlayerCapability, PlayerCapabilitySet, Song, LoopMode, PlaybackState, PlayerCommand, PlayerState, Track};
use crate::data::library::LibraryInterface;
use crate::constants::API_PREFIX;
use crate::helpers::retry::RetryHandler;
use crate::helpers::url_encoding;
use crate::helpers::songsplitmanager::SongSplitManager;
use crate::helpers::attributecache;
use crate::helpers::backgroundjobs::BackgroundJobs;
use delegate::delegate;
use std::sync::Arc;
use parking_lot::Mutex;
use std::fs;
use std::io::{BufRead, BufReader};
use log::{debug, info, warn, error, trace};
use mpd::{Client, error::Error as MpdError, idle::Subsystem};
use mpd::Idle; // Add the Idle trait import
use std::net::TcpStream;
use std::thread;
use std::time::Duration;
use std::sync::atomic::{AtomicBool, Ordering};
use std::collections::HashMap;
use std::any::Any;
use once_cell::sync::Lazy;

/// Constant for MPD image API URL prefix including API prefix
pub fn mpd_image_url() -> String {
    format!("{}/library/mpd/image", API_PREFIX)
}

/// Check whether an MPD song URI refers to a remote stream rather than a file
/// in the local library. For streams MPD reports the stream URL as the song's
/// "file", and neither seeking nor album art retrieval is possible for those.
pub fn is_stream_uri(uri: &str) -> bool {
    uri.contains("://")
}

/// Where cover art offered by a URL's cached metadata came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UrlCoverArtOrigin {
    /// `logo_url`: a station's logo, never artwork for a track.
    StationLogo,
    /// `coverart_url`: artwork for the URL. For a radio stream that is still
    /// the station's; for a track a client queued it is the track's own.
    CoverArt,
}

/// Pick the cover art a URL's cached metadata offers, and say where it came
/// from. This is the only cover art available for a stream.
fn cover_art_from_metadata(
    metadata: &HashMap<String, serde_json::Value>,
) -> Option<(String, UrlCoverArtOrigin)> {
    let from = |key: &str, origin: UrlCoverArtOrigin| {
        metadata
            .get(key)
            .and_then(|value| value.as_str())
            .filter(|url| !url.is_empty())
            .map(|url| (url.to_string(), origin))
    };

    from("coverart_url", UrlCoverArtOrigin::CoverArt)
        .or_else(|| from("logo_url", UrlCoverArtOrigin::StationLogo))
}

/// Whether cover art from a URL's cached metadata is only a placeholder for
/// whatever is playing from that URL right now, rather than its artwork.
///
/// The cache holds metadata per URL, and a single writer fills it: the
/// `add_track` API. A radio station is queued with its own name and logo and
/// then plays a different song every few minutes, so its image describes the
/// URL, not the track. A client that queues one track supplies artwork for
/// that track, and it must not be thrown away.
///
/// The two are told apart by the name: metadata naming something other than
/// the track now playing describes the URL. Metadata carrying no name at all
/// is treated as the track's own, which is the safe answer -- it leaves the
/// image alone.
fn cover_art_is_placeholder(
    origin: UrlCoverArtOrigin,
    metadata: &HashMap<String, serde_json::Value>,
    song: &Song,
) -> bool {
    if origin == UrlCoverArtOrigin::StationLogo {
        return true;
    }

    match metadata.get("title").and_then(|value| value.as_str()) {
        // The URL carries a name. The artwork describes the track only if that
        // name is the track's.
        Some(url_name) => !names_match(url_name, song),
        // No name to compare against. For a stream the artwork is the
        // station's -- one URL, a different song every few minutes -- and
        // nothing obliges whoever queued it to have supplied a name or to have
        // used logo_url. For anything else it belongs to the one track it was
        // queued with, and is left alone.
        None => song
            .stream_url
            .as_deref()
            .map(is_stream_uri)
            .unwrap_or(false),
    }
}

/// Whether a name from a URL's metadata names the track that is playing.
///
/// Compared without surrounding space and without regard to case, neither of
/// which is a difference of identity. `convert_mpd_song` also splits an
/// artist-less "Artist - Title" before this point, so the combined form counts
/// as the same name: otherwise a client that queued its track under that form
/// would see its own artwork marked replaceable over a difference this code
/// had made itself.
fn names_match(url_name: &str, song: &Song) -> bool {
    let normalise = |value: &str| value.trim().to_lowercase();

    let name = normalise(url_name);
    let title = match song.title.as_deref() {
        Some(title) => normalise(title),
        None => return false,
    };

    if name == title {
        return true;
    }

    match song.artist.as_deref().map(normalise) {
        Some(artist) => {
            name == format!("{} - {}", artist, title) || name == format!("{} - {}", title, artist)
        }
        None => false,
    }
}

/// A stream has no cover art in the library, so fall back to the station logo
/// from the URL metadata. Without this the song would reach the clients with no
/// usable cover art at all.
fn apply_station_logo(song: &mut Song) {
    if song.cover_art_url.is_some() {
        return;
    }
    let Some((url, origin)) = cover_art_from_metadata(&song.metadata) else {
        return;
    };

    let placeholder = cover_art_is_placeholder(origin, &song.metadata, song);
    debug!(
        "Using cover art from URL metadata: {} (placeholder: {})",
        url, placeholder
    );
    song.cover_art_url = Some(url);

    if placeholder {
        song.metadata.insert(
            crate::data::song::COVER_ART_SOURCE.to_string(),
            serde_json::Value::String(
                crate::data::song::COVER_ART_SOURCE_STATION_LOGO.to_string(),
            ),
        );
    }
}

/// Derive the loop mode that MPD's `repeat` and `single` flags represent.
///
/// MPD has no single "loop mode" setting: `repeat` says whether playback
/// repeats at all, and `single` says whether it repeats one song or the whole
/// queue. `single` alone, without `repeat`, stops after the current song and
/// is not a loop.
fn loop_mode_from_mpd(repeat: bool, single: bool) -> LoopMode {
    match (repeat, single) {
        (false, _) => LoopMode::None,
        (true, true) => LoopMode::Track,
        (true, false) => LoopMode::Playlist,
    }
}

/// What changed between two observations of MPD's option flags. `None` means
/// the value did not move and no event is owed for it.
#[derive(Debug, Default, PartialEq, Eq)]
struct OptionsTransition {
    shuffle: Option<bool>,
    loop_mode: Option<LoopMode>,
}

impl OptionsTransition {
    /// Whether anything at all changed.
    fn is_empty(&self) -> bool {
        self.shuffle.is_none() && self.loop_mode.is_none()
    }
}

/// Last-known values of the MPD options we emit events for.
///
/// `Subsystem::Options` fires for any option change, `consume` and crossfade
/// included, so the controller has to diff against what it last saw rather
/// than emit on every event.
#[derive(Debug, Default)]
struct OptionsSnapshot {
    /// `(random, repeat, single)` as of the last observation, or `None` before
    /// the first one.
    seen: Option<(bool, bool, bool)>,
}

impl OptionsSnapshot {
    /// Record an observation and report what changed since the previous one.
    ///
    /// The first observation after a connect establishes the baseline and
    /// reports nothing: a reconnect must not look like someone toggling a
    /// setting.
    fn observe(&mut self, random: bool, repeat: bool, single: bool) -> OptionsTransition {
        let previous = self.seen.replace((random, repeat, single));

        let Some((was_random, was_repeat, was_single)) = previous else {
            return OptionsTransition::default();
        };

        let mode = loop_mode_from_mpd(repeat, single);
        let was_mode = loop_mode_from_mpd(was_repeat, was_single);

        OptionsTransition {
            shuffle: (random != was_random).then_some(random),
            loop_mode: (mode != was_mode).then_some(mode),
        }
    }
}

/// MPD player controller implementation
pub struct MPDPlayerController {
    /// Base controller for managing state listeners
    base: BasePlayerController,
    
    /// MPD server hostname
    hostname: String,
    
    /// MPD server port
    port: u16,
    
    // current player state
    current_state: Arc<Mutex<PlayerState>>,

    /// Current audio stream format (from MPD status `audio`)
    current_stream_details: Arc<Mutex<Option<crate::data::stream_details::StreamDetails>>>,
    
    /// Whether to load the MPD library into memory
    load_mpd_library: bool,
    
    /// Flag to control metadata enhancement
    enhance_metadata: bool,
    
    /// Flag to control cover art extraction from music files
    extract_coverart: bool,
    
    /// Custom artist separators for splitting artist names
    artist_separators: Option<Vec<String>>,
    
    /// MPD music directory path (if empty, will attempt to read from /etc/mpd.conf)
    music_directory: String,

    /// If true, the library is read-only and deletion is not supported
    library_read_only: bool,
    
    /// Cached effective music directory (to avoid parsing /etc/mpd.conf repeatedly)
    effective_music_directory: Arc<Mutex<Option<String>>>,
    
    /// MPD library instance wrapped in Arc and Mutex for thread-safe access
    library: Arc<Mutex<Option<crate::players::mpd::library::MPDLibrary>>>,
    
    /// Maximum number of reconnection attempts before giving up
    max_reconnect_attempts: u32,
    
    /// Current reconnection attempt counter
    reconnect_attempts: Arc<Mutex<u32>>,
    
    /// Flag indicating if connection has been permanently disabled due to max attempts
    connection_disabled: Arc<AtomicBool>,
    
    /// Song title splitter manager for radio stations that combine artist and song in title
    song_split_manager: SongSplitManager,
    
    /// Current MPD database update job ID (if any)
    current_update_job_id: Arc<Mutex<Option<String>>>,

    /// Last-known MPD option flags, so `Subsystem::Options` events emit only
    /// on an actual transition
    option_snapshot: Arc<Mutex<OptionsSnapshot>>,
}

// Manually implement Clone for MPDPlayerController
impl Clone for MPDPlayerController {
    fn clone(&self) -> Self {
        MPDPlayerController {
            // Share the BasePlayerController instance to maintain listener registrations
            base: self.base.clone(),
            hostname: self.hostname.clone(),
            port: self.port,
            current_state: Arc::clone(&self.current_state),
            current_stream_details: Arc::clone(&self.current_stream_details),
            load_mpd_library: self.load_mpd_library,
            enhance_metadata: self.enhance_metadata,
            extract_coverart: self.extract_coverart,
            artist_separators: self.artist_separators.clone(),
            music_directory: self.music_directory.clone(),
            effective_music_directory: Arc::clone(&self.effective_music_directory),
            library: Arc::clone(&self.library),
            max_reconnect_attempts: self.max_reconnect_attempts,
            reconnect_attempts: Arc::clone(&self.reconnect_attempts),
            connection_disabled: Arc::clone(&self.connection_disabled),
            song_split_manager: self.song_split_manager.clone(),
            current_update_job_id: Arc::clone(&self.current_update_job_id),
            option_snapshot: Arc::clone(&self.option_snapshot),
            library_read_only: self.library_read_only,
        }
    }
}

impl Default for MPDPlayerController {
    fn default() -> Self {
        Self::new()
    }
}

impl MPDPlayerController {
    /// Create a new MPD player controller with default settings
    pub fn new() -> Self {
        debug!("Creating new MPDPlayerController with default settings");
        let host = "localhost";
        let port = 6600;
        
        // Create a base controller with player name and ID
        let base = BasePlayerController::with_player_info("mpd", &format!("{}:{}", host, port));
        
        let player = Self {
            base,
            hostname: host.to_string(),
            port,
            current_state: Arc::new(Mutex::new(PlayerState::new())),
            current_stream_details: Arc::new(Mutex::new(None)),
            load_mpd_library: true,
            enhance_metadata: true,
            extract_coverart: true,
            artist_separators: None,
            music_directory: String::new(),
            library_read_only: false,
            effective_music_directory: Arc::new(Mutex::new(None)),
            library: Arc::new(Mutex::new(None)),
            max_reconnect_attempts: 5, // Default value
            reconnect_attempts: Arc::new(Mutex::new(0)),
            connection_disabled: Arc::new(AtomicBool::new(false)),
            song_split_manager: SongSplitManager::new(),
            current_update_job_id: Arc::new(Mutex::new(None)),
            option_snapshot: Arc::new(Mutex::new(OptionsSnapshot::default())),
        };
        
        // Set default capabilities
        player.set_default_capabilities();
        
        player
    }
    
    /// Create a new MPD player controller with custom settings
    pub fn with_connection(hostname: &str, port: u16) -> Self {
        debug!("Creating new MPDPlayerController with connection {}:{}", hostname, port);
        
        // Create a base controller with player name and ID
        let base = BasePlayerController::with_player_info("mpd", &format!("{}:{}", hostname, port));
        
        let player = Self {
            base,
            hostname: hostname.to_string(),
            port,
            current_state: Arc::new(Mutex::new(PlayerState::new())),
            current_stream_details: Arc::new(Mutex::new(None)),
            load_mpd_library: true,
            enhance_metadata: true,
            extract_coverart: true,
            artist_separators: None,
            music_directory: String::new(),
            library_read_only: false,
            effective_music_directory: Arc::new(Mutex::new(None)),
            library: Arc::new(Mutex::new(None)),
            max_reconnect_attempts: 5, // Default value
            reconnect_attempts: Arc::new(Mutex::new(0)),
            connection_disabled: Arc::new(AtomicBool::new(false)),
            song_split_manager: SongSplitManager::new(),
            current_update_job_id: Arc::new(Mutex::new(None)),
            option_snapshot: Arc::new(Mutex::new(OptionsSnapshot::default())),
        };
        
        // Set default capabilities
        player.set_default_capabilities();
        
        player
    }
    
    /// Set the default capabilities for this player
    fn set_default_capabilities(&self) {
        debug!("Setting default MPDPlayerController capabilities");
        self.base.set_capabilities(vec![
            PlayerCapability::Play,
            PlayerCapability::Pause,
            PlayerCapability::PlayPause,
            PlayerCapability::Stop,
            PlayerCapability::Next,
            PlayerCapability::Previous,
            PlayerCapability::Seek,
            PlayerCapability::Loop,
            PlayerCapability::Shuffle,
            PlayerCapability::Killable,
            PlayerCapability::Queue,
        ], false); // Don't notify on initialization
    }
    
    /// Attempt to reconnect to the MPD server
    pub fn reconnect(&self) -> Result<(), MpdError> {
        let addr = format!("{}:{}", self.hostname, self.port);
        debug!("Attempting to reconnect to MPD at {}", addr);
        
        match Client::connect(&addr) {
            Ok(_) => {
                info!("Successfully reconnected to MPD at {}", addr);
                self.reset_reconnect_attempts(); // Reset counter on successful connection
                Ok(())
            },
            Err(e) => {
                warn!("Failed to reconnect to MPD at {}: {}", addr, e);
                Err(e)
            }
        }
    }
    
    /// Check if connected to MPD server
    pub fn is_connected(&self) -> bool {
        // Create a fresh connection to check connectivity
        if let Some(mut client) = self.get_fresh_client() {
            // Try a simple ping to verify the connection
            match client.ping() {
                Ok(_) => {
                    debug!("MPD connection is active");
                    return true;
                },
                Err(e) => {
                    debug!("MPD connection lost: {}", e);
                    return false;
                }
            }
        }
        false
    }
    
    /// Get the current MPD server hostname
    pub fn hostname(&self) -> &str {
        &self.hostname
    }
    
    /// Get the current MPD server port
    pub fn port(&self) -> u16 {
        self.port
    }
    
    /// Update the connection settings and reconnect
    pub fn set_connection(&mut self, hostname: &str, port: u16) {
        debug!("Updating MPD connection to {}:{}", hostname, port);
        self.hostname = hostname.to_string();
        self.port = port;
    }
    
    /// Get whether to load MPD library into memory
    pub fn load_mpd_library(&self) -> bool {
        self.load_mpd_library
    }
    
    /// Set whether to load MPD library into memory
    pub fn set_load_mpd_library(&mut self, load: bool) {
        self.load_mpd_library = load;
    }
    
    /// Get whether to enhance metadata
    pub fn get_enhance_metadata(&self) -> Option<bool> {
        Some(self.enhance_metadata)
    }

    /// Set whether to enhance metadata
    pub fn set_enhance_metadata(&mut self, enhance: bool) {
        self.enhance_metadata = enhance;
    }
    
    /// Get whether to extract cover art from music files
    pub fn get_extract_coverart(&self) -> Option<bool> {
        Some(self.extract_coverart)
    }

    /// Set whether to extract cover art from music files
    pub fn set_extract_coverart(&mut self, extract: bool) {
        self.extract_coverart = extract;
    }
    
    /// Get the configured music directory path
    pub fn get_music_directory(&self) -> &str {
        &self.music_directory
    }
    
    /// Set the music directory path
    pub fn set_music_directory(&mut self, directory: String) {
        debug!("Setting music directory to: {}", directory);
        self.music_directory = directory;
        // Clear the cached effective music directory so it will be recalculated
        {
            let mut cached = self.effective_music_directory.lock();
            *cached = None;
        }
    }

    /// Get whether the library is configured as read-only (deletion disabled)
    pub fn get_library_read_only(&self) -> bool {
        self.library_read_only
    }

    /// Set whether the library is read-only (disables deletion support)
    pub fn set_library_read_only(&mut self, read_only: bool) {
        self.library_read_only = read_only;
    }
    
    /// Get the effective music directory path
    /// If configured music_directory is empty, attempts to parse it from /etc/mpd.conf
    pub fn get_effective_music_directory(&self) -> Option<String> {
        // First check if we have a cached result
        {
            let cached = self.effective_music_directory.lock();
            if let Some(ref cached_dir) = *cached {
                return Some(cached_dir.clone());
            }
        }

        // If music_directory is configured in the JSON, use it
        let effective_dir = if !self.music_directory.is_empty() {
            debug!("Using configured music directory: {}", self.music_directory);
            Some(self.music_directory.clone())
        } else {
            // 1. Parse from mpd.conf in known locations (system-wide + hifiberry user home)
            // 2. Fall back to asking MPD directly via the config command (requires admin password)
            // 3. Fall back to well-known HiFiBerry OS default paths
            self.parse_music_directory_from_config()
                .or_else(|| self.query_music_directory_from_mpd())
                .or_else(|| {
                    // Well-known defaults used by the HiFiBerry OS MPD package.
                    // These match the fallback list already used by cover art extraction.
                    for candidate in &["/var/lib/mpd/music", "/music", "/srv/music"] {
                        if std::path::Path::new(candidate).is_dir() {
                            info!("Using well-known fallback music directory: {}", candidate);
                            return Some(candidate.to_string());
                        }
                    }
                    warn!("Could not determine MPD music directory from any source");
                    None
                })
        };

        // Only cache on success to allow retrying after transient failures
        if let Some(ref dir) = effective_dir {
            let mut cached = self.effective_music_directory.lock();
            *cached = Some(dir.clone());
        }

        effective_dir
    }
    
    /// Parse the music directory from mpd.conf, trying multiple candidate paths.
    ///
    /// Search order:
    /// 1. `/etc/mpd.conf` (system-wide, root-run MPD)
    /// 2. `~<hifiberry_user>/etc/mpd.conf` (HiFiBerry user service, start-mpd.sh style)
    /// 3. `~<hifiberry_user>/.config/mpd/mpd.conf` (XDG default for user MPD)
    ///
    /// The hifiberry user is read from `/etc/hifiberry.user`; their home directory
    /// is resolved from `/etc/passwd`.
    fn parse_music_directory_from_config(&self) -> Option<String> {
        let mut candidates = vec!["/etc/mpd.conf".to_string()];

        // Read the hifiberry username and resolve their home dir
        if let Ok(username) = fs::read_to_string("/etc/hifiberry.user") {
            let username = username.trim();
            if let Some(home) = Self::home_dir_for_user(username) {
                candidates.push(format!("{}/etc/mpd.conf", home));
                candidates.push(format!("{}/.config/mpd/mpd.conf", home));
            }
        }

        for config_path in &candidates {
            if let Some(dir) = Self::parse_music_directory_from_file(config_path) {
                return Some(dir);
            }
        }

        warn!("music_directory not found in any of: {:?}", candidates);
        None
    }

    /// Extract the `music_directory` value from a single mpd.conf file.
    fn parse_music_directory_from_file(config_path: &str) -> Option<String> {
        let file = match fs::File::open(config_path) {
            Ok(f) => f,
            Err(_) => return None,
        };

        for line in BufReader::new(file).lines().map_while(Result::ok) {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            if trimmed.starts_with("music_directory") {
                if let (Some(s), Some(e)) = (trimmed.find('"'), trimmed.rfind('"')) {
                    if s < e {
                        let directory = &trimmed[s + 1..e];
                        info!("Auto-detected MPD music directory from {}: {}", config_path, directory);
                        return Some(directory.to_string());
                    }
                }
            }
        }

        debug!("No music_directory found in {}", config_path);
        None
    }

    /// Look up a user's home directory from `/etc/passwd`.
    fn home_dir_for_user(username: &str) -> Option<String> {
        let file = fs::File::open("/etc/passwd").ok()?;
        for line in BufReader::new(file).lines().map_while(Result::ok) {
            let fields: Vec<&str> = line.splitn(7, ':').collect();
            if fields.len() >= 6 && fields[0] == username {
                return Some(fields[5].to_string());
            }
        }
        None
    }

    /// Query MPD directly for its music_directory via the `config` command.
    fn query_music_directory_from_mpd(&self) -> Option<String> {
        use std::io::{BufRead, BufReader, Write};
        use std::net::TcpStream;

        let stream = TcpStream::connect(format!("{}:{}", self.hostname, self.port)).ok()?;
        stream.set_read_timeout(Some(std::time::Duration::from_secs(3))).ok()?;

        let mut reader = BufReader::new(stream.try_clone().ok()?);
        let mut writer = stream;

        // Read welcome line
        let mut welcome = String::new();
        reader.read_line(&mut welcome).ok()?;
        if !welcome.starts_with("OK") {
            return None;
        }

        writer.write_all(b"config\n").ok()?;

        for line in reader.lines().map_while(Result::ok) {
            if line == "OK" {
                break;
            }
            if let Some(rest) = line.strip_prefix("music_directory: ") {
                let dir = rest.trim().to_string();
                info!("Auto-detected MPD music directory via config command: {}", dir);
                return Some(dir);
            }
        }

        warn!("MPD config command did not return music_directory");
        None
    }


    /// Get the maximum number of reconnection attempts
    pub fn get_max_reconnect_attempts(&self) -> u32 {
        self.max_reconnect_attempts
    }
    
    /// Set the maximum number of reconnection attempts before giving up
    pub fn set_max_reconnect_attempts(&mut self, attempts: u32) {
        debug!("Setting maximum reconnection attempts to {}", attempts);
        self.max_reconnect_attempts = attempts;
    }
    
    /// Reset the reconnection attempt counter
    fn reset_reconnect_attempts(&self) {
        {
            let mut counter = self.reconnect_attempts.lock();
            *counter = 0;
        }
        // Re-enable connections when we successfully connect
        self.connection_disabled.store(false, Ordering::Relaxed);
    }
    
    /// Increment the reconnection attempt counter and return the new value
    fn increment_reconnect_attempts(&self) -> u32 {
        let mut counter = self.reconnect_attempts.lock();
        *counter += 1;
        *counter
    }
    
    /// Disable further connection attempts after max attempts reached
    fn disable_connections(&self) {
        self.connection_disabled.store(true, Ordering::Relaxed);
    }
    
    /// Check if connections are disabled
    fn are_connections_disabled(&self) -> bool {
        self.connection_disabled.load(Ordering::Relaxed)
    }
    
    /// Get a reference to the MPD library, if available
    pub fn get_library(&self) -> Option<crate::players::mpd::library::MPDLibrary> {
        // Lock the mutex and clone the library if it exists
        let library_guard = self.library.lock();
        library_guard.clone()
    }
    
    /// Force a refresh of the MPD library
    pub fn refresh_library(&self) -> Result<(), crate::data::library::LibraryError> {
        debug!("Requesting MPD library refresh");
        
        // Get the library instance if available
        if let Some(mut library) = self.get_library() {
            // Pass the artist separators to the library before refreshing
            if let Some(separators) = &self.artist_separators {
                library.set_artist_separators(separators.clone());
            }
            
            // Run the refresh in a separate thread
            let library_clone = library;
            thread::spawn(move || {
                match library_clone.refresh_library() {
                    Ok(_) => info!("MPD library refreshed successfully"),
                    Err(e) => warn!("Failed to refresh MPD library: {}", e),
                }
            });
            
            return Ok(());
        }
        
        Err(crate::data::library::LibraryError::InternalError("Library not initialized".to_string()))
    }
    
    /// Set the custom artist separators for splitting artist names
    pub fn set_artist_separators(&mut self, separators: Vec<String>) {
        debug!("Setting custom artist separators: {:?}", separators);
        self.artist_separators = Some(separators);
    }
    
    /// Get the current custom artist separators if set
    pub fn get_artist_separators(&self) -> Option<&[String]> {
        self.artist_separators.as_deref()
    }
    
    /// Clear all title splitters (useful for cleanup or configuration changes)
    pub fn clear_title_splitters(&self) {
        self.song_split_manager.clear_all_splitters();
    }
    
    /// Get the number of active title splitters
    pub fn get_title_splitter_count(&self) -> usize {
        self.song_split_manager.get_splitter_count()
    }
    
    /// Get statistics for a specific URL's title splitter
    pub fn get_title_splitter_stats(&self, url: &str) -> Option<(u32, u32, u32, u32, bool)> {
        self.song_split_manager.get_splitter_stats(url)
    }
    
    /// Get all splitter IDs currently being managed
    pub fn get_all_splitter_ids(&self) -> Vec<String> {
        self.song_split_manager.get_splitter_ids()
    }
    
    /// Get statistics for all active splitters
    pub fn get_all_splitter_stats(&self) -> HashMap<String, (u32, u32, u32, u32, bool)> {
        self.song_split_manager.get_all_splitter_stats()
    }
    
    /// Remove a specific splitter (useful for cleanup)
    pub fn remove_title_splitter(&self, url: &str) -> bool {
        self.song_split_manager.remove_splitter(url)
    }

    /// Full splitting state for one station
    pub fn get_splitter_state(&self, url: &str) -> Option<crate::helpers::songsplitmanager::SplitterState> {
        self.song_split_manager.get_state(url)
    }

    /// Full splitting state for every station currently held in memory
    pub fn get_all_splitter_states(&self) -> Vec<crate::helpers::songsplitmanager::SplitterState> {
        self.song_split_manager.get_all_states()
    }

    /// Set — or clear, with `None` — a station's title order and separator
    pub fn set_splitter_forced(
        &self,
        url: &str,
        order: Option<crate::helpers::songtitlesplitter::OrderResult>,
        separator: Option<char>,
    ) -> Option<crate::helpers::songsplitmanager::SplitterState> {
        self.song_split_manager.set_forced(url, order, separator)
    }

    /// Persist a station's splitter so its settings survive a restart
    pub fn save_title_splitter(&self, url: &str) -> Result<(), String> {
        self.song_split_manager.save(url)
    }
      /// Notify all registered listeners that the database is being updated
    pub fn notify_database_update(&self, artist: Option<String>, album: Option<String>, 
                                 song: Option<String>, percentage: Option<f32>) {
        // The source parameter is redundant since BasePlayerController creates its own source
        // Just pass the remaining parameters to the base method
        self.base.notify_database_update(artist, album, song, percentage);
    }
    
    /// Initialize the MPD library with retry logic
    /// 
    /// This method attempts to initialize the library and will retry with exponential backoff
    /// if the initial connection fails. The retry intervals are: 1s, 2s, 4s, 8s, 15s, 30s, 60s
    /// 
    /// # Arguments
    /// * `player_arc` - Arc reference to the player controller
    /// * `running` - Atomic boolean to check if the player is still running
    fn initialize_library_with_retry(player_arc: Arc<Self>, running: Arc<AtomicBool>) {
        info!("Starting MPD library initialization with retry logic");
        
        // Run in a separate thread to avoid blocking the main startup
        thread::spawn(move || {
            let mut retry_handler = RetryHandler::connection_retry();
              let library_initialized = retry_handler.execute_with_retry(
                || {
                    // Check if we should stop due to shutdown signal
                    if !running.load(Ordering::SeqCst) {
                        debug!("Library initialization interrupted by shutdown signal");
                        return None;
                    }
                    
                    debug!("Attempting to initialize MPD library");
                    
                    // Try to connect to MPD to test connectivity
                    if let Some(_client) = player_arc.get_fresh_client() {
                        info!("Successfully connected to MPD, initializing library");
                        
                        // Import MPDLibrary here to ensure it's available
                        use crate::players::mpd::library::MPDLibrary;
                        
                        // Create a library with the same connection parameters
                        let library = MPDLibrary::with_connection(
                            &player_arc.hostname, 
                            player_arc.port, 
                            player_arc.clone()
                        );
                        
                        // Store the library in the controller
                        {
                            let mut library_guard = player_arc.library.lock();
                            *library_guard = Some(library.clone());
                            debug!("Library instance stored in controller");
                        }
                        
                        // Start the library refresh in the current thread
                        info!("Starting MPD library refresh...");
                        match library.refresh_library() {
                            Ok(_) => {
                                info!("MPD library loaded successfully");
                                Some(()) // Success
                            },
                            Err(e) => {
                                warn!("Failed to refresh MPD library: {}", e);
                                None // Failed, will retry
                            }
                        }
                    } else {
                        debug!("Failed to connect to MPD for library initialization");
                        None // Failed, will retry
                    }
                },
                Some(&running),
                "MPD library initialization"
            );
            
            if library_initialized.is_none() {
                warn!("MPD library initialization failed after all retry attempts");
            }
        });
    }
    
    /// Starts a background thread that listens for MPD events
    /// The thread will run until the running flag is set to false
    fn start_event_listener(&self, running: Arc<AtomicBool>, self_arc: Arc<Self>) {
        let hostname = self.hostname.clone();
        let port = self.port;
        
        info!("Starting MPD event listener thread");
        
        // Spawn a new thread for event listening
        thread::spawn(move || {
            info!("MPD event listener thread started");
            Self::run_event_loop(&hostname, port, running, self_arc);
            info!("MPD event listener thread shutting down");
        });
    }

    /// Main event loop for listening to MPD events
    fn run_event_loop(hostname: &str, port: u16, running: Arc<AtomicBool>, player_arc: Arc<Self>) {
        while running.load(Ordering::SeqCst) {
            // Try to establish a connection for idle mode
            let idle_addr = format!("{}:{}", hostname, port);
            let idle_client = match Client::connect(&idle_addr) {
                Ok(client) => {
                    debug!("Connected to MPD for idle listening at {}", idle_addr);
                    player_arc.reset_reconnect_attempts(); // Reset counter on successful connection
                    client
                },
                Err(e) => {
                    warn!("Failed to connect to MPD for idle mode: {}", e);
                    
                    // Increment attempt counter and check if we should give up
                    let attempts = player_arc.increment_reconnect_attempts();
                    let max_attempts = player_arc.get_max_reconnect_attempts();
                    
                    if attempts >= max_attempts {
                        error!("Failed to connect to MPD after {} attempts, giving up", attempts);
                        player_arc.disable_connections(); // Mark connections as disabled
                        break; // Exit the loop and stop trying
                    }
                    
                    info!("Will attempt to reconnect in 5 seconds (attempt {}/{})", attempts, max_attempts);
                    Self::wait_for_reconnect(&running);
                    continue;
                }
            };
            
            // Process events until connection fails or shutdown requested
            Self::process_events(idle_client, &running, &player_arc);
            
            // If we get here, either there was a connection error or the connection was lost
            if running.load(Ordering::SeqCst) {
                // Only wait for reconnect if we haven't exceeded the limit yet
                let attempts = player_arc.increment_reconnect_attempts();
                let max_attempts = player_arc.get_max_reconnect_attempts();
                
                if attempts >= max_attempts {
                    error!("Connection lost and maximum reconnection attempts ({}) reached, giving up", max_attempts);
                    player_arc.disable_connections(); // Mark connections as disabled
                    break;
                }
                
                info!("Connection lost, will attempt to reconnect in 5 seconds (attempt {}/{})", attempts, max_attempts);
                Self::wait_for_reconnect(&running);
            }
        }
    }
    
    /// Process MPD events until connection fails or shutdown requested
    fn process_events(mut idle_client: Client<TcpStream>, 
                     running: &Arc<AtomicBool>, player: &Arc<Self>) {
        while running.load(Ordering::SeqCst) {
            let subsystems = match idle_client.idle(&[
                Subsystem::Player,
                Subsystem::Mixer,
                Subsystem::Options,
                Subsystem::Playlist,
                Subsystem::Database,
                Subsystem::Update,
            ]) {
                Ok(subs) => subs,
                Err(e) => {
                    warn!("MPD idle error: {}", e);
                    // Connection may have been lost, break out to try reconnecting
                    break;
                }
            };
            
            // Get the subsystems that changed
            let events = match subsystems.get() {
                Ok(events) => events,
                Err(e) => {
                    warn!("Error getting MPD events: {}", e);
                    continue;
                }
            };
            
            if events.is_empty() {
                continue;
            }
            
            // Convert to a format we can log
            let events_str: Vec<String> = events.iter()
                .map(|s| format!("{:?}", s))
                .collect();
            
            info!("Received MPD events: {}", events_str.join(", "));
            
            // Create a fresh command connection for handling events
            if let Some(mut cmd_client) = player.get_fresh_client() {
                // Process each subsystem event with our fresh connection
                for subsystem in events {
                    Self::handle_subsystem_event(subsystem, &mut cmd_client, player.clone());
                }
            } else {
                warn!("Failed to create command connection for event processing");
            }
        }
    }
    
    /// Handle a specific MPD subsystem event
    fn handle_subsystem_event(subsystem: Subsystem, client: &mut Client<TcpStream>, player: Arc<Self>) {
        // mark player as alive
        player.base.alive();

        match subsystem {
            Subsystem::Player => {
                debug!("Player state changed");
                // Pass the existing client connection to reuse it
                Self::handle_player_event(client, player);
            },
            Subsystem::Playlist => {
                warn!("Playlist changed");
                // Could notify about playlist/song changes
            },
            Subsystem::Options => {
                debug!("Options changed (repeat, random, etc.)");
                Self::handle_options_event(client, player);
            },
            Subsystem::Mixer => {
                debug!("Mixer changed (volume)");
            },
            Subsystem::Database => {
                debug!("Database changed, refreshing library");
                // Refresh the library if it's available
                if let Some(library) = player.get_library() {
                    // Run the refresh in a separate thread to avoid blocking the event handler
                    let library_clone = library.clone();
                    thread::spawn(move || {
                        match library_clone.refresh_library() {
                            Ok(_) => info!("MPD library refreshed successfully after database change"),
                            Err(e) => warn!("Failed to refresh MPD library after database change: {}", e),
                        }
                    });
                }
            },
            Subsystem::Update => {
                debug!("MPD database update status changed");
                // Get fresh status to check the current update state
                if let Some(mut status_client) = player.get_fresh_client() {
                    match status_client.status() {
                        Ok(status) => {
                            player.check_database_update_status(&status);
                        },
                        Err(e) => {
                            warn!("Failed to get MPD status for update event: {}", e);
                        }
                    }
                } else {
                    warn!("Failed to get client for update status check");
                }
            },
            _ => {
                debug!("Other subsystem changed: {:?}", subsystem);
            }
        }
    }
    
    /// Handle an MPD `Options` event: emit shuffle and loop mode changes.
    ///
    /// MPD raises this subsystem for any option change, `consume` and
    /// crossfade included, so the values are diffed against the last
    /// observation and only real transitions are reported. The first
    /// observation after a connect only establishes the baseline.
    fn handle_options_event(client: &mut Client<TcpStream>, player: Arc<Self>) {
        let status = match client.status() {
            Ok(status) => status,
            Err(e) => {
                // Leave the snapshot untouched: the next Options event
                // re-synchronises and no transition is lost.
                warn!("Failed to get MPD status for options event: {}", e);
                return;
            }
        };

        // Take the transition and release the lock before notifying. The
        // notify path reaches the event bus and every subscribed WebSocket
        // client, and holding a player lock across it risks a deadlock.
        let transition = {
            let mut snapshot = player.option_snapshot.lock();
            snapshot.observe(status.random, status.repeat, status.single)
        };

        if transition.is_empty() {
            return;
        }

        if let Some(enabled) = transition.shuffle {
            debug!("MPD random is now {}", enabled);
            player.base.notify_random_changed(enabled);
        }

        if let Some(mode) = transition.loop_mode {
            debug!("MPD loop mode is now {}", mode);
            player.base.notify_loop_mode_changed(mode);
        }
    }

    /// Handle player events and log song information
    fn handle_player_event(client: &mut Client<TcpStream>, player: Arc<Self>) {

        // Update the song information and capabilities
        Self::update_song_from_mpd(client, player.clone());
        
        // Get and update the player state
        match client.status() {
            Ok(status) => {
                info!("Player status: {:?}, volume: {}%", 
                    status.state, status.volume);
                
                // Convert MPD state to our PlaybackState
                let player_state = match status.state {
                    mpd::State::Play => PlaybackState::Playing,
                    mpd::State::Pause => PlaybackState::Paused,
                    mpd::State::Stop => PlaybackState::Stopped,
                };
                
                // Notify listeners about the state change
                debug!("MPDPlayerController forwarding state change notification: {}", player_state);
                player.base.notify_state_changed(player_state);
            },
            Err(e) => {
                warn!("Failed to get player status: {}", e);
                // In case of error, assume stopped state
                player.base.notify_state_changed(PlaybackState::Stopped);
            }
        }
    }
    
    /// Wait for a short period before attempting to reconnect
    fn wait_for_reconnect(running: &Arc<AtomicBool>) {
        for _ in 0..50 {
            if !running.load(Ordering::SeqCst) {
                break;
            }
            thread::sleep(Duration::from_millis(100));
        }
    }
    
    /// Enhance a song with cached metadata if available
    fn enhance_song_with_cache(&self, mut song: Song) -> Song {
        // Check if the song has a stream URL that might be in our cache
        if let Some(ref stream_url) = song.stream_url {
            let cache_key = format!("mpd.urlmeta.{}", stream_url);
            
            match attributecache::get::<HashMap<String, serde_json::Value>>(&cache_key) {
                Ok(Some(cached_metadata)) => {
                    debug!("Found cached metadata for URL: {}", stream_url);
                    
                    // Add all cached metadata to the song's metadata
                    for (key, value) in &cached_metadata {
                        song.metadata.insert(key.clone(), value.clone());
                        debug!("Added cached metadata: {} = {:?}", key, value);
                    }
                },
                Ok(None) => {
                    debug!("No cached metadata found for URL: {}", stream_url);
                },
                Err(e) => {
                    debug!("Failed to retrieve cached metadata for URL {}: {}", stream_url, e);
                }
            }
        }

        apply_station_logo(&mut song);

        song
    }
    
    /// Update the current song and notify listeners
    fn update_current_song(&self, song: Option<Song>) {
        let enhanced_song = song.map(|s| self.enhance_song_with_cache(s));
        self.base.set_song(enhanced_song);
    }

    /// Create a fresh MPD client connection for sending commands
    /// This creates a new connection each time, rather than reusing an existing one
    pub fn get_fresh_client(&self) -> Option<Client<TcpStream>> {
        // Check if connections have been disabled due to max reconnection attempts
        if self.are_connections_disabled() {
            debug!("MPD connections are disabled due to max reconnection attempts reached");
            return None;
        }
        
        debug!("Creating fresh MPD command connection");
        let addr = format!("{}:{}", self.hostname, self.port);
        
        match Client::connect(&addr) {
            Ok(client) => {
                debug!("Successfully created new MPD command connection");
                // Reset connection attempts on successful connection
                self.reset_reconnect_attempts();
                Some(client)
            },
            Err(e) => {
                warn!("Failed to create MPD command connection: {}", e);
                None
            }
        }
    }
    
    /// Update player state and capabilities based on the current MPD status
    /// 
    /// Updates the PlayerState object with current information from MPD including:
    /// - Playback state (playing/paused/stopped)
    /// - Volume
    /// - Loop mode
    /// - Shuffle status
    /// - Current position
    /// - Available capabilities (Next/Previous/Seek)
    /// 
    /// Returns an updated song with lyrics metadata if applicable
    fn update_state_and_capabilities_from_mpd(client: &mut Client<TcpStream>, player: Arc<Self>, song: Option<Song>) -> Option<Song> {
        debug!("Updating player state and capabilities based on MPD status");
        
        let updated_song = song;
        
        // Try to get current status to determine playlist position and other state info
        match client.status() {
            Ok(status) => {
                // Update audio stream format from MPD's status `audio` field
                // (rate:bits:channels). MPD does not report the source codec.
                {
                    let mut sd = player.current_stream_details.lock();
                    *sd = status.audio.map(|a| crate::data::stream_details::StreamDetails {
                        sample_rate: Some(a.rate),
                        bits_per_sample: Some(a.bits),
                        channels: Some(a.chans),
                        ..Default::default()
                    });
                }
                // Get a lock on the current_state to update it
                {
                    let mut current_state = player.current_state.lock();
                    // Update playback state
                    current_state.state = match status.state {
                        mpd::State::Play => PlaybackState::Playing,
                        mpd::State::Pause => PlaybackState::Paused,
                        mpd::State::Stop => PlaybackState::Stopped,
                    };
                    debug!("Updated player state: {:?}", current_state.state);
                    
                    // Update volume if available (MPD returns -1 for no volume control)
                    if status.volume >= 0 {
                        current_state.volume = Some(status.volume as i32);
                        debug!("Updated volume: {}%", status.volume);
                    }
                    
                    // Update loop mode based on MPD repeat and single flags
                    current_state.loop_mode = if status.repeat {
                        if status.single {
                            LoopMode::Track
                        } else {
                            LoopMode::Playlist
                        }
                    } else {
                        LoopMode::None
                    };
                    debug!("Updated loop mode: {:?}", current_state.loop_mode);
                    
                    // Update shuffle status
                    current_state.shuffle = status.random;
                    debug!("Updated shuffle: {}", status.random);
                    
                    // Update playback position if available
                    if let Some(elapsed) = status.elapsed {
                        current_state.position = Some(elapsed.as_secs_f64());
                        debug!("Updated position: {:.1}s", elapsed.as_secs_f64());
                    }
                    
                    // Store current song information in player metadata if available
                    if let Some(sng) = &updated_song {
                        let mut metadata = HashMap::new();
                        
                        if let Some(duration) = sng.duration {
                            if let Some(num) = serde_json::Number::from_f64(duration) {
                                metadata.insert("duration".to_string(), serde_json::Value::Number(num));
                            }
                        }
                        
                        if let Some(track) = sng.track_number {
                            metadata.insert("track".to_string(), serde_json::Value::Number(serde_json::Number::from(track)));
                        }
                        
                        // Queue status info
                        metadata.insert("queue_length".to_string(), serde_json::Value::Number(serde_json::Number::from(status.queue_len)));
                        
                        if let Some(song_id) = status.song.map(|s| s.id) {
                            // Convert the mpd::Id to a number that can be stored in metadata
                            let id_value = song_id.0; // Access the inner numeric value directly
                            metadata.insert("song_id".to_string(), serde_json::Value::Number(serde_json::Number::from(id_value)));
                        }
                        
                        if let Some(song_pos) = status.song.map(|s| s.pos) {
                            metadata.insert("queue_position".to_string(), serde_json::Value::Number(serde_json::Number::from(song_pos)));
                        }
                        
                        // Update metadata in state
                        current_state.metadata = metadata;
                    }
                }
                
                // Total songs in playlist
                let queue_len = status.queue_len;
                
                // Current song position (0-indexed)
                let current_pos = status.song.map(|s| s.pos).unwrap_or(0);
                
                // Check if we have a next song
                let has_next = current_pos + 1 < queue_len;
                
                // Check if we have a previous song
                let has_previous = current_pos > 0;
                
                // Check if player is stopped - if so, disable stop/next/previous buttons
                let is_stopped = status.state == mpd::State::Stop;
                
                debug!("Playlist status: position {}/{}, has_next={}, has_previous={}, is_stopped={}", 
                       current_pos, queue_len, has_next, has_previous, is_stopped);
                
                // Update capabilities without sending notifications yet
                let mut capabilities_changed = false;
                
                // Update Next capability if needed - disable when stopped
                capabilities_changed |= player.base.set_capability(
                    PlayerCapability::Next, 
                    has_next && !is_stopped, 
                    false // Don't notify yet
                );
                
                // Update Previous capability if needed - disable when stopped
                capabilities_changed |= player.base.set_capability(
                    PlayerCapability::Previous, 
                    has_previous && !is_stopped, 
                    false // Don't notify yet
                );
                
                // Update Stop capability - disable when already stopped
                capabilities_changed |= player.base.set_capability(
                    PlayerCapability::Stop,
                    !is_stopped,
                    false // Don't notify yet
                );

                // Check if the current song is seekable
                let is_seekable = match &updated_song {
                    Some(current_song) => {
                        // Check if the song has a duration
                        if let Some(duration) = current_song.duration {
                            // Check if the file is not a streaming URL
                            let file_path = current_song.stream_url.as_deref().unwrap_or("");
                            let is_stream = is_stream_uri(file_path);

                            // Seekable if it has duration and is not a stream
                            let seekable = duration > 0.0 && !is_stream;
                            debug!("Song seekability check: duration={:?}s, is_stream={}, seekable={}", 
                                  duration, is_stream, seekable);
                            seekable
                        } else {
                            debug!("Song has no duration, not seekable");
                            false
                        }
                    },
                    None => {
                        debug!("No current song, marking as not seekable");
                        false
                    }
                };
                
                // Update Seek capability based on our assessment
                capabilities_changed |= player.base.set_capability(
                    PlayerCapability::Seek,
                    is_seekable,
                    false // Don't notify yet
                );
                
                // Update capabilities with a single notification
                if capabilities_changed {
                    let current_caps = player.base.get_capabilities();
                    player.base.notify_capabilities_changed(&current_caps);
                    debug!("Player capabilities updated: Next={}, Previous={}, Stop={}, Seek={}", 
                          has_next && !is_stopped, has_previous && !is_stopped, !is_stopped, is_seekable);
                }
            },
            Err(e) => {
                warn!("Failed to get MPD status for player state and capability update: {}", e);
                
                // If we can't get status, disable navigation capabilities
                let mut capabilities_changed = false;
                
                capabilities_changed |= player.base.set_capability(
                    PlayerCapability::Next, 
                    false, 
                    false // Don't notify yet
                );
                
                capabilities_changed |= player.base.set_capability(
                    PlayerCapability::Previous, 
                    false, 
                    false // Don't notify yet
                );
                
                capabilities_changed |= player.base.set_capability(
                    PlayerCapability::Stop,
                    false,
                    false // Don't notify yet
                );

                // Also disable seek capability when there's an error
                capabilities_changed |= player.base.set_capability(
                    PlayerCapability::Seek,
                    false,
                    false // Don't notify yet
                );
                
                if capabilities_changed {
                    let current_caps = player.base.get_capabilities();
                    player.base.notify_capabilities_changed(&current_caps);
                    debug!("Player capabilities updated: disabled Next/Previous/Stop/Seek due to error");
                }
                
                // Update state to reflect error condition
                {
                    let mut current_state = player.current_state.lock();
                    current_state.state = PlaybackState::Stopped;
                }
            }
        }
        
        updated_song
    }
    
    /// Convert an MPD song to our Song format
    fn convert_mpd_song(mpd_song: mpd::Song, player_arc: Option<Arc<Self>>) -> Song {
        // Generate cover art URL using the file path/URI from MPD song.
        // Streams are skipped: their "file" is the stream URL itself, and MPD
        // cannot serve album art for one ("Cannot get size for stream"), so any
        // URL generated here would be guaranteed to 404. Radio stations get
        // their cover art from the cached URL metadata instead, see
        // enhance_song_with_cache().
        let cover_url = if !mpd_song.file.is_empty() && !is_stream_uri(&mpd_song.file) {
            // Try to use encoded URL if library is available
            if let Some(player) = &player_arc {
                let library_guard = player.library.lock();
                if let Some(library) = library_guard.as_ref() {
                    // Use the library's create_encoded_image_url method
                    Some(library.create_encoded_image_url(&mpd_song.file))
                } else {
                    // Fallback to base64 encoded URL if library not available
                    Some(format!("{}/{}", mpd_image_url(), url_encoding::encode_url_safe(&mpd_song.file)))
                }
            } else {
                // Fallback to base64 encoded URL if no player provided
                Some(format!("{}/{}", mpd_image_url(), url_encoding::encode_url_safe(&mpd_song.file)))
            }
        } else {
            None
        };
        
        // Extract album from tags
        let album = mpd_song.tags.iter()
            .find(|(tag, _)| tag == "Album")
            .map(|(_, value)| value.clone());
            
        // Extract album artist from tags
        let album_artist = mpd_song.tags.iter()
            .find(|(tag, _)| tag == "AlbumArtist")
            .map(|(_, value)| value.clone());
            
        // Extract genre from tags
        let genre = mpd_song.tags.iter()
            .find(|(tag, _)| tag == "Genre")
            .map(|(_, value)| value.clone());
        
        // Handle title splitting for radio stations.
        //
        // Note this can flap the artist across two polls of one track. A
        // stream's ICY title is a single "Artist - Title" string and the
        // splitter learns per URL whether to split it, so the same track can
        // be reported with an artist and then without one (or the other way
        // round) while the splitter is still deciding, or if a station
        // changes the shape of its ICY line mid-track. The artist is part of
        // song identity, so each flap reads as a new song: one song_changed,
        // and the enrichment merged into the stored song -- including any
        // artwork a lookup found -- goes with it, so the station logo
        // re-flashes until the next lookup completes.
        //
        // Left as is deliberately. It is self-healing and costs a lookup, not
        // correctness; the alternative of keeping the artist out of identity
        // silently misreports every same-titled track by a different artist
        // on four backends, which is not self-healing at all.
        let (final_title, final_artist) = if mpd_song.artist.is_none() && mpd_song.title.is_some() {
            // No artist but has title - try to split it (common for web radio)
            let title_str = mpd_song.title.as_ref().unwrap();
            
            if let Some(player) = &player_arc {
                // Use the song URL as the splitter ID for radio stations
                let splitter_id = &mpd_song.file;
                
                // Try to split the title using the manager
                if let Some((artist, song)) = player.song_split_manager.split_song(splitter_id, title_str) {
                    debug!("Split title '{}' into artist='{}', song='{}'", title_str, artist, song);
                    
                    // Save the splitter state after successful split
                    if let Err(e) = player.song_split_manager.save(splitter_id) {
                        debug!("Failed to save splitter state for '{}': {}", splitter_id, e);
                    }
                    
                    (Some(song), Some(artist))
                } else {
                    debug!("Could not split title '{}', keeping as-is", title_str);
                    (mpd_song.title.clone(), mpd_song.artist.clone())
                }
            } else {
                // No player reference, can't split
                (mpd_song.title.clone(), mpd_song.artist.clone())
            }
        } else {
            // Artist exists or no title, use as-is
            (mpd_song.title.clone(), mpd_song.artist.clone())
        };
            
        Song {
            title: final_title,
            artist: final_artist,
            album,
            album_artist,
            track_number: mpd_song.place.as_ref().map(|p| p.pos as i32),
            total_tracks: None,
            duration: mpd_song.duration.map(|d| d.as_secs_f32() as f64),
            genre: genre.clone(),
            genres: genre.map(|g| vec![g]).unwrap_or_default(),
            year: None,
            cover_art_url: cover_url,
            stream_url: Some(mpd_song.file.clone()),
            source: Some("mpd".to_string()),
            liked: None,
            composer: None,
            metadata: HashMap::new(),
        }
    }
    
    /// Update the player's current song from MPD
    fn update_song_from_mpd(client: &mut Client<TcpStream>, player: Arc<Self>) {
        // Variable to store the obtained song for later use in updating capabilities
        let mut obtained_song: Option<Song> = None;
        
        // Use the provided client connection
        match client.currentsong() {
            Ok(song_opt) => {
                if let Some(mpd_song) = song_opt {
                    // Convert MPD song to our Song format
                    let mut song = Self::convert_mpd_song(mpd_song, Some(player.clone()));
                    
                    // Check for lyrics and add to song metadata
                    if let Some(library) = player.get_library() {
                        if let Some(music_dir) = library.get_music_directory() {
                            use crate::helpers::lyrics::{MPDLyricsProvider, LyricsProvider};
                            let lyrics_provider = MPDLyricsProvider::new(music_dir.clone());
                            
                            // Try to find lyrics by URL (file path) from stream_url
                            let has_lyrics = if let Some(file_path) = &song.stream_url {
                                debug!("Checking for lyrics for file: {}", file_path);
                                match lyrics_provider.get_lyrics_by_url(file_path) {
                                    Ok(_) => {
                                        debug!("Found lyrics for: {}", file_path);
                                        true
                                    },
                                    Err(e) => {
                                        debug!("No lyrics found for {}: {:?}", file_path, e);
                                        false
                                    }
                                }
                            } else {
                                debug!("No stream_url available for lyrics check");
                                false
                            };
                            
                            if has_lyrics {
                                song.metadata.insert("lyrics_available".to_string(), serde_json::Value::Bool(true));
                                debug!("Added lyrics_available=true to song metadata");
                                
                                // Add API endpoint for lyrics by song ID
                                if let (Some(artist), Some(title), Some(file_path)) = (&song.artist, &song.title, &song.stream_url) {
                                    // Use the encoded file path as the song ID for the lyrics API
                                    let encoded_file_path = url_encoding::encode_url_safe(file_path);
                                    let lyrics_url = format!("{}/lyrics/mpd/{}", crate::constants::API_PREFIX, encoded_file_path);
                                    song.metadata.insert("lyrics_url".to_string(), serde_json::Value::String(lyrics_url));
                                    debug!("Added lyrics_url with song ID to metadata: {}", encoded_file_path);
                                    
                                    // Also add the metadata that can be used for the POST request
                                    let mut lyrics_metadata = serde_json::Map::new();
                                    lyrics_metadata.insert("artist".to_string(), serde_json::Value::String(artist.clone()));
                                    lyrics_metadata.insert("title".to_string(), serde_json::Value::String(title.clone()));
                                    
                                    if let Some(duration) = song.duration {
                                        lyrics_metadata.insert("duration".to_string(), serde_json::Value::Number(serde_json::Number::from_f64(duration).unwrap_or(serde_json::Number::from(0))));
                                    }
                                    
                                    if let Some(album) = &song.album {
                                        lyrics_metadata.insert("album".to_string(), serde_json::Value::String(album.clone()));
                                    }
                                    
                                    song.metadata.insert("lyrics_metadata".to_string(), serde_json::Value::Object(lyrics_metadata));
                                    debug!("Added lyrics_metadata object to song metadata");
                                }
                            } else {
                                song.metadata.insert("lyrics_available".to_string(), serde_json::Value::Bool(false));
                                debug!("Added lyrics_available=false to song metadata");
                            }
                        }
                    }
                    
                    info!("Now playing: {} - {}", 
                        song.title.as_deref().unwrap_or("Unknown"),
                        song.artist.as_deref().unwrap_or("Unknown"));
                    
                    debug!("Song metadata contains {} entries: {:?}", song.metadata.len(), song.metadata.keys().collect::<Vec<_>>());
                    
                    // Log additional song details if available
                    if let Some(duration) = song.duration {
                        debug!("Duration: {:.1} seconds", duration);
                    }
                    if let Some(track) = song.track_number {
                        debug!("Position: {} in queue", track);
                    }
                    
                    // Store the song for capability update (with lyrics metadata already added)
                    obtained_song = Some(song.clone());
                } else {
                    info!("No song currently playing");
                }
            },
            Err(e) => warn!("Failed to get current song information: {}", e),
        }
        
        // Update player capabilities based on the current playlist state and the song we just got
        let final_song = Self::update_state_and_capabilities_from_mpd(client, player.clone(), obtained_song);
        
        // Update stored song and notify listeners with the final version
        player.update_current_song(final_song);
    }

    /// Add URLs to the play queue in one connection.
    ///
    /// # Arguments
    /// * `urls` - The URLs/paths of the songs to add, in the order they should appear
    /// * `at_beginning` - If true, insert at the front of the queue, otherwise append
    ///
    /// # Returns
    /// * `bool` - true if every URL was added, false if any failed
    pub fn queue_urls(&self, urls: &[String], at_beginning: bool) -> bool {
        if urls.is_empty() {
            return true;
        }

        debug!(
            "Adding {} URLs to queue, at_beginning: {}",
            urls.len(),
            at_beginning
        );

        // One connection for the whole batch. Opening a fresh client per track
        // meant queueing an album opened a connection per track.
        let Some(mut client) = self.get_fresh_client() else {
            warn!("Failed to get MPD client connection for queue_urls");
            return false;
        };

        let mut all_success = true;
        for (index, url) in urls.iter().enumerate() {
            let song_path = mpd::Song {
                file: url.to_string(),
                ..Default::default()
            };

            let result = match queue_insert_position(index, at_beginning) {
                Some(position) => {
                    debug!("Inserting track at position {}: {}", position, url);
                    client.insert(&song_path, position)
                }
                None => {
                    debug!("Pushing track to end of queue: {}", url);
                    client.push(&song_path).map(|_id| 0)
                }
            };

            match result {
                Ok(_) => debug!("Successfully added URL to queue: {}", url),
                Err(e) => {
                    warn!("Failed to add URL to queue: {} - Error: {}", url, e);
                    all_success = false;
                }
            }
        }

        all_success
    }
}

/// Position for the nth track of a batch being queued.
///
/// Appending leaves placement to MPD. Inserting at the beginning has to
/// advance one slot per track: inserting every track at 0 pushes the previous
/// one down and lands the batch reversed.
fn queue_insert_position(index: usize, at_beginning: bool) -> Option<usize> {
    if at_beginning {
        Some(index)
    } else {
        None
    }
}

/// Structure to store player state for each instance
struct PlayerInstanceData {
    running_flag: Arc<AtomicBool>
}

/// A map to store running state for each player instance
type PlayerStateMap = HashMap<usize, PlayerInstanceData>;
static PLAYER_STATE: Lazy<Mutex<PlayerStateMap>> = Lazy::new(|| Mutex::new(HashMap::new()));

impl MPDPlayerController {
    /// Check MPD database update status and manage background job
    fn check_database_update_status(&self, status: &mpd::Status) {
        let job_id = "mpd_database_update";
        
        // Check if MPD is currently updating the database
        let is_updating = status.updating_db.is_some();
        
        {
            let mut current_job_guard = self.current_update_job_id.lock();
            let has_active_job = current_job_guard.is_some();

            if is_updating && !has_active_job {
                // MPD started updating and we don't have an active job - start one
                match BackgroundJobs::instance().register_job(
                    job_id.to_string(),
                    "MPD Database Update".to_string()
                ) {
                    Ok(_) => {
                        info!("Started background job for MPD database update");
                        *current_job_guard = Some(job_id.to_string());
                    },
                    Err(e) => {
                        warn!("Failed to register MPD database update job: {}", e);
                    }
                }
            } else if !is_updating && has_active_job {
                // MPD finished updating and we have an active job - finish it
                if let Some(active_job_id) = current_job_guard.take() {
                    if let Err(e) = BackgroundJobs::instance().complete_job(&active_job_id) {
                        warn!("Failed to complete MPD database update job: {}", e);
                    } else {
                        info!("Finished background job for MPD database update");
                    }
                }
            } else if is_updating && has_active_job {
                // MPD is still updating and we have an active job - update progress if available
                if let Some(update_id) = status.updating_db {
                    let progress_msg = format!("Updating database (job {})", update_id);
                    if let Err(e) = BackgroundJobs::instance().update_job(job_id, Some(progress_msg), None, None) {
                        debug!("Failed to update MPD database update job progress: {}", e);
                    }
                }
            }
        }
    }
}

impl PlayerController for MPDPlayerController {
    delegate! {
        to self.base {
            fn get_capabilities(&self) -> PlayerCapabilitySet;
            fn get_last_seen(&self) -> Option<std::time::SystemTime>;
        }
    }
    
    fn get_song(&self) -> Option<Song> {
        // Re-run the cache enhancement in case the URL metadata was written
        // after the song was stored.
        self.base.song().map(|song| self.enhance_song_with_cache(song))
    }

    fn apply_song_information(&self, partial: &Song) -> bool {
        self.base.apply_song_information(partial)
    }

    fn get_stream_details(&self) -> Option<crate::data::stream_details::StreamDetails> {
        self.current_stream_details.lock().clone()
    }

    fn get_loop_mode(&self) -> LoopMode {
        trace!("MPDController: get_loop_mode called");
        if let Some(mut mpd_client) = self.get_fresh_client() {
            if let Ok(status) = mpd_client.status() {
                return match (status.repeat, status.single) {
                    (true, true) => LoopMode::Track,
                    (true, false) => LoopMode::Playlist,
                    _ => LoopMode::None,
                };
            }
        }
        debug!("Failed to get loop mode from MPD");
        LoopMode::None
    }
    
    fn get_playback_state(&self) -> PlaybackState {
        trace!("MPDController: get_playback_state called");
        if let Some(mut mpd_client) = self.get_fresh_client() {
            if let Ok(status) = mpd_client.status() {
                match status.state {
                    mpd::State::Play => return PlaybackState::Playing,
                    mpd::State::Pause => return PlaybackState::Paused,
                    mpd::State::Stop => return PlaybackState::Stopped,
                }
            }
        }
        debug!("Failed to get state from MPD");
        PlaybackState::Unknown
    }
    
    fn get_position(&self) -> Option<f64> {
        trace!("MPDController: get_position called");
        if let Some(mut mpd_client) = self.get_fresh_client() {
            if let Ok(status) = mpd_client.status() {
                if let Some(elapsed) = status.elapsed {
                    // Convert Duration to f64 seconds
                    return Some(elapsed.as_secs_f64());
                }
            }
        }
        debug!("Failed to get position from MPD");
        None
    }
    
    fn get_shuffle(&self) -> bool {
        trace!("MPDController: get_shuffle called");
        if let Some(mut mpd_client) = self.get_fresh_client() {
            if let Ok(status) = mpd_client.status() {
                return status.random;
            }
        }
        debug!("Failed to get shuffle status from MPD");
        false
    }
    
    fn get_player_name(&self) -> String {
        "mpd".to_string()
    }
    
    fn get_aliases(&self) -> Vec<String> {
        vec!["mpd".to_string()]
    }
    
    fn get_player_id(&self) -> String {
        format!("{}:{}", self.hostname, self.port)
    }
    
    fn send_command(&self, command: PlayerCommand) -> bool {
        info!("Sending command to MPD: {}", command);
        
        let mut success = false;
        
        // Create a fresh connection for each command
        if let Some(mut client) = self.get_fresh_client() {
            // Process the command based on its type
            match command {
                PlayerCommand::Play => {
                    // Start playback
                    success = client.play().is_ok();
                    if success {
                        debug!("MPD playback started");
                    }
                },
                
                PlayerCommand::Pause => {
                    // Pause playback
                    success = client.pause(true).is_ok();
                    if success {
                        debug!("MPD playback paused");
                    }
                },
                
                PlayerCommand::PlayPause => {
                    // Toggle between play and pause
                    match client.status() {
                        Ok(status) => {
                            match status.state {
                                mpd::State::Play => {
                                    success = client.pause(true).is_ok();
                                    if success {
                                        debug!("MPD playback paused (toggle)");
                                    }
                                },
                                _ => {
                                    success = client.play().is_ok();
                                    if success {
                                        debug!("MPD playback started (toggle)");
                                    }
                                }
                            }
                        },
                        Err(e) => {
                            warn!("Failed to get MPD status for play/pause toggle: {}", e);
                        }
                    }
                },
                
                PlayerCommand::Stop => {
                    // Stop playback
                    success = client.stop().is_ok();
                    if success {
                        debug!("MPD playback stopped");
                    } else {
                        warn!("Failed to stop MPD playback");
                    }
                },
                
                PlayerCommand::Next => {
                    // Skip to next track
                    success = client.next().is_ok();
                    if success {
                        debug!("Skipped to next track in MPD");
                    }
                },
                
                PlayerCommand::Previous => {
                    // Go back to previous track
                    success = client.prev().is_ok();
                    if success {
                        debug!("Went back to previous track in MPD");
                    }
                },
                
                PlayerCommand::SetLoopMode(mode) => {
                    // Map our loop mode to MPD repeat/single settings
                    match mode {
                        LoopMode::None => {
                            // Turn off both repeat and single
                            let repeat_ok = client.repeat(false).is_ok();
                            let single_ok = client.single(false).is_ok();
                            success = repeat_ok && single_ok;
                            if success {
                                debug!("MPD loop mode set to None");
                            }
                        },
                        LoopMode::Track => {
                            // Single track repeat (single=true)
                            let repeat_ok = client.repeat(true).is_ok();
                            let single_ok = client.single(true).is_ok();
                            success = repeat_ok && single_ok;
                            if success {
                                debug!("MPD loop mode set to Track (single repeat)");
                            }
                        },
                        LoopMode::Playlist => {
                            // Whole playlist repeat (repeat=true, single=false)
                            let repeat_ok = client.repeat(true).is_ok();
                            let single_ok = client.single(false).is_ok();
                            success = repeat_ok && single_ok;
                            if success {
                                debug!("MPD loop mode set to Playlist (whole playlist repeat)");
                            }
                        },
                    }
                },
                
                PlayerCommand::Seek(position) => {
                    // Seek to a position in seconds
                    match client.currentsong() {
                        Ok(song_opt) => {
                            if let Some(song) = song_opt {
                                if let Some(place) = song.place {
                                    // Use the song's position in the queue
                                    // Position needs to be f64 to satisfy ToSeconds trait
                                    let position_seconds: f64 = position; 
                                    success = client.seek(place.pos, position_seconds).is_ok();
                                    if success {
                                        debug!("Sought to position {}s in current track", position);
                                    }
                                } else {
                                    warn!("Current song has no position, cannot seek");
                                }
                            } else {
                                warn!("No current song to seek in");
                            }
                        },
                        Err(e) => {
                            warn!("Failed to get current song for seeking: {}", e);
                        }
                    }
                },
                
                PlayerCommand::SetRandom(enabled) => {
                    // Set shuffle/random mode
                    success = client.random(enabled).is_ok();
                    if success {
                        debug!("MPD random mode set to: {}", enabled);
                    }
                },
                
                PlayerCommand::Kill => {
                    // Kill the MPD process via the kill command
                    // Note: this requires the MPD server to have proper permissions configured
                    success = client.kill().is_ok();
                    if success {
                        debug!("MPD kill command sent successfully");
                        
                        // Stop the player controller since MPD process is now killed
                        self.stop();
                    } else {
                        warn!("Failed to kill MPD process, might not have permission");
                    }
                },
                
                PlayerCommand::QueueTracks { uris, insert_at_beginning, metadata } => {
                    debug!("Adding {} tracks to MPD queue at {}", uris.len(), 
                          if insert_at_beginning { "beginning" } else { "end" });
                    
                    if uris.is_empty() {
                        debug!("No URIs provided to queue");
                        success = true; // Nothing to do, but not an error
                    } else {
                        // Cache any metadata first; it does not need MPD.
                        for (i, uri) in uris.iter().enumerate() {
                            // Get metadata for this URI if available
                            let track_metadata = metadata.get(i).and_then(|m| m.as_ref());

                            // Store metadata in cache if provided
                            if let Some(meta) = track_metadata {
                                if !meta.metadata.is_empty() {
                                    debug!("Caching metadata for URI {}: {:?}", 
                                           uri, meta.metadata);
                                    let cache_key = format!("mpd.urlmeta.{}", uri);
                                    
                                    match attributecache::set(&cache_key, &meta.metadata) {
                                        Ok(_) => {
                                            debug!("Successfully cached metadata for URI: {}", uri);
                                        },
                                        Err(e) => {
                                            warn!("Failed to cache metadata for URI {}: {}", uri, e);
                                        }
                                    }
                                }
                            }
                        }

                        // The whole batch goes over one connection.
                        success = self.queue_urls(&uris, insert_at_beginning);
                    }
                    
                    if success {
                        debug!("Successfully added all tracks to MPD queue");
                    } else {
                        warn!("Failed to add some or all tracks to MPD queue");
                    }
                },
                    
                PlayerCommand::RemoveTrack(position) => {
                    debug!("Removing track at position {} from MPD queue", position);
                    
                    // Remove the track at the specified position
                    let result = client.delete(position as u32);
                    
                    if let Err(e) = result {
                        warn!("Failed to remove track at position {}: {}", position, e);
                        success = false;
                    } else {
                        debug!("Successfully removed track at position {}", position);
                        success = true;
                        
                        // Notify listeners that the queue has been modified
                        self.base.notify_queue_changed();
                    }
                },
                  PlayerCommand::ClearQueue => {
                    debug!("Clearing MPD queue");
                    
                    success = client.clear().is_ok();
                    if success {
                        debug!("Successfully cleared MPD queue");
                        
                        // Notify listeners that the queue has been cleared
                        self.base.notify_queue_changed();
                    } else {
                        warn!("Failed to clear MPD queue");
                    }
                },                  PlayerCommand::PlayQueueIndex(index) => {
                    debug!("Playing track at index {} in MPD queue", index);
                    
                    // Use MPD's switch function to start playback from a specific position
                    // This plays the song at the specified position in the playlist (0-based)
                    success = client.switch(index as u32).is_ok();
                    if success {
                        debug!("Started playback of track at position {} in MPD queue", index);
                    } else {
                        warn!("Failed to play track at position {} in MPD queue", index);
                    }
                },
            }
            
            // If the command was successful, we may want to update our stored state
            if success {
                // We'll update our state asynchronously via the MPD idle events
            }
        } else {
            warn!("Cannot send command to MPD: failed to create a fresh connection");
        }
        
        success
    }
    
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn start(&self) -> bool {
        info!("Starting MPD player controller");
        
        // Create a new Arc<Self> for thread-safe sharing of player instance
        let player_arc = Arc::new(self.clone());
        
        // Create a new running flag
        let running = Arc::new(AtomicBool::new(true));
          // Try to get the current song from MPD first
        if let Some(mut client) = self.get_fresh_client() {
            // Initialize song state and capabilities
            info!("Fetching initial song state from MPD");
            Self::update_song_from_mpd(&mut client, player_arc.clone());
            
            // Load MPD library if configured to do so
            if self.load_mpd_library {
                info!("Starting MPD library initialization with retry mechanism");
                
                // Use the new retry mechanism for library initialization
                Self::initialize_library_with_retry(player_arc.clone(), running.clone());
            } else {
                debug!("Skipping MPD library loading (disabled in config)");
            }
        } else {
            warn!("Could not connect to MPD to fetch initial song state");
            
            // Even if we can't connect initially, still try to initialize the library with retry
            // if library loading is enabled
            if self.load_mpd_library {
                info!("Initial MPD connection failed, but starting library initialization with retry mechanism");
                Self::initialize_library_with_retry(player_arc.clone(), running.clone());
            }
        }
        
        // Store the running flag in the MPD player instance
        {
            let mut state = PLAYER_STATE.lock();
            let instance_id = self as *const _ as usize;

            if let Some(data) = state.get(&instance_id) {
                // Stop any existing thread
                data.running_flag.store(false, Ordering::SeqCst);
            }

            // Start a new listener thread
            self.start_event_listener(running.clone(), player_arc.clone());

            // Store the running flag
            state.insert(instance_id, PlayerInstanceData { running_flag: running });
            true
        }
    }
    
    fn stop(&self) -> bool {
        info!("Stopping MPD player controller");
        
        // Signal the event listener thread to stop
        {
            let mut state = PLAYER_STATE.lock();
            let instance_id = self as *const _ as usize;

            if let Some(data) = state.remove(&instance_id) {
                data.running_flag.store(false, Ordering::SeqCst);
                debug!("Signaled event listener thread to stop");
                return true;
            }
        }

        debug!("No active event listener thread found");
        false
    }
    
    // Implement the get_library method for MPDPlayerController
    fn get_library(&self) -> Option<Box<dyn LibraryInterface>> {
        if let Some(library) = self.get_library() {
            Some(Box::new(library))
        } else {
            None
        }
    }

    fn get_queue(&self) -> Vec<Track> {
        debug!("MPDController: get_queue called - fetching playlist");
        
        // Get a fresh client connection
        if let Some(mut client) = self.get_fresh_client() {
            // Use the queue method to get all songs in the current queue
            match client.queue() {
                Ok(songs) => {
                    debug!("Retrieved {} songs from MPD queue", songs.len());
                    
                    // Convert MPD songs to our Track format
                    let tracks: Vec<Track> = songs.into_iter()
                        .map(|mpd_song| {
                            // Extract useful information from the song
                            let title = mpd_song.title.unwrap_or_else(|| "Unknown Title".to_string());
                            let artist = mpd_song.artist;
                            
                            // Create a Track with just the name
                            let mut track = Track::with_name(title);
                            
                            // Set artist if available
                            if let Some(artist_name) = artist {
                                track.artist = Some(artist_name);
                            }
                            
                            // Set URI if available
                            if !mpd_song.file.is_empty() {
                                track.uri = Some(mpd_song.file);
                            }
                            
                            track
                        })
                        .collect();
                    
                    return tracks;
                },
                Err(e) => {
                    warn!("Failed to retrieve queue from MPD: {}", e);
                }
            }
        } else {
            warn!("Failed to create MPD client connection for get_queue");
        }
        
        // Return empty vector if anything fails
        Vec::new()
    }

    fn get_meta_keys(&self) -> Vec<String> {
        vec![
            "hostname".to_string(),
            "port".to_string(),
            "connection_status".to_string(),
            "queue_length".to_string(),
            "volume".to_string(),
            "playback_state".to_string(),
            "last_seen".to_string(),
            "stats".to_string(),
            "library_loaded".to_string(),
            "library_loading_progress".to_string(),
            "mpd_version".to_string(),
        ]
    }

    fn get_metadata_value(&self, key: &str) -> Option<String> {
        match key {
            "hostname" => Some(self.hostname.clone()),
            "port" => Some(self.port.to_string()),
            "connection_status" => {
                let connected = self.is_connected();
                Some(if connected { "connected".to_string() } else { "disconnected".to_string() })
            },
            "queue_length" => {
                if let Some(mut client) = self.get_fresh_client() {
                    match client.status() {
                        Ok(status) => Some(status.queue_len.to_string()),
                        Err(_) => Some("0".to_string())
                    }
                } else {
                    Some("0".to_string())
                }
            },
            "mpd_version" => {
                if let Some(client) = self.get_fresh_client() {
                    // Get MPD version from the client and format it as major.minor.patch
                    Some(format!("{}.{}.{}", client.version.0, client.version.1, client.version.2))
                } else {
                    Some("unknown".to_string())
                }
            },
            "volume" => {
                if let Some(mut client) = self.get_fresh_client() {
                    match client.status() {
                        Ok(status) => {
                            if status.volume >= 0 {
                                Some(status.volume.to_string())
                            } else {
                                Some("unknown".to_string())
                            }
                        },
                        Err(_) => Some("unknown".to_string())
                    }
                } else {
                    Some("unknown".to_string())
                }
            },
            "playback_state" => Some(self.get_playback_state().to_string()),
            "last_seen" => {
                if let Some(timestamp) = self.get_last_seen() {
                    let duration = std::time::SystemTime::now()
                        .duration_since(timestamp)
                        .unwrap_or_else(|_| std::time::Duration::from_secs(0));
                    Some(format!("{} seconds ago", duration.as_secs()))
                } else {
                    Some("never".to_string())
                }
            },
            "stats" => {
                if let Some(mut client) = self.get_fresh_client() {
                    match client.stats() {
                        Ok(stats) => {
                            // Format MPD stats as JSON
                            // Note: db_update is not a duration but rather a timestamp
                            Some(serde_json::json!({
                                "artists": stats.artists,
                                "albums": stats.albums,
                                "songs": stats.songs,
                                "uptime": stats.uptime.as_secs(),
                                "db_playtime": stats.db_playtime.as_secs(),
                                "db_update": stats.db_update,
                                "playtime": stats.playtime.as_secs()
                            }).to_string())
                        },
                        Err(_) => Some("{}".to_string())
                    }
                } else {
                    Some("{}".to_string())
                }
            },
            "library_loaded" => {
                // Check if library is loaded
                if let Some(library) = self.get_library() {
                    Some(library.is_loaded().to_string())
                } else {
                    Some("false".to_string())
                }
            },
            "library_loading_progress" => {
                // Get library loading progress
                if let Some(library) = self.get_library() {
                    Some(format!("{:.1}%", library.get_loading_progress() * 100.0))
                } else {
                    Some("0.0%".to_string())
                }
            },
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use tempfile::TempDir;

    /// Appending leaves placement to MPD, so every track of the batch gets the
    /// same "no position" answer.
    #[test]
    fn queue_insert_position_is_absent_when_appending() {
        let positions: Vec<_> = (0..4).map(|i| queue_insert_position(i, false)).collect();

        assert_eq!(positions, vec![None, None, None, None]);
    }

    /// Inserting a batch at the front has to advance one slot per track.
    /// Inserting every track at 0 pushes the previous one down, which lands a
    /// 12-track album in the queue backwards.
    #[test]
    fn queue_insert_position_advances_so_a_batch_keeps_its_order() {
        let positions: Vec<_> = (0..4).map(|i| queue_insert_position(i, true)).collect();

        assert_eq!(positions, vec![Some(0), Some(1), Some(2), Some(3)]);
    }

    /// Test that songs without cached metadata are not affected
    #[test]
    fn test_mpd_no_cached_metadata() {
        // Create a temporary directory for the test cache
        let temp_dir = TempDir::new().expect("Failed to create temp directory");
        
        // Initialize AttributeCache with the temporary directory
        attributecache::AttributeCache::initialize_global(temp_dir.path()).expect("Failed to configure cache");
        
        let mut song = Song::default();
        song.stream_url = Some("http://example.com/not-cached.mp3".to_string());
        song.metadata.insert("existing".to_string(), Value::String("data".to_string()));
        
        // Create an MPD player controller
        let player = MPDPlayerController::with_connection("localhost", 6600);
        
        // Enhance the song (should not find any cached metadata)
        let enhanced_song = player.enhance_song_with_cache(song);
        
        // Verify that no cached metadata was added, but existing metadata is preserved
        assert_eq!(enhanced_song.metadata.len(), 1);
        assert_eq!(enhanced_song.metadata.get("existing"), Some(&Value::String("data".to_string())));
    }

    fn mpd_song_with_file(file: &str) -> mpd::Song {
        mpd::Song {
            file: file.to_string(),
            ..Default::default()
        }
    }

    /// A library track gets a cover art URL pointing at the image endpoint
    #[test]
    fn test_library_track_gets_cover_art_url() {
        let song = MPDPlayerController::convert_mpd_song(
            mpd_song_with_file("music/Artist/Album/01 - Track.flac"),
            None,
        );

        let cover_art_url = song.cover_art_url.expect("library track should have cover art URL");
        assert!(
            cover_art_url.starts_with(&mpd_image_url()),
            "unexpected cover art URL: {}",
            cover_art_url
        );
    }

    /// A radio stream must not get a library cover art URL: MPD cannot serve
    /// album art for a stream, so such a URL would always be a 404.
    #[test]
    fn test_stream_gets_no_library_cover_art_url() {
        for uri in [
            "https://bytefm.cast.addradio.de/bytefm/main/high/stream",
            "http://example.com/stream.mp3",
            "rtsp://example.com/stream",
        ] {
            let song = MPDPlayerController::convert_mpd_song(mpd_song_with_file(uri), None);
            assert_eq!(
                song.cover_art_url, None,
                "stream {} should not get a library cover art URL",
                uri
            );
        }
    }

    #[test]
    fn test_is_stream_uri() {
        assert!(is_stream_uri("https://example.com/stream"));
        assert!(is_stream_uri("http://example.com/stream"));
        assert!(!is_stream_uri("music/Artist/Album/01 - Track.flac"));
        assert!(!is_stream_uri(""));
    }

    /// The station logo from the URL metadata is used as cover art
    #[test]
    fn test_station_logo_used_as_cover_art() {
        let mut metadata = HashMap::new();
        metadata.insert(
            "coverart_url".to_string(),
            Value::String("https://www.byte.fm/favicon.png".to_string()),
        );

        assert_eq!(
            cover_art_from_metadata(&metadata).map(|(url, _)| url),
            Some("https://www.byte.fm/favicon.png".to_string())
        );
    }

    /// logo_url is accepted when coverart_url is absent
    #[test]
    fn test_logo_url_used_as_fallback() {
        let mut metadata = HashMap::new();
        metadata.insert(
            "logo_url".to_string(),
            Value::String("https://example.com/logo.png".to_string()),
        );

        assert_eq!(
            cover_art_from_metadata(&metadata).map(|(url, _)| url),
            Some("https://example.com/logo.png".to_string())
        );
    }

    /// Metadata without any usable logo yields no cover art
    #[test]
    fn test_no_station_logo_in_metadata() {
        let mut metadata = HashMap::new();
        assert_eq!(cover_art_from_metadata(&metadata), None);

        metadata.insert("coverart_url".to_string(), Value::String(String::new()));
        assert_eq!(cover_art_from_metadata(&metadata), None, "empty URL is not cover art");

        metadata.insert("coverart_url".to_string(), Value::Bool(true));
        assert_eq!(cover_art_from_metadata(&metadata), None, "non-string is not cover art");
    }

    /// A stream has no cover art in the library, so what the URL metadata
    /// offers is applied. Whether that image is a placeholder is a separate
    /// question, decided by the tests below.
    #[test]
    fn url_metadata_cover_art_is_applied_to_a_song_with_none() {
        let mut song = Song::default();
        song.metadata.insert(
            "coverart_url".to_string(),
            Value::String("https://www.byte.fm/favicon.png".to_string()),
        );

        apply_station_logo(&mut song);

        assert_eq!(
            song.cover_art_url,
            Some("https://www.byte.fm/favicon.png".to_string())
        );
    }

    /// A radio station is queued under its own name and then plays a different
    /// song every few minutes, so its image describes the URL rather than what
    /// is playing. This is the ByteFM case that started all this.
    #[test]
    fn a_stations_image_is_a_placeholder_for_the_song_playing() {
        let mut song = Song {
            title: Some("Listen To The News".to_string()),
            ..Default::default()
        };
        song.metadata.insert(
            "coverart_url".to_string(),
            Value::String("https://www.byte.fm/favicon.png".to_string()),
        );
        song.metadata.insert(
            "title".to_string(),
            Value::String("ByteFM (192k)".to_string()),
        );

        apply_station_logo(&mut song);

        assert_eq!(
            song.metadata
                .get(crate::data::song::COVER_ART_SOURCE)
                .and_then(|v| v.as_str()),
            Some(crate::data::song::COVER_ART_SOURCE_STATION_LOGO)
        );
    }

    /// A client that queues one track through add_track supplies artwork for
    /// that track. It reaches the same per-URL cache the station metadata uses,
    /// and must not be marked replaceable -- doing so lets a lookup overwrite
    /// the caller's own cover with some other pressing.
    #[test]
    fn artwork_a_client_supplied_for_a_track_is_not_a_placeholder() {
        let mut song = Song {
            title: Some("Yellow Submarine".to_string()),
            ..Default::default()
        };
        song.metadata.insert(
            "coverart_url".to_string(),
            Value::String("https://example.com/yellow_submarine.jpg".to_string()),
        );
        song.metadata.insert(
            "title".to_string(),
            Value::String("Yellow Submarine".to_string()),
        );

        apply_station_logo(&mut song);

        assert_eq!(
            song.cover_art_url,
            Some("https://example.com/yellow_submarine.jpg".to_string())
        );
        assert!(
            !song
                .metadata
                .contains_key(crate::data::song::COVER_ART_SOURCE),
            "a client's own artwork must not be marked replaceable"
        );
    }

    /// A station queued with only a cover art URL and no name. The metadata
    /// format has no enforced semantics -- doc/api.md calls the field names
    /// suggestions -- so nothing obliges whoever queues a station to supply
    /// either a name or logo_url. For a stream, cover art attached to the URL
    /// describes the station whatever else is missing.
    #[test]
    fn an_unnamed_stations_image_is_still_a_placeholder() {
        let mut song = Song {
            title: Some("Listen To The News".to_string()),
            stream_url: Some("https://bytefm.cast.addradio.de/bytefm/main/high/stream".to_string()),
            ..Default::default()
        };
        song.metadata.insert(
            "coverart_url".to_string(),
            Value::String("https://www.byte.fm/favicon.png".to_string()),
        );

        apply_station_logo(&mut song);

        assert_eq!(
            song.metadata
                .get(crate::data::song::COVER_ART_SOURCE)
                .and_then(|v| v.as_str()),
            Some(crate::data::song::COVER_ART_SOURCE_STATION_LOGO)
        );
    }

    /// A file in the library is not a stream, so unnamed artwork attached to it
    /// is the track's own and stays untouched.
    #[test]
    fn an_unnamed_library_track_is_not_a_placeholder() {
        let mut song = Song {
            title: Some("Yellow Submarine".to_string()),
            stream_url: Some("music/The Beatles/Revolver/06 - Yellow Submarine.flac".to_string()),
            ..Default::default()
        };
        song.metadata.insert(
            "coverart_url".to_string(),
            Value::String("https://example.com/revolver.jpg".to_string()),
        );

        apply_station_logo(&mut song);

        assert!(!song
            .metadata
            .contains_key(crate::data::song::COVER_ART_SOURCE));
    }

    /// convert_mpd_song splits an artist-less "A - B" title before this runs, so
    /// a client that queued its track under the combined name would otherwise
    /// see its own artwork marked replaceable over a difference it did not make.
    #[test]
    fn a_split_title_still_matches_the_name_it_was_split_from() {
        let mut song = Song {
            title: Some("Listen To The News".to_string()),
            artist: Some("Radical Friendship Theory".to_string()),
            stream_url: Some("https://example.com/track.mp3".to_string()),
            ..Default::default()
        };
        song.metadata.insert(
            "coverart_url".to_string(),
            Value::String("https://example.com/cover.jpg".to_string()),
        );
        song.metadata.insert(
            "title".to_string(),
            Value::String("Radical Friendship Theory - Listen To The News".to_string()),
        );

        apply_station_logo(&mut song);

        assert!(
            !song
                .metadata
                .contains_key(crate::data::song::COVER_ART_SOURCE),
            "the combined name is the same track, not a different one"
        );
    }

    /// Case and surrounding spaces are not a difference of identity.
    #[test]
    fn a_name_differing_only_in_case_or_spacing_matches() {
        let mut song = Song {
            title: Some("Yellow Submarine".to_string()),
            stream_url: Some("https://example.com/track.mp3".to_string()),
            ..Default::default()
        };
        song.metadata.insert(
            "coverart_url".to_string(),
            Value::String("https://example.com/cover.jpg".to_string()),
        );
        song.metadata.insert(
            "title".to_string(),
            Value::String("  yellow submarine ".to_string()),
        );

        apply_station_logo(&mut song);

        assert!(!song
            .metadata
            .contains_key(crate::data::song::COVER_ART_SOURCE));
    }

    /// With no name in the metadata and nothing to say the URL is a stream,
    /// the image belongs to the one track it was queued with.
    #[test]
    fn unnamed_url_metadata_is_not_a_placeholder() {
        let mut song = Song {
            title: Some("Some Track".to_string()),
            ..Default::default()
        };
        song.metadata.insert(
            "coverart_url".to_string(),
            Value::String("https://example.com/cover.jpg".to_string()),
        );

        apply_station_logo(&mut song);

        assert!(!song
            .metadata
            .contains_key(crate::data::song::COVER_ART_SOURCE));
    }

    /// A logo is a logo whatever is playing, named or not.
    #[test]
    fn a_logo_url_is_always_a_placeholder() {
        let mut song = Song {
            title: Some("Some Track".to_string()),
            ..Default::default()
        };
        song.metadata.insert(
            "logo_url".to_string(),
            Value::String("https://example.com/logo.png".to_string()),
        );

        apply_station_logo(&mut song);

        assert_eq!(
            song.metadata
                .get(crate::data::song::COVER_ART_SOURCE)
                .and_then(|v| v.as_str()),
            Some(crate::data::song::COVER_ART_SOURCE_STATION_LOGO)
        );
    }

    /// A song that already carries its own artwork keeps it, and must not be
    /// marked replaceable — that artwork is the track's, not a placeholder.
    #[test]
    fn real_cover_art_is_not_marked_replaceable() {
        let mut song = Song {
            cover_art_url: Some("https://example.com/cover.jpg".to_string()),
            ..Default::default()
        };
        song.metadata.insert(
            "coverart_url".to_string(),
            Value::String("https://www.byte.fm/favicon.png".to_string()),
        );

        apply_station_logo(&mut song);

        assert_eq!(
            song.cover_art_url,
            Some("https://example.com/cover.jpg".to_string())
        );
        assert!(
            !song
                .metadata
                .contains_key(crate::data::song::COVER_ART_SOURCE),
            "real cover art must not be marked replaceable"
        );
    }

    /// A cover art URL that is already set is never replaced by the station logo
    #[test]
    fn test_existing_cover_art_wins_over_station_logo() {
        let temp_dir = TempDir::new().expect("Failed to create temp directory");
        attributecache::AttributeCache::initialize_global(temp_dir.path()).expect("Failed to configure cache");

        let mut song = Song::default();
        song.stream_url = Some("http://example.com/uncached-stream".to_string());
        song.cover_art_url = Some("/api/audiocontrol/library/mpd/image/abc".to_string());
        song.metadata.insert(
            "coverart_url".to_string(),
            Value::String("https://example.com/logo.png".to_string()),
        );

        let player = MPDPlayerController::with_connection("localhost", 6600);
        let enhanced_song = player.enhance_song_with_cache(song);

        assert_eq!(
            enhanced_song.cover_art_url,
            Some("/api/audiocontrol/library/mpd/image/abc".to_string())
        );
    }

    /// A stream carrying a station logo in its metadata ends up with cover art
    #[test]
    fn test_stream_metadata_logo_becomes_cover_art() {
        let temp_dir = TempDir::new().expect("Failed to create temp directory");
        attributecache::AttributeCache::initialize_global(temp_dir.path()).expect("Failed to configure cache");

        let mut song = Song::default();
        song.stream_url = Some("http://example.com/another-uncached-stream".to_string());
        song.metadata.insert(
            "coverart_url".to_string(),
            Value::String("https://www.byte.fm/favicon.png".to_string()),
        );

        let player = MPDPlayerController::with_connection("localhost", 6600);
        let enhanced_song = player.enhance_song_with_cache(song);

        assert_eq!(
            enhanced_song.cover_art_url,
            Some("https://www.byte.fm/favicon.png".to_string())
        );
    }

    // --- MPD options -> events ---------------------------------------------

    #[test]
    fn loop_mode_is_none_when_repeat_is_off() {
        assert_eq!(loop_mode_from_mpd(false, false), LoopMode::None);
    }

    /// `single` without `repeat` means "stop after this song", not a loop.
    #[test]
    fn loop_mode_is_none_when_single_is_set_without_repeat() {
        assert_eq!(loop_mode_from_mpd(false, true), LoopMode::None);
    }

    #[test]
    fn loop_mode_is_playlist_when_repeat_is_on_alone() {
        assert_eq!(loop_mode_from_mpd(true, false), LoopMode::Playlist);
    }

    #[test]
    fn loop_mode_is_track_when_repeat_and_single_are_both_on() {
        assert_eq!(loop_mode_from_mpd(true, true), LoopMode::Track);
    }

    #[test]
    fn first_observation_establishes_a_baseline_and_emits_nothing() {
        let mut snapshot = OptionsSnapshot::default();

        let transition = snapshot.observe(true, true, true);

        assert!(transition.is_empty(), "a reconnect must not look like a user toggle");
    }

    #[test]
    fn unchanged_options_emit_nothing() {
        let mut snapshot = OptionsSnapshot::default();
        snapshot.observe(false, false, false);

        // An Options event caused by something we do not report, consume say.
        let transition = snapshot.observe(false, false, false);

        assert!(transition.is_empty());
    }

    #[test]
    fn toggling_random_emits_shuffle_only() {
        let mut snapshot = OptionsSnapshot::default();
        snapshot.observe(false, false, false);

        let transition = snapshot.observe(true, false, false);

        assert_eq!(transition.shuffle, Some(true));
        assert_eq!(transition.loop_mode, None);
    }

    #[test]
    fn toggling_repeat_emits_loop_mode_only() {
        let mut snapshot = OptionsSnapshot::default();
        snapshot.observe(false, false, false);

        let transition = snapshot.observe(false, true, false);

        assert_eq!(transition.shuffle, None);
        assert_eq!(transition.loop_mode, Some(LoopMode::Playlist));
    }

    /// One Options event can carry both changes, and both are owed.
    #[test]
    fn changing_random_and_loop_together_emits_both() {
        let mut snapshot = OptionsSnapshot::default();
        snapshot.observe(false, false, false);

        let transition = snapshot.observe(true, true, true);

        assert_eq!(transition.shuffle, Some(true));
        assert_eq!(transition.loop_mode, Some(LoopMode::Track));
    }

    /// single alone flips repeat=true from Playlist to Track; repeat=false
    /// keeps LoopMode::None either way, so no event is owed.
    #[test]
    fn toggling_single_without_repeat_emits_nothing() {
        let mut snapshot = OptionsSnapshot::default();
        snapshot.observe(false, false, false);

        let transition = snapshot.observe(false, false, true);

        assert!(transition.is_empty());
    }

    #[test]
    fn each_observation_becomes_the_new_baseline() {
        let mut snapshot = OptionsSnapshot::default();
        snapshot.observe(false, false, false);
        snapshot.observe(true, false, false);

        // Same values as the previous observation, so nothing further is owed.
        let transition = snapshot.observe(true, false, false);

        assert!(transition.is_empty());
    }
}
