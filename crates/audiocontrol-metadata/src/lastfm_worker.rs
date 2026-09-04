//! Last.fm scrobbling and track enrichment, as a worker on a channel.
//!
//! This was an `ActionPlugin` living inside the player daemon: it subscribed to
//! the event bus itself and reached into `AudioController` to report what it
//! found. It now reads [`NowPlayingEvent`] from a channel and answers through a
//! [`SongInformationSink`], so nothing here needs a player in the same process.
//! The `action_plugins` entry named `lastfm` still configures it, with the same
//! keys and the same effects.
//!
//! Two things the channel cannot carry on its own. The scrobble timer needs the
//! player's real state, which it asks for through [`PlaybackStateSource`]
//! rather than trusting that no `StateChanged` was ever missed; and the
//! daemon's action-plugin list still names this worker, which `plugin_factory`
//! does from [`WORKER_NAME`].

use std::thread;
use std::time::Duration;
use std::sync::Arc;
use parking_lot::Mutex;
use std::time::SystemTime;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::lastfm::{LastfmClient, LastfmTrackInfoDetails};
use acr_types::now_playing::{NowPlayingEvent, PlaybackStateSource, SongInformationSink};
use acr_types::{PlaybackState, PlayerSource, Song};
use crossbeam::channel::Receiver;
use log::{debug, error, info, warn};
use serde::Deserialize;

/// The name this worker logs under, and the name the daemon's action-plugin
/// list reports for the `action_plugins` entry that configures it. That list is
/// a shipped API, so the name is fixed even though nothing here is a plugin any
/// more.
pub const WORKER_NAME: &str = "Lastfm";

/// The `action_plugins` entry named `lastfm`, unchanged: same keys, same
/// default, so an existing configuration keeps working.
#[derive(Debug, Deserialize, Clone)]
pub struct LastfmWorkerConfig {
    pub enabled: bool,
    pub api_key: String,
    pub api_secret: String,
    #[serde(default = "default_scrobble_config")]
    pub scrobble: bool,
}

fn default_scrobble_config() -> bool {
    true
}

pub struct Lastfm {
    config: LastfmWorkerConfig,
    worker_thread: Option<thread::JoinHandle<()>>,
    current_track_data: Arc<Mutex<CurrentScrobbleTrack>>,
    lastfm_client: Option<LastfmClient>,
    worker_running: Arc<AtomicBool>, // Added for graceful shutdown
    /// Where an enrichment result goes. How a partial is merged, and whether it
    /// still describes the song being played, belongs entirely to whatever
    /// implements this.
    sink: Arc<dyn SongInformationSink>,
    /// What the player is actually doing, for the periodic reconciliation
    /// below.
    state: Arc<dyn PlaybackStateSource>,
}

#[derive(Clone, Debug)]
struct CurrentScrobbleTrack {
    name: Option<String>,
    artists: Option<Vec<String>>,
    length: Option<u32>, 
    started_timestamp: Option<SystemTime>, // When the song was first seen/changed to
    scrobbled_song: bool,
    // New fields for playback state tracking
    current_playback_state: PlaybackState,
    last_play_timestamp: Option<SystemTime>, // When playback last started/resumed for this song
    accumulated_play_duration_ms: u64, // Total milliseconds played for this song
    song_details: Option<Song>, // Added to store the full Song object
    track_info_fetched: bool, // Added to track if get_track_info has been called
    player_source: Option<PlayerSource>, // Added to store the source of the song
}

impl Default for CurrentScrobbleTrack {
    fn default() -> Self {
        Self {
            name: None,
            artists: None,
            length: None,
            started_timestamp: None,
            scrobbled_song: false,
            current_playback_state: PlaybackState::Stopped, // Default to Stopped
            last_play_timestamp: None,
            accumulated_play_duration_ms: 0,
            song_details: None, // Initialize new field
            track_info_fetched: false, // Initialize new field
            player_source: None, // Initialize new field
        }
    }
}

fn merge_song_updates(original_song: &mut Song, partial_update: &Song) {
    // Title and artist in partial_update are for identification, not merging.
    // original_song.title and original_song.artist should remain as they are.

    if partial_update.cover_art_url.is_some() {
        original_song.cover_art_url = partial_update.cover_art_url.clone();
        debug!("merge_song_updates: Merged cover_art_url: {:?}", original_song.cover_art_url);
    }

    if partial_update.liked.is_some() {
        original_song.liked = partial_update.liked;
        debug!("merge_song_updates: Merged liked status: {:?}", original_song.liked);
    }

    if !partial_update.metadata.is_empty() {
        for (key, value) in &partial_update.metadata {
            original_song.metadata.insert(key.clone(), value.clone());
            debug!("merge_song_updates: Merged metadata key \'{}\': {:?}", key, value);
        }
    }
    // Note: This merge logic assumes that if a field is None/empty in partial_update,
    // it means "no change for this field", not "clear this field".
    // calculate_updates is designed to only populate fields in partial_update if they represent
    // a change or a new piece of information (like cover art if previously None).
}

