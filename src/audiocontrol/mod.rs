// Audio controller module for managing multiple players
pub mod audiocontrol;
// EventBus for distributing PlayerEvents to subscribers
pub mod eventbus;
// The in-process forwarder that used to be the seam to metadata enrichment.
// The daemon no longer calls it: the metadata side subscribes to /api/events
// instead (audiocontrol_metadata::now_playing_ws).
pub mod now_playing_bridge;
// Splitting an album-artist string on separators alone. Still named for the
// seam it replaced: this used to be where an injected client asked the metadata
// daemon what a name split into.
//
// **There is no client here any more, and nowhere left to put one.** No route
// on the metadata daemon may be called by this one, so `metadata_client` and
// the `enrichment` injection point it was installed through are both gone, and
// with them the last thing in this daemon that held the metadata daemon's
// address. See doc/communications.md.
pub mod resolver;

// Re-export the AudioController
pub use audiocontrol::AudioController;
// Re-export the EventBus and related types
pub use eventbus::{EventBus, EventSubscription, EventSubscriber, SubscriberId};