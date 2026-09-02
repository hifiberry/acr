use crate::data::{PlayerCapability, PlayerCapabilitySet, Song, Track, LoopMode, PlaybackState, PlayerCommand, PlayerEvent, PlayerSource, PlayerState, PlayerUpdate};
use crate::data::library::LibraryInterface;
use std::sync::Arc;
use parking_lot::RwLock;
use std::any::Any;
use std::time::SystemTime;
use log::debug;

/// PlayerController trait - abstract interface for player implementations
/// 
/// This trait defines the core functionality that any player implementation must provide.
/// It serves as an abstraction layer for different media player backends.
pub trait PlayerController: Send + Sync {
    /// Get the capabilities of the player
    /// 
    /// Returns a PlayerCapabilitySet with the capabilities supported by this player
    fn get_capabilities(&self) -> PlayerCapabilitySet;
    
    /// Get the current song being played
    ///
    /// Returns the current song, or None if no song is playing
    fn get_song(&self) -> Option<Song>;

    /// Get the current audio stream format details (sample rate, bit depth,
    /// codec, ...). Returns None if the player does not expose them.
    fn get_stream_details(&self) -> Option<crate::data::stream_details::StreamDetails> {
        None
    }

    /// Get the queue of songs
    /// 
    /// Returns a vector of songs in the queue (can be empty if no songs are queued)
    /// If the player does not support queues, this will return an empty vector
    fn get_queue(&self) -> Vec<Track>;
    
    /// Get the current loop mode setting
    /// 
    /// Returns the current loop mode of the player
    fn get_loop_mode(&self) -> LoopMode;
    
    /// Get the current player state
    /// 
    /// Returns the current state of the player (playing, paused, stopped, etc.)
    fn get_playback_state(&self) -> PlaybackState;
    
    /// Get the current playback position in seconds
    ///
    /// Returns the current position as seconds from the start of the track, or None if position is unknown
    fn get_position(&self) -> Option<f64>;
    
    /// Get whether shuffle is enabled
    /// 
    /// Returns true if shuffle is enabled, false otherwise
    fn get_shuffle(&self) -> bool;
    
    /// Get the name of this player controller
    /// 
    /// Returns a string identifier for this type of player (e.g., "mpd", "null")
    fn get_player_name(&self) -> String;
    
    /// Get a unique identifier for this player instance
    /// 
    /// Returns a string that uniquely identifies this player instance
    fn get_player_id(&self) -> String;
    
    /// Get the aliases for this player
    /// 
    /// Returns a vector of string aliases that can be used to identify this player type
    /// Default implementation returns just the player name
    fn get_aliases(&self) -> Vec<String> {
        vec![self.get_player_name()]
    }
    
    /// Get the last time this player was seen active
    /// 
    /// Returns the timestamp when the player was last seen, or None if not tracked
    fn get_last_seen(&self) -> Option<SystemTime>;
    
    /// Send a command to the player
    /// 
    /// # Arguments
    /// 
    /// * `command` - The command to send to the player
    /// 
    /// # Returns
    /// 
    /// Return s`true` if the command was successfully processed, `false` otherwise
    fn send_command(&self, command: PlayerCommand) -> bool;
    
    /// Downcasts the player controller to a concrete type via Any
    /// 
    /// This allows accessing implementation-specific functionality when needed.
    fn as_any(&self) -> &dyn Any;
    
    /// Starts the player controller
    /// 
    /// This initializes any background threads and connections needed for the player to operate.
    /// Returns true if the player was successfully started, false otherwise.
    fn start(&self) -> bool;
    
    /// Stops the player controller
    /// 
    /// This cleans up any resources used by the player, including stopping background threads
    /// and closing connections. Returns true if the player was successfully stopped, false otherwise.
    fn stop(&self) -> bool;

    /// Receive an update. This could be a song change,
    /// position change, random/loop mode change, etc.
    ///
    /// # Arguments
    ///
    /// * `update` - The player update
    ///
    /// # Returns
    ///
    /// `true` if the update was successfully processed, `false` otherwise
    fn receive_update(&self, update: PlayerUpdate) -> bool {
        // Default implementation does nothing and returns true
        // Player implementations should override this if they support receiving updates
        debug!("Player {} received update {:?}, but does not implement receive_update", self.get_player_name(), update);
        true
    }

    /// Get the library interface for this player, if available
    /// 
    /// Returns a library interface that can be used to query albums, artists, and tracks,
    /// or None if the player does not support library functionality.
    fn get_library(&self) -> Option<Box<dyn LibraryInterface>> {
        None  // Default implementation returns None
    }
    
    /// Check if this player offers library functionality
    /// 
    /// Returns true if the player has a library interface, false otherwise
    /// This is a convenience method that checks if get_library() would return Some
    fn has_library(&self) -> bool {
        // Since get_library consumes resources to create the Box, we just want to check
        // if the player has the capability rather than actually creating the library interface
        self.get_library().is_some()
    }

    /// Get a list of metadata keys available for this player
    /// 
    /// Returns a list of metadata keys that can be queried
    /// via get_metadata_value(). Default implementation returns an empty vector.
    fn get_meta_keys(&self) -> Vec<String> {
        vec![]
    }
    
