// Audio controller module for managing multiple players
pub mod audiocontrol;
// EventBus for distributing PlayerEvents to subscribers
pub mod eventbus;
// The in-process forwarder that used to be the seam to metadata enrichment.
// The daemon no longer calls it: that seam is HTTP now (see metadata_client
// below, and audiocontrol_metadata::now_playing_ws for the other direction).
pub mod now_playing_bridge;
// Where the player side finds the library enricher, if one was injected
pub mod enrichment;
// The player side's HTTP client for the two seams the metadata side answers
pub mod metadata_client;
// Where the player side finds the resolver, if one was injected
pub mod resolver;

// Re-export the AudioController
pub use audiocontrol::AudioController;
// Re-export the EventBus and related types
pub use eventbus::{EventBus, EventSubscription, EventSubscriber, SubscriberId};