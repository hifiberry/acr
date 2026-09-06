use std::sync::Arc;
use parking_lot::Mutex;
use std::collections::{HashMap, HashSet, VecDeque};
use std::time::{Duration, Instant};
use serde::{Serialize, Deserialize};
use log::{debug, info, error};

// Use the correct rocket_ws imports
use rocket_ws::{WebSocket, Channel, Message};
use rocket::futures::{Sink, SinkExt, Stream, StreamExt};

use crate::data::PlayerEvent;
use crate::audiocontrol::eventbus::EventBus;

/// How long a client may go without a frame arriving from it before the prune
/// task drops it.
///
/// This is the reaping policy and it is deliberately unchanged: a connection
/// whose peer has vanished without a FIN produces no frames and no send error
/// for a long time, and dropping it after an hour of silence is the only thing
/// that notices. What changed is *what counts as a frame* - see `PING_INTERVAL`.
const CLIENT_TIMEOUT: Duration = Duration::from_secs(3600);

/// How often the prune task looks for silent clients and stale events.
const PRUNE_INTERVAL: Duration = Duration::from_secs(300);

/// How long a queued event stays available for delivery.
const EVENT_TIMEOUT: Duration = Duration::from_secs(30);

/// How often each connection polls for events to deliver.
const EVENT_POLL_INTERVAL: Duration = Duration::from_millis(500);

/// How often the server sends a WebSocket ping on each open connection.
///
/// Only a frame arriving *from* the peer refreshes that peer's last-activity
/// time, and a browser page cannot produce one on demand: the JavaScript
/// WebSocket API exposes no ping. So a client that subscribes once and then
/// only listens - the WebUI is exactly that - used to be pruned an hour after
/// connecting with its socket still open: no error, no close frame, events
/// simply stopped. A server ping fixes it without any client change, because
/// the peer's WebSocket stack answers a ping with a pong at the protocol level,
/// and that pong is an inbound frame.
///
/// 30 s against a `CLIENT_TIMEOUT` of 3600 s means a live peer answers about
/// 120 times per timeout window, so reaching the timeout takes a full hour of
/// consecutively unanswered pings - not one lost packet, one slow scheduler
/// tick, or one long garbage collection in the browser. A peer that has really
/// gone away answers none of them and is still reaped on schedule.
const PING_INTERVAL: Duration = Duration::from_secs(30);

/// New format for WebSocket messages with source at top level
#[derive(Debug, Clone, Serialize)]
struct WebSocketMessage {
    #[serde(flatten)]
    event_data: serde_json::Value,
    source: serde_json::Value,
}

/// Subscription request from client
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventSubscription {
    /// Player names to subscribe to (empty for all players)
    pub players: Option<Vec<String>>,
    
    /// Event types to subscribe to (empty for all events)
    pub event_types: Option<Vec<String>>,
}

/// Command from client (could be subscription or song update)
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)] // Allows trying to deserialize into one variant then the other
enum ClientMessage {
    Subscription(EventSubscription),
}

/// WebSocket client connection manager
#[derive(Clone)]
pub struct WebSocketManager {
    /// Active subscriptions
    subscriptions: Arc<Mutex<HashMap<usize, ClientSubscription>>>,
    
    /// Last activity timestamp for pruning stale connections
    last_activity: Arc<Mutex<HashMap<usize, Instant>>>,
    
    /// Counter for generating unique IDs for clients
    next_id: Arc<Mutex<usize>>,

    /// Recent events that need to be sent to clients
    recent_events: Arc<Mutex<VecDeque<(PlayerEvent, Instant)>>>,

    /// Our subscription ID to the global event bus
    event_bus_subscription: Arc<Mutex<Option<(u64, crossbeam::channel::Receiver<PlayerEvent>)>>>,
}

/// Client subscription details
#[derive(Clone)]
struct ClientSubscription {
    /// Player names the client is subscribed to (empty = all)
    players: Option<HashSet<String>>,
    
    /// Event types the client is subscribed to (empty = all)
    event_types: Option<HashSet<String>>,
    
    /// Last event timestamp processed for this client
    last_event_time: Instant,
}

impl Default for WebSocketManager {
    fn default() -> Self {
        Self::new()
    }
}

impl WebSocketManager {    /// Create a new WebSocket manager
    pub fn new() -> Self {
        let manager = WebSocketManager {
            subscriptions: Arc::new(Mutex::new(HashMap::new())),
            last_activity: Arc::new(Mutex::new(HashMap::new())),
            next_id: Arc::new(Mutex::new(0)),
            recent_events: Arc::new(Mutex::new(VecDeque::with_capacity(100))),
            event_bus_subscription: Arc::new(Mutex::new(None)),
        };

        // Subscribe to all events from the global event bus
        let event_bus = EventBus::instance();
        let (id, receiver) = event_bus.subscribe_all();
        
        // Store our subscription ID (we'll need it to unsubscribe later)
        {
            let mut sub = manager.event_bus_subscription.lock();
            *sub = Some((id, receiver.clone()));
        }

        // Start a thread to listen for events from the event bus
        let manager_clone = manager.clone();
        std::thread::spawn(move || {
            debug!("Started WebSocketManager event bus listener thread");
            
            // This thread will continuously receive events from the event bus
            while let Ok(event) = receiver.recv() {
                debug!("WebSocketManager received event from global event bus: {}", event_type_name(&event));
                manager_clone.queue_event(event);
            }
            
            debug!("WebSocketManager event bus listener thread exiting");
        });

        // Return the manager
        manager
    }
    
