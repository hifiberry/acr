use crate::data::PlayerEvent;
use crate::plugins::plugin::{Plugin, BasePlugin};
use std::any::Any;
use delegate::delegate;

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

/// Type for event filter functions that can be used with BaseEventFilter
pub type EventFilterFn = fn(&dyn Plugin, PlayerEvent, bool) -> Option<PlayerEvent>;

/// Base implementation of EventFilter that delegates to a filter function
pub struct BaseEventFilter {
    /// Base plugin implementation
    base: BasePlugin,
    
    /// Filter function to delegate to
    filter_fn: EventFilterFn,
}

impl BaseEventFilter {
    /// Create a new BaseEventFilter
    pub fn new(name: &str, filter_fn: EventFilterFn) -> Self {
        Self {
            base: BasePlugin::new(name),
            filter_fn,
        }
    }
}

impl Plugin for BaseEventFilter {
    delegate! {
        to self.base {
            fn name(&self) -> &str;
            fn version(&self) -> &str;
            fn init(&mut self) -> bool;
            fn shutdown(&mut self) -> bool;
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl EventFilter for BaseEventFilter {
    fn filter_event(&self, event: PlayerEvent, is_active_player: bool) -> Option<PlayerEvent> {
        (self.filter_fn)(self, event, is_active_player)
    }
}