// Background worker function
fn lastfm_worker(
    track_data_arc: Arc<Mutex<CurrentScrobbleTrack>>,
    plugin_name: String,
    client: LastfmClient,
    worker_running: Arc<AtomicBool>, // Added
    scrobble_enabled: bool, // Added
    sink: Arc<dyn SongInformationSink>,
    state: Arc<dyn PlaybackStateSource>,
) {
    info!(
        "Lastfm background worker started for plugin: {}. Client available: {}. Scrobbling enabled: {}",
        plugin_name,
        client.is_authenticated(),
        scrobble_enabled
    );
    let mut loop_count: u32 = 0; // Counter for periodic checks

    while worker_running.load(Ordering::SeqCst) {
        thread::sleep(Duration::from_secs(1)); // Main loop delay
        loop_count += 1;

        let mut track_data = track_data_arc.lock();

        // Fetch track info if new song and not yet fetched
        // Not gated on a linked account. Cover art is not user-specific, and
        // the signed lookup refuses without one -- which would have left the
        // station logo marked as replaceable on those devices with nothing
        // ever coming along to replace it.
        if !track_data.track_info_fetched {
            // Separate the immutable borrow for player_source
            let player_source_clone = track_data.player_source.clone();
            let song_title_clone = track_data.song_details.as_ref().and_then(|sd| sd.title.clone());
            let song_artist_clone = track_data.song_details.as_ref().and_then(|sd| sd.artist.clone());

            if let (Some(title), Some(artist)) = (song_title_clone, song_artist_clone) {
                if let Some(current_player_source) = player_source_clone {
                    info!("LastFMWorker: Attempting to get track info for '{}' by '{}'", title, artist);

                    // A signed lookup where there is an account to sign for,
                    // which also answers whether the track is loved and how
                    // often it has been played; an unsigned one otherwise,
                    // which answers only with the album and its images.
                    let (lookup, user_data) = if client.is_authenticated() {
                        (client.get_track_info(&artist, &title), UserData::Present)
                    } else {
                        debug!("LastFMWorker: No linked account; looking up cover art unsigned");
                        (client.get_track_album_info(&artist, &title), UserData::Absent)
                    };

                    match lookup {
                        Ok(track_info_details) => {
                            // Now, we need to re-access song_details mutably.
                            // It's important that the immutable borrows above are out of scope.
                            if let Some(original_song_details_ref) = &mut track_data.song_details {
                                let updated_song_partial = calculate_updates(original_song_details_ref, &track_info_details, user_data);

                                sink.apply(&current_player_source, &updated_song_partial);
                                merge_song_updates(original_song_details_ref, &updated_song_partial);
                                debug!("LastFMWorker: Merged partial song info. New song_details: {:?}", track_data.song_details);
                            } else {
                                 warn!("LastFMWorker: song_details became None unexpectedly before mutable access for update.");
                            }
                        }
                        Err(e) => {
                            warn!("LastFMWorker: Failed to get track info for '{} - {}': {:?}", title, artist, e);
                        }
                    }
                    track_data.track_info_fetched = true;
                } else {
                    warn!("LastFMWorker: player_source was None when attempting to fetch track info. Title: {:?}, Artist: {:?}, Fetched Flag: {}", track_data.song_details.as_ref().and_then(|s| s.title.as_ref()), track_data.song_details.as_ref().and_then(|s| s.artist.as_ref()), track_data.track_info_fetched);
                    // Potentially set track_info_fetched to true here as well if we don't want to retry without a source
                     track_data.track_info_fetched = true; 
                }
            } else {
                warn!("LastFMWorker: Cannot get track info, title or artist missing from stored song details. Title: {:?}, Artist: {:?}, Fetched Flag: {}", track_data.song_details.as_ref().and_then(|s| s.title.as_ref()), track_data.song_details.as_ref().and_then(|s| s.artist.as_ref()), track_data.track_info_fetched);
                track_data.track_info_fetched = true; 
            }
        }

        // Periodic state check (e.g., every 30 seconds)
        if loop_count % 30 == 0 {
            debug!("LastFMWorker: Performing periodic state check.");
            // Asked rather than awaited: a StateChanged that never arrived
            // would otherwise leave the timer measuring a paused player.
            let actual_player_state = state.playback_state();

            if actual_player_state != track_data.current_playback_state {
                info!(
                    "LastFMWorker: Discrepancy detected! Worker state: {:?}, Actual player state: {:?}. Updating worker state.",
                    track_data.current_playback_state, actual_player_state
                );

                // Logic similar to StateChanged event
                if track_data.current_playback_state == PlaybackState::Playing && actual_player_state != PlaybackState::Playing {
                    // Was playing, now not
                    if let Some(lpt) = track_data.last_play_timestamp {
                        let played_ms = lpt.elapsed().unwrap_or_default().as_millis() as u64;
                        track_data.accumulated_play_duration_ms += played_ms;
                        info!("LastFMWorker (Periodic): Playback now '{:?}'. Added {}ms. Total accumulated: {}ms", actual_player_state, played_ms, track_data.accumulated_play_duration_ms);
                    }
                    track_data.last_play_timestamp = None;
                } else if track_data.current_playback_state != PlaybackState::Playing && actual_player_state == PlaybackState::Playing {
                    // Was not playing, now playing
                    info!("LasFMWorker (Periodic): Playback now 'Playing'. Setting last_play_timestamp.");
                    track_data.last_play_timestamp = Some(SystemTime::now());
                }
                track_data.current_playback_state = actual_player_state;
            }
        }


        if let (Some(name), Some(artists), Some(length_val), Some(actual_started_time)) =
            (&track_data.name, &track_data.artists, &track_data.length, &track_data.started_timestamp) {
            
            let artists_str = artists.join(", ");

            let mut current_segment_ms = 0;
            if track_data.current_playback_state == PlaybackState::Playing {
                if let Some(lpt) = track_data.last_play_timestamp {
                    current_segment_ms = lpt.elapsed().unwrap_or_default().as_millis() as u64;
                }
            }
            let effective_elapsed_ms = track_data.accumulated_play_duration_ms + current_segment_ms;
            let effective_elapsed_seconds = effective_elapsed_ms / 1000;

            debug!(
                "LastFMWorker: Song: '{}' by {}. State: {:?}. Length: {}s. Played: {}s (Accum: {}ms, CurrentSeg: {}ms). Scrobbled: {}",
                name,
                artists_str,
                track_data.current_playback_state,
                length_val, // This is &u32, displays fine
                effective_elapsed_seconds,
                track_data.accumulated_play_duration_ms,
                current_segment_ms,
                track_data.scrobbled_song
            );

            // Only attempt to scrobble if the player is currently playing this song
            if track_data.current_playback_state == PlaybackState::Playing
                && !track_data.scrobbled_song && scrobble_enabled { // Added scrobble_enabled check
                    // let scrobble_point_duration_secs = *length_val / 2; // length_val is &u32
                    let scrobble_point_time_secs = 240; // 4 minutes in seconds, Last.fm recommendation
                    

                    if effective_elapsed_seconds >= u64::from(*length_val).saturating_mul(50) / 100 || effective_elapsed_seconds >= scrobble_point_time_secs {
                        
                        if client.is_authenticated() { // Check if client is authenticated before scrobbling
                            if let Some(primary_artist) = artists.first() {
                                let scrobble_timestamp = match actual_started_time.duration_since(SystemTime::UNIX_EPOCH) { // Used actual_started_time
                                    Ok(duration) => duration.as_secs(),
                                    Err(e) => {
                                        error!(
                                            "LastFMWorker: Failed to calculate timestamp for scrobbling (SystemTime error: {}). Using current time as fallback.",
                                            e
                                        );
                                        SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap_or_default().as_secs()
                                    }
                                };

                                debug!(
                                    "LastFMWorker: Attempting to scrobble '{}' by '{}'. Played: {}s. Timestamp: {}",
                                    name,
                                    primary_artist,
                                    effective_elapsed_seconds,
                                    scrobble_timestamp
                                );

                                match client.scrobble(
                                    primary_artist.as_str(),
                                    name.as_str(),      // name is &String
                                    None,               // Album not tracked yet
                                    None,               // Album artist not tracked yet
                                    scrobble_timestamp,
                                    None,               // Track number not tracked
                                    Some(*length_val),  // length_val is &u32
                                ) {
                                    Ok(_) => {
                                        info!(
                                            "LastFMWorker: Successfully scrobbled '{}' by '{}'",
                                            name,
                                            primary_artist
                                        );
                                        track_data.scrobbled_song = true;
                                    }
                                    Err(e) => {
                                        error!(
                                            "LastFMWorker: Failed to scrobble '{}' by '{}': {}",
                                            name,
                                            primary_artist,
                                            e
                                        );
                                        // Keep scrobbled_song = false to allow retry on next tick
                                    }
                                }
                            } else {
                                warn!("LastFMWorker: Cannot scrobble '{}', artist information is missing or empty.", name);
                                // Mark as scrobbled to avoid retries if artist will never be available for this track
                                track_data.scrobbled_song = true; // Or handle differently
                            }
                        } else {
                            debug!(
                                "LastFMWorker: Scrobble attempt for '{}' by '{}' skipped: Last.fm client not authenticated.",
                                name,
                                artists_str
                            );
                            track_data.scrobbled_song = true; // Mark as scrobbled to avoid retries
                        }
                    }
                }
        } else if track_data.name.is_none() {
             debug!("LastFMWorker: No song actively tracked.");
        } else {
             debug!("LastFMWorker: Track data incomplete. Name: {:?}, Artists: {:?}, Length: {:?}, Started: {:?}",
                track_data.name.is_some(), track_data.artists.is_some(), track_data.length.is_some(), track_data.started_timestamp.is_some());
        }
    }
}

