use std::sync::{Arc, Weak};
use std::any::Any;
use crate::data::PlayerEvent;
use crate::plugins::plugin::Plugin;
use crate::audiocontrol::AudioController;

/// A plugin that can respond to events from an AudioController
/// and take actions based on those events, potentially controlling
/// the AudioController itself.
pub trait ActionPlugin: Plugin {
    /// Initialize the plugin with a reference to the AudioController
    /// This allows the plugin to interact with the AudioController
    fn initialize(&mut self, controller: Weak<AudioController>);
    
    /// Start the plugin functionality
    /// This is called after initialization and should set up any event listeners or workers
    fn start(&mut self) -> bool;
    
    /// Stop the plugin functionality
    /// This is called before shutdown and should clean up any event listeners or workers
    fn stop(&mut self) -> bool;
}

/// Base implementation for ActionPlugin
pub struct BaseActionPlugin {
    /// Name of the plugin
    name: String,
    
    /// Version of the plugin
    version: String,
    
    /// Weak reference to the AudioController
    controller: Option<Weak<AudioController>>,
}

impl BaseActionPlugin {
    /// Create a new BaseActionPlugin with the given name
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            controller: None,
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
        // Default implementation does nothing
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
    
    fn start(&mut self) -> bool {
        log::debug!("BaseActionPlugin '{}' started", self.name);
        true
    }
    
    fn stop(&mut self) -> bool {
        log::debug!("BaseActionPlugin '{}' stopped", self.name);
        true
    }
}