use std::sync::{Arc, Weak};
use std::any::Any;
use crate::data::{PlayerEvent, PlaybackState};
use crate::plugins::plugin::Plugin;
use crate::plugins::action_plugin::{ActionPlugin, BaseActionPlugin};
use crate::audiocontrol::AudioController;
use log::{debug, info, warn};
use delegate::delegate;

/// A plugin that monitors player state changes and sets the active player
/// to any player that enters the Playing state.
pub struct ActiveMonitor {
    /// Base implementation for common functionality
    base: BaseActionPlugin,
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
            for (idx, player_controller) in controllers.iter().enumerate() {
                if let Ok(player) = player_controller.read() {
                    if player.get_player_name() == player_name && player.get_player_id() == player_id {
                        info!("ActiveMonitor: Setting player {}:{} as active", player_name, player_id);
                        if controller_ref.set_active_controller(idx) {
                            info!("ActiveMonitor: Successfully set active player to {}:{}", 
                                  player_name, player_id);
                        } else {
                            warn!("ActiveMonitor: Failed to set active player");
                        }
                        return;
                    }
                }
            }
            
            warn!("ActiveMonitor: Could not find player {}:{} to set active", player_name, player_id);
        } else {
            warn!("ActiveMonitor: No valid AudioController reference available");
        }
    }
}

impl Plugin for ActiveMonitor {
    delegate! {
        to self.base {
            fn name(&self) -> &str;
            fn version(&self) -> &str;
            fn init(&mut self) -> bool;
            fn shutdown(&mut self) -> bool;
        }
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
    
    fn on_event(&mut self, event: &PlayerEvent, _is_active_player: bool) {
        // We only care about state changed events
        // log events for debugging
        if let PlayerEvent::StateChanged { source, state } = event {
            // If a player state changes to Playing, make it the active player
            if *state == PlaybackState::Playing {
                debug!("ActiveMonitor: Detected player {}:{} state changed to Playing", 
                       source.player_name(), source.player_id());
                self.set_active_player(source.player_name(), source.player_id());
            }
        }
    }
}