impl Lastfm {
    pub fn new(
        config: LastfmWorkerConfig,
        sink: Arc<dyn SongInformationSink>,
        state: Arc<dyn PlaybackStateSource>,
    ) -> Self {
        Self {
            config,
            worker_thread: None,
            current_track_data: Arc::new(Mutex::new(CurrentScrobbleTrack::default())),
            lastfm_client: None,
            worker_running: Arc::new(AtomicBool::new(true)), // Initialize worker_running
            sink,
            state,
        }
    }

    /// Get the Last.fm client ready and start the scrobble timer thread.
    ///
    /// Returns whether the worker can run at all: with no usable client there is
    /// nothing to scrobble to and nothing to look a track up against.
    fn init(&mut self) -> bool {
        info!("Initializing Lastfm... Scrobbling enabled: {}", self.config.scrobble);

        let init_result = if self.config.api_key.is_empty() || self.config.api_secret.is_empty() {
            info!("Lastfm: API key or secret is empty in plugin configuration. Attempting to use default credentials.");
            LastfmClient::initialize_with_defaults()
        } else {
            LastfmClient::initialize(
                self.config.api_key.clone(),
                self.config.api_secret.clone(),
            )
        };

        if let Err(e) = init_result {
            error!("Lastfm: Failed to initialize Last.fm client: {}", e);
            return false;
        }
        info!("Lastfm: Last.fm client connection initialized/verified successfully.");

        let client_instance = match LastfmClient::get_instance() {
            Ok(client) => client,
            Err(e) => {
                error!("Lastfm: Failed to get Last.fm client instance: {}", e);
                return false;
            }
        };
        self.lastfm_client = Some(client_instance.clone());

        let track_data_for_thread = Arc::clone(&self.current_track_data);
        let worker_running_for_thread = Arc::clone(&self.worker_running);
        let scrobble_config_for_thread = self.config.scrobble;
        let sink_for_thread = Arc::clone(&self.sink);
        let state_for_thread = Arc::clone(&self.state);

        match thread::Builder::new()
            .name("lastfm-scrobbler".into())
            .spawn(move || {
                lastfm_worker(
                    track_data_for_thread,
                    WORKER_NAME.to_string(),
                    client_instance,
                    worker_running_for_thread,
                    scrobble_config_for_thread,
                    sink_for_thread,
                    state_for_thread,
                );
            })
        {
            Ok(handle) => {
                self.worker_thread = Some(handle);
                true
            }
            Err(e) => {
                error!("Lastfm: Failed to start the scrobble timer thread: {}", e);
                false
            }
        }
    }

