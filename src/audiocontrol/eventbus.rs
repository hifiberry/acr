use crate::data::player_event::PlayerEvent;
use crossbeam::channel::{unbounded, Receiver, Sender};
use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::sync::Arc;
use parking_lot::Mutex;
use std::thread;

/// Defines what kinds of events a subscriber wants to receive
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum EventSubscription {
    /// Subscribe to all player events
    All,
    
    /// Subscribe to state change events only
    StateChanged,
    
    /// Subscribe to song change events only
    SongChanged,
    
    /// Subscribe to loop mode change events only
    LoopModeChanged,
    
    /// Subscribe to random mode change events only
    RandomChanged,
    
    /// Subscribe to capabilities change events only
    CapabilitiesChanged,
    
    /// Subscribe to playback position change events only
    PositionChanged,
    
    /// Subscribe to database update events only
    DatabaseUpdating,
    
    /// Subscribe to queue change events only
    QueueChanged,

    /// Subscribe to song information update events only
    SongInformationUpdate,
    
    /// Subscribe to active player changed events only
    ActivePlayerChanged,
    
    /// Subscribe to volume changed events only
    VolumeChanged,
}

impl From<&PlayerEvent> for EventSubscription {
    fn from(event: &PlayerEvent) -> Self {
        match event {
            PlayerEvent::StateChanged { .. } => EventSubscription::StateChanged,
            PlayerEvent::SongChanged { .. } => EventSubscription::SongChanged,
            PlayerEvent::LoopModeChanged { .. } => EventSubscription::LoopModeChanged,
            PlayerEvent::RandomChanged { .. } => EventSubscription::RandomChanged,
            PlayerEvent::CapabilitiesChanged { .. } => EventSubscription::CapabilitiesChanged,
            PlayerEvent::PositionChanged { .. } => EventSubscription::PositionChanged,
            PlayerEvent::DatabaseUpdating { .. } => EventSubscription::DatabaseUpdating,
            PlayerEvent::QueueChanged { .. } => EventSubscription::QueueChanged,
            PlayerEvent::SongInformationUpdate { .. } => EventSubscription::SongInformationUpdate,
            PlayerEvent::ActivePlayerChanged { .. } => EventSubscription::ActivePlayerChanged,
            PlayerEvent::VolumeChanged { .. } => EventSubscription::VolumeChanged,
        }
    }
}

/// Type alias for a subscriber ID
pub type SubscriberId = u64;

/// Global singleton instance of the EventBus.
static GLOBAL_EVENT_BUS: Lazy<EventBus> = Lazy::new(EventBus::new);

/// EventBus for distributing PlayerEvents to subscribers
#[derive(Clone)]
pub struct EventBus {
    subscribers: Arc<Mutex<HashMap<SubscriberId, (Sender<PlayerEvent>, Vec<EventSubscription>)>>>,
    next_id: Arc<Mutex<SubscriberId>>,
}

impl EventBus {
    /// Create a new EventBus instance
    /// Note: For a global singleton, use EventBus::instance()
    pub fn new() -> Self {
        EventBus {
            subscribers: Arc::new(Mutex::new(HashMap::new())),
            next_id: Arc::new(Mutex::new(0)),
        }
    }
    
    /// Get a clone of the global EventBus singleton instance.
    pub fn instance() -> Self {
        GLOBAL_EVENT_BUS.clone()
    }
    
    /// Subscribe to receive all events
    pub fn subscribe_all(&self) -> (SubscriberId, Receiver<PlayerEvent>) {
        self.subscribe(vec![EventSubscription::All])
    }
    
    /// Subscribe to receive specific event types
    pub fn subscribe(&self, event_types: Vec<EventSubscription>) -> (SubscriberId, Receiver<PlayerEvent>) {
        let (sender, receiver) = unbounded();
        
        let mut id_guard = self.next_id.lock();
        let id = *id_guard;
        *id_guard += 1;
        
        let mut subscribers = self.subscribers.lock();
        subscribers.insert(id, (sender, event_types));
        
        (id, receiver)
    }
    
