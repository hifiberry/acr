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