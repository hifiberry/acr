//! Configuration of an MPRIS player controller.

use std::path::PathBuf;
use std::time::Duration;

use crate::data::PlayerCapability;

/// Which D-Bus bus the player is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BusPreference {
    /// Look on the session bus, then the system bus, until the player is found.
    Auto,
    System,
    Session,
}

/// Settings for one MPRIS player.
#[derive(Debug, Clone)]
pub struct MprisConfig {
    /// D-Bus name, e.g. `org.mpris.MediaPlayer2.vlc`.
    pub bus_name: String,
    /// Player name; defaults to the last part of the bus name.
    pub name: Option<String>,
    pub bus: BusPreference,
    pub poll_interval: Duration,
    /// Directories local `mpris:artUrl` files may be imported from.
    pub artwork_dirs: Vec<PathBuf>,
    /// Report no song while the player is stopped. For players that keep the
    /// last song in `Metadata` after playback ends, like shairport-sync.
    pub clear_song_when_stopped: bool,
    /// Capabilities the embedding player adds to those MPRIS reports. Code only.
    pub extra_capabilities: Vec<PlayerCapability>,
}

impl MprisConfig {
    pub fn new(bus_name: &str) -> Self {
        Self {
            bus_name: bus_name.to_string(),
            name: None,
            bus: BusPreference::Auto,
            poll_interval: Duration::from_secs(1),
            artwork_dirs: Vec::new(),
            clear_song_when_stopped: false,
            extra_capabilities: Vec::new(),
        }
    }

    /// Read the settings of an `"mpris"` player entry.
    pub fn from_json(v: &serde_json::Value) -> Result<Self, String> {
        let bus_name = v
            .get("bus_name")
            .and_then(|b| b.as_str())
            .filter(|b| !b.is_empty())
            .ok_or_else(|| "bus_name is required".to_string())?;
        let mut config = Self::new(bus_name);
        config.name = v.get("name").and_then(|n| n.as_str()).map(str::to_string);
        if let Some(bus) = v.get("bus") {
            config.bus = match bus.as_str() {
                Some("auto") => BusPreference::Auto,
                Some("system") => BusPreference::System,
                Some("session") => BusPreference::Session,
                _ => return Err(format!("bus must be \"auto\", \"system\" or \"session\", not {}", bus)),
            };
        }
        if let Some(secs) = v.get("poll_interval").and_then(|p| p.as_f64()) {
            if secs > 0.0 {
                config.poll_interval = Duration::from_secs_f64(secs);
            }
        }
        if let Some(dirs) = v.get("artwork_dirs").and_then(|d| d.as_array()) {
            config.artwork_dirs = dirs.iter().filter_map(|d| d.as_str()).map(PathBuf::from).collect();
        }
        if let Some(clear) = v.get("clear_song_when_stopped").and_then(|c| c.as_bool()) {
            config.clear_song_when_stopped = clear;
        }
        Ok(config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn minimal_json_uses_defaults() {
        let c = MprisConfig::from_json(&json!({"bus_name": "org.mpris.MediaPlayer2.vlc"})).unwrap();
        assert_eq!(c.bus_name, "org.mpris.MediaPlayer2.vlc");
        assert_eq!(c.bus, BusPreference::Auto);
        assert_eq!(c.poll_interval, Duration::from_secs(1));
        assert_eq!(c.name, None);
        assert!(c.artwork_dirs.is_empty());
        assert!(!c.clear_song_when_stopped);
        assert!(c.extra_capabilities.is_empty());
    }

    #[test]
    fn full_json_is_read() {
        let c = MprisConfig::from_json(&json!({
            "bus_name": "org.mpris.MediaPlayer2.ShairportSync",
            "name": "airplay",
            "bus": "system",
            "poll_interval": 0.5,
            "artwork_dirs": ["/tmp/a", "/tmp/b"],
            "clear_song_when_stopped": true
        })).unwrap();
        assert_eq!(c.name.as_deref(), Some("airplay"));
        assert_eq!(c.bus, BusPreference::System);
        assert_eq!(c.poll_interval, Duration::from_millis(500));
        assert_eq!(c.artwork_dirs, vec![PathBuf::from("/tmp/a"), PathBuf::from("/tmp/b")]);
        assert!(c.clear_song_when_stopped);
        assert_eq!(
            MprisConfig::from_json(&json!({"bus_name": "x", "bus": "session"})).unwrap().bus,
            BusPreference::Session
        );
    }

    #[test]
    fn bad_bus_is_an_error() {
        let e = MprisConfig::from_json(&json!({"bus_name": "x", "bus": "dbus"})).unwrap_err();
        assert!(e.contains("bus"), "{}", e);
    }

    #[test]
    fn missing_bus_name_is_an_error() {
        let e = MprisConfig::from_json(&json!({})).unwrap_err();
        assert!(e.contains("bus_name"), "{}", e);
    }
}
