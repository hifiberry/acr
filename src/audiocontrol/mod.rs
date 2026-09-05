// Audio controller module for managing multiple players
pub mod audiocontrol;
// EventBus for distributing PlayerEvents to subscribers
pub mod eventbus;
// The one seam between the player side and metadata enrichment
pub mod now_playing_bridge;
// Where the player side finds the library enricher, if one was injected
pub mod enrichment;
// Where the player side finds the resolver, if one was injected
pub mod resolver;
// Where the player side finds a Spotify access token source, if one was injected
pub mod token;

// Re-export the AudioController
pub use audiocontrol::AudioController;
// Re-export the EventBus and related types
pub use eventbus::{EventBus, EventSubscription, EventSubscriber, SubscriberId};