    /// Generate a new unique ID for a client
    fn next_id(&self) -> usize {
        let mut id = self.next_id.lock();
        let current = *id;
        *id += 1;
        current
    }
    
    /// Register a new client subscription
    pub fn register(&self, subscription: EventSubscription) -> usize {
        let id = self.next_id();
        let now = Instant::now();
        
        let client_sub = ClientSubscription {
            players: subscription.players.map(|p| p.into_iter().collect()),
            event_types: subscription.event_types.map(|e| e.into_iter().collect()),
            last_event_time: now,
        };
        
        // Update last activity timestamp
        self.last_activity.lock().insert(id, now);

        // Store the subscription
        let mut subs = self.subscriptions.lock();
        subs.insert(id, client_sub);
        info!("WebSocket client registered (id: {}), total clients: {}", id, subs.len());
        
        id
    }
    
    /// Update a client's subscription
    pub fn update_subscription(&self, id: usize, subscription: EventSubscription) -> bool {
        // Update last activity timestamp
        self.last_activity.lock().insert(id, Instant::now());

        // Update the subscription
        let mut subs = self.subscriptions.lock();
        if let Some(sub) = subs.get_mut(&id) {
            sub.players = subscription.players.map(|p| p.into_iter().collect());
            sub.event_types = subscription.event_types.map(|e| e.into_iter().collect());
            debug!("Updated subscription for client {}", id);
            return true;
        }

        false
    }
    
    /// Record client activity to prevent timeout
    pub fn record_activity(&self, id: usize) {
        self.record_activity_at(id, Instant::now());
    }

    /// Record client activity as of a given instant.
    ///
    /// Splitting the clock out of `record_activity` lets a test drive the
    /// interplay between activity and pruning at chosen instants instead of
    /// waiting for real time to pass.
    pub fn record_activity_at(&self, id: usize, at: Instant) {
        self.last_activity.lock().insert(id, at);
    }

    /// Record that a frame arrived from a client.
    ///
    /// Every frame counts, whatever it carries - a `Pong` answering one of our
    /// pings included. That is what keeps a listen-only client alive, so this
    /// deliberately does not look at the frame to decide whether it counts.
    pub fn record_inbound_frame(&self, id: usize, msg: &Message) {
        self.record_inbound_frame_at(id, msg, Instant::now());
    }

    /// `record_inbound_frame` with the clock supplied, for tests.
    pub fn record_inbound_frame_at(&self, id: usize, msg: &Message, at: Instant) {
        debug!("Inbound frame: Client: {}, Kind: {}", id, frame_kind(msg));
        self.record_activity_at(id, at);
    }

    /// Queue a new event to be sent to clients
    pub fn queue_event(&self, event: PlayerEvent) {
        let now = Instant::now();
        
        // Add the event to the recent events queue
        let mut events = self.recent_events.lock();
        // Add to the back of the queue to maintain chronological order
        events.push_back((event.clone(), now));

        // Limit the queue size to prevent memory issues
        if events.len() > 100 {
            events.pop_front();
        }

        debug!("Event queued: Player: {}, Type: {:?}, Queue size: {}",
              event.player_name().unwrap_or("system"), event_type_name(&event), events.len());
    }
    
    /// Get events for a specific client that have occurred since the client last checked
    pub fn get_events_for_client(&self, client_id: usize) -> Vec<PlayerEvent> {
        let mut matching_events = Vec::new();
        
        // Get the client's subscription
        let mut last_event_time = Instant::now();
        let subscription = {
            let mut subs = self.subscriptions.lock();
            if let Some(sub) = subs.get_mut(&client_id) {
                let sub_copy = sub.clone();
                // Update the last event time
                last_event_time = sub.last_event_time;
                sub.last_event_time = Instant::now();
                Some(sub_copy)
            } else {
                None
            }
        };
        
        if let Some(sub) = subscription {
            debug!("Checking events: Client: {}, Last check: {:?} ago", 
                  client_id, Instant::now().duration_since(last_event_time));
            
            // Get recent events that occurred after the client's last check
            let events = self.recent_events.lock();
            debug!("Event queue size: {}", events.len());

            for (event, time) in events.iter() {
                // Only check events that happened after the client's last check
                if *time > last_event_time {
                    let should_send = self.should_send_to_client(event, &sub);
                    debug!("Event check: Player: {}, Type: {:?}, Time: {:?} ago, Should send: {}",
                          event.player_name().unwrap_or("system"), event_type_name(event),
                          Instant::now().duration_since(*time), should_send);

                    if should_send {
                        matching_events.push(event.clone());
                    }
                }
            }
            
            debug!("Sending events: Client: {}, Events to send: {}", 
                  client_id, matching_events.len());
        } else {
            debug!("Client not found: {}", client_id);
        }
        
        matching_events
    }
    
