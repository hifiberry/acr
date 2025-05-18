use crate::data::PlayerEvent;
use crate::plugins::plugin::Plugin;
use crate::plugins::action_plugin::{ActionPlugin, BaseActionPlugin};
use std::any::Any;
use std::collections::HashSet;
use delegate::delegate;
use crate::audiocontrol::AudioController;
use std::sync::Weak;

/// Log level for the EventLogger
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    Debug,
    Info,
    Warning,
    Error,
}

impl Default for LogLevel {
    fn default() -> Self {
        LogLevel::Info
    }
}

impl From<&str> for LogLevel {
    fn from(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "debug" => LogLevel::Debug,
            "info" => LogLevel::Info,
            "warning" | "warn" => LogLevel::Warning,
            "error" | "err" => LogLevel::Error,
            _ => LogLevel::Info, // Default to Info for unrecognized values
        }
    }
}

/// A simple plugin that logs player events
pub struct EventLogger {
    /// Base action plugin implementation
    base: BaseActionPlugin,

    /// Whether to only log events from the active player
    only_active: bool,

    /// Log level to use for output
    log_level: LogLevel,

    /// Set of event types to log (if empty, log all events)
    event_types: Option<HashSet<String>>,
}

impl EventLogger {
    /// Create a new EventLogger
    pub fn new(only_active: bool) -> Self {
        Self {
            base: BaseActionPlugin::new("EventLogger"),
            only_active,
            log_level: LogLevel::default(),
            event_types: None,
        }
    }

    /// Create a new EventLogger with custom configuration
    pub fn with_config(only_active: bool, log_level: LogLevel, event_types: Option<HashSet<String>>) -> Self {
        Self {
            base: BaseActionPlugin::new("EventLogger"),
            only_active,
            log_level,
            event_types,
        }
    }

    /// Set the log level
    pub fn set_log_level(&mut self, level: LogLevel) {
        self.log_level = level;
    }

    /// Set the event types to log
    pub fn set_event_types(&mut self, event_types: Option<HashSet<String>>) {
        self.event_types = event_types;
    }

    /// Check if an event type should be logged
    fn should_log_event_type(&self, event_type: &str) -> bool {
        match &self.event_types {
            Some(types) => types.contains(event_type),
            None => true, // Log all event types if none are specified
        }
    }

