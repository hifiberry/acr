use crate::players::player_controller::{BasePlayerController, PlayerController};
use crate::data::{PlayerCapability, PlayerCapabilitySet, Song, LoopMode, PlaybackState, PlayerCommand, PlayerState, Track};
use crate::data::stream_details::StreamDetails;
use crate::helpers::mpris::{retrieve_mpris_metadata, extract_song_from_mpris_metadata};
use std::sync::{Arc, RwLock};
use log::{debug, info, warn, error};
use std::any::Any;
use mpris::{PlayerFinder, Player, PlaybackStatus, LoopStatus, Metadata};

/// MPRIS player controller implementation
/// This controller interfaces with MPRIS-compatible media players via D-Bus
pub struct MprisPlayerController {
    /// Base controller
    base: BasePlayerController,
    
    /// MPRIS bus name to connect to
    bus_name: String,
    
    /// Current song information
    current_song: Arc<RwLock<Option<Song>>>,

    /// Current player state
    current_state: Arc<RwLock<PlayerState>>,
    
    /// Current stream details
    stream_details: Arc<RwLock<Option<StreamDetails>>>,
}

// Manually implement Clone for MprisPlayerController
impl Clone for MprisPlayerController {
    fn clone(&self) -> Self {
        MprisPlayerController {
            // Share the BasePlayerController instance to maintain listener registrations
            base: self.base.clone(),
            bus_name: self.bus_name.clone(),
            current_song: Arc::clone(&self.current_song),
            current_state: Arc::clone(&self.current_state),
            stream_details: Arc::clone(&self.stream_details),
        }
    }
}

impl MprisPlayerController {
    /// Create a new MPRIS player controller for the specified bus name
    pub fn new(bus_name: &str) -> Self {
        debug!("Creating new MprisPlayerController for bus: {}", bus_name);
        
        // Create a base controller with player name and ID derived from bus name
        let player_name = Self::extract_player_name(bus_name);
        let base = BasePlayerController::with_player_info(&player_name, bus_name);
        
        let controller = Self {
            base,
            bus_name: bus_name.to_string(),
            current_song: Arc::new(RwLock::new(None)),
            current_state: Arc::new(RwLock::new(PlayerState::new())),
            stream_details: Arc::new(RwLock::new(None)),
        };
        
        // Set capabilities based on what MPRIS typically supports
        controller.set_default_capabilities();
        
        controller
    }
    
    /// Extract a friendly player name from the bus name
    fn extract_player_name(bus_name: &str) -> String {
        // Extract the last part of the bus name as the player name
        // e.g., "org.mpris.MediaPlayer2.vlc" -> "vlc"
        if let Some(last_part) = bus_name.split('.').last() {
            last_part.to_string()
        } else {
            bus_name.to_string()
        }
    }
    
    /// Set the default capabilities for MPRIS players
    fn set_default_capabilities(&self) {
        debug!("Setting default MprisPlayerController capabilities");
        
        // MPRIS players typically support most playback controls
        self.base.set_capabilities(vec![
            PlayerCapability::Play,
            PlayerCapability::Pause,
            PlayerCapability::Stop,
            PlayerCapability::Previous,
            PlayerCapability::Next,
            PlayerCapability::Seek,
            PlayerCapability::Position,
            PlayerCapability::Volume,
            PlayerCapability::Shuffle,
            PlayerCapability::Loop,
            PlayerCapability::Killable, // Can always try to kill via D-Bus
        ], false); // Don't notify on initialization
    }
    
    /// Get or create an MPRIS player connection
    fn get_mpris_player(&self) -> Result<Player, String> {
        // Create new connection each time (no caching to avoid threading issues)
        debug!("Creating new MPRIS connection to {}", self.bus_name);
        let finder = PlayerFinder::new().map_err(|e| format!("Failed to create PlayerFinder: {}", e))?;
        
        let player = finder.find_by_name(&self.bus_name)
            .map_err(|e| format!("Failed to find MPRIS player '{}': {}", self.bus_name, e))?;
        
        info!("Connected to MPRIS player: {}", self.bus_name);
        Ok(player)
    }
    