    /// Check if an event should be sent to a specific client based on subscription
    fn should_send_to_client(&self, event: &PlayerEvent, subscription: &ClientSubscription) -> bool {
        // Check player filter
        if let Some(players) = &subscription.players {
            let event_player = event.player_name().unwrap_or("system");
            
            // Allow "*" as wildcard for all players, or check if the specific player is in the list
            if !players.contains("*") && !players.contains(event_player) {
                return false;
            }
        }
        
        // Check event type filter
        if let Some(event_types) = &subscription.event_types {
            // Get event type as string
            let event_type = event_type_name(event);

            // Accept the pre-rename name so clients that subscribed to
            // `random_changed` keep receiving the event.
            let legacy_alias = match event_type {
                "shuffle_changed" => Some("random_changed"),
                _ => None,
            };

            if !event_types.contains(event_type)
                && !legacy_alias.is_some_and(|alias| event_types.contains(alias))
            {
                return false;
            }
        }
        
        true
    }
    
    /// Remove a client subscription
    pub fn remove_client(&self, id: usize) {
        // Remove from subscriptions
        let mut subs = self.subscriptions.lock();
        if subs.remove(&id).is_some() {
            info!("WebSocket client disconnected (id: {}), remaining clients: {}",
                id, subs.len());
        }
        drop(subs);

        // Clean up activity tracker
        self.last_activity.lock().remove(&id);
    }
    
    /// Prune inactive connections and old events
    pub fn prune_inactive_and_old(&self, client_timeout: Duration, event_timeout: Duration) {
        self.prune_inactive_and_old_at(Instant::now(), client_timeout, event_timeout);
    }

    /// Prune as of a given instant.
    ///
    /// The instant is a parameter for the same reason the timeouts are: a test
    /// can then ask "what would this do an hour from now" without sleeping.
    pub fn prune_inactive_and_old_at(
        &self,
        now: Instant,
        client_timeout: Duration,
        event_timeout: Duration,
    ) {
        // Prune inactive clients
        let clients_to_remove = {
            let mut to_remove = Vec::new();
            let last_activity = self.last_activity.lock();
            for (id, last) in last_activity.iter() {
                if now.duration_since(*last) > client_timeout {
                    to_remove.push(*id);
                }
            }
            to_remove
        };
        
        // Remove inactive clients
        for id in &clients_to_remove {
            self.remove_client(*id);
        }
        
        if !clients_to_remove.is_empty() {
            info!("Pruned {} inactive WebSocket connections", clients_to_remove.len());
        }
        
        // Prune old events
        {
            let mut events = self.recent_events.lock();
            // Since events are now stored in chronological order (oldest first),
            // we need to remove elements from the front of the queue
            let mut to_remove = 0;
            
            for (_, time) in events.iter() {
                if now.duration_since(*time) > event_timeout {
                    to_remove += 1;
                } else {
                    // Once we find a non-old event, we can stop checking
                    break;
                }
            }
            
            // Remove old events from the front of the queue
            if to_remove > 0 {
                for _ in 0..to_remove {
                    events.pop_front();
                }
                debug!("Pruned {} old WebSocket events", to_remove);
            }
        }
    }

    /// Whether a client is still registered.
    #[cfg(test)]
    fn is_registered(&self, id: usize) -> bool {
        self.subscriptions.lock().contains_key(&id)
    }

    /// The recorded last-activity time of a client, if it has one.
    #[cfg(test)]
    fn last_activity_of(&self, id: usize) -> Option<Instant> {
        self.last_activity.lock().get(&id).copied()
    }
}

/// Name a frame for the log. Only used for logging - nothing decides anything
/// from the kind of an inbound frame.
fn frame_kind(msg: &Message) -> &'static str {
    match msg {
        Message::Text(_) => "text",
        Message::Binary(_) => "binary",
        Message::Ping(_) => "ping",
        Message::Pong(_) => "pong",
        Message::Close(_) => "close",
        Message::Frame(_) => "raw frame",
    }
}

