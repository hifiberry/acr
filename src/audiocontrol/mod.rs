// Audio controller module for managing multiple players
pub mod audiocontrol;
// EventBus for distributing PlayerEvents to subscribers
pub mod eventbus;
// The one seam between the player side and metadata enrichment
pub mod now_playing_bridge;

// Re-export the AudioController
pub use audiocontrol::AudioController;
// Re-export the EventBus and related types
pub use eventbus::{EventBus, EventSubscription, EventSubscriber, SubscriberId};