    /// Read now-playing events until the channel closes, then stop the timer.
    fn run(mut self, events: Receiver<NowPlayingEvent>) {
        for event in events {
            match event {
                NowPlayingEvent::SongChanged { source, song } => {
                    self.handle_song_changed(&song, &source);
                }
                NowPlayingEvent::StateChanged { source, state } => {
                    self.handle_state_changed(&state, &source);
                }
            }
        }

        self.shutdown();
    }

    /// Stop the timer thread and wait for it, so a scrobble already in flight is
    /// not cut off half way.
    fn shutdown(&mut self) {
        info!("Lastfm shutdown initiated.");

        self.worker_running.store(false, Ordering::SeqCst);

        if let Some(handle) = self.worker_thread.take() {
            info!("Lastfm: Waiting for worker thread to join...");
            match handle.join() {
                Ok(_) => info!("Lastfm: Worker thread joined successfully."),
                Err(e) => error!("Lastfm: Failed to join worker thread: {:?}", e),
            }
        } else {
            info!("Lastfm: No worker thread to join.");
        }
    }

    /// Handle a song changed event
    fn handle_song_changed(&mut self, song_event_opt: &Option<Song>, source: &PlayerSource) {
        let mut track_data = self.current_track_data.lock();
        
        if let Some(song_event) = song_event_opt { 
            let new_name = song_event.title.clone(); 
            let new_artists_vec = song_event.artist.clone().map(|a| vec![a]); 
            let new_length = song_event.duration.map(|d| d.round() as u32);

            let is_different_song = track_data.name != new_name ||
                                    track_data.artists != new_artists_vec ||
                                    track_data.length != new_length;

            if is_different_song {
                let mut was_playing_before_change = false;
                if track_data.current_playback_state == PlaybackState::Playing {
                    if let Some(lpt) = track_data.last_play_timestamp {
                        let old_song_final_segment_ms = lpt.elapsed().unwrap_or_default().as_millis() as u64;
                        track_data.accumulated_play_duration_ms += old_song_final_segment_ms;
                        debug!("Lastfm: Old song ('{:?}') final segment {}ms. Total for old song: {}ms", track_data.name.as_deref(), old_song_final_segment_ms, track_data.accumulated_play_duration_ms);
                    }
                    was_playing_before_change = true;
                }
                
                track_data.name = new_name;
                track_data.artists = new_artists_vec;
                track_data.length = new_length;
                track_data.started_timestamp = Some(SystemTime::now());
                track_data.scrobbled_song = false; 
                track_data.accumulated_play_duration_ms = 0;
                track_data.song_details = Some(song_event.clone()); // Store the full Song object
                track_data.player_source = Some(source.clone()); // Store the PlayerSource
                track_data.track_info_fetched = false; // Reset flag for new song

                if was_playing_before_change {
                    track_data.last_play_timestamp = Some(SystemTime::now());
                } else {
                    track_data.last_play_timestamp = None;
                }
                
                info!(
                    "Lastfm: Song changed. New: {:?}-{:?} ({:?})s. Source: {:?}. Play counters reset. Assumed playing: {}. Stored song details.",
                    track_data.name.as_deref().unwrap_or("N/A"), 
                    track_data.artists.as_ref().map_or_else(
                        || "N/A".to_string(), 
                        |a_vec| a_vec.join(", ")
                    ), 
                    track_data.length.map_or_else(|| "N/A".to_string(), |l| l.to_string()),
                    track_data.player_source, // Log the source
                    was_playing_before_change
                );

                // Update Now Playing if the song changed and is now considered playing
                if (track_data.current_playback_state == PlaybackState::Playing || was_playing_before_change) && self.config.scrobble {
                     if let (Some(client), Some(name_str), Some(artists_vec)) =
                        (&self.lastfm_client, &track_data.name, &track_data.artists) {
                        if let Some(primary_artist) = artists_vec.first() {
                            info!("Lastfm: Updating Now Playing for '{}' by '{}' due to SongChanged.", name_str, primary_artist);
                            if let Err(e) = client.update_now_playing(primary_artist, name_str, None, None, None, track_data.length) {
                                warn!("Lastfm: Failed to update Now Playing: {}", e);
                            }
                        }
                    }
                }
            }
        } else { // song_event_opt is None
            if track_data.name.is_some() { 
                info!("Lastfm: Song changed to None (playback stopped), clearing track data.");
                if track_data.current_playback_state == PlaybackState::Playing {
                    if let Some(lpt) = track_data.last_play_timestamp {
                        let played_ms = lpt.elapsed().unwrap_or_default().as_millis() as u64;
                        debug!("Lastfm: Added {}ms from final segment of '{:?}'. Total for song: {}ms", 
                               played_ms, track_data.name.as_deref(), track_data.accumulated_play_duration_ms + played_ms);
                    }
                }
                let current_state = track_data.current_playback_state; // Preserve current playback state
                *track_data = CurrentScrobbleTrack::default(); 
                track_data.current_playback_state = current_state; // Restore playback state
                // player_source is now None due to default()
                info!("Lastfm: Track data cleared. Player source is now None.");
            }
        }
    }