    /// Get a specific metadata value as string
    /// 
    /// # Arguments
    /// 
    /// * `key` - The metadata key to retrieve
    /// 
    /// # Returns
    /// 
    /// The metadata value as a string, or None if the key is not found
    /// or the player doesn't support metadata
    fn get_metadata_value(&self, _key: &str) -> Option<String> {
        None
    }
    
    /// Get all metadata as a HashMap with JSON values
    /// 
    /// # Returns
    /// 
    /// All metadata for the player as a HashMap with JSON values, 
    /// or None if the player doesn't support metadata
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
    
    /// Check if this player supports metadata
    /// 
    /// Returns true if the player provides metadata functionality
    fn has_metadata(&self) -> bool {
        !self.get_meta_keys().is_empty()
    }
    
    /// Check if this player supports API events
    /// 
    /// Returns true if the player can process API events, false otherwise
    fn supports_api_events(&self) -> bool {
        false
    }
    
    /// Process an API event
    /// 
    /// # Arguments
    /// 
    /// * `event_data` - The event data to process
    /// 
    /// # Returns
    /// 
    /// `true` if the event was successfully processed, `false` otherwise
    fn process_api_event(&self, _event_data: &serde_json::Value) -> bool {
        false
    }

    /// Apply information a lookup found to this player's current song.
    /// Backends that store their song in the base need no implementation.
    fn apply_song_information(&self, _partial: &Song) -> bool {
        false
    }
}

/// Base implementation of PlayerController that handles state listener management
/// 
/// This struct provides common functionality for managing state listeners that
/// can be used by concrete player implementations.
#[derive(Clone)]
pub struct BasePlayerController {
    /// Current capabilities of the player
    capabilities: Arc<RwLock<PlayerCapabilitySet>>,
    
    /// Player name identifier (e.g., "mpd", "null")
    player_name: Arc<RwLock<String>>,
    
    /// Player unique ID (e.g., "hostname:port" for MPD)
    player_id: Arc<RwLock<String>>,
    
    /// Player state
    player_state: Arc<RwLock<PlayerState>>,
}

impl Default for BasePlayerController {
    fn default() -> Self {
        Self::new()
    }
}

/// Whether `new` is a different song from `old` -- the one identity rule every
/// backend is measured against, shared by [`BasePlayerController::set_song`]
/// and [`BasePlayerController::replace_song`] so the two cannot drift apart.
///
/// Title, artist and stream URL. The artist has to be in it: only the generic
/// controller and MPD ever assign a `stream_url`, so for MPRIS, Bluetooth,
/// RAAT and Shairport the rest of the rule is the title alone, and two
/// consecutive tracks sharing a title -- a cover, a live version, a
/// collaboration relisted under a featured artist -- would read as one song.
/// That is not merely a missed notification: `set_song` skips the store on an
/// identity miss, so the second track would be reported with the first one's
/// artist, album and artwork for as long as it played.
fn song_identity_changed(old: Option<&Song>, new: Option<&Song>) -> bool {
    match (old, new) {
        (Some(old), Some(new)) => {
            old.stream_url != new.stream_url
                || old.title != new.title
                || old.artist != new.artist
        }
        (None, None) => false,
        _ => true,
    }
}

impl BasePlayerController {
    /// Create a new BasePlayerController with no listeners
    pub fn new() -> Self {
        debug!("Creating new BasePlayerController");
        Self {
            capabilities: Arc::new(RwLock::new(PlayerCapabilitySet::empty())),
            player_name: Arc::new(RwLock::new("unknown".to_string())),
            player_id: Arc::new(RwLock::new("unknown".to_string())),
            player_state: Arc::new(RwLock::new(PlayerState::new())),
        }
    }
    
    /// Initialize the controller with player name and ID
    pub fn with_player_info(name: &str, id: &str) -> Self {
        debug!("Creating BasePlayerController with name='{}', id='{}'", name, id);
        Self {
            capabilities: Arc::new(RwLock::new(PlayerCapabilitySet::empty())),
            player_name: Arc::new(RwLock::new(name.to_string())),
            player_id: Arc::new(RwLock::new(id.to_string())),
            player_state: Arc::new(RwLock::new(PlayerState::new())),
        }
    }
    
    /// Set the player name
    pub fn set_player_name(&self, name: &str) {
        *self.player_name.write() = name.to_string();
        debug!("Player name set to '{}'", name);
    }
    
    /// Set the player ID
    pub fn set_player_id(&self, id: &str) {
        *self.player_id.write() = id.to_string();
        debug!("Player ID set to '{}'", id);
    }
    
    /// Get the player name
    pub fn get_player_name(&self) -> String {
        self.player_name.read().clone()
    }
    
    /// Get the player ID
    pub fn get_player_id(&self) -> String {
        self.player_id.read().clone()
    }
    
    /// Get the current capabilities
    pub fn get_capabilities(&self) -> PlayerCapabilitySet {
        *self.capabilities.read()
    }
    