    /// Get the event type name from a PlayerEvent
    fn get_event_type(event: &PlayerEvent) -> &'static str {
        match event {
            PlayerEvent::StateChanged { .. } => "state",
            PlayerEvent::SongChanged { .. } => "song",
            PlayerEvent::LoopModeChanged { .. } => "loop",
            PlayerEvent::RandomChanged { .. } => "random",
            PlayerEvent::CapabilitiesChanged { .. } => "capabilities",
            PlayerEvent::PositionChanged { .. } => "position",
            PlayerEvent::DatabaseUpdating { .. } => "database",
            PlayerEvent::QueueChanged { .. } => "queue",
        }
    }

    /// Log a message with the appropriate log level
    fn log_message(&self, msg: &str, is_active_player: bool) {
        let active_suffix = if is_active_player { " [ACTIVE]" } else { "" };
        let full_msg = format!("{}{}", msg, active_suffix);

        match self.log_level {
            LogLevel::Debug => log::debug!("{}", full_msg),
            LogLevel::Info => log::info!("{}", full_msg),
            LogLevel::Warning => log::warn!("{}", full_msg),
            LogLevel::Error => log::error!("{}", full_msg),
        }
    }

    /// Implementation of the event logging logic
    fn log_event(&self, event: &PlayerEvent, is_active_player: bool) {
        // Only log events from the active player if only_active is true
        if self.only_active && !is_active_player {
            return;
        }

        // Check if we should log this event type
        let event_type = Self::get_event_type(&event);
        if !self.should_log_event_type(event_type) {
            return;
        }

        match &event {
            PlayerEvent::StateChanged { source, state } => {
                self.log_message(
                    &format!(
                        "Player {} (ID: {}) state changed to {:?}",
                        source.player_name(),
                        source.player_id(),
                        state
                    ),
                    is_active_player
                );
            },
            PlayerEvent::SongChanged { source, song } => {
                if let Some(song) = song {
                    self.log_message(
                        &format!(
                            "Player {} (ID: {}) changed song to \'{}\' by \'{}\'",
                            source.player_name(),
                            source.player_id(),
                            song.title.as_deref().unwrap_or("Unknown"),
                            song.artist.as_deref().unwrap_or("Unknown")
                        ),
                        is_active_player
                    );
                } else {
                    self.log_message(
                        &format!(
                            "Player {} (ID: {}) cleared current song",
                            source.player_name(),
                            source.player_id()
                        ),
                        is_active_player
                    );
                }
            },
            PlayerEvent::LoopModeChanged { source, mode } => {
                self.log_message(
                    &format!(
                        "Player {} (ID: {}) changed loop mode to {:?}",
                        source.player_name(),
                        source.player_id(),
                        mode
                    ),
                    is_active_player
                );
            },
            PlayerEvent::RandomChanged { source, enabled } => {
                self.log_message(
                    &format!(
                        "Player {} (ID: {}) changed random/shuffle mode to {}",
                        source.player_name(),
                        source.player_id(),
                        if *enabled { "enabled" } else { "disabled" }
                    ),
                    is_active_player
                );
            },
            PlayerEvent::CapabilitiesChanged { source, capabilities } => {
                self.log_message(
                    &format!(
                        "Player {} (ID: {}) capabilities changed: {:?}",
                        source.player_name(),
                        source.player_id(),
                        capabilities
                    ),
                    is_active_player
                );
            },
            PlayerEvent::PositionChanged { source, position } => {
                self.log_message(
                    &format!(
                        "Player {} (ID: {}) position changed to {:.1}s",
                        source.player_name(),
                        source.player_id(),
                        position
                    ),
                    is_active_player
                );
            },
            PlayerEvent::DatabaseUpdating { source, artist, album, song, percentage } => {
                let progress_str = if let Some(pct) = percentage {
                    format!(" - {:.1}%", pct)
                } else {
                    String::new()
                };

                let item_str = match (artist, album, song) {
                    (Some(a), Some(b), Some(s)) => format!("artist: {}, album: {}, song: {}", a, b, s),
                    (Some(a), Some(b), None) => format!("artist: {}, album: {}", a, b),
                    (Some(a), None, None) => format!("artist: {}", a),
                    (None, Some(b), None) => format!("album: {}", b),
                    (None, None, Some(s)) => format!("song: {}", s),
                    (None, Some(b), Some(s)) => format!("album: {}, song: {}", b, s),
                    (Some(a), None, Some(s)) => format!("artist: {}, song: {}", a, s),
                    _ => "database".to_string(),
                };

                self.log_message(
                    &format!(
                        "Player {} (ID: {}) updating {}{}",
                        source.player_name(),
                        source.player_id(),
                        item_str,
                        progress_str
                    ),
                    is_active_player
                );
            },
            PlayerEvent::QueueChanged { source } => {
                self.log_message(
                    &format!(
                        "Player {} (ID: {}) queue changed",
                        source.player_name(),
                        source.player_id()
                    ),
                    is_active_player
                );
            },
        }
    }
}

impl Plugin for EventLogger {
    delegate! {
        to self.base {
            fn name(&self) -> &str;
            fn version(&self) -> &str;
        }
    }

    fn init(&mut self) -> bool {
        log::info!(
            "EventLogger initialized. Only active: {}, Log level: {:?}, Event types: {:?}",
            self.only_active,
            self.log_level,
            self.event_types
        );
        self.base.init()
    }

    fn shutdown(&mut self) -> bool {
        log::info!("EventLogger shutdown");
        self.base.shutdown()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl ActionPlugin for EventLogger {
    fn initialize(&mut self, controller: Weak<AudioController>) {
        self.base.set_controller(controller);
    }

    fn on_event(&mut self, event: &PlayerEvent, is_active_player: bool) {
        self.log_event(event, is_active_player);
    }
}