    /// Handle a state changed event
    fn handle_state_changed(&mut self, new_player_state: &PlaybackState, event_source: &PlayerSource) {
        let mut track_data = self.current_track_data.lock();

        // If state changes, ensure player_source is consistent if a song is active
        if track_data.song_details.is_some() && track_data.player_source.as_ref() != Some(event_source) {
            // This might happen if events are interleaved, or if a player changes its source ID
            // For now, let's update it if different and a song is active.
            // Or, we might decide that the source from SongChanged is authoritative for the current song.
            // For now, let's prioritize the source from SongChanged.
            // If track_data.player_source is None but song_details is Some, it's an inconsistent state.
            if track_data.player_source.is_none() {
                 warn!("Lastfm: StateChanged for source {:?} while song {:?} is active but player_source was None. Updating to event_source.", event_source, track_data.name.as_deref());
                 track_data.player_source = Some(event_source.clone());
            }
        }

        if track_data.name.is_none() {
            debug!("Lastfm: StateChanged event ({:?}) but no active song. Current internal state: {:?}", new_player_state, track_data.current_playback_state);
            if *new_player_state == PlaybackState::Stopped || *new_player_state == PlaybackState::Killed || *new_player_state == PlaybackState::Disconnected {
                track_data.current_playback_state = *new_player_state;
                track_data.last_play_timestamp = None; 
            }
            return;
        }

        let old_player_state = track_data.current_playback_state;
        if old_player_state == *new_player_state {
            debug!("Lastfm: StateChanged event but state is the same ({:?}). No action.", new_player_state);
            return;
        }

        info!("Lastfm: StateChanged. Song: {:?}. Old state: {:?}, New state: {:?}.",
            track_data.name.as_deref().unwrap_or("N/A"),
            old_player_state,
            new_player_state);

        if old_player_state == PlaybackState::Playing && *new_player_state != PlaybackState::Playing {
            if let Some(lpt) = track_data.last_play_timestamp {
                let played_ms = lpt.elapsed().unwrap_or_default().as_millis() as u64;
                track_data.accumulated_play_duration_ms += played_ms;
                info!("Lastfm: Playback now '{:?}'. Added {}ms. Total accumulated: {}ms", new_player_state, played_ms, track_data.accumulated_play_duration_ms);
            }
            track_data.last_play_timestamp = None;
        } else if old_player_state != PlaybackState::Playing && *new_player_state == PlaybackState::Playing {
            info!("Lastfm: Playback now 'Playing'. Setting last_play_timestamp.");
            track_data.last_play_timestamp = Some(SystemTime::now());
            
            // Update Now Playing as state changed to Playing for the current song
            if let (Some(client), Some(name_str), Some(artists_vec)) =
                (&self.lastfm_client, &track_data.name, &track_data.artists) {
                if let Some(primary_artist) = artists_vec.first() {
                     info!("Lastfm: Updating Now Playing for '{}' by '{}' due to StateChanged to Playing.", name_str, primary_artist);
                    if self.config.scrobble { // Added self.config.scrobble check
                        if let Err(e) = client.update_now_playing(primary_artist, name_str, None, None, None, track_data.length) {
                            warn!("Lastfm: Failed to update Now Playing: {}", e);
                        }
                    }
                }
            }
        }
        
        track_data.current_playback_state = *new_player_state;
    }
}

