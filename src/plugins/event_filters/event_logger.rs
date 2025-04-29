use crate::data::PlayerEvent;
use crate::plugins::plugin::Plugin;
use crate::plugins::event_filters::event_filter::{EventFilter, BaseEventFilter};
use std::any::Any;
use delegate::delegate;

/// A simple plugin that logs player events
pub struct EventLogger {
    /// Base filter implementation that handles common functionality
    base: BaseEventFilter,
    
    /// Whether to only log events from the active player
    only_active: bool,
}

impl EventLogger {
    /// Create a new EventLogger
    pub fn new(only_active: bool) -> Self {
        Self {
            base: BaseEventFilter::new("EventLogger", Self::filter_event_impl),
            only_active,
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
                        song.title.as_deref().unwrap_or("Unknown"),
                        song.artist.as_deref().unwrap_or("Unknown"),
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

impl Plugin for EventLogger {
    delegate! {
        to self.base {
            fn name(&self) -> &str;
            fn version(&self) -> &str;
        }
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
        Self::filter_event_impl(self, event, is_active_player)
    }
}