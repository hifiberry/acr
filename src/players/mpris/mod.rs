pub mod config;
pub mod reconcile;

use crate::players::player_controller::{BasePlayerController, PlayerController};
use crate::data::{PlayerCapability, PlayerCapabilitySet, Song, LoopMode, PlaybackState, PlayerCommand, PlayerState, Track};
use crate::helpers::mpris::{
    retrieve_mpris_metadata, extract_song_from_mpris_metadata, create_connection,
    create_player_proxy, get_string_property, get_bool_property,
    get_i64_property, send_player_method, send_player_method_with_args,
    set_player_property, bool_to_dbus_variant, player_exists, BusType
};
use crate::helpers::mpris_artwork::{ArtworkResolver, ImageCacheArtworkStore};
use config::{BusPreference, MprisConfig};
use reconcile::{capabilities_from_flags, reconcile, MprisFlags, MprisSnapshot};
use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
use parking_lot::{Mutex, RwLock};
use std::thread;
use std::time::{Duration, Instant};
use log::{debug, info, warn, error};
use std::any::Any;
use dbus::blocking::Connection;

const PLAYER_INTERFACE: &str = "org.mpris.MediaPlayer2.Player";

/// Store the song a poll produced, with its artwork made reachable.
///
/// A local `mpris:artUrl` is replaced by the image cache URL the resolver
/// returns. A new song is stored with `set_song`. Details that arrive after
/// the title -- the same song, now with artwork, album, length, track number
/// or genre -- are merged with `update_song`, which `set_song`'s identity
/// check would otherwise drop. A detail the player leaves out is never
/// erased, so what a lookup added survives. Returns whether anything was
/// published.
pub fn apply_artwork(base: &BasePlayerController, resolver: &ArtworkResolver, song: Option<Song>) -> bool {
    let song = song.map(|mut s| {
        s.cover_art_url = s.cover_art_url.as_deref().and_then(|u| resolver.resolve(u));
        s
    });
    let incoming = song.clone();
    if base.set_song(song) {
        return true;
    }
    let (Some(new), Some(stored)) = (incoming, base.song()) else { return false };
    // Work out the changes before taking the lock: update_song's closure must
    // not call back into the base controller.
    let cover = new.cover_art_url.filter(|u| stored.cover_art_url.as_ref() != Some(u));
    let album = new.album.filter(|v| stored.album.as_ref() != Some(v));
    let album_artist = new.album_artist.filter(|v| stored.album_artist.as_ref() != Some(v));
    let duration = new.duration.filter(|v| stored.duration != Some(*v));
    let track_number = new.track_number.filter(|v| stored.track_number != Some(*v));
    let genre = new.genre.filter(|v| stored.genre.as_ref() != Some(v));
    let genres = Some(new.genres).filter(|g| !g.is_empty() && *g != stored.genres);
    if cover.is_none() && album.is_none() && album_artist.is_none() && duration.is_none()
        && track_number.is_none() && genre.is_none() && genres.is_none() {
        return false;
    }
    base.update_song(|s| {
        if cover.is_some() { s.cover_art_url = cover.clone(); }
        if album.is_some() { s.album = album.clone(); }
        if album_artist.is_some() { s.album_artist = album_artist.clone(); }
        if duration.is_some() { s.duration = duration; }
        if track_number.is_some() { s.track_number = track_number; }
        if genre.is_some() { s.genre = genre.clone(); }
        if let Some(g) = &genres { s.genres = g.clone(); }
        true
    })
}

/// A poll whose `PlaybackStatus` could not be read says nothing reliable;
/// it is skipped rather than published as a change.
pub fn skip_poll(snap: &MprisSnapshot) -> bool {
    snap.status.is_none()
}

/// Keep the previous capabilities when the `Can*` properties could not be read.
pub fn stabilize(mut snap: MprisSnapshot, prev_flags: Option<MprisFlags>) -> MprisSnapshot {
    if snap.flags_failed {
        if let Some(prev) = prev_flags {
            snap.flags = prev;
        }
    }
    snap
}

/// Everything the polling thread and the API calls share.
struct Inner {
    base: BasePlayerController,
    config: MprisConfig,
    /// The bus the player was last found on; `None` while it is not found.
    bus_type: RwLock<Option<BusType>>,
    /// The flags last published as capabilities; `None` before the first poll.
    flags: RwLock<Option<MprisFlags>>,
    artwork: ArtworkResolver,
    current_state: RwLock<PlayerState>,
    /// Held for a whole poll, so the poll thread and a command's follow-up
    /// poll cannot interleave and publish a stale state last.
    poll_lock: Mutex<()>,
}

