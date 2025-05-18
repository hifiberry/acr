use std::any::Any;
use std::sync::Weak;
use std::thread;
use std::time::Duration;
use std::sync::{Arc, Mutex};
use std::time::SystemTime;
use std::sync::atomic::{AtomicBool, Ordering}; // Added

use crate::audiocontrol::AudioController;
use crate::data::PlayerEvent;
use crate::data::Song; // Added import for Song struct
use crate::helpers::lastfm::LastfmClient;
use crate::plugins::action_plugin::{ActionPlugin, BaseActionPlugin};
use crate::plugins::plugin::Plugin;
use log::{debug, error, info, warn};
use serde::Deserialize;
use crate::data::PlaybackState;
use crate::players::PlayerController; // Added for get_playback_state

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
        }
    }
}

// Background worker function
fn lastfm_worker(
    track_data_arc: Arc<Mutex<CurrentScrobbleTrack>>,
    plugin_name: String,
    client: LastfmClient,
    worker_running: Arc<AtomicBool>, // Added
    scrobble_enabled: bool, // Added
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

        let mut track_data = track_data_arc.lock().unwrap();

        // Fetch track info if new song and not yet fetched
        if let Some(song) = &track_data.song_details {
            if !track_data.track_info_fetched && client.is_authenticated() {
                if let (Some(title), Some(artist)) = (&song.title, &song.artist) {
                    info!("LastFMWorker: Attempting to get track info for '{}' by '{}'", title, artist);
                    match client.get_track_info(artist, title) {
                        Ok(track_info_details) => {
                            warn!("LastFMWorker: Track Info Details: {:?}", track_info_details);
                        }
                        Err(e) => {
                            warn!("LastFMWorker: Failed to get track info for '{} - {}': {:?}", title, artist, e);
                        }
                    }
                    track_data.track_info_fetched = true;
                } else {
                    // This case should ideally not happen if song_details is Some and populated correctly
                    warn!("LastFMWorker: Cannot get track info, title or artist missing from stored song details. Title: {:?}, Artist: {:?}, Fetched Flag: {}", song.title, song.artist, track_data.track_info_fetched);
                    // Mark as fetched to avoid retrying if data is persistently missing for this song object
                    track_data.track_info_fetched = true; 
                }
            }
        }

        // Periodic state check (e.g., every 30 seconds)
        if loop_count % 30 == 0 {
            info!("LastFMWorker: Performing periodic state check.");
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
            if track_data.current_playback_state == PlaybackState::Playing {
                if !track_data.scrobbled_song && scrobble_enabled { // Added scrobble_enabled check
                    // let scrobble_point_duration_secs = *length_val / 2; // length_val is &u32
                    let scrobble_point_time_secs = 240; // 4 minutes in seconds, Last.fm recommendation
                    

                    if effective_elapsed_seconds >= u64::from(*length_val).saturating_mul(50) / 100 || effective_elapsed_seconds >= scrobble_point_time_secs {
                        
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
                            // Potentially mark as scrobbled to avoid retries if artist will never be available for this track
                            // track_data.scrobbled_song = true; // Or handle differently
                        }
                    }
                }
            }
        } else {
            if track_data.name.is_none() {
                 debug!("LastFMWorker: No song actively tracked.");
            } else {
                 info!("LastFMWorker: Track data incomplete. Name: {:?}, Artists: {:?}, Length: {:?}, Started: {:?}",
                    track_data.name.is_some(), track_data.artists.is_some(), track_data.length.is_some(), track_data.started_timestamp.is_some());
            }
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
}

impl Plugin for Lastfm {
    fn name(&self) -> &str {
        self.base.name()
    }

    fn version(&self) -> &str {
        self.base.version()
    }

    fn init(&mut self) -> bool {
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
                            lastfm_worker(track_data_for_thread, plugin_name_for_thread, client_for_thread, worker_running_for_thread, scrobble_config_for_thread); // Pass worker_running and scrobble_config
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
    }

    fn shutdown(&mut self) -> bool {
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
        
        // Perform other shutdown tasks from BaseActionPlugin
        self.base.shutdown()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl ActionPlugin for Lastfm {
    fn initialize(&mut self, controller: Weak<AudioController>) {
        self.base.set_controller(controller);
        info!("Lastfm received controller reference."); // Updated log
    }

    fn on_event(&mut self, event: &PlayerEvent, _is_active_player: bool) {
        if !self.config.enabled {
            return;
        }

        match event {
            PlayerEvent::SongChanged { song: song_event_opt, .. } => { 
                let mut track_data = self.current_track_data.lock().unwrap();
                
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
                        track_data.track_info_fetched = false; // Reset flag for new song

                        if was_playing_before_change {
                            track_data.last_play_timestamp = Some(SystemTime::now());
                        } else {
                            track_data.last_play_timestamp = None;
                        }
                        
                        info!(
                            "Lastfm: Song changed. New: {:?}-{:?} ({:?})s. Play counters reset. Assumed playing: {}. Stored song details.",
                            track_data.name.as_deref().unwrap_or("N/A"), 
                            track_data.artists.as_ref().map_or_else(
                                || "N/A".to_string(), 
                                |a_vec| a_vec.join(", ")
                            ), 
                            track_data.length.map_or_else(|| "N/A".to_string(), |l| l.to_string()),
                            was_playing_before_change
                        );

                        // Update Now Playing if the song changed and is now considered playing
                        if (track_data.current_playback_state == PlaybackState::Playing || was_playing_before_change) && self.config.scrobble {
                             if let (Some(client), Some(name_str), Some(artists_vec)) =
                                (&self.lastfm_client, &track_data.name, &track_data.artists) {
                                if let Some(primary_artist) = artists_vec.first() {
                                    info!("Lastfm: Updating Now Playing for \\'{}\\' by \\'{}\\' due to SongChanged.", name_str, primary_artist);
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
                                debug!("Lastfm: Added {}ms from final segment of \'{:?}\'. Total for song: {}ms", 
                                       played_ms, track_data.name.as_deref(), track_data.accumulated_play_duration_ms + played_ms);
                            }
                        }
                        *track_data = CurrentScrobbleTrack::default(); // This will also clear song_details
                    }
                }
            }
            PlayerEvent::StateChanged { state: new_player_state, .. } => { 
                let mut track_data = self.current_track_data.lock().unwrap();

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
                             info!("Lastfm: Updating Now Playing for \'{}\' by \'{}\' due to StateChanged to Playing.", name_str, primary_artist);
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
            _ => {
                // Other events are ignored for now
            }
        }
    }
}