    /// Set multiple capabilities at once using a PlayerCapabilitySet
    /// 
    /// Replaces all current capabilities with the provided ones
    /// When auto_notify is true, listeners will be notified of changes automatically
    /// Returns true if the capabilities were changed
    pub fn set_capabilities_set(&self, capabilities: PlayerCapabilitySet, auto_notify: bool) -> bool {
        debug!("Setting all capabilities to a new capability set");
        
        let mut changed = false;
        
        // Update stored capabilities
        let mut caps = self.capabilities.write();
        // Check if there's any difference
        if *caps != capabilities {
            // Replace with new capabilities
            *caps = capabilities;
            debug!("Updated capabilities");
            changed = true;
        } else {
            debug!("Capabilities unchanged, not updating");
        }
        drop(caps);
        
        // If capabilities changed and auto_notify is true, notify listeners
        if changed && auto_notify {
            self.notify_capabilities_changed(&capabilities);
        }
        
        changed
    }
    
    /// Set multiple capabilities at once using a Vec of PlayerCapability
    /// 
    /// Replaces all current capabilities with the provided ones
    /// When auto_notify is true, listeners will be notified of changes automatically
    /// Returns true if the capabilities were changed
    pub fn set_capabilities(&self, capabilities: Vec<PlayerCapability>, auto_notify: bool) -> bool {
        debug!("Setting all capabilities to a list of {} capabilities", capabilities.len());
        
        let new_set = PlayerCapabilitySet::from_slice(&capabilities);
        self.set_capabilities_set(new_set, auto_notify)
    }

    /// Set a capability as enabled or disabled
    /// 
    /// If enabled is true, adds the capability if not already present
    /// If enabled is false, removes the capability if present
    /// When auto_notify is true, listeners will be notified of changes automatically
    /// Returns true if the capabilities were changed
    pub fn set_capability(&self, capability: PlayerCapability, enabled: bool, auto_notify: bool) -> bool {
        debug!("Setting capability {:?} to {}", capability, enabled);
        
        let mut changed = false;
        
        // Update stored capabilities
        let mut caps = self.capabilities.write();
        let had_capability = caps.has_capability(capability);

        if enabled && !had_capability {
            // Add capability
            caps.add_capability(capability);
            debug!("Added capability {:?}", capability);
            changed = true;
        } else if !enabled && had_capability {
            // Remove capability
            caps.remove_capability(capability);
            debug!("Removed capability {:?}", capability);
            changed = true;
        }
        drop(caps);
        
        // If capabilities changed and auto_notify is true, notify listeners
        if changed && auto_notify {
            let current_caps = self.get_capabilities();
            self.notify_capabilities_changed(&current_caps);
        }
        
        changed
    }    /// Notify all registered listeners that the player state has changed
    pub fn notify_state_changed(&self, state: PlaybackState) {
        let player_name = self.get_player_name();
        let player_id = self.get_player_id();
        
        let source = PlayerSource::new(player_name, player_id);
        
        let event = PlayerEvent::StateChanged {
            source,
            state,
        };
        
        // Publish to the global event bus
        debug!("Publishing state change event to the global event bus");
        crate::audiocontrol::eventbus::EventBus::instance().publish(event.clone());
        
    }    
    
    /// Notify all listeners that the song has changed
    pub fn notify_song_changed(&self, song: Option<&Song>) {
        let player_name = self.get_player_name();
        let player_id = self.get_player_id();
        
        // Create a cloned version of the song to pass to listeners
        let song_copy = song.cloned();
        
        let source = PlayerSource::new(player_name, player_id);
        
        let event = PlayerEvent::SongChanged {
            source,
            song: song_copy,
        };
        
        // Publish to the global event bus
        debug!("Publishing song change event to the global event bus");
        crate::audiocontrol::eventbus::EventBus::instance().publish(event.clone());
        
    }    
    
    /// Notify all registered listeners that the loop mode has changed
    pub fn notify_loop_mode_changed(&self, mode: LoopMode) {
        let player_name = self.get_player_name();
        let player_id = self.get_player_id();
        
        debug!("Notifying listeners of loop mode change: {}", mode);

        let source = PlayerSource::new(player_name, player_id);
        
        let event = PlayerEvent::LoopModeChanged {
            source,
            mode,
        };
        
        // Publish to the global event bus
        debug!("Publishing loop mode change event to the global event bus");
        crate::audiocontrol::eventbus::EventBus::instance().publish(event.clone());
        
        // do not notify listeners anymore
        
    }    /// Notify all registered listeners that the random mode has changed
    pub fn notify_random_changed(&self, enabled: bool) {
        let player_name = self.get_player_name();
        let player_id = self.get_player_id();
        
        debug!("Notifying listeners of random mode change: {}", enabled);

        let source = PlayerSource::new(player_name, player_id);
        
        let event = PlayerEvent::RandomChanged {
            source,
            enabled,
        };
        
        // Publish to the global event bus
        debug!("Publishing random mode change event to the global event bus");
        crate::audiocontrol::eventbus::EventBus::instance().publish(event.clone());
        
    }    
    
    /// Notify all listeners that the capabilities have changed
    pub fn notify_capabilities_changed(&self, capabilities: &PlayerCapabilitySet) {
        let player_name = self.get_player_name();
        let player_id = self.get_player_id();
        
        debug!("Notifying listeners of capabilities change");
        
        // Store the capabilities internally
        let mut caps = self.capabilities.write();
        *caps = *capabilities;
        debug!("Updated capabilities");
        drop(caps);
        
        let source = PlayerSource::new(player_name, player_id);
        
        let event = PlayerEvent::CapabilitiesChanged {
            source,
            capabilities: *capabilities,
        };
        
        // Publish to the global event bus
        debug!("Publishing capabilities change event to the global event bus");
        crate::audiocontrol::eventbus::EventBus::instance().publish(event.clone());
        
    }    
    