/// Start the Last.fm worker on `events`.
///
/// Returns whether it is running and wants those events, so a caller with
/// nothing to feed can stop feeding it. A disabled entry starts nothing, which
/// is what `enabled: false` has always meant.
pub fn start(
    config: LastfmWorkerConfig,
    events: Receiver<NowPlayingEvent>,
    sink: Arc<dyn SongInformationSink>,
    state: Arc<dyn PlaybackStateSource>,
) -> bool {
    if !config.enabled {
        info!("Lastfm is disabled by configuration. Skipping initialization.");
        return false;
    }

    let mut worker = Lastfm::new(config, sink, state);
    if !worker.init() {
        return false;
    }

    match thread::Builder::new()
        .name("lastfm-worker".into())
        .spawn(move || worker.run(events))
    {
        Ok(_) => true,
        Err(e) => {
            error!("Lastfm: Failed to start the worker thread: {}", e);
            false
        }
    }
}

// calculate_updates is a free function rather than a method: it is a pure
// mapping from a song and a Last.fm answer to the partial that describes what
// changed, which is what makes it testable without a client.

/// Whether a Last.fm answer came from a signed request, and so carries the
/// fields that need a linked account.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UserData {
    /// A signed lookup: `userloved` and `userplaycount` mean what they say.
    Present,
    /// An unsigned lookup: those fields are absent and take their defaults,
    /// which must not be read as "not loved" or "never played".
    Absent,
}

