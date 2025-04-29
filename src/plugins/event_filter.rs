use crate::data::PlayerEvent;
use crate::plugins::plugin::Plugin;
use std::any::Any;

/// A plugin that can filter player events
pub trait EventFilter: Plugin {
    /// Process an event and return a filtered version or None to remove the event
    /// 
    /// # Arguments
    /// 
    /// * `event` - The event to filter
    /// * `is_active_player` - Whether the event came from the currently active player
    /// 
    /// # Returns
    /// 
    /// The filtered event, or None if the event should be dropped
    fn filter_event(&self, event: PlayerEvent, is_active_player: bool) -> Option<PlayerEvent>;
}

/// A simple plugin that logs player events
pub struct EventLogger {
    /// Plugin name
    name: String,
    
    /// Plugin version
    version: String,
    
    /// Whether to only log events from the active player
    only_active: bool,
}

impl EventLogger {
    /// Create a new EventLogger
    pub fn new(only_active: bool) -> Self {
        Self {
            name: "EventLogger".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            only_active,
        }
    }
}

impl Plugin for EventLogger {
    fn name(&self) -> &str {
        &self.name
    }

    fn version(&self) -> &str {
        &self.version
    }

    fn init(&mut self) -> bool {
        log::info!("EventLogger initialized. Only active: {}", self.only_active);
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
        // Only log events from the active player if only_active is true
        if self.only_active && !is_active_player {
            return Some(event); // Pass through without logging
        }
        
        match &event {
            PlayerEvent::StateChanged { source, state } => {
                log::info!(
                    "Player {} (ID: {}) state changed to {:?}{}",
                    source.player_name(),
                    source.player_id(),
                    state,
                    if is_active_player { " [ACTIVE]" } else { "" }
                );
            },
            PlayerEvent::SongChanged { source, song } => {
                if let Some(song) = song {
                    log::info!(
                        "Player {} (ID: {}) changed song to '{}' by '{}'{}",
                        source.player_name(),
                        source.player_id(),
                        song.title(),
                        song.artist().unwrap_or("Unknown"),
                        if is_active_player { " [ACTIVE]" } else { "" }
                    );
                } else {
                    log::info!(
                        "Player {} (ID: {}) cleared current song{}",
                        source.player_name(),
                        source.player_id(),
                        if is_active_player { " [ACTIVE]" } else { "" }
                    );
                }
            },
            PlayerEvent::LoopModeChanged { source, mode } => {
                log::info!(
                    "Player {} (ID: {}) changed loop mode to {:?}{}",
                    source.player_name(),
                    source.player_id(),
                    mode,
                    if is_active_player { " [ACTIVE]" } else { "" }
                );
            },
            PlayerEvent::CapabilitiesChanged { source, capabilities } => {
                log::info!(
                    "Player {} (ID: {}) capabilities changed: {:?}{}",
                    source.player_name(),
                    source.player_id(),
                    capabilities,
                    if is_active_player { " [ACTIVE]" } else { "" }
                );
            },
        }
        
        // Return the event unchanged so it can be processed by other components
        Some(event)
    }
}