    /// Unsubscribe from the event bus
    pub fn unsubscribe(&self, id: SubscriberId) -> bool {
        let mut subscribers = self.subscribers.lock();
        subscribers.remove(&id).is_some()
    }
    
    /// Publish an event to all relevant subscribers
    pub fn publish(&self, event: PlayerEvent) {
        let subscribers = self.subscribers.lock();
        let event_type = EventSubscription::from(&event);
        
        for (_, (sender, subscriptions)) in subscribers.iter() {
            // Send if subscriber wants all events or this specific event type
            if subscriptions.contains(&EventSubscription::All) || subscriptions.contains(&event_type) {
                // Clone the event for each subscriber
                let event_clone = event.clone();
                // Use try_send to avoid blocking if a subscriber is not consuming events
                let _ = sender.try_send(event_clone);
            }
        }
    }

    /// Spawn a worker thread that consumes events from a receiver and processes them
    pub fn spawn_worker<F>(&self, id: SubscriberId, receiver: Receiver<PlayerEvent>, worker: F) -> thread::JoinHandle<()>
    where
        F: FnMut(PlayerEvent) + Send + 'static,
    {
        let event_bus = self.clone();
        
        thread::spawn(move || {
            let mut worker = worker;
            
            // Process events until the channel is closed
            while let Ok(event) = receiver.recv() {
                worker(event);
            }
            
            // Clean up subscription when the thread exits
            event_bus.unsubscribe(id);
        })
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}

/// Helper struct to provide filter methods for subscribers
pub struct EventSubscriber {
    receiver: Receiver<PlayerEvent>,
}

impl EventSubscriber {
    /// Create a new subscriber from a receiver
    pub fn new(receiver: Receiver<PlayerEvent>) -> Self {
        Self { receiver }
    }
    
    /// Get the underlying receiver
    pub fn receiver(&self) -> &Receiver<PlayerEvent> {
        &self.receiver
    }
    
    /// Wait for the next event
    pub fn next_event(&self) -> Result<PlayerEvent, crossbeam::channel::RecvError> {
        self.receiver.recv()
    }
    
    /// Try to get the next event without blocking
    pub fn try_next_event(&self) -> Result<PlayerEvent, crossbeam::channel::TryRecvError> {
        self.receiver.try_recv()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::{PlayerSource, PlaybackState};
    
    #[test]
    fn test_subscribe_all() {
        let bus = EventBus::new();
        let (_, receiver) = bus.subscribe_all();
        
        let source = PlayerSource::new("test".to_string(), "1".to_string());
        let event = PlayerEvent::StateChanged {
            source,
            state: PlaybackState::Playing,
        };
        
        bus.publish(event.clone());
        
        let received = receiver.recv().unwrap();
        assert!(matches!(received, PlayerEvent::StateChanged { .. }));
    }
    
    #[test]
    fn test_subscribe_specific() {
        let bus = EventBus::new();
        let (_, state_receiver) = bus.subscribe(vec![EventSubscription::StateChanged]);
        let (_, song_receiver) = bus.subscribe(vec![EventSubscription::SongChanged]);
        
        let source = PlayerSource::new("test".to_string(), "1".to_string());
        
        // Publish a state change event
        let state_event = PlayerEvent::StateChanged {
            source: source.clone(),
            state: PlaybackState::Playing,
        };
        bus.publish(state_event);
        
        // State subscriber should receive it
        assert!(matches!(state_receiver.recv().unwrap(), PlayerEvent::StateChanged { .. }));
        
        // Song subscriber should not receive it
        assert!(song_receiver.try_recv().is_err());
    }
    
    #[test]
    fn test_unsubscribe() {
        let bus = EventBus::new();
        let (id, receiver) = bus.subscribe_all();
        
        // Unsubscribe
        assert!(bus.unsubscribe(id));
        
        let source = PlayerSource::new("test".to_string(), "1".to_string());
        let event = PlayerEvent::StateChanged {
            source,
            state: PlaybackState::Playing,
        };
        
        bus.publish(event);
        
        // Should not receive the event after unsubscribing
        assert!(receiver.try_recv().is_err());
    }
}