fn calculate_updates(
    original_song: &Song,
    lastfm_data: &LastfmTrackInfoDetails,
    user_data: UserData,
) -> Song {
    let mut updated_song = Song {
        title: original_song.title.clone(),
        artist: original_song.artist.clone(),
        ..Default::default()
    };

    // --- 1. Handle cover_art_url ---
    // The same selection the cover art provider uses, rather than a second
    // copy: Last.fm leaves size slots empty, and taking extralarge alone gave
    // up on any album whose extralarge slot happened to be one of them.
    let lastfm_provided_cover_art_url =
        crate::coverart_providers::album_image_urls(lastfm_data)
            .into_iter()
            .next();

    // Only update cover_art_url if the original song's is replaceable — it has
    // none, or what it has is a placeholder such as a radio station's logo — and
    // Last.fm provides one. Where the song carries its own artwork,
    // updated_song.cover_art_url stays None (from Song::default()), indicating
    // no change for this field in the partial update event.
    if original_song.cover_art_is_replaceable() {
        if let Some(ref url) = lastfm_provided_cover_art_url {
            updated_song.cover_art_url = Some(url.clone());
            // Record where the new image came from, so what replaced a
            // placeholder is not itself mistaken for one later on.
            updated_song.metadata.insert(
                acr_types::song::COVER_ART_SOURCE.to_string(),
                serde_json::Value::String(
                    acr_types::song::COVER_ART_SOURCE_LASTFM.to_string(),
                ),
            );
            debug!("calculate_updates: cover_art_url updated to {}", url);
        }
    }

    // --- 2. Handle liked status ---
    // Only from a signed answer. An unsigned one carries no userloved at all,
    // and it deserialises to false in its absence, which would be reported as
    // the track having been un-loved.
    if user_data == UserData::Present {
        let lastfm_liked_value = Some(lastfm_data.userloved);

        // Check if the liked status from Last.fm is different from the original song's liked status.
        if lastfm_liked_value != original_song.liked {
            updated_song.liked = lastfm_liked_value;
            debug!("calculate_updates: liked status updated to {:?}.", updated_song.liked);
        }
    }

    // --- 3. Handle metadata: lastfm_playcount ---
    // Also signed-only, for the same reason: absence is not a playcount of nil.
    if user_data == UserData::Absent {
        return updated_song;
    }

    let mut lastfm_provided_playcount_json: Option<serde_json::Value> = None;
    if let Some(user_playcount_str) = &lastfm_data.user_playcount {
        if !user_playcount_str.is_empty() {
            lastfm_provided_playcount_json = Some(serde_json::Value::String(user_playcount_str.clone()));
        }
    }
    let original_playcount_json = original_song.metadata.get("lastfm_playcount").cloned();

    // Check if the playcount from Last.fm (or its absence) is different from the original.
    if lastfm_provided_playcount_json != original_playcount_json {
        if let Some(pc_json) = lastfm_provided_playcount_json {
            updated_song.metadata.insert("lastfm_playcount".to_string(), pc_json.clone());
            debug!("calculate_updates: metadata 'lastfm_playcount' updated to {:?}.", pc_json);
        } else {
            // lastfm_provided_playcount_json is None. If original_song had this metadata, it's a change.
            // updated_song.metadata will not contain "lastfm_playcount" by default.
            if original_playcount_json.is_some() {
                debug!("calculate_updates: metadata 'lastfm_playcount' changed from Some to None.");
            }
        }
    }
    
    updated_song
}

#[cfg(test)]
mod tests {
    use super::*;
    use acr_types::song::{
        COVER_ART_SOURCE, COVER_ART_SOURCE_LASTFM, COVER_ART_SOURCE_STATION_LOGO,
    };

    /// Track info carrying the given size slots, as Last.fm fills them.
    fn track_info_with_images(images: &[(&str, &str)]) -> LastfmTrackInfoDetails {
        let image: Vec<_> = images
            .iter()
            .map(|(size, url)| serde_json::json!({ "#text": url, "size": size }))
            .collect();
        serde_json::from_value(serde_json::json!({
            "name": "Listen To The News",
            "url": "https://www.last.fm/music/example",
            "duration": "0",
            "listeners": "1",
            "playcount": "1",
            "artist": {
                "name": "Radical Friendship Theory",
                "url": "https://www.last.fm/music/example"
            },
            "album": {
                "artist": "Radical Friendship Theory",
                "title": "Radical Friendship Theory",
                "url": "https://www.last.fm/music/example/album",
                "image": image
            }
        }))
        .expect("track info fixture should deserialize")
    }

    /// Track info as Last.fm returns it for a track whose album it knows.
    fn track_info_with_album_image(url: &str) -> LastfmTrackInfoDetails {
        serde_json::from_value(serde_json::json!({
            "name": "Listen To The News",
            "url": "https://www.last.fm/music/example",
            "duration": "0",
            "listeners": "1",
            "playcount": "1",
            "artist": {
                "name": "Radical Friendship Theory",
                "url": "https://www.last.fm/music/example"
            },
            "album": {
                "artist": "Radical Friendship Theory",
                "title": "Radical Friendship Theory",
                "url": "https://www.last.fm/music/example/album",
                "image": [{ "#text": url, "size": "extralarge" }]
            }
        }))
        .expect("track info fixture should deserialize")
    }

    /// A radio stream reaches the plugin carrying the station's logo as cover
    /// art. The logo identifies the station, not what is playing, so the real
    /// album art Last.fm knows about has to win.
    #[test]
    fn station_logo_is_replaced_by_lastfm_album_art() {
        let mut song = Song {
            cover_art_url: Some("https://www.byte.fm/favicon.png".to_string()),
            ..Default::default()
        };
        song.metadata.insert(
            COVER_ART_SOURCE.to_string(),
            serde_json::Value::String(COVER_ART_SOURCE_STATION_LOGO.to_string()),
        );
        let info = track_info_with_album_image("https://lastfm.example/cover.png");

        let update = calculate_updates(&song, &info, UserData::Present);

        assert_eq!(
            update.cover_art_url,
            Some("https://lastfm.example/cover.png".to_string()),
            "a station logo should give way to the track's own album art"
        );
    }

