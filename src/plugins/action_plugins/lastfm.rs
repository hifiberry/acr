use std::any::Any;
use std::sync::Weak;
use std::thread;
use std::time::Duration;
use std::sync::Arc;
use parking_lot::Mutex;
use std::time::SystemTime;
use std::sync::atomic::{AtomicBool, Ordering}; // Added

use crate::audiocontrol::AudioController;
use crate::data::PlayerEvent;
use crate::data::Song; // Added import for Song struct
use crate::helpers::lastfm::{LastfmClient, LastfmTrackInfoDetails}; // Added LastfmTrackInfoDetails
use crate::plugins::action_plugin::{ActionPlugin, BaseActionPlugin};
use crate::plugins::plugin::Plugin;
use log::{debug, error, info, warn, trace};
use serde::Deserialize;
use crate::data::PlaybackState;
use crate::players::PlayerController; // Added for get_playback_state
use crate::data::PlayerSource; // Added PlayerSource

#[derive(Debug, Deserialize, Clone)]
pub struct LastfmConfig {
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
    base: BaseActionPlugin,
    config: LastfmConfig,
    worker_thread: Option<thread::JoinHandle<()>>,
    current_track_data: Arc<Mutex<CurrentScrobbleTrack>>,
    lastfm_client: Option<LastfmClient>,
    worker_running: Arc<AtomicBool>, // Added for graceful shutdown
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
    // TODO: Consider passing audiocontrol_tx here if needed for sending events
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
        if !track_data.track_info_fetched && client.is_authenticated() {
            // Separate the immutable borrow for player_source
            let player_source_clone = track_data.player_source.clone();
            let song_title_clone = track_data.song_details.as_ref().and_then(|sd| sd.title.clone());
            let song_artist_clone = track_data.song_details.as_ref().and_then(|sd| sd.artist.clone());

            if let (Some(title), Some(artist)) = (song_title_clone, song_artist_clone) {
                if let Some(current_player_source) = player_source_clone {
                    info!("LastFMWorker: Attempting to get track info for '{}' by '{}'", title, artist);
                    match client.get_track_info(&artist, &title) {
                        Ok(track_info_details) => {
                            // Now, we need to re-access song_details mutably.
                            // It's important that the immutable borrows above are out of scope.
                            if let Some(original_song_details_ref) = &mut track_data.song_details {
                                let updated_song_partial = calculate_updates(original_song_details_ref, &track_info_details);
                                
                                let event = PlayerEvent::SongInformationUpdate {
                                    source: current_player_source.clone(), // Use the cloned source
                                    song: updated_song_partial.clone()
                                };
                                debug!("LastFMWorker: Publishing SongInformationUpdate to event bus with partial data: {:?}", updated_song_partial);
                                crate::audiocontrol::eventbus::EventBus::instance().publish(event);

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
            let audio_controller = AudioController::instance(); // Get global instance
            let actual_player_state = audio_controller.get_playback_state(); // Get state of active player

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
    pub fn new(config: LastfmConfig) -> Self {
        Self {
            base: BaseActionPlugin::new("Lastfm"),
            config,
            worker_thread: None,
            current_track_data: Arc::new(Mutex::new(CurrentScrobbleTrack::default())),
            lastfm_client: None,
            worker_running: Arc::new(AtomicBool::new(true)), // Initialize worker_running
        }
    }
    
    /// Start the worker thread for Last.fm scrobbling
    fn start_worker_thread(&mut self) {
        if self.lastfm_client.is_none() {
            if let Ok(client_instance) = LastfmClient::get_instance() {
                self.lastfm_client = Some(client_instance.clone());
                
                // Set up the worker thread
                let track_data_for_thread = Arc::clone(&self.current_track_data);
                let plugin_name_for_thread = self.name().to_string();
                let client_for_thread = client_instance; 
                let worker_running_for_thread = Arc::clone(&self.worker_running);
                let scrobble_config_for_thread = self.config.scrobble;

                let handle = thread::spawn(move || {
                    lastfm_worker(
                        track_data_for_thread,
                        plugin_name_for_thread,
                        client_for_thread,
                        worker_running_for_thread,
                        scrobble_config_for_thread
                    );
                });
                
                self.worker_thread = Some(handle);
                log::info!("Lastfm: Worker thread started");
            } else {
                log::error!("Lastfm: Failed to get Last.fm client instance, cannot start worker thread");
            }
        } else {
            // We already have a client, but need to start the worker
            if let Some(client_instance) = &self.lastfm_client {
                let track_data_for_thread = Arc::clone(&self.current_track_data);
                let plugin_name_for_thread = self.name().to_string();
                let client_for_thread = client_instance.clone();
                let worker_running_for_thread = Arc::clone(&self.worker_running);
                let scrobble_config_for_thread = self.config.scrobble;

                let handle = thread::spawn(move || {
                    lastfm_worker(
                        track_data_for_thread,
                        plugin_name_for_thread,
                        client_for_thread,
                        worker_running_for_thread,
                        scrobble_config_for_thread
                    );
                });
                
                self.worker_thread = Some(handle);
                log::info!("Lastfm: Worker thread started");
            }
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

    /// Create a handler for events coming from the event bus
    fn handle_event_bus_events(&self, event: PlayerEvent) {
        trace!("Received event from event bus");
        
        // First determine if this is from the active player
        let _is_active_player = if let Some(controller) = self.base.get_controller() {
            // Get player ID from the event
            let event_player_id = match event.source() {
                Some(source) => source.player_id(),
                None => "system",
            };
            
            // Get ID of the active player from AudioController
            let active_player_id = controller.get_player_id();
            
            // Event is from active player if IDs match
            event_player_id == active_player_id
        } else {
            false
        };
        
        // Now handle the event the same way we would in on_event
        // We use a clone here since our method takes &self rather than &mut self
        // and we need to update internal state
        if !self.config.enabled {
            return;
        }
        
        match &event {
            PlayerEvent::SongChanged { song: song_event_opt, source, .. } => {
                let lastfm_arc = Arc::new(Mutex::new(self.clone()));
                let mut lastfm = lastfm_arc.lock();
                lastfm.handle_song_changed(song_event_opt, source);
            }
            PlayerEvent::StateChanged { state: new_player_state, source: event_source, .. } => {
                let lastfm_arc = Arc::new(Mutex::new(self.clone()));
                let mut lastfm = lastfm_arc.lock();
                lastfm.handle_state_changed(new_player_state, event_source);
            }
            _ => {
                // Other events are ignored for now
            }
        }
    }
}

impl Plugin for Lastfm {
    fn name(&self) -> &str {
        self.base.name()
    }

    fn version(&self) -> &str {
        self.base.version()
    }    fn init(&mut self) -> bool {
        if !self.config.enabled {
            info!("Lastfm is disabled by configuration. Skipping initialization.");
            return true;
        }

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

        match init_result {
            Ok(_) => {
                info!("Lastfm: Last.fm client connection initialized/verified successfully.");
                
                match LastfmClient::get_instance() {
                    Ok(client_instance) => {
                        self.lastfm_client = Some(client_instance.clone()); 

                        let track_data_for_thread = Arc::clone(&self.current_track_data);
                        let plugin_name_for_thread = self.name().to_string();
                        let client_for_thread = client_instance; 
                        let worker_running_for_thread = Arc::clone(&self.worker_running); // Clone for thread
                        let scrobble_config_for_thread = self.config.scrobble; // Added

                        let handle = thread::spawn(move || {
                            lastfm_worker(track_data_for_thread, plugin_name_for_thread, client_for_thread, worker_running_for_thread, scrobble_config_for_thread);
                        });
                        self.worker_thread = Some(handle);
                        
                        self.base.init()
                    }
                    Err(e) => {
                        error!("Lastfm: Failed to get Last.fm client instance: {}", e);
                        false
                    }
                }
            }
            Err(e) => {
                error!("Lastfm: Failed to initialize Last.fm client: {}", e); // Updated log
                false
            }
        }
    }    fn shutdown(&mut self) -> bool {
        info!("Lastfm shutdown initiated."); // Updated log
        
        // Signal the worker thread to stop
        self.worker_running.store(false, Ordering::SeqCst);

        // Wait for the worker thread to finish
        if let Some(handle) = self.worker_thread.take() {
            info!("Lastfm: Waiting for worker thread to join...");
            match handle.join() {
                Ok(_) => info!("Lastfm: Worker thread joined successfully."),
                Err(e) => error!("Lastfm: Failed to join worker thread: {:?}", e),
            }
        } else {
            info!("Lastfm: No worker thread to join.");
        }
        
        // Unsubscribe from event bus
        self.base.unsubscribe_from_event_bus();
        log::debug!("Lastfm: Unsubscribed from event bus");
        
        // Perform shutdown tasks from BaseActionPlugin
        self.base.shutdown()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl ActionPlugin for Lastfm {
    fn initialize(&mut self, controller: Weak<AudioController>) {
        self.base.set_controller(controller);
        
        // Only subscribe if enabled
        if !self.config.enabled {
            log::info!("Lastfm plugin is disabled, not subscribing to events");
            return;
        }
        
        // Subscribe to event bus in the initialize method
        log::debug!("Lastfm initializing and subscribing to event bus");
        let self_clone = self.clone();
        self.base.subscribe_to_event_bus(move |event| {
            self_clone.handle_event(event);
        });
        
        // Initialize worker thread
        if self.config.enabled && self.worker_thread.is_none() {
            // Start the worker thread
            log::debug!("Starting lastfm worker thread");
            self.start_worker_thread();
        }
    }
    
    fn handle_event(&self, event: PlayerEvent) {
        // Handle events using the existing method
        self.handle_event_bus_events(event);
    }
}

// Clone implementation for Lastfm to allow for passing to thread
impl Clone for Lastfm {
    fn clone(&self) -> Self {
        let mut new_base = BaseActionPlugin::new(self.base.name());
        
        // Get the controller reference from the original object
        if let Some(controller) = self.base.get_controller() {
            // The controller is already an Arc, we need to downgrade it to a Weak
            let controller_weak = Arc::downgrade(&controller);
            new_base.set_controller(controller_weak);
        }
        
        Self {
            base: new_base,
            config: self.config.clone(),
            worker_thread: None,
            current_track_data: Arc::clone(&self.current_track_data),
            lastfm_client: self.lastfm_client.clone(),
            worker_running: Arc::clone(&self.worker_running),
        }
    }
}

// Add the calculate_updates function definition here
// It should be outside any impl blocks, typically as a free function in the module.

fn calculate_updates(original_song: &Song, lastfm_data: &LastfmTrackInfoDetails) -> Song {
    let mut updated_song = Song {
        title: original_song.title.clone(),
        artist: original_song.artist.clone(),
        ..Default::default()
    };

    // --- 1. Handle cover_art_url ---
    let mut lastfm_provided_cover_art_url: Option<String> = None;
    if let Some(album_info) = &lastfm_data.album {
        if let Some(extralarge_image) = album_info.image.iter().find(|img| img.size == "extralarge") {
            if !extralarge_image.url.is_empty() {
                lastfm_provided_cover_art_url = Some(extralarge_image.url.clone());
            }
        }
    }

    // Only update cover_art_url if the original song does not have one,
    // and Last.fm provides one. This signifies a change from None to Some.
    // If the original song already has a cover_art_url, updated_song.cover_art_url
    // will remain None (from Song::default()), indicating no change for this field
    // in the partial update event.
    if original_song.cover_art_url.is_none() {
        if let Some(ref url) = lastfm_provided_cover_art_url {
            updated_song.cover_art_url = Some(url.clone());
            debug!("calculate_updates: cover_art_url updated to {}", url);
        }
    }

    // --- 2. Handle liked status ---
    let lastfm_liked_value = Some(lastfm_data.userloved); // lastfm_data.userloved is bool

    // Check if the liked status from Last.fm is different from the original song's liked status.
    if lastfm_liked_value != original_song.liked {
        updated_song.liked = lastfm_liked_value;
        debug!("calculate_updates: liked status updated to {:?}.", updated_song.liked);
    }

    // --- 3. Handle metadata: lastfm_playcount ---
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