    /// Notify all registered listeners that the player position has changed
    pub fn notify_position_changed(&self, position: f64) {
        let player_name = self.get_player_name();
        let player_id = self.get_player_id();
        
        let source = PlayerSource::new(player_name, player_id);
        
        let event = PlayerEvent::PositionChanged {
            source,
            position,
        };
        
        // Publish to the global event bus
        debug!("Publishing position change event to the global event bus");
        crate::audiocontrol::eventbus::EventBus::instance().publish(event.clone());
    }

    /// Create a PlayerSource object for the current player
    pub fn create_player_source(&self) -> PlayerSource {
        PlayerSource::new(self.get_player_name(), self.get_player_id())
    }    
    
    /// Notify listeners that the database is being updated
    pub fn notify_database_update(&self, artist: Option<String>, album: Option<String>,
                                song: Option<String>, percentage: Option<f32>) {
        let event = PlayerEvent::DatabaseUpdating {
            source: self.create_player_source(),
            artist,
            album,
            song,
            percentage,
        };
        
        // Publish to the global event bus
        debug!("Publishing database update event to the global event bus");
        crate::audiocontrol::eventbus::EventBus::instance().publish(event.clone());
        
    }    
    
    /// Notify listeners that the player's queue has changed
    pub fn notify_queue_changed(&self) {
        let event = PlayerEvent::QueueChanged {
            source: self.create_player_source(),
        };
        
        // Publish to the global event bus
        debug!("Publishing queue changed event to the global event bus");
        crate::audiocontrol::eventbus::EventBus::instance().publish(event.clone());
        
    }
    
    /// Notify listeners that the active player has changed
    pub fn notify_active_player_changed(&self, player_id: String) {
        let event = PlayerEvent::ActivePlayerChanged {
            source: self.create_player_source(),
            player_id,
        };
        
        // Publish to the global event bus
        debug!("Publishing active player changed event to the global event bus");
        crate::audiocontrol::eventbus::EventBus::instance().publish(event.clone());
        
    }

    /// Get the last time this player was seen active
    pub fn get_last_seen(&self) -> Option<SystemTime> {
        self.player_state.read().last_seen
    }

    /// Update the last_seen timestamp for this player
    /// 
    /// This should be called by player implementations whenever they are accessed
    /// or when they update their status to indicate that the player is still active.
    pub fn alive(&self) {
        let mut state = self.player_state.write();
        state.last_seen = Some(SystemTime::now());
        debug!("Updated last_seen timestamp for player {}:{}",
              self.get_player_name(), self.get_player_id());
    }

    /// Get the current playback position
    /// Implementation for the PlayerController trait
    pub fn get_position(&self) -> Option<f64> {
        self.player_state.read().position
    }

    /// The song this player last reported.
    pub fn song(&self) -> Option<Song> {
        self.player_state.read().song.clone()
    }

    /// Record the song this player is now playing.
    ///
    /// Returns whether it differed from the last one, in which case listeners
    /// have been notified.
    ///
    /// This is the call for a *continuous* source: one that re-delivers the
    /// current state on a timer, on every `get_song()`, or on every line of a
    /// stream, whether or not anything changed. Being called back does not
    /// make a source discrete -- a pipe that streams player state on a timer
    /// is a poller wearing a callback, and `raat`'s `update_metadata` is
    /// exactly that. Only a real change reaches the event bus, and whatever a
    /// lookup merged into the stored song survives every re-delivery.
    pub fn set_song(&self, song: Option<Song>) -> bool {
        let changed = {
            let mut state = self.player_state.write();
            let changed = song_identity_changed(state.song.as_ref(), song.as_ref());
            if changed {
                state.song = song.clone();
            }
            changed
        };

        if changed {
            self.notify_song_changed(song.as_ref());
        }
        changed
    }

    /// Replace the song being played, whether or not it is the same song, and
    /// notify listeners either way.
    ///
    /// This is the call for a *discrete* source: one that speaks only when
    /// something actually happened. A delivery is then always news, including
    /// a metadata-only refresh of the song already playing -- cover art
    /// arriving late is the usual one -- which has to reach clients, so both
    /// the store and the notification are unconditional. Returns whether the
    /// song's identity changed, which is what a caller gates a position reset
    /// on.
    ///
    /// [`Self::set_song`] is the continuous counterpart. The question is never
    /// the transport but whether the source speaks only on a change or speaks
    /// regardless: an unconditional store on a continuous path discards
    /// whatever a lookup had added, once per delivery.
    pub fn replace_song(&self, song: Option<Song>) -> bool {
        let changed = {
            let mut state = self.player_state.write();
            let changed = song_identity_changed(state.song.as_ref(), song.as_ref());
            state.song = song.clone();
            changed
        };

        self.notify_song_changed(song.as_ref());
        changed
    }

