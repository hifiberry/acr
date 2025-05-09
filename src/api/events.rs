use std::sync::{Arc, Mutex};
use std::collections::{HashMap, HashSet, VecDeque};
use std::any::Any;
use std::time::{Duration, Instant};
use serde::{Serialize, Deserialize};
use log::{debug, info, error};

// Use the correct rocket_ws imports
use rocket_ws::{WebSocket, Channel, Message};
use rocket::futures::{SinkExt, StreamExt};

use crate::data::PlayerEvent;
use crate::players::PlayerStateListener;

/// Subscription request from client
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventSubscription {
    /// Player names to subscribe to (empty for all players)
    pub players: Option<Vec<String>>,
    
    /// Event types to subscribe to (empty for all events)
    pub event_types: Option<Vec<String>>,
}

/// WebSocket client connection manager
pub struct WebSocketManager {
    /// Active subscriptions
    subscriptions: Mutex<HashMap<usize, ClientSubscription>>,
    
    /// Last activity timestamp for pruning stale connections
    last_activity: Mutex<HashMap<usize, Instant>>,
    
    /// Counter for generating unique IDs for clients
    next_id: Mutex<usize>,

    /// Recent events that need to be sent to clients
    recent_events: Mutex<VecDeque<(PlayerEvent, Instant)>>,
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

impl WebSocketManager {
    /// Create a new WebSocket manager
    pub fn new() -> Self {
        WebSocketManager {
            subscriptions: Mutex::new(HashMap::new()),
            last_activity: Mutex::new(HashMap::new()),
            next_id: Mutex::new(0),
            recent_events: Mutex::new(VecDeque::with_capacity(100)),
        }
    }
    
    /// Generate a new unique ID for a client
    fn next_id(&self) -> usize {
        let mut id = self.next_id.lock().unwrap();
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
        if let Ok(mut last_activity) = self.last_activity.lock() {
            last_activity.insert(id, now);
        }
        
        // Store the subscription
        if let Ok(mut subs) = self.subscriptions.lock() {
            subs.insert(id, client_sub);
            info!("WebSocket client registered (id: {}), total clients: {}", id, subs.len());
        } else {
            error!("Failed to acquire lock on WebSocket subscriptions");
        }
        
        id
    }
    
    /// Update a client's subscription
    pub fn update_subscription(&self, id: usize, subscription: EventSubscription) -> bool {
        // Update last activity timestamp
        if let Ok(mut last_activity) = self.last_activity.lock() {
            last_activity.insert(id, Instant::now());
        }
        
        // Update the subscription
        if let Ok(mut subs) = self.subscriptions.lock() {
            if let Some(sub) = subs.get_mut(&id) {
                sub.players = subscription.players.map(|p| p.into_iter().collect());
                sub.event_types = subscription.event_types.map(|e| e.into_iter().collect());
                debug!("Updated subscription for client {}", id);
                return true;
            }
        }
        
        false
    }
    
    /// Record client activity to prevent timeout
    pub fn record_activity(&self, id: usize) {
        if let Ok(mut last_activity) = self.last_activity.lock() {
            last_activity.insert(id, Instant::now());
        }
    }
    
    /// Queue a new event to be sent to clients
    pub fn queue_event(&self, event: PlayerEvent) {
        let now = Instant::now();
        
        // Add the event to the recent events queue
        if let Ok(mut events) = self.recent_events.lock() {
            // Add to the back of the queue to maintain chronological order
            events.push_back((event.clone(), now));
            
            // Limit the queue size to prevent memory issues
            if events.len() > 100 {
                events.pop_front();
            }
            
            debug!("Event queued: Player: {}, Type: {:?}, Queue size: {}", 
                  event.player_name(), event_type_name(&event), events.len());
        }
    }
    
