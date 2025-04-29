use std::collections::HashMap;
use std::collections::HashSet;
use log::{info, error, warn};
use serde_json::{Value, Map};

use crate::plugins::plugin::Plugin;
use crate::plugins::event_filters::event_filter::{EventFilter};
use crate::plugins::event_filters::event_logger::{EventLogger, LogLevel};
use crate::plugins::action_plugin::ActionPlugin;
use crate::plugins::action_plugins::ActiveMonitor;

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
            if let Some(config) = config {
                let only_active = config.get("only_active")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                
                // Get log level from config
                let log_level = config.get("log_level")
                    .and_then(Value::as_str)
                    .map(LogLevel::from)
                    .unwrap_or_default();
                
                // Get event types to log if specified
                let event_types = config.get("event_types")
                    .and_then(|v| {
                        if v.is_array() {
                            let mut types = HashSet::new();
                            if let Some(arr) = v.as_array() {
                                for item in arr {
                                    if let Some(s) = item.as_str() {
                                        types.insert(s.to_string());
                                    }
                                }
                            }
                            Some(types)
                        } else {
                            None
                        }
                    });
                
                Some(Box::new(EventLogger::with_config(only_active, log_level, event_types)) as Box<dyn Plugin>)
            } else {
                Some(Box::new(EventLogger::new(false)) as Box<dyn Plugin>)
            }
        });
        
        // Register ActiveMonitor that automatically sets active player on play events
        self.register("active-monitor", |_config| {
            Some(Box::new(ActiveMonitor::new()) as Box<dyn Plugin>)
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
    pub fn create_event_filter(&self, name: &str) -> Option<Box<dyn EventFilter + Send + Sync>> {
        self.create_event_filter_with_config(name, None)
    }
    
    /// Create a new instance of an EventFilter plugin by name with configuration
    pub fn create_event_filter_with_config(&self, name: &str, config: Option<&Value>) -> Option<Box<dyn EventFilter + Send + Sync>> {
        let plugin = self.create_with_config(name, config)?;
        
        // Try to downcast the plugin to EventFilter
        if plugin.as_any().downcast_ref::<EventLogger>().is_some() {
            // For EventLogger, we need to create a new instance with the right configuration
            if let Some(config) = config {
                let only_active = config.get("only_active")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                
                // Get log level from config
                let log_level = config.get("log_level")
                    .and_then(Value::as_str)
                    .map(LogLevel::from)
                    .unwrap_or_default();
                
                // Get event types to log if specified
                let event_types = config.get("event_types")
                    .and_then(|v| {
                        if v.is_array() {
                            let mut types = HashSet::new();
                            if let Some(arr) = v.as_array() {
                                for item in arr {
                                    if let Some(s) = item.as_str() {
                                        types.insert(s.to_string());
                                    }
                                }
                            }
                            Some(types)
                        } else {
                            None
                        }
                    });
                
                Some(Box::new(EventLogger::with_config(only_active, log_level, event_types)) as Box<dyn EventFilter + Send + Sync>)
            } else {
                // Use default values
                Some(Box::new(EventLogger::new(false)) as Box<dyn EventFilter + Send + Sync>)
            }
        } else {
            error!("Plugin '{}' is not a compatible EventFilter", name);
            None
        }
    }
    
    /// Create an event filter from a JSON configuration string
    pub fn create_event_filter_from_json(&self, json_config: &str) -> Option<Box<dyn EventFilter + Send + Sync>> {
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
    
    /// Create a new instance of an ActionPlugin by name
    pub fn create_action_plugin(&self, name: &str) -> Option<Box<dyn ActionPlugin + Send + Sync>> {
        self.create_action_plugin_with_config(name, None)
    }
    
    /// Create a new instance of an ActionPlugin by name with configuration
    pub fn create_action_plugin_with_config(&self, name: &str, config: Option<&Value>) -> Option<Box<dyn ActionPlugin + Send + Sync>> {
        let plugin = self.create_with_config(name, config)?;
        
        // Try to downcast the plugin to the specific ActionPlugin type
        if plugin.as_any().downcast_ref::<ActiveMonitor>().is_some() {
            // For ActiveMonitor, create a new instance
            Some(Box::new(ActiveMonitor::new()) as Box<dyn ActionPlugin + Send + Sync>)
        } else {
            error!("Plugin '{}' is not a compatible ActionPlugin", name);
            None
        }
    }
    
    /// Create an action plugin from a JSON configuration string
    pub fn create_action_plugin_from_json(&self, json_config: &str) -> Option<Box<dyn ActionPlugin + Send + Sync>> {
        match serde_json::from_str::<Map<String, Value>>(json_config) {
            Ok(config_map) => {
                // We expect only one key (the plugin type)
                if config_map.len() != 1 {
                    error!("Invalid JSON config: expected a single action plugin configuration");
                    return None;
                }
                
                // Get the first (and only) entry
                let (plugin_type, params) = config_map.iter().next().unwrap();
                
                info!("Creating action plugin of type '{}' from JSON", plugin_type);
                self.create_action_plugin_with_config(plugin_type, Some(params))
            }
            Err(err) => {
                error!("Failed to parse action plugin JSON configuration: {}", err);
                None
            }
        }
    }
    
    /// Returns a default JSON configuration for all available action plugins
    ///
    /// This function provides a complete configuration for all action plugins
    /// in the system with default settings. Each filter includes an "enabled" attribute
    /// that can be used to selectively enable/disable plugins.
    ///
    /// # Returns
    ///
    /// A JSON string containing the complete action plugin configuration array
    pub fn sample_action_plugins_config() -> String {
        let plugins = vec![
            serde_json::json!({
                "active-monitor": {
                    "enabled": true
                }
            }),
            // Add other built-in action plugins here with their default configuration
        ];
        
        serde_json::to_string_pretty(&plugins).unwrap_or_else(|_| "[]".to_string())
    }
    
    /// Returns a default JSON configuration for all available event filters
    ///
    /// This function provides a complete configuration for all event filters 
    /// in the system with default settings. Each filter includes an "enabled" attribute
    /// that can be used to selectively enable/disable filters.
    ///
    /// # Returns
    ///
    /// A JSON string containing the complete event filter configuration array
    pub fn sample_json_config() -> String {
        let filters = vec![
            serde_json::json!({
                "event-logger": {
                    "only_active": false,
                    "log_level": "info",
                    "event_types": ["state", "song", "loop", "capabilities"],
                    "enabled": true
                }
            }),
            // Add other built-in filters here with their default configuration
        ];
        
        serde_json::to_string_pretty(&filters).unwrap_or_else(|_| "[]".to_string())
    }
}