use std::any::Any;
use std::sync::Weak;
use std::thread;
use std::time::Duration;
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use crate::audiocontrol::AudioController;
use crate::data::PlayerEvent;
use crate::helpers::lastfm::LastfmClient;
use crate::plugins::action_plugin::{ActionPlugin, BaseActionPlugin};
use crate::plugins::plugin::Plugin;
use log::{debug, error, info, warn};
use serde::Deserialize;
use crate::data::PlaybackState; // Added import

#[derive(Debug, Deserialize, Clone)]
pub struct LastfmConfig {
    pub enabled: bool,
    pub api_key: String,
    pub api_secret: String,
}

pub struct Lastfm {
    base: BaseActionPlugin,
    config: LastfmConfig,
    worker_thread: Option<thread::JoinHandle<()>>,
    current_track_data: Arc<Mutex<CurrentScrobbleTrack>>,
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
        }
    }
}

// Background worker function
fn lastfm_worker(track_data_arc: Arc<Mutex<CurrentScrobbleTrack>>, plugin_name: String) {
    info!("Lastfm background worker started for plugin: {}", plugin_name);
    loop {
        thread::sleep(Duration::from_secs(1)); 

        let mut track_data = track_data_arc.lock().unwrap();

        if let (Some(name), Some(artists), Some(length_val), Some(_started_time)) =
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

            info!(
                "LastfmWorker: Song: '{}' by {}. State: {:?}. Length: {}s. Played: {}s (Accum: {}ms, CurrentSeg: {}ms). Scrobbled: {}",
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
                if !track_data.scrobbled_song {
                    let scrobble_point_duration_secs = *length_val / 2; // length_val is &u32
                    let scrobble_point_time_secs = 120; // 2 minutes in seconds

                    if effective_elapsed_seconds > scrobble_point_duration_secs as u64 || effective_elapsed_seconds > scrobble_point_time_secs as u64 {
                        warn!("LastfmWorker: Scrobble '{}' by {}! (Played {}s)", name, artists_str, effective_elapsed_seconds);
                        track_data.scrobbled_song = true;
                    }
                }
            }
        } else {
            if track_data.name.is_none() {
                 info!("LastfmWorker: No song actively tracked.");
            } else {
                 info!("LastfmWorker: Track data incomplete. Name: {:?}, Artists: {:?}, Length: {:?}, Started: {:?}",
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
            current_track_data: Arc::new(Mutex::new(CurrentScrobbleTrack::default())), // Initialize shared data
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
            info!("Lastfm is disabled by configuration. Skipping initialization."); // Updated log
            return true;
        }

        info!("Initializing Lastfm..."); // Updated log

        let init_result = if self.config.api_key.is_empty() || self.config.api_secret.is_empty() {
            info!("Lastfm: API key or secret is empty in plugin configuration. Attempting to use default credentials."); // Updated log
            LastfmClient::initialize_with_defaults()
        } else {
            LastfmClient::initialize(
                self.config.api_key.clone(),
                self.config.api_secret.clone(),
            )
        };

        match init_result {
            Ok(_) => {
                info!("Lastfm: Last.fm client connection initialized/verified successfully."); // Updated log

                // Prepare data for the worker thread
                let track_data_for_thread = Arc::clone(&self.current_track_data);
                let plugin_name_for_thread = self.name().to_string();
                
                let handle = thread::spawn(move || {
                    lastfm_worker(track_data_for_thread, plugin_name_for_thread);
                });
                self.worker_thread = Some(handle);

                self.base.init()
            }
            Err(e) => {
                error!("Lastfm: Failed to initialize Last.fm client: {}", e); // Updated log
                false
            }
        }
    }

    fn shutdown(&mut self) -> bool {
        info!("Lastfm shutdown."); // Updated log
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
                    let new_artists = song_event.artist.clone().map(|a| vec![a]); 
                    let new_length = song_event.duration.map(|d| d.round() as u32);

                    let is_different_song = track_data.name != new_name ||
                                            track_data.artists != new_artists ||
                                            track_data.length != new_length;

                    if is_different_song {
                        let mut was_playing_before_change = false;
                        if track_data.current_playback_state == PlaybackState::Playing {
                            if let Some(lpt) = track_data.last_play_timestamp {
                                // Add remaining duration of the old song before switching
                                let old_song_final_segment_ms = lpt.elapsed().unwrap_or_default().as_millis() as u64;
                                track_data.accumulated_play_duration_ms += old_song_final_segment_ms;
                                debug!("Lastfm: Old song ('{:?}') final segment {}ms. Total for old song: {}ms", track_data.name.as_deref(), old_song_final_segment_ms, track_data.accumulated_play_duration_ms);
                            }
                            was_playing_before_change = true;
                        }
                        
                        track_data.name = new_name;
                        track_data.artists = new_artists;
                        track_data.length = new_length;
                        track_data.started_timestamp = Some(SystemTime::now());
                        track_data.scrobbled_song = false; 
                        track_data.accumulated_play_duration_ms = 0; // Reset for the new song

                        if was_playing_before_change {
                            // If the player was playing, assume the new song starts playing immediately.
                            track_data.last_play_timestamp = Some(SystemTime::now());
                            // current_playback_state remains PlaybackState::Playing
                        } else {
                            // If player wasn't playing, new song starts in a non-playing state regarding its own counters.
                            track_data.last_play_timestamp = None;
                            // current_playback_state will be updated by a subsequent StateChanged event if it changes.
                        }
                        
                        info!(
                            "Lastfm: Song changed. New: {:?}-{:?} ({:?})s. Play counters reset. Assumed playing: {}",
                            track_data.name.as_deref().unwrap_or("N/A"), 
                            track_data.artists.as_ref().map_or_else(
                                || "N/A".to_string(), 
                                |a_vec| a_vec.join(", ")
                            ), 
                            track_data.length.map_or_else(|| "N/A".to_string(), |l| l.to_string()),
                            was_playing_before_change
                        );
                    }
                } else {
                    // Current song is None, meaning playback stopped or track is unknown
                    if track_data.name.is_some() { // Check if there was a song before
                        info!("Lastfm: Song changed to None (playback stopped), clearing track data.");
                        if track_data.current_playback_state == PlaybackState::Playing {
                            if let Some(lpt) = track_data.last_play_timestamp {
                                let played_ms = lpt.elapsed().unwrap_or_default().as_millis() as u64;
                                // This accumulation is for the song that just ended.
                                // It's logged here but will be wiped by default() next.
                                debug!("Lastfm: Added {}ms from final segment of '{:?}'. Total for song: {}ms", 
                                       played_ms, track_data.name.as_deref(), track_data.accumulated_play_duration_ms + played_ms);
                            }
                        }
                        *track_data = CurrentScrobbleTrack::default(); 
                    }
                }
            }
            PlayerEvent::StateChanged { state: new_player_state, .. } => {
                let mut track_data = self.current_track_data.lock().unwrap();

                if track_data.name.is_none() {
                    // If no song is active, we might still want to update the global state if it's, for example, stopped.
                    // However, CurrentScrobbleTrack is per-song. If it's default(), its state is Stopped.
                    // If a stop event comes, and track_data is already default, this is fine.
                    debug!("Lastfm: StateChanged event ({:?}) but no active song. Current internal state: {:?}", new_player_state, track_data.current_playback_state);
                    // Ensure the default state reflects this if it's a stop.
                    if *new_player_state == PlaybackState::Stopped || *new_player_state == PlaybackState::Killed || *new_player_state == PlaybackState::Disconnected {
                        track_data.current_playback_state = *new_player_state;
                        track_data.last_play_timestamp = None; // Ensure no dangling play timestamp
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
                    // Transitioning from Playing to Paused/Stopped/Other non-Playing state
                    if let Some(lpt) = track_data.last_play_timestamp {
                        let played_ms = lpt.elapsed().unwrap_or_default().as_millis() as u64;
                        track_data.accumulated_play_duration_ms += played_ms;
                        info!("Lastfm: Playback now '{:?}'. Added {}ms. Total accumulated: {}ms", new_player_state, played_ms, track_data.accumulated_play_duration_ms);
                    }
                    track_data.last_play_timestamp = None;
                } else if old_player_state != PlaybackState::Playing && *new_player_state == PlaybackState::Playing {
                    // Transitioning to Playing from Paused/Stopped/Other non-Playing state
                    info!("Lastfm: Playback now 'Playing'. Setting last_play_timestamp.");
                    track_data.last_play_timestamp = Some(SystemTime::now());
                }
                
                track_data.current_playback_state = *new_player_state;
            }
            _ => {
                // Other events are ignored for now
            }
        }
    }
}
