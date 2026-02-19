use std::sync::{Arc, Weak};
use std::any::Any;
use crate::data::{PlayerEvent, PlaybackState};
use crate::plugins::plugin::Plugin;
use crate::plugins::action_plugin::{ActionPlugin, BaseActionPlugin};
use crate::audiocontrol::AudioController;
use log::{debug, info, warn, trace};
use delegate::delegate;

/// A plugin that monitors player state changes and sets the active player
/// to any player that enters the Playing state.
pub struct ActiveMonitor {
    /// Base implementation for common functionality
    base: BaseActionPlugin,
}

impl Default for ActiveMonitor {
    fn default() -> Self {
        Self::new()
    }
}

impl ActiveMonitor {
    /// Create a new ActiveMonitor plugin
    pub fn new() -> Self {
        Self {
            base: BaseActionPlugin::new("ActiveMonitor"),
        }
    }
    
    /// Try to find a player controller by name and ID and make it active
    fn set_active_player(&self, player_name: &str, player_id: &str) {
        if let Some(controller) = self.base.get_controller() {
            // First check if the given player is already active
            if let Some(active_controller) = controller.get_active_controller() {
                let active_player = active_controller.read();
                if active_player.get_player_name() == player_name &&
                   active_player.get_player_id() == player_id {
                    debug!("ActiveMonitor: Player {}:{} is already active, no change needed",
                           player_name, player_id);
                    return;
                }
            }

            // Find the controller with matching name and ID
            let controllers = controller.list_controllers();
            let mut target_index = None;

            // First find the matching player and store its index
            for (idx, player_controller) in controllers.iter().enumerate() {
                let player = player_controller.read();
                if player.get_player_name() == player_name && player.get_player_id() == player_id {
                    target_index = Some(idx);
                    break;
                }
            }

            // Now set the active controller after all locks have been released
            if let Some(idx) = target_index {
                info!("ActiveMonitor: Setting player {}:{} as active", player_name, player_id);
                if controller.set_active_controller(idx) {
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
        log::info!("ActiveMonitor initializing");
        self.base.init()
    }

    fn shutdown(&mut self) -> bool {
        log::info!("ActiveMonitor shutting down");
        self.base.shutdown()
    }
    
    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl ActionPlugin for ActiveMonitor {
    fn initialize(&mut self, controller: Weak<AudioController>) {
        self.base.set_controller(controller);
        
        // Subscribe to event bus in the initialize method
        log::debug!("ActiveMonitor initializing and subscribing to event bus");
        let self_clone = self.clone();
        self.base.subscribe_to_event_bus(move |event| {
            self_clone.handle_event(event);
        });
    }
    
    fn handle_event(&self, event: PlayerEvent) {
        // Handle events using the existing method
        self.handle_event_bus_events(event);
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
        }
    }
}