/// Convert PlayerEvent to WebSocketMessage format with source at top level
///
/// `forwarded_prefix` is the prefix captured when this connection was
/// upgraded. Two clients on different prefixes - one through nginx, one
/// direct - each get paths correct for themselves, because this runs inside
/// each connection's own loop rather than once per event.
fn convert_to_websocket_message(
    event: &PlayerEvent,
    forwarded_prefix: Option<&str>,
) -> WebSocketMessage {
    // Extract source information
    let source = serde_json::json!({
        "player_name": event.player_name(),
        "player_id": format!("{}:{}", event.player_name().unwrap_or("system"), "6600") // Default port for MPD
    });
      // Create event-specific data
    let event_data = match event {
        PlayerEvent::StateChanged { source, state } => {
            serde_json::json!({
                "type": "state_changed",
                "player_name": source.player_name(),
                "player_id": source.player_id(),
                "state": state.to_string()
            })
        },
        PlayerEvent::SongChanged { source, song } => {
            // Only clone when there is something to rewrite. Without a prefix
            // the song goes into the payload by reference, as it did before
            // this rewriting existed - these events fire per connected client.
            let rewritten;
            let song = if forwarded_prefix.is_some() {
                rewritten = song.clone().map(|mut song| {
                    crate::api::urlprefix::rewrite_song_urls(&mut song, forwarded_prefix);
                    song
                });
                &rewritten
            } else {
                song
            };
            serde_json::json!({
                "type": "song_changed",
                "player_name": source.player_name(),
                "player_id": source.player_id(),
                "song": song
            })
        },
        PlayerEvent::LoopModeChanged { source, mode } => {
            serde_json::json!({
                "type": "loop_mode_changed",
                "player_name": source.player_name(),
                "player_id": source.player_id(),
                "mode": mode.to_string()
            })
        },
        PlayerEvent::RandomChanged { source, enabled } => {
            serde_json::json!({
                "type": "shuffle_changed",
                "player_name": source.player_name(),
                "player_id": source.player_id(),
                // `shuffle` is the canonical field; `enabled` is emitted alongside
                // it for clients written against the previous `random_changed`
                // event and can be dropped once those have aged out.
                "shuffle": enabled,
                "enabled": enabled
            })
        },
        PlayerEvent::CapabilitiesChanged { source, capabilities } => {
            serde_json::json!({
                "type": "capabilities_changed",
                "player_name": source.player_name(),
                "player_id": source.player_id(),
                "capabilities": capabilities.to_vec()
            })
        },
        PlayerEvent::PositionChanged { source, position } => {
            serde_json::json!({
                "type": "position_changed",
                "player_name": source.player_name(),
                "player_id": source.player_id(),
                "position": position
            })
        },
        PlayerEvent::DatabaseUpdating { source, artist, album, song, percentage } => {
            serde_json::json!({
                "type": "database_updating",
                "player_name": source.player_name(),
                "player_id": source.player_id(),
                "artist": artist,
                "album": album,
                "song": song,
                "percentage": percentage
            })
        },
        PlayerEvent::QueueChanged { source } => {
            serde_json::json!({
                "type": "queue_changed",
                "player_name": source.player_name(),
                "player_id": source.player_id()
            })
        },
        PlayerEvent::SongInformationUpdate { source , song} => {
            // As above: no prefix, no clone.
            let rewritten;
            let song = if forwarded_prefix.is_some() {
                let mut owned = song.clone();
                crate::api::urlprefix::rewrite_song_urls(&mut owned, forwarded_prefix);
                rewritten = owned;
                &rewritten
            } else {
                song
            };
            serde_json::json!({
                "type": "song_information_update",
                "player_name": source.player_name(),
                "player_id": source.player_id(),
                "song": song
            })
        },
        PlayerEvent::ActivePlayerChanged { source, player_id } => {
            serde_json::json!({
                "type": "active_player_changed",
                "player_name": source.player_name(),
                "player_id": source.player_id(),
                "new_player_id": player_id
            })
        },
        PlayerEvent::VolumeChanged { control_name, display_name, percentage, decibels, raw_value } => {
            serde_json::json!({
                "type": "volume_changed",
                "control_name": control_name,
                "display_name": display_name,
                "percentage": percentage,
                "decibels": decibels,
                "raw_value": raw_value
            })
        },
    };
    
    WebSocketMessage {
        event_data,
        source,
    }
}

/// Get event type name as a string
fn event_type_name(event: &PlayerEvent) -> &'static str {
    match event {
        PlayerEvent::StateChanged { .. } => "state_changed",
        PlayerEvent::SongChanged { .. } => "song_changed",
        PlayerEvent::LoopModeChanged { .. } => "loop_mode_changed",
        PlayerEvent::RandomChanged { .. } => "shuffle_changed",
        PlayerEvent::CapabilitiesChanged { .. } => "capabilities_changed",
        PlayerEvent::PositionChanged { .. } => "position_changed",
        PlayerEvent::DatabaseUpdating { .. } => "database_updating",
        PlayerEvent::QueueChanged { .. } => "queue_changed",
        PlayerEvent::SongInformationUpdate { .. } => "song_information_update",
        PlayerEvent::ActivePlayerChanged { .. } => "active_player_changed",
        PlayerEvent::VolumeChanged { .. } => "volume_changed",
    }
}

/// How a connection's message loop ended.
#[derive(Debug, PartialEq, Eq)]
enum ClientLoopEnd {
    /// The peer closed, or the stream ended or errored: the caller unregisters
    /// the client.
    Closed,
    /// A send failed, so the connection is already gone. The registration is
    /// left for the prune task, which is what happened before the two loops
    /// were shared and is deliberately unchanged here.
    SendFailed,
}