impl Inner {
    /// A connection on which the player is present, or `None`.
    fn connect(&self) -> Option<Connection> {
        let candidates = match self.config.bus {
            BusPreference::System => vec![BusType::System],
            BusPreference::Session => vec![BusType::Session],
            BusPreference::Auto => match self.bus_type.read().clone() {
                Some(known) => vec![known],
                None => vec![BusType::Session, BusType::System],
            },
        };
        for bus in candidates {
            let Ok(conn) = create_connection(bus.clone()) else { continue };
            if player_exists(&conn, &self.config.bus_name) {
                *self.bus_type.write() = Some(bus);
                return Some(conn);
            }
        }
        // Not (or no longer) there: in auto mode, search both buses next time.
        *self.bus_type.write() = None;
        None
    }

    fn capabilities_for(&self, flags: &MprisFlags) -> Vec<PlayerCapability> {
        let mut caps = capabilities_from_flags(flags);
        for extra in &self.config.extra_capabilities {
            if !caps.contains(extra) {
                caps.push(*extra);
            }
        }
        caps
    }

    /// Read the player once and publish what changed.
    fn poll_once(&self) {
        let _poll = self.poll_lock.lock();
        let prev_flags = *self.flags.read();
        let snap = self.connect().and_then(|conn| read_snapshot(&conn, &self.config.bus_name));
        if snap.as_ref().is_some_and(skip_poll) {
            debug!("MPRIS {}: PlaybackStatus unreadable, skipping this poll", self.config.bus_name);
            return;
        }
        let snap = snap.map(|s| stabilize(s, prev_flags));
        let metadata_failed = snap.as_ref().is_some_and(|s| s.metadata_failed);
        let prev = self.current_state.read().clone();
        let r = reconcile(&prev, prev_flags, snap.as_ref(), self.config.clear_song_when_stopped);
        {
            let mut state = self.current_state.write();
            state.state = r.state;
            state.shuffle = r.shuffle;
            state.loop_mode = r.loop_mode;
            state.position = r.position;
        }
        if r.flags_changed {
            *self.flags.write() = Some(r.flags);
            self.base.set_capabilities(self.capabilities_for(&r.flags), true);
        }
        if r.state_changed {
            debug!("MPRIS {}: state {:?} -> {:?}", self.config.bus_name, prev.state, r.state);
            self.base.notify_state_changed(r.state);
        }
        if r.shuffle_changed {
            self.base.notify_random_changed(r.shuffle);
        }
        if r.loop_changed {
            self.base.notify_loop_mode_changed(r.loop_mode);
        }
        if !metadata_failed {
            apply_artwork(&self.base, &self.artwork, r.song);
        }
        if snap.is_some() {
            self.base.alive();
        }
    }
}

/// Read one snapshot of the player's properties. `None` if it is not on the bus.
fn read_snapshot(conn: &Connection, bus_name: &str) -> Option<MprisSnapshot> {
    if !player_exists(conn, bus_name) {
        return None;
    }
    let proxy = create_player_proxy(conn, bus_name);
    let flag = |p: &str| get_bool_property(&proxy, PLAYER_INTERFACE, p).unwrap_or(false);
    let can_control = get_bool_property(&proxy, PLAYER_INTERFACE, "CanControl");
    let shuffle = get_bool_property(&proxy, PLAYER_INTERFACE, "Shuffle");
    let loop_status = get_string_property(&proxy, PLAYER_INTERFACE, "LoopStatus");
    let flags = MprisFlags {
        can_control: can_control.unwrap_or(false),
        can_play: flag("CanPlay"),
        can_pause: flag("CanPause"),
        can_go_next: flag("CanGoNext"),
        can_go_previous: flag("CanGoPrevious"),
        can_seek: flag("CanSeek"),
        has_shuffle: shuffle.is_some(),
        has_loop: loop_status.is_some(),
    };
    let metadata = retrieve_mpris_metadata(&proxy);
    Some(MprisSnapshot {
        status: get_string_property(&proxy, PLAYER_INTERFACE, "PlaybackStatus"),
        flags_failed: can_control.is_none(),
        metadata_failed: metadata.is_none(),
        shuffle,
        loop_status,
        position_us: get_i64_property(&proxy, PLAYER_INTERFACE, "Position"),
        flags,
        song: metadata.and_then(|m| extract_song_from_mpris_metadata(&m)),
    })
}