    /// Get events for a specific client that have occurred since the client last checked
    pub fn get_events_for_client(&self, client_id: usize) -> Vec<PlayerEvent> {
        let mut matching_events = Vec::new();
        
        // Get the client's subscription
        let mut last_event_time = Instant::now();
        let subscription = {
            if let Ok(mut subs) = self.subscriptions.lock() {
                if let Some(sub) = subs.get_mut(&client_id) {
                    let sub_copy = sub.clone();
                    // Update the last event time
                    last_event_time = sub.last_event_time;
                    sub.last_event_time = Instant::now();
                    Some(sub_copy)
                } else {
                    None
                }
            } else {
                None
            }
        };
        
        if let Some(sub) = subscription {
            debug!("Checking events: Client: {}, Last check: {:?} ago", 
                  client_id, Instant::now().duration_since(last_event_time));
            
            // Get recent events that occurred after the client's last check
            if let Ok(events) = self.recent_events.lock() {
                debug!("Event queue size: {}", events.len());
                
                for (event, time) in events.iter() {
                    // Only check events that happened after the client's last check
                    if *time > last_event_time {
                        let should_send = self.should_send_to_client(event, &sub);
                        debug!("Event check: Player: {}, Type: {:?}, Time: {:?} ago, Should send: {}", 
                              event.player_name(), event_type_name(event), 
                              Instant::now().duration_since(*time), should_send);
                        
                        if should_send {
                            matching_events.push(event.clone());
                        }
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
            if !players.contains(event.player_name()) {
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
        if let Ok(mut subs) = self.subscriptions.lock() {
            if subs.remove(&id).is_some() {
                info!("WebSocket client disconnected (id: {}), remaining clients: {}", 
                    id, subs.len());
            }
        }
        
        // Clean up activity tracker
        if let Ok(mut last_activity) = self.last_activity.lock() {
            last_activity.remove(&id);
        }
    }
    
    /// Prune inactive connections and old events
    pub fn prune_inactive_and_old(&self, client_timeout: Duration, event_timeout: Duration) {
        let now = Instant::now();
        
        // Prune inactive clients
        let clients_to_remove = {
            let mut to_remove = Vec::new();
            if let Ok(last_activity) = self.last_activity.lock() {
                for (id, last) in last_activity.iter() {
                    if now.duration_since(*last) > client_timeout {
                        to_remove.push(*id);
                    }
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
        if let Ok(mut events) = self.recent_events.lock() {
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

/// Get event type name as a string
fn event_type_name(event: &PlayerEvent) -> &'static str {
    match event {
        PlayerEvent::StateChanged { .. } => "state_changed",
        PlayerEvent::SongChanged { .. } => "song_changed",
        PlayerEvent::LoopModeChanged { .. } => "loop_mode_changed",
        PlayerEvent::CapabilitiesChanged { .. } => "capabilities_changed",
        PlayerEvent::PositionChanged { .. } => "position_changed",
        PlayerEvent::DatabaseUpdating { .. } => "database_updating",
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

/// Implement PlayerStateListener for WebSocketManager
impl PlayerStateListener for WebSocketManager {
    // Main event handler - required by the trait
    fn on_event(&self, event: PlayerEvent) {
        // Log the event
        debug!("event received: Player: {}, Type: {:?}", 
              event.player_name(), event_type_name(&event));
        
        // Store the event for clients to retrieve
        self.queue_event(event);
    }
    
    // Required by the trait for dynamic casting
    fn as_any(&self) -> &dyn Any {
        self
    }
}

// WebSocket handler for the event messages endpoint
#[rocket::get("/events")]
pub fn event_messages(ws: WebSocket, ws_manager: &rocket::State<Arc<WebSocketManager>>) -> Channel<'static> {
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
                return Err(e.into());
            }
            
            // Create a polling interval
            let mut interval = tokio::time::interval(tokio::time::Duration::from_millis(500));
            
            loop {
                tokio::select! {
                    _ = interval.tick() => {
                        // Check for new events
                        let events = manager.get_events_for_client(client_id);
                        for event in events {
                            if let Ok(json) = serde_json::to_string(&event) {
                                debug!("sending event: Client: {}, Player: {}, Type: {:?}, JSON length: {}", 
                                      client_id, event.player_name(), event_type_name(&event), json.len());
                                
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
                                        debug!("Received message: Client: {}, Text: {}", client_id, text);
                                        
                                        // Try to parse as subscription update
                                        match serde_json::from_str::<EventSubscription>(&text) {
                                            Ok(subscription) => {
                                                debug!("Subscription update: Client: {}, Players: {:?}, Event types: {:?}", 
                                                      client_id, subscription.players, subscription.event_types);
                                                
                                                if manager.update_subscription(client_id, subscription) {
                                                    let response = serde_json::json!({
                                                        "type": "subscription_updated",
                                                        "message": "Subscription updated successfully"
                                                    }).to_string();
                                                    
                                                    stream.send(Message::Text(response)).await?;
                                                }
                                            },
                                            Err(e) => {
                                                // Send error back to client
                                                let error_msg = serde_json::json!({
                                                    "type": "error",
                                                    "message": format!("Invalid subscription format: {}", e)
                                                }).to_string();
                                                
                                                stream.send(Message::Text(error_msg)).await?;
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
pub fn player_event_messages(ws: WebSocket, player_name: &str, ws_manager: &rocket::State<Arc<WebSocketManager>>) -> Channel<'static> {
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
                return Err(e.into());
            }
            
            // Create a polling interval
            let mut interval = tokio::time::interval(tokio::time::Duration::from_millis(500));
            
            loop {
                tokio::select! {
                    _ = interval.tick() => {
                        // Check for new events
                        let events = manager.get_events_for_client(client_id);
                        for event in events {
                            if let Ok(json) = serde_json::to_string(&event) {
                                debug!("Sending event: Client: {}, Player: {}, Type: {:?}, JSON length: {}", 
                                      client_id, event.player_name(), event_type_name(&event), json.len());
                                
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
                                        debug!("Received message: Client: {}, Text: {}", client_id, text);
                                        
                                        // Try to parse as subscription update
                                        match serde_json::from_str::<EventSubscription>(&text) {
                                            Ok(subscription) => {
                                                debug!("Subscription update: Client: {}, Players: {:?}, Event types: {:?}", 
                                                      client_id, subscription.players, subscription.event_types);
                                                
                                                if manager.update_subscription(client_id, subscription) {
                                                    let response = serde_json::json!({
                                                        "type": "subscription_updated",
                                                        "message": "Subscription updated successfully"
                                                    }).to_string();
                                                    
                                                    stream.send(Message::Text(response)).await?;
                                                }
                                            },
                                            Err(e) => {
                                                // Send error back to client
                                                let error_msg = serde_json::json!({
                                                    "type": "error",
                                                    "message": format!("Invalid subscription format: {}", e)
                                                }).to_string();
                                                
                                                stream.send(Message::Text(error_msg)).await?;
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