/// The message loop shared by every WebSocket connection.
///
/// Both mount points run this. They differ only in the subscription they
/// register and the welcome message they send, and having one loop is what
/// keeps the ping below from depending on which URL a client connected to.
///
/// `player_filter` is the player this connection was opened for, for the log
/// only; `None` is the unfiltered mount point.
///
/// Generic over the stream rather than taking `DuplexStream`, whose constructor
/// is private to `rocket_ws`, so the tests can run this loop over a pair of
/// channels and watch what it sends.
async fn run_client_loop<S>(
    manager: &WebSocketManager,
    stream: &mut S,
    client_id: usize,
    forwarded_prefix: Option<&str>,
    player_filter: Option<&str>,
    ping_interval: Duration,
) -> rocket_ws::result::Result<ClientLoopEnd>
where
    S: Stream<Item = rocket_ws::result::Result<Message>>
        + Sink<Message, Error = rocket_ws::result::Error>
        + Unpin,
{
    let player = player_filter.unwrap_or("all");
    let mut poll = tokio::time::interval(EVENT_POLL_INTERVAL);

    // `interval` fires its first tick immediately; start the ping clock one
    // interval out so opening a connection does not ping it straight away.
    let mut ping = tokio::time::interval_at(
        tokio::time::Instant::now() + ping_interval,
        ping_interval,
    );

    loop {
        tokio::select! {
            _ = poll.tick() => {
                // Check for new events
                let events = manager.get_events_for_client(client_id);
                for event in events {
                    // Convert to new format with source at top level
                    let message = convert_to_websocket_message(&event, forwarded_prefix);

                    if let Ok(json) = serde_json::to_string(&message) {
                        debug!("Sending event: Client: {}, Player: {}, Type: {:?}, JSON length: {}",
                              client_id, event.player_name().unwrap_or("system"), event_type_name(&event), json.len());

                        if let Err(e) = stream.send(Message::Text(json)).await {
                            debug!("Error sending event to client {}: {}", client_id, e);
                            // Connection might be broken, exit the loop
                            return Ok(ClientLoopEnd::SendFailed);
                        } else {
                            debug!("Event sent successfully: Client: {}", client_id);
                        }
                    } else {
                        debug!("Event serialization failed: Client: {}", client_id);
                    }
                }
            }
            _ = ping.tick() => {
                // A protocol frame, not an application message: no client has
                // to know about it, and the peer's WebSocket stack answers it
                // without any application code. The pong that comes back is
                // what refreshes this client's last-activity time, which is the
                // only thing keeping a listen-only client off the prune list.
                if let Err(e) = stream.send(Message::Ping(Vec::new())).await {
                    debug!("Error sending ping to client {}: {}", client_id, e);
                    return Ok(ClientLoopEnd::SendFailed);
                }
                debug!("Ping sent: Client: {}", client_id);
            }
            Some(msg_result) = stream.next() => {
                match msg_result {
                    Ok(msg) => {
                        // Any frame from the peer is activity - a Pong
                        // answering our ping included. Recorded before the
                        // dispatch below so no frame kind can be forgotten.
                        manager.record_inbound_frame(client_id, &msg);

                        match msg {
                            Message::Text(text) => {
                                debug!("Received message: Client: {}, Player: {}, Text: {}", client_id, player, text);

                                // Try to parse as ClientMessage (EventSubscription)
                                match serde_json::from_str::<ClientMessage>(&text) {
                                    Ok(ClientMessage::Subscription(subscription)) => {
                                        debug!("Subscription update: Client: {}, Player: {}, Players: {:?}, Event types: {:?}",
                                              client_id, player, subscription.players, subscription.event_types);

                                        if manager.update_subscription(client_id, subscription) {
                                            let response = serde_json::json!({
                                                "type": "subscription_updated",
                                                "message": "Subscription updated successfully"
                                            }).to_string();
                                            if let Err(e) = stream.send(Message::Text(response)).await {
                                                debug!("Error sending subscription update confirmation to client {}: {}", client_id, e);
                                            }
                                        }
                                    },
                                    Err(e) => {
                                        // Send error back to client
                                        let error_msg = serde_json::json!({
                                            "type": "error",
                                            "message": format!("Invalid message format: {}. Expected EventSubscription.", e)
                                        }).to_string();
                                        if let Err(e_send) = stream.send(Message::Text(error_msg)).await {
                                            debug!("Error sending error message to client {}: {}", client_id, e_send);
                                        }
                                    }
                                }
                            },
                            Message::Ping(data) => {
                                debug!("Received ping: Client: {}, Data length: {}", client_id, data.len());
                                // Reply with a pong containing the same data
                                stream.send(Message::Pong(data)).await?;
                            },
                            Message::Close(_) => {
                                debug!("Received close: Client: {}", client_id);
                                // Client is closing the connection
                                break;
                            },
                            // Pong answers our own ping; the activity it
                            // represents is already recorded above.
                            _ => {}
                        }
                    },
                    Err(e) => {
                        debug!("WebSocket error: {}", e);
                        break;
                    }
                }
            }
            else => break,
        }
    }

    Ok(ClientLoopEnd::Closed)
}

/// Create a task to periodically prune inactive connections and old events
pub fn start_prune_task(ws_manager: Arc<WebSocketManager>) {
    // Create a thread for periodic pruning
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(PRUNE_INTERVAL);

            // Drop clients silent for longer than CLIENT_TIMEOUT and events
            // older than EVENT_TIMEOUT.
            ws_manager.prune_inactive_and_old(CLIENT_TIMEOUT, EVENT_TIMEOUT);
        }
    });
}

/// Drop implementation to clean up event bus subscription
impl Drop for WebSocketManager {
    fn drop(&mut self) {
        let sub_guard = self.event_bus_subscription.lock();
        if let Some((id, _)) = &*sub_guard {
            EventBus::instance().unsubscribe(*id);
        }
    }
}

// WebSocketManager implements Clone via #[derive(Clone)] above
// since all fields are already Arc<Mutex<>>

