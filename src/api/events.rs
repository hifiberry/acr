use std::sync::Arc;
use parking_lot::Mutex;
use std::collections::{HashMap, HashSet, VecDeque};
use std::time::{Duration, Instant};
use serde::{Serialize, Deserialize};
use log::{debug, info, error};

// Use the correct rocket_ws imports
use rocket_ws::{WebSocket, Channel, Message};
use rocket::futures::{SinkExt, StreamExt};

use crate::data::PlayerEvent;
use crate::audiocontrol::eventbus::EventBus;

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
        self.last_activity.lock().insert(id, Instant::now());
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
            
            if !event_types.contains(event_type) {
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
        let now = Instant::now();
        
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
}

/// Convert PlayerEvent to WebSocketMessage format with source at top level
fn convert_to_websocket_message(event: &PlayerEvent) -> WebSocketMessage {
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
                "type": "random_changed",
                "player_name": source.player_name(),
                "player_id": source.player_id(),
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
        PlayerEvent::RandomChanged { .. } => "random_changed",
        PlayerEvent::CapabilitiesChanged { .. } => "capabilities_changed",
        PlayerEvent::PositionChanged { .. } => "position_changed",
        PlayerEvent::DatabaseUpdating { .. } => "database_updating",
        PlayerEvent::QueueChanged { .. } => "queue_changed",
        PlayerEvent::SongInformationUpdate { .. } => "song_information_update",
        PlayerEvent::ActivePlayerChanged { .. } => "active_player_changed",
        PlayerEvent::VolumeChanged { .. } => "volume_changed",
    }
}

/// Create a task to periodically prune inactive connections and old events
pub fn start_prune_task(ws_manager: Arc<WebSocketManager>) {
    // Create a thread for periodic pruning
    std::thread::spawn(move || {
        loop {
            // Sleep for 5 minutes
            std::thread::sleep(Duration::from_secs(300));
            
            // Prune connections inactive for more than 1 hour and
            // events older than 30 seconds
            ws_manager.prune_inactive_and_old(
                Duration::from_secs(3600), // 1 hour
                Duration::from_secs(30)  // 30 seconds
            );
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
pub fn event_messages(ws: WebSocket, ws_manager: &rocket::State<Arc<WebSocketManager>>) -> Channel<'static> { // Removed audio_controller
    // Clone the manager to avoid lifetime issues
    let manager = ws_manager.inner().clone();

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
            
            // Create a polling interval
            let mut interval = tokio::time::interval(tokio::time::Duration::from_millis(500));
            
            loop {
                tokio::select! {
                    _ = interval.tick() => {
                        // Check for new events
                        let events = manager.get_events_for_client(client_id);
                        for event in events {
                            // Convert to new format with source at top level
                            let message = convert_to_websocket_message(&event);
                            
                            if let Ok(json) = serde_json::to_string(&message) {
                                debug!("sending event: Client: {}, Player: {}, Type: {:?}, JSON length: {}", 
                                      client_id, event.player_name().unwrap_or("system"), event_type_name(&event), json.len());
                                
                                if let Err(e) = stream.send(Message::Text(json)).await {
                                    debug!("Error sending event to client {}: {}", client_id, e);
                                    // Connection might be broken, exit the loop
                                    return Ok(());
                                } else {
                                    debug!("Event sent successfully: Client: {}", client_id);
                                }
                            } else {
                                debug!("Event serialization failed: Client: {}", client_id);
                            }
                        }
                    }
                    Some(msg_result) = stream.next() => {
                        match msg_result {
                            Ok(msg) => {
                                // Record activity to prevent timeout
                                manager.record_activity(client_id);
                                
                                match msg {
                                    Message::Text(text) => {
                                        // Record activity to prevent timeout
                                        manager.record_activity(client_id);
                                        debug!("Received message: Client: {}, Text: {}", client_id, text);

                                        // Try to parse as ClientMessage (EventSubscription)
                                        match serde_json::from_str::<ClientMessage>(&text) {
                                            Ok(ClientMessage::Subscription(subscription)) => {
                                                debug!("Subscription update: Client: {}, Players: {:?}, Event types: {:?}",
                                                      client_id, subscription.players, subscription.event_types);

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
                                    _ => {} // Ignore other message types
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
            
            // Clean up when the connection is closed
            debug!("WebSocket disconnected: Client: {}", client_id);
            manager.remove_client(client_id);
            Ok(())
        })
    })
}

// WebSocket handler for the player-specific event messages endpoint
#[rocket::get("/events/<player_name>")]
pub fn player_event_messages(ws: WebSocket, player_name: &str, ws_manager: &rocket::State<Arc<WebSocketManager>>) -> Channel<'static> { // Removed audio_controller
    // Clone the manager and player name to avoid lifetime issues
    let manager = ws_manager.inner().clone();
    let player_filter = player_name.to_string();
    
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
            
            // Create a polling interval
            let mut interval = tokio::time::interval(tokio::time::Duration::from_millis(500));
            
            loop {
                tokio::select! {
                    _ = interval.tick() => {
                        // Check for new events
                        let events = manager.get_events_for_client(client_id);
                        for event in events {
                            // Convert to new format with source at top level
                            let message = convert_to_websocket_message(&event);
                            
                            if let Ok(json) = serde_json::to_string(&message) {
                                debug!("Sending event: Client: {}, Player: {}, Type: {:?}, JSON length: {}", 
                                      client_id, event.player_name().unwrap_or("system"), event_type_name(&event), json.len());
                                
                                if let Err(e) = stream.send(Message::Text(json)).await {
                                    debug!("Error sending event to client {}: {}", client_id, e);
                                    // Connection might be broken, exit the loop
                                    return Ok(());
                                } else {
                                    debug!("Event sent successfully: Client: {}", client_id);
                                }
                            } else {
                                debug!("Event serialization failed: Client: {}", client_id);
                            }
                        }
                    }
                    Some(msg_result) = stream.next() => {
                        match msg_result {
                            Ok(msg) => {
                                // Record activity to prevent timeout
                                manager.record_activity(client_id);
                                
                                match msg {
                                    Message::Text(text) => {
                                        debug!("Received message: Client: {}, Player: {}, Text: {}", client_id, player_filter, text);
                                        
                                        // Try to parse as ClientMessage (EventSubscription only)
                                        match serde_json::from_str::<ClientMessage>(&text) {
                                            Ok(ClientMessage::Subscription(subscription)) => {
                                                debug!("Subscription update: Client: {}, Player: {}, Players: {:?}, Event types: {:?}", 
                                                      client_id, player_filter, subscription.players, subscription.event_types);
                                                
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
                                    _ => {} // Ignore other message types
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
            
            // Clean up when the connection is closed
            debug!("WebSocket disconnected: Client: {}", client_id);
            manager.remove_client(client_id);
            Ok(())
        })
    })
}