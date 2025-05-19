use std::sync::{Arc, Weak, Mutex};
use std::any::Any;
use crate::data::{PlayerEvent, PlaybackState};
use crate::plugins::plugin::Plugin;
use crate::plugins::action_plugin::{ActionPlugin, BaseActionPlugin};
use crate::audiocontrol::AudioController;
use crate::audiocontrol::eventbus::EventBus;
use log::{debug, info, warn, trace};
use delegate::delegate;

/// A plugin that monitors player state changes and sets the active player
/// to any player that enters the Playing state.
pub struct ActiveMonitor {
    /// Base implementation for common functionality
    base: BaseActionPlugin,
    
    /// Subscription to the global event bus
    event_bus_subscription: Arc<Mutex<Option<(u64, crossbeam::channel::Receiver<PlayerEvent>)>>>,
    
    /// Handle to the event listener thread
    event_listener_thread: Arc<Mutex<Option<std::thread::JoinHandle<()>>>>,
}

impl ActiveMonitor {
    /// Create a new ActiveMonitor plugin
    pub fn new() -> Self {
        Self {
            base: BaseActionPlugin::new("ActiveMonitor"),
            event_bus_subscription: Arc::new(Mutex::new(None)),
            event_listener_thread: Arc::new(Mutex::new(None)),
        }
    }
    
    /// Try to find a player controller by name and ID and make it active
    fn set_active_player(&self, player_name: &str, player_id: &str) {
        if let Some(controller) = self.base.get_controller() {
            // Get a mutable reference to the AudioController to set active player
            // This is safe because we're not modifying any shared state that would affect
            // concurrent reads from other threads
            let controller_ref = unsafe { &mut *(Arc::as_ptr(&controller) as *mut AudioController) };
            
            // First check if the given player is already active
            if let Some(active_controller) = controller_ref.get_active_controller() {
                if let Ok(active_player) = active_controller.read() {
                    if active_player.get_player_name() == player_name && 
                       active_player.get_player_id() == player_id {
                        debug!("ActiveMonitor: Player {}:{} is already active, no change needed", 
                               player_name, player_id);
                        return;
                    }
                }
            }
            
            // Find the controller with matching name and ID
            let controllers = controller_ref.list_controllers();
            let mut target_index = None;
            
            // First find the matching player and store its index
            for (idx, player_controller) in controllers.iter().enumerate() {
                if let Ok(player) = player_controller.read() {
                    if player.get_player_name() == player_name && player.get_player_id() == player_id {
                        target_index = Some(idx);
                        break;
                    }
                }
            }
            
            // Now set the active controller after all locks have been released
            if let Some(idx) = target_index {
                info!("ActiveMonitor: Setting player {}:{} as active", player_name, player_id);
                if controller_ref.set_active_controller(idx) {
                    info!("ActiveMonitor: Successfully set active player to {}:{}", 
                          player_name, player_id);
                } else {
                    warn!("ActiveMonitor: Failed to set active player");
                }
            } else {
                warn!("ActiveMonitor: Could not find player {}:{} to set active", player_name, player_id);
            }
        } else {
            warn!("ActiveMonitor: No valid AudioController reference available");
        }
    }
    
    /// Handle events coming from the event bus
    fn handle_event_bus_events(&self, event: PlayerEvent) {
        trace!("Received event from event bus");
        
        // We only care about state changed events
        if let PlayerEvent::StateChanged { source, state } = event {
            // If a player state changes to Playing, make it the active player
            if state == PlaybackState::Playing {
                debug!("ActiveMonitor: Detected player {}:{} state changed to Playing", 
                       source.player_name(), source.player_id());
                self.set_active_player(source.player_name(), source.player_id());
            }
        }
    }
}

impl Plugin for ActiveMonitor {
    delegate! {
        to self.base {
            fn name(&self) -> &str;
            fn version(&self) -> &str;
        }
    }
    
    fn init(&mut self) -> bool {
        log::info!("ActiveMonitor initializing with event bus subscription");
        
        // Set up subscription to the global event bus
        let event_bus = EventBus::instance();
        let (id, receiver) = event_bus.subscribe_all();
        
        // Store our subscription ID (we'll need it to unsubscribe later)
        if let Ok(mut sub) = self.event_bus_subscription.lock() {
            *sub = Some((id, receiver.clone()));
        }

        // Create a thread-safe reference to self for the worker thread
        let monitor = Arc::new(Mutex::new(self.clone()));
        
        // Start a thread to listen for events from the event bus
        let thread_handle = std::thread::spawn(move || {
            log::debug!("ActiveMonitor event bus listener thread started");
            
            // Process events until the channel is closed
            while let Ok(event) = receiver.recv() {
                // Get a lock on the monitor
                if let Ok(monitor_guard) = monitor.lock() {
                    // Handle the event
                    monitor_guard.handle_event_bus_events(event);
                }
            }
            
            log::debug!("ActiveMonitor event bus listener thread exiting");
        });

        // Store the thread handle
        if let Ok(mut handle) = self.event_listener_thread.lock() {
            *handle = Some(thread_handle);
        }
        
        self.base.init()
    }

    fn shutdown(&mut self) -> bool {
        log::info!("ActiveMonitor shutting down");
        
        // Unsubscribe from the event bus
        if let Ok(mut sub_guard) = self.event_bus_subscription.lock() {
            if let Some((id, _)) = sub_guard.take() {
                EventBus::instance().unsubscribe(id);
                log::debug!("ActiveMonitor unsubscribed from event bus");
            }
        }
        
        // Wait for the event listener thread to exit
        if let Ok(mut thread_guard) = self.event_listener_thread.lock() {
            if thread_guard.is_some() {
                // Just take the handle and drop it, which detaches the thread
                let _ = thread_guard.take();
                log::debug!("ActiveMonitor detaching event bus listener thread");
            }
        }
        
        self.base.shutdown()
    }
    
    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl ActionPlugin for ActiveMonitor {
    fn initialize(&mut self, controller: Weak<AudioController>) {
        self.base.set_controller(controller);
        debug!("ActiveMonitor initialized with AudioController reference");
    }
    
    fn start(&mut self) -> bool {
        debug!("ActiveMonitor starting");
        true
    }
    
    fn stop(&mut self) -> bool {
        debug!("ActiveMonitor stopping");
        true
    }
}

// Clone implementation for ActiveMonitor to allow for passing to thread
impl Clone for ActiveMonitor {
    fn clone(&self) -> Self {
        let mut new_base = BaseActionPlugin::new(self.base.name());
        
        // Get the controller reference from the original object
        if let Some(controller) = self.base.get_controller() {
            // The controller is already an Arc, we need to downgrade it to a Weak
            let controller_weak = Arc::downgrade(&controller);
            new_base.set_controller(controller_weak);
        }
        
        Self {
            base: new_base,
            event_bus_subscription: Arc::new(Mutex::new(None)),
            event_listener_thread: Arc::new(Mutex::new(None)),
        }
    }
}