// WebSocket handler for the event messages endpoint
#[rocket::get("/events")]
pub fn event_messages(
    ws: WebSocket,
    forwarded_prefix: crate::api::urlprefix::ForwardedPrefix,
    ws_manager: &rocket::State<Arc<WebSocketManager>>,
) -> Channel<'static> { // Removed audio_controller
    // Clone the manager to avoid lifetime issues
    let manager = ws_manager.inner().clone();
    // Captured once, for the life of the connection: the upgrade request is
    // the only place the header is available.
    let forwarded_prefix = forwarded_prefix.into_inner();

    // Create a WebSocket channel
    ws.channel(move |mut stream| {
        Box::pin(async move {
            // Register client with default subscription
            let client_id = manager.register(EventSubscription {
                players: None,
                event_types: None,
            });

            debug!("websocket connected: Client ID: {}, All players", client_id);

            // Send welcome message
            let welcome_msg = serde_json::json!({
                "type": "welcome",
                "client_id": client_id,
                "message": "Connected to ACR WebSocket API"
            }).to_string();

            if let Err(e) = stream.send(Message::Text(welcome_msg)).await {
                error!("Failed to send welcome message: {}", e);
                return Err(e);
            }

            let end = run_client_loop(
                &manager,
                &mut stream,
                client_id,
                forwarded_prefix.as_deref(),
                None,
                PING_INTERVAL,
            ).await?;

            if end == ClientLoopEnd::Closed {
                // Clean up when the connection is closed
                debug!("WebSocket disconnected: Client: {}", client_id);
                manager.remove_client(client_id);
            }

            Ok(())
        })
    })
}