    /// Revise the song being played in place, for a player correcting or
    /// completing its *own* data about it.
    ///
    /// `f` is applied to the stored song under a single write lock, so there is
    /// no read-then-write window in which another writer could interleave; the
    /// guard is dropped before listeners are notified. Returns whether there
    /// was a song to update; with nothing playing, nothing happens and no event
    /// is published.
    ///
    /// This is deliberately not [`Self::apply_song_information`]. That method
    /// enforces the enrichment override policy, under which artwork supplied by
    /// the player is precisely what an outside lookup may not replace — so a
    /// player revising its own artwork through it would always be refused. The
    /// policy is about outside answers; it has nothing to say about a player
    /// correcting itself.
    pub fn update_song<F: FnOnce(&mut Song)>(&self, f: F) -> bool {
        let updated = {
            let mut state = self.player_state.write();
            match state.song.as_mut() {
                Some(song) => {
                    f(song);
                    Some(song.clone())
                }
                None => None,
            }
        };

        match updated {
            Some(song) => {
                self.notify_song_changed(Some(&song));
                true
            }
            None => {
                debug!("A player updated its song with nothing playing; nothing to update");
                false
            }
        }
    }

    /// Merge information a lookup found into the song being played.
    ///
    /// `partial` carries only what changed; an absent field means unchanged,
    /// never "cleared". Returns whether it was applied — an answer that no
    /// longer describes the song being played is dropped, because a lookup is
    /// a network round trip and a radio track can finish while it is in
    /// flight.
    ///
    /// "Applied" means the stored song actually changed. A partial the
    /// override policy refuses, or one that only restates what the song
    /// already says, changes nothing and is reported as such; no
    /// `song_information_update` is published for it either, since there is
    /// nothing for a client to re-read.
    pub fn apply_song_information(&self, partial: &Song) -> bool {
        let updated = {
            let mut state = self.player_state.write();
            let Some(current) = state.song.as_mut() else {
                debug!("Song information arrived with no song playing; dropping it");
                return false;
            };

            if partial.title.is_none() && partial.artist.is_none() {
                debug!("Song information carries no title or artist; dropping it");
                return false;
            }

            // Every field the partial DOES carry must agree with the song
            // playing; a field it omits is not asserted about. Requiring
            // both would wrongly drop a legitimate update for a song that
            // has no artist at all (e.g. an AirPlay source that never sends
            // one) -- title alone is enough to identify it.
            let title_matches = partial
                .title
                .as_deref()
                .is_none_or(|t| current.title.as_deref() == Some(t));
            let artist_matches = partial
                .artist
                .as_deref()
                .is_none_or(|a| current.artist.as_deref() == Some(a));

            if !title_matches || !artist_matches {
                debug!(
                    "Song information for {:?} no longer applies to {:?}; dropping it",
                    partial.title, current.title
                );
                return false;
            }

            // Whether any of this actually moved the stored song. A partial
            // the policy refuses, or one that only restates what the song
            // already says, is not "applied": saying it was would be a lie to
            // the caller, and publishing it would wake every client to re-read
            // a song identical to the one they hold.
            let mut changed = false;

            // The override policy, enforced here rather than in whichever
            // plugin happens to be calling. Artwork belonging to the song is
            // never replaced; only a placeholder is.
            if partial.cover_art_url.is_some() && current.cover_art_is_replaceable() {
                if current.cover_art_url != partial.cover_art_url {
                    current.cover_art_url = partial.cover_art_url.clone();
                    changed = true;
                }

                // Provenance is part of the write, not something a caller may
                // forget. The URL replaced is usually a placeholder, and its
                // COVER_ART_SOURCE marker is still on the song: left there it
                // would say the real artwork is a station logo, keeping it
                // replaceable by every later lookup and telling clients the
                // good image is a stand-in. A partial that names its own
                // source is honoured by the metadata merge below.
                if !partial.metadata.contains_key(crate::data::song::COVER_ART_SOURCE) {
                    let provenance = serde_json::Value::String(
                        crate::data::song::COVER_ART_SOURCE_ENRICHMENT.to_string(),
                    );
                    if current.metadata.get(crate::data::song::COVER_ART_SOURCE)
                        != Some(&provenance)
                    {
                        current
                            .metadata
                            .insert(crate::data::song::COVER_ART_SOURCE.to_string(), provenance);
                        changed = true;
                    }
                }
            }
            if partial.liked.is_some() && current.liked != partial.liked {
                current.liked = partial.liked;
                changed = true;
            }
            for (key, value) in &partial.metadata {
                if current.metadata.get(key) != Some(value) {
                    current.metadata.insert(key.clone(), value.clone());
                    changed = true;
                }
            }

            if !changed {
                debug!(
                    "Song information for {:?} changes nothing already stored; not publishing it",
                    partial.title
                );
                return false;
            }
            current.clone()
        };

        let source = PlayerSource::new(self.get_player_name(), self.get_player_id());
        crate::audiocontrol::eventbus::EventBus::instance().publish(
            PlayerEvent::SongInformationUpdate {
                source,
                song: updated,
            },
        );
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::Song;

    fn base() -> BasePlayerController {
        BasePlayerController::with_player_info("test", "test:0")
    }

    fn song(title: &str, artist: &str) -> Song {
        Song {
            title: Some(title.to_string()),
            artist: Some(artist.to_string()),
            ..Default::default()
        }
    }

    /// A different song is a change; the same song twice is not, so a poll loop
    /// that re-reads identical state does not spam the event bus.
    #[test]
    fn only_a_different_song_counts_as_a_change() {
        let base = base();

        assert!(base.set_song(Some(song("Battery", "Metallica"))));
        assert!(!base.set_song(Some(song("Battery", "Metallica"))));
        assert!(base.set_song(Some(song("One", "Metallica"))));
        assert!(base.set_song(None));
        assert!(!base.set_song(None));
    }

    /// Four backends -- MPRIS, Bluetooth, RAAT and Shairport -- never assign
    /// `stream_url`, so if identity were title and stream URL alone it would
    /// be title alone for them: two consecutive tracks sharing a title would
    /// read as one song, and the second would be reported with the first's
    /// artist, album and artwork for its whole duration, because set_song
    /// skips the *store* on an identity miss, not merely the notification.
    #[test]
    fn a_same_titled_track_by_a_different_artist_is_a_new_song() {
        let base = base();

        let mut first = song("Hurt", "Nine Inch Nails");
        first.album = Some("The Downward Spiral".to_string());
        first.cover_art_url = Some("https://example.com/nin.jpg".to_string());
        assert!(base.set_song(Some(first)));

        let mut second = song("Hurt", "Johnny Cash");
        second.album = Some("American IV".to_string());
        second.cover_art_url = Some("https://example.com/cash.jpg".to_string());
        assert!(
            base.set_song(Some(second)),
            "a same-titled track by a different artist is a different song"
        );

        let stored = base.song().expect("a song must be stored");
        assert_eq!(stored.artist, Some("Johnny Cash".to_string()));
        assert_eq!(
            stored.album,
            Some("American IV".to_string()),
            "the new track's album must replace the previous one's, not be skipped"
        );
        assert_eq!(
            stored.cover_art_url,
            Some("https://example.com/cash.jpg".to_string()),
            "the new track's artwork must replace the previous one's"
        );
    }

    /// The same rule in replace_song, whose return value is what an
    /// event-driven backend gates a playback position reset on: a different
    /// artist under the same title is a new track and must start from zero.
    #[test]
    fn replace_song_reports_a_different_artist_under_one_title_as_a_change() {
        let base = base();

        assert!(base.replace_song(Some(song("Hurt", "Nine Inch Nails"))));
        assert!(
            base.replace_song(Some(song("Hurt", "Johnny Cash"))),
            "a same-titled track by a different artist is an identity change"
        );
    }

    /// Unlike set_song, replace_song stores a same-identity refresh (e.g. new
    /// cover art) rather than dropping it -- an event-driven backend must not
    /// lose metadata that arrives after the song was first reported. It still
    /// reports whether identity changed, so a caller can gate a position
    /// reset on that.
    #[test]
    fn replace_song_stores_a_same_identity_refresh_but_reports_no_identity_change() {
        let base = base();

        assert!(base.replace_song(Some(song("Battery", "Metallica"))));

        let mut refreshed = song("Battery", "Metallica");
        refreshed.cover_art_url = Some("https://example.com/cover.jpg".to_string());
        assert!(!base.replace_song(Some(refreshed)));

        assert_eq!(
            base.song().unwrap().cover_art_url,
            Some("https://example.com/cover.jpg".to_string())
        );

        assert!(base.replace_song(Some(song("One", "Metallica"))));
    }

    /// A player revising its own current song is not enrichment, and must not
    /// be routed through the enrichment override policy: by that policy the
    /// player's own artwork is exactly what may never be replaced, so a
    /// revision of it would always be dropped. update_song mutates the stored
    /// song under a single write lock, so there is no read-then-write race
    /// with whatever else is writing to the same song.
    #[test]
    fn a_player_updating_its_own_song_is_not_subject_to_the_override_policy() {
        let base = base();
        let mut playing = song("Battery", "Metallica");
        playing.cover_art_url = Some("https://example.com/first.jpg".to_string());
        base.set_song(Some(playing));

        assert!(base.update_song(|song| {
            song.cover_art_url = Some("https://example.com/second.jpg".to_string());
        }));

        assert_eq!(
            base.song().unwrap().cover_art_url,
            Some("https://example.com/second.jpg".to_string()),
            "a player may revise the artwork it supplied itself"
        );
    }

    /// With nothing playing there is nothing to update, and the caller is told
    /// so rather than a song being invented.
    #[test]
    fn a_player_update_with_no_song_playing_reports_that_there_was_none() {
        let base = base();

        assert!(!base.update_song(|song| {
            song.cover_art_url = Some("https://example.com/cover.jpg".to_string());
        }));
        assert!(base.song().is_none());
    }

    /// The stored song is what get_song answers with.
    #[test]
    fn the_stored_song_is_readable() {
        let base = base();
        base.set_song(Some(song("Battery", "Metallica")));

        assert_eq!(base.song().unwrap().title, Some("Battery".to_string()));
    }

    /// A partial update fills in only the fields it carries.
    #[test]
    fn a_partial_update_leaves_absent_fields_alone() {
        let base = base();
        base.set_song(Some(song("Battery", "Metallica")));

        let applied = base.apply_song_information(&Song {
            title: Some("Battery".to_string()),
            artist: Some("Metallica".to_string()),
            cover_art_url: Some("https://example.com/cover.jpg".to_string()),
            ..Default::default()
        });

        assert!(applied);
        let stored = base.song().unwrap();
        assert_eq!(stored.cover_art_url, Some("https://example.com/cover.jpg".to_string()));
        assert_eq!(stored.title, Some("Battery".to_string()));
    }

    /// A lookup is a network round trip and a radio track may be short, so an
    /// answer can arrive after the song it was about has finished. Applying it
    /// would put the previous track's artwork on the current one.
    #[test]
    fn an_update_for_a_song_that_has_finished_is_dropped() {
        let base = base();
        base.set_song(Some(song("Listen To The News", "Radical Friendship Theory")));

        let applied = base.apply_song_information(&Song {
            title: Some("Battery".to_string()),
            artist: Some("Metallica".to_string()),
            cover_art_url: Some("https://example.com/wrong.jpg".to_string()),
            ..Default::default()
        });

        assert!(!applied);
        assert_eq!(base.song().unwrap().cover_art_url, None);
    }

    /// An update that arrives with no song playing has nothing to apply to.
    #[test]
    fn an_update_with_no_song_playing_is_dropped() {
        let base = base();

        assert!(!base.apply_song_information(&song("Battery", "Metallica")));
    }

    /// A partial carrying neither title nor artist cannot be checked against
    /// the song playing, so it cannot be trusted to still apply.
    #[test]
    fn an_unidentified_partial_is_dropped() {
        let base = base();
        base.set_song(Some(song("Battery", "Metallica")));

        assert!(!base.apply_song_information(&Song {
            cover_art_url: Some("https://example.com/cover.jpg".to_string()),
            ..Default::default()
        }));
    }

    /// A song with no artist (e.g. an AirPlay source that never sends an
    /// ARTIST line) is a legitimate "now playing" state. A partial that
    /// carries only a matching title -- artist absent, not mismatched --
    /// must still be applied: the guard can only assert about fields it
    /// actually carries.
    #[test]
    fn a_partial_matching_by_title_alone_is_applied_to_an_artistless_song() {
        let base = base();
        base.set_song(Some(Song {
            title: Some("Some AirPlay Track".to_string()),
            artist: None,
            ..Default::default()
        }));

        let applied = base.apply_song_information(&Song {
            title: Some("Some AirPlay Track".to_string()),
            cover_art_url: Some("https://example.com/cover.jpg".to_string()),
            ..Default::default()
        });

        assert!(applied);
        assert_eq!(
            base.song().unwrap().cover_art_url,
            Some("https://example.com/cover.jpg".to_string())
        );
    }

    /// The same title-only partial dropped when the title actually
    /// disagrees with what is playing -- the guard still catches a stale
    /// answer, it just no longer requires an artist to do so.
    #[test]
    fn a_partial_matching_by_title_alone_is_dropped_when_title_disagrees() {
        let base = base();
        base.set_song(Some(Song {
            title: Some("Some AirPlay Track".to_string()),
            artist: None,
            ..Default::default()
        }));

        let applied = base.apply_song_information(&Song {
            title: Some("A Different Track".to_string()),
            cover_art_url: Some("https://example.com/wrong.jpg".to_string()),
            ..Default::default()
        });

        assert!(!applied);
        assert_eq!(base.song().unwrap().cover_art_url, None);
    }

    /// The override policy. Artwork belonging to the song is never replaced,
    /// however good the replacement might be.
    #[test]
    fn artwork_belonging_to_the_song_is_not_replaced() {
        let base = base();
        let mut playing = song("Battery", "Metallica");
        playing.cover_art_url = Some("https://example.com/players-own.jpg".to_string());
        base.set_song(Some(playing));

        base.apply_song_information(&Song {
            title: Some("Battery".to_string()),
            artist: Some("Metallica".to_string()),
            cover_art_url: Some("https://example.com/lookup.jpg".to_string()),
            ..Default::default()
        });

        assert_eq!(
            base.song().unwrap().cover_art_url,
            Some("https://example.com/players-own.jpg".to_string()),
            "the song's own artwork must survive a lookup"
        );
    }

    /// A placeholder is exactly what may be replaced -- the reason any of this
    /// machinery exists.
    #[test]
    fn a_placeholder_is_replaced() {
        use crate::data::song::{
            COVER_ART_SOURCE, COVER_ART_SOURCE_ENRICHMENT, COVER_ART_SOURCE_STATION_LOGO,
        };

        let base = base();
        let mut playing = song("Battery", "Metallica");
        playing.cover_art_url = Some("https://station.example/logo.png".to_string());
        playing.metadata.insert(
            COVER_ART_SOURCE.to_string(),
            serde_json::Value::String(COVER_ART_SOURCE_STATION_LOGO.to_string()),
        );
        base.set_song(Some(playing));

        base.apply_song_information(&Song {
            title: Some("Battery".to_string()),
            artist: Some("Metallica".to_string()),
            cover_art_url: Some("https://example.com/lookup.jpg".to_string()),
            ..Default::default()
        });

        let stored = base.song().unwrap();
        assert_eq!(
            stored.cover_art_url,
            Some("https://example.com/lookup.jpg".to_string())
        );

        // Provenance travels with the URL. A partial that names no source
        // must not leave the placeholder's marker behind: that would mark
        // real artwork as a station logo, replaceable by the next lookup
        // for ever, and tell every client the good image is a stand-in.
        assert_ne!(
            stored.metadata.get(COVER_ART_SOURCE).and_then(|v| v.as_str()),
            Some(COVER_ART_SOURCE_STATION_LOGO),
            "the replaced placeholder's marker must not survive onto the new artwork"
        );
        assert_eq!(
            stored.metadata.get(COVER_ART_SOURCE).and_then(|v| v.as_str()),
            Some(COVER_ART_SOURCE_ENRICHMENT),
            "artwork that names no source is recorded as having come from enrichment"
        );
        assert!(
            !stored.cover_art_is_replaceable(),
            "the new artwork is not a placeholder, so nothing may overwrite it"
        );
    }

    /// The return value says whether the update was applied, so a partial
    /// that changes nothing must report false -- and must not put a
    /// `song_information_update` on the bus telling every client to re-read a
    /// song that is exactly as they last saw it. Artwork the policy refuses
    /// is the ordinary case: a lookup answers for a track whose own artwork
    /// the player already supplied.
    #[test]
    fn a_partial_that_changes_nothing_is_not_reported_as_applied() {
        use crate::audiocontrol::eventbus::{EventBus, EventSubscription};

        let bus = EventBus::instance();
        let (id, receiver) = bus.subscribe(vec![EventSubscription::SongInformationUpdate]);

        let player_id = "test:no-op-update";
        let base = BasePlayerController::with_player_info("test", player_id);
        let mut playing = song("Battery", "Metallica");
        playing.cover_art_url = Some("https://example.com/players-own.jpg".to_string());
        base.set_song(Some(playing));

        let applied = base.apply_song_information(&Song {
            title: Some("Battery".to_string()),
            artist: Some("Metallica".to_string()),
            cover_art_url: Some("https://example.com/lookup.jpg".to_string()),
            ..Default::default()
        });

        let published = receiver
            .try_iter()
            .filter(|event| {
                matches!(event, PlayerEvent::SongInformationUpdate { source, .. }
                    if source.player_id() == player_id)
            })
            .count();
        bus.unsubscribe(id);

        assert!(!applied, "nothing was applied, so nothing was applied");
        assert_eq!(
            published, 0,
            "an update that changed nothing must not be announced"
        );
    }

    /// The same partial arriving twice -- a plugin retrying, two lookups
    /// agreeing -- changes the song once. The second is a no-op.
    #[test]
    fn the_same_partial_applied_twice_reports_a_change_only_once() {
        let base = base();
        base.set_song(Some(song("Battery", "Metallica")));

        let partial = Song {
            title: Some("Battery".to_string()),
            artist: Some("Metallica".to_string()),
            cover_art_url: Some("https://example.com/lookup.jpg".to_string()),
            ..Default::default()
        };

        assert!(base.apply_song_information(&partial));
        assert!(!base.apply_song_information(&partial));
    }

    /// Count the `song_changed` events one player published, ignoring what
    /// every other test in the process puts on the same bus. The event bus is
    /// a process-wide singleton, so the player id is what separates one
    /// test's traffic from another's.
    fn song_changed_events_from(
        receiver: &crossbeam::channel::Receiver<PlayerEvent>,
        player_id: &str,
    ) -> usize {
        receiver
            .try_iter()
            .filter(|event| {
                matches!(event, PlayerEvent::SongChanged { source, .. }
                    if source.player_id() == player_id)
            })
            .count()
    }

    /// A poller re-reads the same song every interval, and each re-read must
    /// cost nothing on the bus: one `song_changed` for the song, not one per
    /// observation. This is the guard against the event storm a polling
    /// backend causes by notifying unconditionally -- a subscriber that falls
    /// behind loses genuine state and position events to the duplicates.
    #[test]
    fn a_repeated_observation_publishes_no_second_event() {
        use crate::audiocontrol::eventbus::{EventBus, EventSubscription};

        let bus = EventBus::instance();
        let (id, receiver) = bus.subscribe(vec![EventSubscription::SongChanged]);

        let player_id = "test:repeat-observation";
        let base = BasePlayerController::with_player_info("test", player_id);

        base.set_song(Some(song("Battery", "Metallica")));
        base.set_song(Some(song("Battery", "Metallica")));
        base.set_song(Some(song("Battery", "Metallica")));

        let published = song_changed_events_from(&receiver, player_id);
        bus.unsubscribe(id);

        assert_eq!(
            published, 1,
            "three observations of one song must publish one song_changed, not three"
        );
    }

    /// The point of the whole arrangement: a lookup finds real artwork for a
    /// radio track, and the next poll must not wipe it. A polling backend
    /// rebuilds the song from its source every interval, so the observation it
    /// hands over carries no cover art at all -- storing that blindly erases
    /// the enrichment a second after it arrived.
    #[test]
    fn a_rebuilt_observation_does_not_erase_enrichment() {
        let base = base();
        base.set_song(Some(song("Listen To The News", "Radical Friendship Theory")));

        assert!(base.apply_song_information(&Song {
            title: Some("Listen To The News".to_string()),
            artist: Some("Radical Friendship Theory".to_string()),
            cover_art_url: Some("https://lastfm.example/1200x1200.png".to_string()),
            ..Default::default()
        }));

        // What the next poll observes: the same song, rebuilt from the source,
        // with no cover art because the source never had any.
        base.set_song(Some(song("Listen To The News", "Radical Friendship Theory")));

        assert_eq!(
            base.song().unwrap().cover_art_url,
            Some("https://lastfm.example/1200x1200.png".to_string()),
            "a rebuilt observation of the same song must not erase enriched artwork"
        );
    }
}