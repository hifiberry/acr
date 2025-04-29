use std::collections::HashMap;
use log::{info, error, warn};

use crate::plugins::plugin::Plugin;
use crate::plugins::event_filter::{EventFilter, EventLogger};

/// Factory for creating and registering plugins
pub struct PluginFactory {
    /// Registry of available plugin constructors by name
    registry: HashMap<String, Box<dyn Fn() -> Box<dyn Plugin>>>,
}

impl PluginFactory {
    /// Create a new plugin factory
    pub fn new() -> Self {
        let mut factory = Self {
            registry: HashMap::new(),
        };
        
        // Register built-in plugins
        factory.register_builtin_plugins();
        
        factory
    }
    
    /// Register all built-in plugins
    fn register_builtin_plugins(&mut self) {
        // Register EventLogger that logs all events
        self.register("event-logger", || {
            Box::new(EventLogger::new(false)) as Box<dyn Plugin>
        });
        
        // Register EventLogger that only logs events from active player
        self.register("active-event-logger", || {
            Box::new(EventLogger::new(true)) as Box<dyn Plugin>
        });
    }
    
    /// Register a new plugin constructor
    pub fn register<F>(&mut self, name: &str, constructor: F)
    where
        F: Fn() -> Box<dyn Plugin> + 'static,
    {
        if self.registry.contains_key(name) {
            warn!("Plugin with name '{}' already registered, overwriting", name);
        }
        
        self.registry.insert(name.to_string(), Box::new(constructor));
        info!("Registered plugin: {}", name);
    }
    
    /// Create a new instance of a plugin by name
    pub fn create(&self, name: &str) -> Option<Box<dyn Plugin>> {
        match self.registry.get(name) {
            Some(constructor) => {
                let plugin = constructor();
                info!("Created plugin: {} v{}", plugin.name(), plugin.version());
                Some(plugin)
            }
            None => {
                error!("Plugin '{}' not found in registry", name);
                None
            }
        }
    }
    
    /// Get a list of all registered plugin names
    pub fn available_plugins(&self) -> Vec<String> {
        self.registry.keys().cloned().collect()
    }
    
    /// Check if a plugin with the given name is registered
    pub fn is_registered(&self, name: &str) -> bool {
        self.registry.contains_key(name)
    }
    
    /// Create a new instance of an EventFilter plugin by name
    pub fn create_event_filter(&self, name: &str) -> Option<Box<dyn EventFilter>> {
        let plugin = self.create(name)?;
        
        match plugin.as_any().downcast_ref::<dyn EventFilter>() {
            Some(_) => {
                // If the downcast succeeds, convert to EventFilter
                // We need to downcast again because we lost the original Box
                let event_filter = plugin;
                match event_filter.as_any().downcast_ref::<EventLogger>() {
                    Some(_) => Some(Box::new(EventLogger::new(
                        if name == "active-event-logger" { true } else { false }
                    ))),
                    None => None,
                }
            }
            None => {
                error!("Plugin '{}' is not an EventFilter", name);
                None
            }
        }
    }
}