/// MPRIS player controller implementation
/// This controller interfaces with MPRIS-compatible media players via D-Bus.
///
/// The player need not be on the bus when the controller starts: it is polled
/// for, reads as stopped with no song while absent, and is picked up when it
/// appears.
pub struct MprisPlayerController {
    inner: Arc<Inner>,
    /// Flag to control the polling thread
    should_poll: Arc<AtomicBool>,
    /// Handle to the polling thread
    poll_thread_handle: Arc<RwLock<Option<thread::JoinHandle<()>>>>,
}

// Manually implement Clone for MprisPlayerController
impl Clone for MprisPlayerController {
    fn clone(&self) -> Self {
        MprisPlayerController {
            inner: Arc::clone(&self.inner),
            should_poll: Arc::clone(&self.should_poll),
            poll_thread_handle: Arc::new(RwLock::new(None)), // New instance gets new thread handle
        }
    }
}

impl MprisPlayerController {
    /// Create a new MPRIS player controller for the specified bus name
    pub fn new(bus_name: &str) -> Self {
        Self::with_config(MprisConfig::new(bus_name))
    }

    /// Create a new MPRIS player controller with configurable polling interval
    pub fn new_with_poll_interval(bus_name: &str, poll_interval: Duration) -> Self {
        let mut config = MprisConfig::new(bus_name);
        config.poll_interval = poll_interval;
        Self::with_config(config)
    }

    /// Create a controller named after `config.name`, or the last part of the bus name.
    pub fn with_config(config: MprisConfig) -> Self {
        let name = config.name.clone().unwrap_or_else(|| Self::extract_player_name(&config.bus_name));
        let base = BasePlayerController::with_player_info(&name, &config.bus_name);
        Self::with_config_and_base(config, base)
    }

    /// Create a controller that publishes through an existing base controller,
    /// whose name and id are kept as they are. For players that wrap MPRIS.
    pub fn with_config_and_base(config: MprisConfig, base: BasePlayerController) -> Self {
        debug!("Creating MprisPlayerController for {} ({:?} bus)", config.bus_name, config.bus);
        let artwork = ArtworkResolver::new(config.artwork_dirs.clone(), Arc::new(ImageCacheArtworkStore));
        let inner = Inner {
            base,
            config,
            bus_type: RwLock::new(None),
            flags: RwLock::new(None),
            artwork,
            current_state: RwLock::new(PlayerState::new()),
            poll_lock: Mutex::new(()),
        };
        let initial = inner.capabilities_for(&MprisFlags::default());
        inner.base.set_capabilities(initial, false);
        Self {
            inner: Arc::new(inner),
            should_poll: Arc::new(AtomicBool::new(false)),
            poll_thread_handle: Arc::new(RwLock::new(None)),
        }
    }

    /// Poll the player now instead of waiting for the next interval.
    pub fn refresh(&self) {
        self.inner.poll_once();
    }

    /// Extract a friendly player name from the bus name
    fn extract_player_name(bus_name: &str) -> String {
        // e.g., "org.mpris.MediaPlayer2.vlc" -> "vlc"
        bus_name.split('.').next_back().unwrap_or(bus_name).to_string()
    }

    /// Start the polling thread
    fn start_polling(&self) {
        if self.should_poll.swap(true, Ordering::Relaxed) {
            debug!("Polling already started for MPRIS player {}", self.inner.config.bus_name);
            return;
        }
        let inner = Arc::clone(&self.inner);
        let should_poll = Arc::clone(&self.should_poll);
        info!("Starting polling thread for MPRIS player {} with interval {:?}",
              inner.config.bus_name, inner.config.poll_interval);
        let handle = thread::spawn(move || {
            let mut last_update: Option<Instant> = None;
            while should_poll.load(Ordering::Relaxed) {
                if last_update.is_none_or(|t| t.elapsed() >= inner.config.poll_interval) {
                    inner.poll_once();
                    last_update = Some(Instant::now());
                }
                // Sleep for a short time to avoid busy waiting
                thread::sleep(Duration::from_millis(100));
            }
            debug!("MPRIS polling thread stopped for {}", inner.config.bus_name);
        });
        *self.poll_thread_handle.write() = Some(handle);
    }

    /// Stop the polling thread
    fn stop_polling(&self) {
        if !self.should_poll.swap(false, Ordering::Relaxed) {
            return;
        }
        info!("Stopping polling thread for MPRIS player {}", self.inner.config.bus_name);
        if let Some(handle) = self.poll_thread_handle.write().take() {
            if let Err(e) = handle.join() {
                warn!("Error joining polling thread for {}: {:?}", self.inner.config.bus_name, e);
            }
        }
    }
}

impl PlayerController for MprisPlayerController {
    fn get_capabilities(&self) -> PlayerCapabilitySet {
        self.inner.base.get_capabilities()
    }