    /// Replacing the logo has to record where the new image came from.
    /// Otherwise the marker still says "station logo" while a real cover is in
    /// place, and the next lookup would feel free to overwrite it.
    #[test]
    fn replacing_the_station_logo_records_the_new_source() {
        let mut song = Song {
            cover_art_url: Some("https://www.byte.fm/favicon.png".to_string()),
            ..Default::default()
        };
        song.metadata.insert(
            COVER_ART_SOURCE.to_string(),
            serde_json::Value::String(COVER_ART_SOURCE_STATION_LOGO.to_string()),
        );
        let info = track_info_with_album_image("https://lastfm.example/cover.png");

        let update = calculate_updates(&song, &info, UserData::Present);

        assert_eq!(
            update
                .metadata
                .get(COVER_ART_SOURCE)
                .and_then(|value| value.as_str()),
            Some(COVER_ART_SOURCE_LASTFM),
            "the replacement should record its own provenance"
        );
    }

    /// Once the real cover art has been merged in, the song must no longer look
    /// replaceable — the placeholder is gone.
    #[test]
    fn replaced_cover_art_is_no_longer_replaceable() {
        let mut song = Song {
            cover_art_url: Some("https://www.byte.fm/favicon.png".to_string()),
            ..Default::default()
        };
        song.metadata.insert(
            COVER_ART_SOURCE.to_string(),
            serde_json::Value::String(COVER_ART_SOURCE_STATION_LOGO.to_string()),
        );
        let info = track_info_with_album_image("https://lastfm.example/cover.png");

        let update = calculate_updates(&song, &info, UserData::Present);
        merge_song_updates(&mut song, &update);

        assert!(
            !song.cover_art_is_replaceable(),
            "real cover art must not stay marked as a placeholder"
        );
    }

    /// Last.fm pads slots it has nothing for with an empty string, so an album
    /// whose extralarge slot is empty but which has a larger or smaller one
    /// must still yield an image. Taking extralarge alone left the station
    /// showing its logo for exactly those albums.
    #[test]
    fn cover_art_uses_the_largest_slot_rather_than_only_extralarge() {
        let mut song = Song {
            cover_art_url: Some("https://www.byte.fm/favicon.png".to_string()),
            ..Default::default()
        };
        song.metadata.insert(
            COVER_ART_SOURCE.to_string(),
            serde_json::Value::String(COVER_ART_SOURCE_STATION_LOGO.to_string()),
        );
        let info = track_info_with_images(&[
            ("large", "https://lastfm.example/i/u/174s/cover.png"),
            ("extralarge", ""),
            ("mega", "https://lastfm.example/i/u/cover.png"),
        ]);

        let update = calculate_updates(&song, &info, UserData::Present);

        assert_eq!(
            update.cover_art_url,
            Some("https://lastfm.example/i/u/cover.png".to_string())
        );
    }

    /// An unsigned lookup carries no user fields. `userloved` deserialises to
    /// false in their absence, which must not be reported as the track having
    /// been un-loved.
    #[test]
    fn an_unsigned_answer_does_not_report_a_liked_status() {
        let song = Song {
            liked: Some(true),
            ..Default::default()
        };
        let info = track_info_with_album_image("https://lastfm.example/cover.png");

        let update = calculate_updates(&song, &info, UserData::Absent);

        assert_eq!(
            update.liked, None,
            "an unsigned answer says nothing about whether the track is loved"
        );
    }

    /// The cover art is the whole point of the unsigned lookup, so it still
    /// arrives.
    #[test]
    fn an_unsigned_answer_still_updates_cover_art() {
        let song = Song::default();
        let info = track_info_with_album_image("https://lastfm.example/cover.png");

        let update = calculate_updates(&song, &info, UserData::Absent);

        assert_eq!(
            update.cover_art_url,
            Some("https://lastfm.example/cover.png".to_string())
        );
    }

    /// A signed answer still reports the loved status, as it always has.
    #[test]
    fn a_signed_answer_reports_the_liked_status() {
        let song = Song {
            liked: Some(true),
            ..Default::default()
        };
        let info = track_info_with_album_image("https://lastfm.example/cover.png");

        let update = calculate_updates(&song, &info, UserData::Present);

        assert_eq!(update.liked, Some(false));
    }

    /// Artwork that belongs to the song itself is not a placeholder, so a
    /// partial update must leave it alone.
    #[test]
    fn real_cover_art_survives_lastfm_update() {
        let song = Song {
            cover_art_url: Some("https://example.com/cover.jpg".to_string()),
            ..Default::default()
        };
        let info = track_info_with_album_image("https://lastfm.example/cover.png");

        let update = calculate_updates(&song, &info, UserData::Present);

        assert_eq!(
            update.cover_art_url, None,
            "the song's own cover art must not be overwritten"
        );
    }

    /// A song with no cover art at all is still filled in from Last.fm.
    #[test]
    fn missing_cover_art_is_filled_from_lastfm() {
        let song = Song::default();
        let info = track_info_with_album_image("https://lastfm.example/cover.png");

        let update = calculate_updates(&song, &info, UserData::Present);

        assert_eq!(
            update.cover_art_url,
            Some("https://lastfm.example/cover.png".to_string())
        );
    }
}
