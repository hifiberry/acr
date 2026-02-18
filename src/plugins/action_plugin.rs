use std::sync::{Arc, Weak};
use parking_lot::Mutex;
use std::any::Any;
use crate::data::PlayerEvent;
use crate::plugins::plugin::Plugin;
use crate::audiocontrol::AudioController;
use crate::audiocontrol::eventbus::EventBus;
use log;

/// A plugin that can respond to events from an AudioController
/// and take actions based on those events, potentially controlling
/// the AudioController itself.
pub trait ActionPlugin: Plugin {
    /// Initialize the plugin with a reference to the AudioController
    /// This allows the plugin to interact with the AudioController
    fn initialize(&mut self, controller: Weak<AudioController>);
    
    /// Handle an event received from the event bus
    /// This is called when an event is received from the global event bus
    /// Default implementation does nothing
    fn handle_event(&self, _event: PlayerEvent) {}
}

/// Base implementation for ActionPlugin
pub struct BaseActionPlugin {
    /// Name of the plugin
    name: String,
    
    /// Version of the plugin
    version: String,
    
    /// Weak reference to the AudioController
    controller: Option<Weak<AudioController>>,
    
    /// Subscription to the global event bus
    event_bus_subscription: Arc<Mutex<Option<(u64, crossbeam::channel::Receiver<PlayerEvent>)>>>,
    
    /// Handle to the event listener thread
    event_listener_thread: Arc<Mutex<Option<std::thread::JoinHandle<()>>>>,
}

impl BaseActionPlugin {
    /// Create a new BaseActionPlugin with the given name
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            controller: None,
            event_bus_subscription: Arc::new(Mutex::new(None)),
            event_listener_thread: Arc::new(Mutex::new(None)),
        }
    }
    
    /// Get a reference to the controller if it's still valid
    pub fn get_controller(&self) -> Option<Arc<AudioController>> {
        self.controller.as_ref()?.upgrade()
    }
    
    /// Set the controller reference
    pub fn set_controller(&mut self, controller: Weak<AudioController>) {
        self.controller = Some(controller);
    }
    
    /// Subscribe to the event bus and start a listener thread
    pub fn subscribe_to_event_bus<F>(&self, event_handler: F) 
    where
        F: Fn(PlayerEvent) + Send + 'static,
    {
        log::debug!("Subscribing to event bus for plugin '{}'", self.name);
        
        // Set up subscription to the global event bus
        let event_bus = EventBus::instance();
        let (id, receiver) = event_bus.subscribe_all();
        
        // Store our subscription ID (we'll need it to unsubscribe later)
        *self.event_bus_subscription.lock() = Some((id, receiver.clone()));
        
        // Start a thread to listen for events from the event bus
        let thread_handle = std::thread::spawn(move || {
            log::debug!("Event bus listener thread started");
            
            // Process events until the channel is closed
            while let Ok(event) = receiver.recv() {
                // Handle the event using the provided handler
                event_handler(event);
            }
            
            log::debug!("Event bus listener thread exiting");
        });
        
        // Store the thread handle
        *self.event_listener_thread.lock() = Some(thread_handle);
    }
    
    /// Unsubscribe from the event bus and clean up the listener thread
    pub fn unsubscribe_from_event_bus(&self) {
        log::debug!("Unsubscribing from event bus for plugin '{}'", self.name);
        
        // Unsubscribe from the event bus
        if let Some((id, _)) = self.event_bus_subscription.lock().take() {
            EventBus::instance().unsubscribe(id);
            log::debug!("Unsubscribed from event bus");
        }
        
        // Wait for the event listener thread to exit
        let mut thread_guard = self.event_listener_thread.lock();
        if thread_guard.is_some() {
            // Just take the handle and drop it, which detaches the thread
            let _ = thread_guard.take();
            log::debug!("Detaching event bus listener thread");
        }
    }
}

impl Plugin for BaseActionPlugin {
    fn name(&self) -> &str {
        &self.name
    }
    
    fn version(&self) -> &str {
        &self.version
    }
    
    fn init(&mut self) -> bool {
        log::info!("Plugin '{}' initialized", self.name);
        true
    }
    
    fn shutdown(&mut self) -> bool {
        // Unsubscribe from the event bus if necessary
        self.unsubscribe_from_event_bus();
        log::info!("Plugin '{}' shut down", self.name);
        true
    }
    
    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl ActionPlugin for BaseActionPlugin {
    fn initialize(&mut self, controller: Weak<AudioController>) {
        self.controller = Some(controller);
        log::debug!("BaseActionPlugin '{}' initialized with controller", self.name);
    }
    
    fn handle_event(&self, _event: PlayerEvent) {
        // Default implementation does nothing
    }
}