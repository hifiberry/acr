use std::collections::HashMap;
use log::{info, error, warn};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json, Map};

use crate::plugins::plugin::Plugin;
use crate::plugins::event_filters::event_filter::{EventFilter};
use crate::plugins::event_filters::event_logger::EventLogger;

/// Factory for creating and registering plugins
pub struct PluginFactory {
    /// Registry of available plugin constructors by name
    registry: HashMap<String, Box<dyn Fn(Option<&Value>) -> Option<Box<dyn Plugin>>>>,
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
        // Register EventLogger that logs all events by default
        self.register("event-logger", |config| {
            let only_active = if let Some(config) = config {
                config.get("only_active")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
            } else {
                false
            };
            
            Some(Box::new(EventLogger::new(only_active)) as Box<dyn Plugin>)
        });
    }
    
    /// Register a new plugin constructor with JSON config support
    pub fn register<F>(&mut self, name: &str, constructor: F)
    where
        F: Fn(Option<&Value>) -> Option<Box<dyn Plugin>> + 'static,
    {
        if self.registry.contains_key(name) {
            warn!("Plugin with name '{}' already registered, overwriting", name);
        }
        
        self.registry.insert(name.to_string(), Box::new(constructor));
        info!("Registered plugin: {}", name);
    }
    
    /// Create a new instance of a plugin by name
    pub fn create(&self, name: &str) -> Option<Box<dyn Plugin>> {
        self.create_with_config(name, None)
    }
    
    /// Create a new instance of a plugin by name with configuration
    pub fn create_with_config(&self, name: &str, config: Option<&Value>) -> Option<Box<dyn Plugin>> {
        match self.registry.get(name) {
            Some(constructor) => {
                let plugin = constructor(config)?;
                info!("Created plugin: {} v{}", plugin.name(), plugin.version());
                Some(plugin)
            }
            None => {
                error!("Plugin '{}' not found in registry", name);
                None
            }
        }
    }
    
    /// Create a plugin instance from a JSON configuration string
    /// The JSON should have format: { "plugin-type": { params } }
    pub fn create_from_json(&self, json_config: &str) -> Option<Box<dyn Plugin>> {
        match serde_json::from_str::<Map<String, Value>>(json_config) {
            Ok(config_map) => {
                // We expect only one key (the plugin type)
                if config_map.len() != 1 {
                    error!("Invalid JSON config: expected a single plugin configuration");
                    return None;
                }
                
                // Get the first (and only) entry
                let (plugin_type, params) = config_map.iter().next().unwrap();
                
                info!("Creating plugin of type '{}' from JSON", plugin_type);
                self.create_with_config(plugin_type, Some(params))
            }
            Err(err) => {
                error!("Failed to parse plugin JSON configuration: {}", err);
                None
            }
        }
    }
    
    /// Create multiple plugins from a JSON array of configurations
    /// The JSON should have format: [ { "plugin-type-1": { params1 } }, { "plugin-type-2": { params2 } } ]
    pub fn create_plugins_from_json(&self, json_configs: &str) -> Vec<Box<dyn Plugin>> {
        match serde_json::from_str::<Vec<Map<String, Value>>>(json_configs) {
            Ok(configs) => {
                info!("Creating {} plugins from JSON array", configs.len());
                configs.iter()
                    .filter_map(|config_map| {
                        if config_map.len() != 1 {
                            error!("Invalid plugin config in array: expected a single plugin configuration");
                            return None;
                        }
                        
                        let (plugin_type, params) = config_map.iter().next().unwrap();
                        self.create_with_config(plugin_type, Some(params))
                    })
                    .collect()
            }
            Err(err) => {
                error!("Failed to parse plugins JSON configuration array: {}", err);
                Vec::new()
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
        self.create_event_filter_with_config(name, None)
    }
    
    /// Create a new instance of an EventFilter plugin by name with configuration
    pub fn create_event_filter_with_config(&self, name: &str, config: Option<&Value>) -> Option<Box<dyn EventFilter>> {
        let plugin = self.create_with_config(name, config)?;
        
        // Try to downcast the plugin to EventFilter
        if let Some(event_filter) = plugin.as_any().downcast_ref::<EventLogger>() {
            // For EventLogger, we need to create a new instance with the right configuration
            let only_active = if let Some(config) = config {
                config.get("only_active")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
            } else if name == "event-logger" {
                false
            } else {
                true
            };
            
            Some(Box::new(EventLogger::new(only_active)))
        } else {
            error!("Plugin '{}' is not a compatible EventFilter", name);
            None
        }
    }
    
    /// Create an event filter from a JSON configuration string
    pub fn create_event_filter_from_json(&self, json_config: &str) -> Option<Box<dyn EventFilter>> {
        match serde_json::from_str::<Map<String, Value>>(json_config) {
            Ok(config_map) => {
                // We expect only one key (the plugin type)
                if config_map.len() != 1 {
                    error!("Invalid JSON config: expected a single event filter configuration");
                    return None;
                }
                
                // Get the first (and only) entry
                let (plugin_type, params) = config_map.iter().next().unwrap();
                
                info!("Creating event filter of type '{}' from JSON", plugin_type);
                self.create_event_filter_with_config(plugin_type, Some(params))
            }
            Err(err) => {
                error!("Failed to parse event filter JSON configuration: {}", err);
                None
            }
        }
    }
}