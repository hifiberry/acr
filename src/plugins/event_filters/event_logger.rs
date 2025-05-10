use crate::data::PlayerEvent;
use crate::plugins::plugin::Plugin;
use crate::plugins::event_filters::event_filter::{EventFilter, BaseEventFilter};
use std::any::Any;
use std::collections::HashSet;
use delegate::delegate;

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
    /// Base filter implementation that handles common functionality
    base: BaseEventFilter,
    
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
            base: BaseEventFilter::new("EventLogger", Self::filter_event_impl),
            only_active,
            log_level: LogLevel::default(),
            event_types: None,
        }
    }
    
    /// Create a new EventLogger with custom configuration
    pub fn with_config(only_active: bool, log_level: LogLevel, event_types: Option<HashSet<String>>) -> Self {
        Self {
            base: BaseEventFilter::new("EventLogger", Self::filter_event_impl),
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
            PlayerEvent::CapabilitiesChanged { .. } => "capabilities",
            PlayerEvent::PositionChanged { .. } => "position",
            PlayerEvent::DatabaseUpdating { .. } => "database",
            PlayerEvent::QueueChanged { .. } => "queue",
        }
    }
    
    /// Log a message with the appropriate log level
    fn log(&self, msg: &str, is_active_player: bool) {
        let active_suffix = if is_active_player { " [ACTIVE]" } else { "" };
        let full_msg = format!("{}{}", msg, active_suffix);
        
        match self.log_level {
            LogLevel::Debug => log::debug!("{}", full_msg),
            LogLevel::Info => log::info!("{}", full_msg),
            LogLevel::Warning => log::warn!("{}", full_msg),
            LogLevel::Error => log::error!("{}", full_msg),
        }
    }
    
    /// Implementation of the event filtering logic
    fn filter_event_impl(plugin: &dyn Plugin, event: PlayerEvent, is_active_player: bool) -> Option<PlayerEvent> {
        // We know this is an EventLogger because we control where this function is passed
        let logger = plugin.as_any().downcast_ref::<EventLogger>().unwrap();
        
        // Only log events from the active player if only_active is true
        if logger.only_active && !is_active_player {
            return Some(event); // Pass through without logging
        }
        
        // Check if we should log this event type
        let event_type = Self::get_event_type(&event);
        if !logger.should_log_event_type(event_type) {
            return Some(event); // Pass through without logging
        }
        
        match &event {
            PlayerEvent::StateChanged { source, state } => {
                logger.log(
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
                    logger.log(
                        &format!(
                            "Player {} (ID: {}) changed song to '{}' by '{}'",
                            source.player_name(),
                            source.player_id(),
                            song.title.as_deref().unwrap_or("Unknown"),
                            song.artist.as_deref().unwrap_or("Unknown")
                        ),
                        is_active_player
                    );
                } else {
                    logger.log(
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
                logger.log(
                    &format!(
                        "Player {} (ID: {}) changed loop mode to {:?}",
                        source.player_name(),
                        source.player_id(),
                        mode
                    ),
                    is_active_player
                );
            },
            PlayerEvent::CapabilitiesChanged { source, capabilities } => {
                logger.log(
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
                logger.log(
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
                
                logger.log(
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
                logger.log(
                    &format!(
                        "Player {} (ID: {}) queue changed",
                        source.player_name(),
                        source.player_id()
                    ),
                    is_active_player
                );
            },
        }
        
        // Return the event unchanged so it can be processed by other components
        Some(event)
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
        true
    }

    fn shutdown(&mut self) -> bool {
        log::info!("EventLogger shutdown");
        true
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl EventFilter for EventLogger {
    fn filter_event(&self, event: PlayerEvent, is_active_player: bool) -> Option<PlayerEvent> {
        Self::filter_event_impl(self, event, is_active_player)
    }
}