    /// Update internal state from MPRIS player
    fn update_state_from_mpris(&self) {
        if let Ok(player) = self.get_mpris_player() {
            // Update playback state
            if let Ok(status) = player.get_playback_status() {
                let state = match status {
                    PlaybackStatus::Playing => PlaybackState::Playing,
                    PlaybackStatus::Paused => PlaybackState::Paused,
                    PlaybackStatus::Stopped => PlaybackState::Stopped,
                };
                
                if let Ok(mut current_state) = self.current_state.write() {
                    current_state.state = state;
                    current_state.shuffle = player.get_shuffle().unwrap_or(false);
                    
                    // Convert MPRIS LoopStatus to our LoopMode
                    if let Ok(loop_status) = player.get_loop_status() {
                        current_state.loop_mode = match loop_status {
                            LoopStatus::None => LoopMode::None,
                            LoopStatus::Track => LoopMode::Track,
                            LoopStatus::Playlist => LoopMode::Playlist,
                        };
                    }
                    
                    // Update position if available
                    if let Ok(position) = player.get_position() {
                        current_state.position = Some(position.as_secs_f64());
                    }
                }
            }
            
            // Update song metadata
            if let Ok(metadata) = player.get_metadata() {
                let song = self.convert_metadata_to_song(&metadata);
                if let Ok(mut current_song) = self.current_song.write() {
                    *current_song = song;
                }
            }
        }
    }
    
    /// Convert MPRIS metadata to our Song struct
    fn convert_metadata_to_song(&self, metadata: &Metadata) -> Option<Song> {
        let title = metadata.title()?.to_string();
        let artist = metadata.artists()
            .and_then(|artists| artists.first().map(|s| s.to_string()))
            .unwrap_or_default();
        let album = metadata.album_name().map(|s| s.to_string()).unwrap_or_default();
        let duration = metadata.length().map(|d| d.as_secs_f64()).unwrap_or(0.0);
        let track_id = metadata.track_id().map(|s| s.to_string()).unwrap_or_default();
        
        // Handle genres - MPRIS can have multiple genres
        let mut genres = Vec::new();
        let mut genre = None;
        
        // Try to get genres from metadata (the mpris crate might expose this)
        // For now, we'll leave this empty and rely on the helper functions for genre extraction
        // The mpris crate doesn't seem to expose genre information directly
        
        Some(Song {
            title: Some(title),
            artist: Some(artist),
            album: Some(album),
            duration: Some(duration),
            stream_url: Some(track_id),
            cover_art_url: metadata.art_url().map(|s| s.to_string()),
            track_number: metadata.track_number(),
            album_artist: None,
            total_tracks: None,
            genre,
            genres,
            year: None,
            source: Some("mpris".to_string()),
            liked: None,
            metadata: std::collections::HashMap::new(),
        })
    }
}

impl PlayerController for MprisPlayerController {
    fn get_capabilities(&self) -> PlayerCapabilitySet {
        self.base.get_capabilities()
    }
    
    fn get_player_name(&self) -> String {
        self.base.get_player_name()
    }
    
    fn get_player_id(&self) -> String {
        self.base.get_player_id()
    }
    
    fn has_library(&self) -> bool {
        false // MPRIS players typically don't expose library functionality
    }
    
    fn supports_api_events(&self) -> bool {
        false // MPRIS doesn't support API events
    }
    
    fn get_last_seen(&self) -> Option<std::time::SystemTime> {
        self.base.get_last_seen()
    }
    
    fn receive_update(&self, _update: crate::data::PlayerUpdate) -> bool {
        false // MPRIS doesn't support receiving updates
    }
    