    fn get_player_name(&self) -> String {
        self.inner.base.get_player_name()
    }

    fn get_player_id(&self) -> String {
        self.inner.base.get_player_id()
    }

    fn has_library(&self) -> bool {
        false // MPRIS players typically don't expose library functionality
    }

    fn supports_api_events(&self) -> bool {
        false // MPRIS doesn't support API events
    }

    fn get_last_seen(&self) -> Option<std::time::SystemTime> {
        self.inner.base.get_last_seen()
    }

    fn receive_update(&self, _update: crate::data::PlayerUpdate) -> bool {
        false // MPRIS doesn't support receiving updates
    }

    fn get_metadata(&self) -> Option<std::collections::HashMap<String, serde_json::Value>> {
        None
    }

    fn get_playback_state(&self) -> PlaybackState {
        self.inner.current_state.read().state
    }

    fn get_song(&self) -> Option<Song> {
        self.inner.base.song()
    }

    fn apply_song_information(&self, partial: &Song) -> bool {
        self.inner.base.apply_song_information(partial)
    }

    fn get_queue(&self) -> Vec<Track> {
        Vec::new()
    }

    fn get_shuffle(&self) -> bool {
        self.inner.current_state.read().shuffle
    }

    fn get_loop_mode(&self) -> LoopMode {
        self.inner.current_state.read().loop_mode
    }

    fn get_position(&self) -> Option<f64> {
        // Position moves continuously, so ask the player rather than the last poll.
        let conn = self.inner.connect()?;
        let proxy = create_player_proxy(&conn, &self.inner.config.bus_name);
        get_i64_property(&proxy, PLAYER_INTERFACE, "Position").map(|us| us as f64 / 1_000_000.0)
    }