// WebSocket handler for the player-specific event messages endpoint
#[rocket::get("/events/<player_name>")]
pub fn player_event_messages(
    ws: WebSocket,
    player_name: &str,
    forwarded_prefix: crate::api::urlprefix::ForwardedPrefix,
    ws_manager: &rocket::State<Arc<WebSocketManager>>,
) -> Channel<'static> { // Removed audio_controller
    // Clone the manager and player name to avoid lifetime issues
    let manager = ws_manager.inner().clone();
    let player_filter = player_name.to_string();
    // Captured once, for the life of the connection: the upgrade request is
    // the only place the header is available.
    let forwarded_prefix = forwarded_prefix.into_inner();

    // Create a WebSocket channel
    ws.channel(move |mut stream| {
        Box::pin(async move {
            // Register client with player-specific subscription
            let client_id = manager.register(EventSubscription {
                players: Some(vec![player_filter.clone()]),
                event_types: None,
            });

            debug!("WebSocket connected: Client ID: {}, Player: {}", client_id, player_filter);

            // Send welcome message
            let welcome_msg = serde_json::json!({
                "type": "welcome",
                "client_id": client_id,
                "message": format!("Connected to ACR WebSocket API for player '{}'", player_filter)
            }).to_string();

            if let Err(e) = stream.send(Message::Text(welcome_msg)).await {
                error!("Failed to send welcome message: {}", e);
                return Err(e);
            }

            // The same loop as the unfiltered mount point: the ping that keeps
            // a listen-only client alive must not depend on the URL it used.
            let end = run_client_loop(
                &manager,
                &mut stream,
                client_id,
                forwarded_prefix.as_deref(),
                Some(&player_filter),
                PING_INTERVAL,
            ).await?;

            if end == ClientLoopEnd::Closed {
                // Clean up when the connection is closed
                debug!("WebSocket disconnected: Client: {}", client_id);
                manager.remove_client(client_id);
            }

            Ok(())
        })
    })
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::player_event::PlayerSource;
    use crate::data::song::Song;

    fn test_song() -> Song {
        let mut song = Song::default();
        song.title = Some("Test".to_string());
        song.cover_art_url = Some("/api/library/mpd/image/album:7".to_string());
        song
    }

    fn source() -> PlayerSource {
        PlayerSource::new("mpd".to_string(), "mpd:6600".to_string())
    }

    // Note the two variants carry different types: SongChanged holds an
    // Option<Song>, SongInformationUpdate holds a Song.
    fn song_changed_event() -> PlayerEvent {
        PlayerEvent::SongChanged {
            source: source(),
            song: Some(test_song()),
        }
    }

    fn song_information_update_event() -> PlayerEvent {
        PlayerEvent::SongInformationUpdate {
            source: source(),
            song: test_song(),
        }
    }

    fn cover_art_of(message: &WebSocketMessage) -> Option<String> {
        message.event_data.get("song")?
            .get("cover_art_url")?
            .as_str()
            .map(ToOwned::to_owned)
    }

    #[test]
    fn a_song_event_gains_the_prefix() {
        let message = convert_to_websocket_message(&song_changed_event(), Some("/api/audiocontrol"));
        assert_eq!(
            cover_art_of(&message).as_deref(),
            Some("/api/audiocontrol/library/mpd/image/album:7")
        );
    }

    #[test]
    fn a_song_event_without_a_prefix_is_unchanged() {
        let message = convert_to_websocket_message(&song_changed_event(), None);
        assert_eq!(
            cover_art_of(&message).as_deref(),
            Some("/api/library/mpd/image/album:7")
        );
    }

    #[test]
    fn a_song_information_update_gains_the_prefix_too() {
        let message =
            convert_to_websocket_message(&song_information_update_event(), Some("/api/audiocontrol"));
        assert_eq!(
            cover_art_of(&message).as_deref(),
            Some("/api/audiocontrol/library/mpd/image/album:7")
        );
    }

    #[test]
    fn an_event_carrying_no_song_is_handled() {
        let event = PlayerEvent::SongChanged { source: source(), song: None };
        let message = convert_to_websocket_message(&event, Some("/api/audiocontrol"));
        assert_eq!(message.event_data.get("song"), Some(&serde_json::Value::Null));
    }

    // The tests below drive the clock rather than waiting on it: activity is
    // recorded at an instant we choose and the prune is asked what it would do
    // at another. Nothing here sleeps, so nothing here can race a real timer.

    fn manager_with_a_client() -> (WebSocketManager, usize) {
        let manager = WebSocketManager::new();
        let id = manager.register(EventSubscription { players: None, event_types: None });
        (manager, id)
    }

    fn pong() -> Message {
        Message::Pong(Vec::new())
    }

    #[test]
    fn a_pong_counts_as_activity() {
        let (manager, id) = manager_with_a_client();
        let answered_at = Instant::now() + Duration::from_secs(60);

        manager.record_inbound_frame_at(id, &pong(), answered_at);

        assert_eq!(
            manager.last_activity_of(id),
            Some(answered_at),
            "a pong did not refresh the client's last-activity time"
        );
    }

    /// The bug this fixes: a client that sends nothing of its own but answers
    /// the server's pings must survive indefinitely.
    ///
    /// Simulated time runs second by second across three timeout windows.
    /// Pongs arrive on `PING_INTERVAL` and the prune runs on `PRUNE_INTERVAL`,
    /// the two independent of each other exactly as they are in the daemon - so
    /// this fails if the ping interval is ever set near or beyond the timeout,
    /// and it fails if a pong stops counting as activity.
    #[test]
    fn a_client_that_answers_pings_is_never_pruned() {
        let (manager, id) = manager_with_a_client();
        let start = Instant::now();
        manager.record_activity_at(id, start);

        let horizon = CLIENT_TIMEOUT * 3;
        let mut elapsed = Duration::ZERO;
        while elapsed < horizon {
            elapsed += Duration::from_secs(1);
            let now = start + elapsed;

            if elapsed.as_secs() % PING_INTERVAL.as_secs() == 0 {
                // The pong answering the ping the loop just sent.
                manager.record_inbound_frame_at(id, &pong(), now);
            }

            if elapsed.as_secs() % PRUNE_INTERVAL.as_secs() == 0 {
                manager.prune_inactive_and_old_at(now, CLIENT_TIMEOUT, EVENT_TIMEOUT);
                assert!(
                    manager.is_registered(id),
                    "a client answering every ping was pruned {:?} after connecting",
                    elapsed
                );
            }
        }
    }

    /// The property the fix could plausibly have broken: a peer that has gone
    /// away without a FIN answers no pings, and must still be reaped. Its
    /// registration survives up to the timeout and is gone once past it.
    #[test]
    fn a_client_that_stops_answering_is_still_pruned() {
        let (manager, id) = manager_with_a_client();
        let start = Instant::now();
        manager.record_activity_at(id, start);

        // Three pongs, then the peer vanishes.
        let mut last_pong = start;
        for _ in 0..3 {
            last_pong += PING_INTERVAL;
            manager.record_inbound_frame_at(id, &pong(), last_pong);
        }

        manager.prune_inactive_and_old_at(last_pong + CLIENT_TIMEOUT, CLIENT_TIMEOUT, EVENT_TIMEOUT);
        assert!(
            manager.is_registered(id),
            "a client was reaped before the timeout had elapsed"
        );

        manager.prune_inactive_and_old_at(
            last_pong + CLIENT_TIMEOUT + Duration::from_secs(1),
            CLIENT_TIMEOUT,
            EVENT_TIMEOUT,
        );
        assert!(
            !manager.is_registered(id),
            "a peer that stopped answering pings was not reaped"
        );
        assert_eq!(
            manager.last_activity_of(id),
            None,
            "the reaped client left its activity entry behind"
        );
    }

    // The loop itself, run over a pair of channels. `DuplexStream` cannot be
    // built outside `rocket_ws`, which is why `run_client_loop` is generic over
    // its stream: everything below drives the real loop, on paused time, so the
    // ping it sends is observed rather than assumed.

    use rocket::futures::{Sink, Stream};
    use std::pin::Pin;
    use std::task::{Context, Poll};
    use tokio::sync::mpsc;

    /// A stand-in for a connection: frames the test writes arrive at the loop,
    /// frames the loop sends are collected for the test.
    struct ChannelStream {
        inbound: mpsc::UnboundedReceiver<rocket_ws::result::Result<Message>>,
        outbound: mpsc::UnboundedSender<Message>,
    }

    impl Stream for ChannelStream {
        type Item = rocket_ws::result::Result<Message>;

        fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
            self.inbound.poll_recv(cx)
        }
    }

    impl Sink<Message> for ChannelStream {
        type Error = rocket_ws::result::Error;

        fn poll_ready(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }

        fn start_send(self: Pin<&mut Self>, item: Message) -> Result<(), Self::Error> {
            self.outbound
                .send(item)
                .map_err(|_| rocket_ws::result::Error::ConnectionClosed)
        }

        fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }

        fn poll_close(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }
    }

    struct LoopHarness {
        manager: WebSocketManager,
        client_id: usize,
        to_server: mpsc::UnboundedSender<rocket_ws::result::Result<Message>>,
        from_server: mpsc::UnboundedReceiver<Message>,
        task: tokio::task::JoinHandle<rocket_ws::result::Result<ClientLoopEnd>>,
    }

    /// Register a client and run the real loop for it on this runtime.
    fn start_loop() -> LoopHarness {
        let manager = WebSocketManager::new();
        let client_id = manager.register(EventSubscription { players: None, event_types: None });

        let (to_server, inbound) = mpsc::unbounded_channel();
        let (outbound, from_server) = mpsc::unbounded_channel();

        let in_loop = manager.clone();
        let task = tokio::spawn(async move {
            let mut stream = ChannelStream { inbound, outbound };
            run_client_loop(&in_loop, &mut stream, client_id, None, None, PING_INTERVAL).await
        });

        LoopHarness { manager, client_id, to_server, from_server, task }
    }

    impl LoopHarness {
        /// The next frame the loop sends, or `None` if it sends nothing within
        /// twenty ping intervals.
        ///
        /// Twenty intervals of *paused* time: the runtime advances its own clock
        /// when it has nothing to run, so this costs no real time and cannot
        /// flake under load.
        async fn next_frame(&mut self) -> Option<Message> {
            tokio::time::timeout(PING_INTERVAL * 20, self.from_server.recv())
                .await
                .ok()
                .flatten()
        }

        /// Close the connection and wait for the loop to say how it ended.
        async fn finish(self) -> ClientLoopEnd {
            let _ = self.to_server.send(Ok(Message::Close(None)));
            tokio::time::timeout(PING_INTERVAL * 20, self.task)
                .await
                .expect("the loop did not return after a close frame")
                .expect("the loop task panicked")
                .expect("the loop returned an error")
        }
    }

    /// Poll a condition, yielding between attempts. Bounded, and on paused time.
    async fn eventually(mut condition: impl FnMut() -> bool) -> bool {
        for _ in 0..100 {
            if condition() {
                return true;
            }
            tokio::task::yield_now().await;
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        false
    }

    /// The fix itself: a connection nobody talks on is pinged anyway, and goes
    /// on being pinged. Delete the `ping.tick()` branch and this fails.
    #[tokio::test(start_paused = true)]
    async fn the_server_pings_an_idle_connection() {
        let mut harness = start_loop();

        let first = harness.next_frame().await;
        assert!(
            matches!(first, Some(Message::Ping(_))),
            "an idle connection was not pinged; got {:?}",
            first
        );

        let second = harness.next_frame().await;
        assert!(
            matches!(second, Some(Message::Ping(_))),
            "the ping did not repeat; got {:?}",
            second
        );

        assert_eq!(harness.finish().await, ClientLoopEnd::Closed);
    }

    /// End to end, and the reason the ping is the fix: the pong a peer's stack
    /// sends back without any application code reaches the manager as activity.
    #[tokio::test(start_paused = true)]
    async fn a_pong_answering_the_ping_refreshes_activity() {
        let mut harness = start_loop();
        let before = harness
            .manager
            .last_activity_of(harness.client_id)
            .expect("a registered client has an activity time");

        let ping = harness.next_frame().await;
        assert!(
            matches!(ping, Some(Message::Ping(_))),
            "expected a ping to answer; got {:?}",
            ping
        );

        harness
            .to_server
            .send(Ok(Message::Pong(Vec::new())))
            .expect("the loop is still reading");

        let manager = harness.manager.clone();
        let client_id = harness.client_id;
        assert!(
            eventually(|| manager.last_activity_of(client_id).is_some_and(|at| at > before)).await,
            "the pong answering the server's ping did not refresh the client's activity time"
        );

        assert_eq!(harness.finish().await, ClientLoopEnd::Closed);
    }

    /// A client that goes away properly is reported as closed, so the caller
    /// unregisters it rather than leaving it to the prune.
    #[tokio::test(start_paused = true)]
    async fn a_close_frame_ends_the_loop() {
        assert_eq!(start_loop().finish().await, ClientLoopEnd::Closed);
    }

    /// Pinging often enough to keep a client alive is worthless if one lost
    /// pong strands it, so state the margin as a test: a run of consecutive
    /// pongs may go missing and the client still lives.
    #[test]
    fn several_missed_pongs_do_not_prune_a_client() {
        let (manager, id) = manager_with_a_client();
        let start = Instant::now();
        manager.record_activity_at(id, start);

        // Ten pings in a row unanswered.
        let ten_missed = start + PING_INTERVAL * 10;
        manager.prune_inactive_and_old_at(ten_missed, CLIENT_TIMEOUT, EVENT_TIMEOUT);

        assert!(
            manager.is_registered(id),
            "ten missed pongs were enough to prune a client; the margin is too thin"
        );
    }
}