    fn get_metadata(&self) -> Option<std::collections::HashMap<String, serde_json::Value>> {
        // MPRIS doesn't provide generic metadata access, return None
        None
    }
    
    fn get_playback_state(&self) -> PlaybackState {
        self.update_state_from_mpris();
        if let Ok(state) = self.current_state.read() {
            state.state
        } else {
            PlaybackState::Unknown
        }
    }
    
    fn get_song(&self) -> Option<Song> {
        self.update_state_from_mpris();
        if let Ok(song) = self.current_song.read() {
            song.clone()
        } else {
            None
        }
    }
    
    fn get_queue(&self) -> Vec<Track> {
        // MPRIS doesn't typically expose queue information
        Vec::new()
    }
    
    fn get_shuffle(&self) -> bool {
        self.update_state_from_mpris();
        if let Ok(state) = self.current_state.read() {
            state.shuffle
        } else {
            false
        }
    }
    
    fn get_loop_mode(&self) -> LoopMode {
        self.update_state_from_mpris();
        if let Ok(state) = self.current_state.read() {
            state.loop_mode
        } else {
            LoopMode::None
        }
    }
    
    fn get_position(&self) -> Option<f64> {
        if let Ok(player) = self.get_mpris_player() {
            if let Ok(position) = player.get_position() {
                return Some(position.as_secs_f64());
            }
        }
        None
    }
    
    fn send_command(&self, command: PlayerCommand) -> bool {
        info!("Sending command to MPRIS player: {}", command);
        
        let player = match self.get_mpris_player() {
            Ok(p) => p,
            Err(e) => {
                error!("Failed to get MPRIS player connection: {}", e);
                return false;
            }
        };
        
        let result = match command {
            PlayerCommand::Play => player.play(),
            PlayerCommand::Pause => player.pause(),
            PlayerCommand::PlayPause => player.play_pause(),
            PlayerCommand::Stop => player.stop(),
            PlayerCommand::Next => player.next(),
            PlayerCommand::Previous => player.previous(),
            PlayerCommand::Seek(offset) => {
                // MPRIS seek expects microseconds as i64
                let microseconds = (offset * 1_000_000.0) as i64;
                player.seek(microseconds)
            },
            PlayerCommand::SetRandom(enabled) => player.set_shuffle(enabled),
            PlayerCommand::SetLoopMode(mode) => {
                let loop_status = match mode {
                    LoopMode::None => LoopStatus::None,
                    LoopMode::Track => LoopStatus::Track,
                    LoopMode::Playlist => LoopStatus::Playlist,
                };
                player.set_loop_status(loop_status)
            },
            PlayerCommand::Kill => {
                // For MPRIS, we can't really "kill" the player, but we can try to quit
                warn!("Kill command not supported for MPRIS players, ignoring");
                return false;
            }
            _ => {
                warn!("Command not supported by MPRIS: {}", command);
                return false;
            }
        };
        
        match result {
            Ok(()) => {
                info!("Successfully sent command {} to MPRIS player", command);
                // Update our state after successful command
                self.update_state_from_mpris();
                true
            }
            Err(e) => {
                error!("Failed to send command {} to MPRIS player: {}", command, e);
                false
            }
        }
    }
    
    fn as_any(&self) -> &dyn Any {
        self
    }
    
    fn start(&self) -> bool {
        info!("Starting MPRIS player controller for {}", self.bus_name);
        
        // Test connection
        match self.get_mpris_player() {
            Ok(_) => {
                info!("Successfully connected to MPRIS player: {}", self.bus_name);
                self.base.alive();
                true
            }
            Err(e) => {
                error!("Failed to connect to MPRIS player {}: {}", self.bus_name, e);
                false
            }
        }
    }
    
    fn stop(&self) -> bool {
        info!("Stopping MPRIS player controller for {}", self.bus_name);
        // No cached connection to clear since we create new connections on demand
        true
    }
}