    fn send_command(&self, command: PlayerCommand) -> bool {
        info!("Sending command to MPRIS player: {}", command);
        let Some(conn) = self.inner.connect() else {
            error!("MPRIS player {} is not on the bus", self.inner.config.bus_name);
            return false;
        };
        let proxy = create_player_proxy(&conn, &self.inner.config.bus_name);

        let result = match command {
            PlayerCommand::Play => send_player_method(&proxy, "Play"),
            PlayerCommand::Pause => send_player_method(&proxy, "Pause"),
            PlayerCommand::PlayPause => send_player_method(&proxy, "PlayPause"),
            PlayerCommand::Stop => send_player_method(&proxy, "Stop"),
            PlayerCommand::Next => send_player_method(&proxy, "Next"),
            PlayerCommand::Previous => send_player_method(&proxy, "Previous"),
            PlayerCommand::Seek(offset) => {
                if !self.get_capabilities().has_capability(PlayerCapability::Seek) {
                    warn!("MPRIS player {} cannot seek", self.inner.config.bus_name);
                    return false;
                }
                // MPRIS seek expects microseconds as i64
                let microseconds = (offset * 1_000_000.0) as i64;
                send_player_method_with_args(&proxy, "Seek", (microseconds,))
            },
            PlayerCommand::SetRandom(enabled) => {
                set_player_property(&proxy, "Shuffle", bool_to_dbus_variant(enabled).0)
            },
            PlayerCommand::SetLoopMode(mode) => {
                let loop_status_str = match mode {
                    LoopMode::None => "None",
                    LoopMode::Track => "Track",
                    LoopMode::Playlist => "Playlist",
                };
                set_player_property(&proxy, "LoopStatus", loop_status_str)
            },
            _ => {
                warn!("Command not supported by MPRIS: {}", command);
                return false;
            }
        };

        match result {
            Ok(()) => {
                info!("Successfully sent command {} to MPRIS player", command);
                // Reflect the effect without waiting for the next poll
                self.inner.poll_once();
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
        info!("Starting MPRIS player controller for {}", self.inner.config.bus_name);
        if self.inner.connect().is_none() {
            info!("MPRIS player {} is not on the bus yet; it will be picked up when it appears",
                  self.inner.config.bus_name);
        }
        self.start_polling();
        true
    }

    fn stop(&self) -> bool {
        info!("Stopping MPRIS player controller for {}", self.inner.config.bus_name);
        self.stop_polling();
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::helpers::mpris_artwork::{ArtworkResolver, ArtworkStore};
    use crate::players::mpris::config::MprisConfig;

    const JPEG: &[u8] = &[0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, b'J', b'F', b'I', b'F'];

    struct UrlStore;
    impl ArtworkStore for UrlStore {
        fn store(&self, key: &str, _data: &[u8]) -> Result<String, String> {
            Ok(format!("/api/imagecache/{}", key))
        }
    }

    fn song(title: &str, art: Option<&str>) -> Song {
        Song {
            title: Some(title.to_string()),
            artist: Some("Artist".to_string()),
            cover_art_url: art.map(str::to_string),
            ..Default::default()
        }
    }

    #[test]
    fn late_artwork_merges_into_same_song() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cover.jpg");
        std::fs::write(&path, JPEG).unwrap();
        let resolver = ArtworkResolver::new(vec![dir.path().to_path_buf()], Arc::new(UrlStore));
        let base = BasePlayerController::with_player_info("test", "test");

        assert!(apply_artwork(&base, &resolver, Some(song("A", None))));
        let art = format!("file://{}", path.display());
        assert!(apply_artwork(&base, &resolver, Some(song("A", Some(&art)))));
        let expected = format!("/api/imagecache/mpris/{:x}.jpg", md5::compute(JPEG));
        assert_eq!(base.song().unwrap().cover_art_url, Some(expected));
        assert!(!apply_artwork(&base, &resolver, Some(song("A", Some(&art)))));
    }

    #[test]
    fn late_metadata_merges_into_same_song() {
        let resolver = ArtworkResolver::new(vec![], Arc::new(UrlStore));
        let base = BasePlayerController::with_player_info("test", "test");
        apply_artwork(&base, &resolver, Some(song("A", None)));
        let mut fuller = song("A", None);
        fuller.album = Some("Album".to_string());
        fuller.duration = Some(312.5);
        fuller.track_number = Some(4);
        fuller.genre = Some("Rock".to_string());
        fuller.genres = vec!["Rock".to_string()];
        assert!(apply_artwork(&base, &resolver, Some(fuller.clone())));
        let stored = base.song().unwrap();
        assert_eq!(stored.album.as_deref(), Some("Album"));
        assert_eq!(stored.duration, Some(312.5));
        assert_eq!(stored.track_number, Some(4));
        assert_eq!(stored.genre.as_deref(), Some("Rock"));
        // Same data again publishes nothing; a sparser resend erases nothing.
        assert!(!apply_artwork(&base, &resolver, Some(fuller)));
        assert!(!apply_artwork(&base, &resolver, Some(song("A", None))));
        assert_eq!(base.song().unwrap().album.as_deref(), Some("Album"));
    }

    #[test]
    fn failed_status_read_skips_the_poll() {
        assert!(skip_poll(&MprisSnapshot { status: None, ..Default::default() }));
        assert!(!skip_poll(&MprisSnapshot { status: Some("Playing".into()), ..Default::default() }));
    }

    #[test]
    fn failed_flags_read_keeps_previous_flags() {
        let prev = MprisFlags { can_control: true, can_play: true, ..Default::default() };
        let snap = MprisSnapshot { status: Some("Playing".into()), flags_failed: true, ..Default::default() };
        assert_eq!(stabilize(snap, Some(prev)).flags, prev);
        let ok = MprisSnapshot { status: Some("Playing".into()), ..Default::default() };
        assert_eq!(stabilize(ok, Some(prev)).flags, MprisFlags::default());
    }

    #[test]
    fn new_song_replaces_old() {
        let resolver = ArtworkResolver::new(vec![], Arc::new(UrlStore));
        let base = BasePlayerController::with_player_info("test", "test");
        apply_artwork(&base, &resolver, Some(song("A", None)));
        assert!(apply_artwork(&base, &resolver, Some(song("B", None))));
        assert_eq!(base.song().unwrap().title.as_deref(), Some("B"));
    }

    #[test]
    fn with_config_and_base_keeps_name() {
        let base = BasePlayerController::with_player_info("shairport", "shairport");
        let c = MprisPlayerController::with_config_and_base(MprisConfig::new("org.mpris.MediaPlayer2.ShairportSync"), base);
        assert_eq!(c.get_player_name(), "shairport");
        assert_eq!(c.get_player_id(), "shairport");
    }

    #[test]
    fn with_config_uses_name_or_bus_name() {
        let mut cfg = MprisConfig::new("org.mpris.MediaPlayer2.vlc");
        assert_eq!(MprisPlayerController::with_config(cfg.clone()).get_player_name(), "vlc");
        cfg.name = Some("video".to_string());
        assert_eq!(MprisPlayerController::with_config(cfg).get_player_name(), "video");
    }

    #[test]
    fn default_capabilities_have_no_volume_or_kill() {
        let c = MprisPlayerController::with_config(MprisConfig::new("x"));
        let caps = c.get_capabilities();
        assert!(!caps.has_capability(PlayerCapability::Volume));
        assert!(!caps.has_capability(PlayerCapability::Killable));
    }
}
