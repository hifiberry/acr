use std::collections::HashMap;
use std::collections::HashSet;
use log::{info, error, warn};
use serde_json::{Value, Map};

use crate::plugins::plugin::Plugin;
use crate::plugins::event_filters::event_filter::{EventFilter};
use crate::plugins::action_plugin::ActionPlugin;
use crate::plugins::action_plugins::ActiveMonitor;
use crate::plugins::action_plugins::event_logger::{EventLogger, LogLevel};
use crate::plugins::action_plugins::lastfm_plugin::{LastfmPlugin, LastfmPluginConfig};

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

        self.register("lastfm", |config_value| { // Renamed from "lastfm-plugin" to "lastfm"
            if let Some(value) = config_value {
                match serde_json::from_value::<LastfmPluginConfig>(value.clone()) {
                    Ok(config) => Some(Box::new(LastfmPlugin::new(config)) as Box<dyn Plugin>),
                    Err(e) => {
                        error!("Failed to parse LastfmPluginConfig for 'lastfm' plugin: {}. Plugin will not be loaded.", e);
                        None
                    }
                }
            } else {
                error!("'lastfm' plugin requires configuration (api_key, api_secret). Plugin will not be loaded.");
                None
            }
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
    pub fn create_event_filter_with_config(&self, name: &str, _config: Option<&Value>) -> Option<Box<dyn EventFilter + Send + Sync>> { // Mark config as unused
        if name == "EventLogger" || name == "event-logger" { // Also check for "event-logger" as it's registered like that
            error!("EventLogger cannot be created as an EventFilter.");
            return None;
        }

        // let plugin_box = self.create_with_config(name, config)?; // This line is removed

        // This is the tricky part: converting Box<dyn Plugin> to Box<dyn EventFilter>.
        // For this to work robustly, the Plugin trait would ideally have a method like:
        // fn into_event_filter(self: Box<Self>) -> Result<Box<dyn EventFilter + Send + Sync>, Self>
        // Or the registration for event filters should store Box<dyn Fn(...) -> Option<Box<dyn EventFilter + Send + Sync>>>
        // Given the current structure, we are relying on the concrete type inside plugin_box
        // also implementing EventFilter and hoping for a downcast to work.

        // Attempt to downcast Box<dyn Plugin> to a concrete type that implements EventFilter,
        // then recast to Box<dyn EventFilter>. This is highly dependent on knowing the concrete types
        // or having a more flexible trait object system.

        // A simplified approach: if the plugin's concrete type (obtained via as_any)
        // can be downcast to something that IS an EventFilter.
        // This still doesn't give us Box<dyn EventFilter> directly from Box<dyn Plugin>.

        // The most direct approach if the underlying object *is* an EventFilter:
        // 1. Get `&dyn Any` from `Box<dyn Plugin>`.
        // 2. Try to `downcast_ref` to `&dyn EventFilter`.
        // 3. If successful, it means the concrete type implements EventFilter.
        // However, we need to return a Box<dyn EventFilter>.
        // Consuming the original Box<dyn Plugin> and re-boxing is one way, if we can get the concrete type.

        // If the constructor registered for "name" is known to produce an EventFilter,
        // the issue is purely a type system one.
        // Let's assume for now that if create_with_config returns a plugin for an event filter name,
        // it *should* be usable as an EventFilter. The problem is the Box<dyn Plugin> type.

        // The previous errors (E0277, E0599) indicate problems with downcasting.
        // If a plugin is registered as Box::new(MyActualEventFilter{}),
        // then plugin_box.as_ref().as_any().downcast_ref::<MyActualEventFilter>() would work.
        // But we want Box<dyn EventFilter>.

        // Given EventFilter: Plugin, if the constructor returned Box<dyn EventFilter> upcast to Box<dyn Plugin>,
        // we'd need to downcast the Box itself. Box::downcast was unstable.
        // A common pattern is `plugin_box.as_any_arc().downcast_arc()` for Arc, similar for Box might need `into_any()`.

        // Let's assume the plugin must be Box-ed as dyn EventFilter from the start by the constructor
        // if it's to be used as such. Since our registry returns Box<dyn Plugin>, this is problematic.

        // For now, let's remove the complex downcasting and rely on the fact that
        // EventLogger (the main offender) is filtered out. Other event filters might work if they are simple.
        // This part of the code is inherently difficult without changing Plugin trait or registration.
        // We are essentially trying to downcast a trait object Box<dyn TraitA> to Box<dyn TraitB>
        // where TraitB: TraitA.

        // If the plugin is indeed an EventFilter, we need to re-box it.
        // This is unsafe if not careful. The safest is if the constructor itself can return the desired type.
        // Since we can't change the registry function signature easily, we'll have to assume that
        // if `create_with_config` returns something for an EventFilter name (that's not EventLogger),
        // it's because the underlying concrete type implements EventFilter.
        // The challenge is getting from Box<dyn Plugin> to Box<dyn EventFilter>.

        // A temporary, potentially unsafe approach if we assume the concrete type is `T: Plugin + EventFilter`:
        // This would involve `Box::from_raw(Box::into_raw(plugin_box) as *mut dyn EventFilter)`. This is generally not recommended.

        // Let's revert to a structure that relies on the plugin being correctly castable by the caller or having specific handling.
        // The main issue was EventLogger. Since it's handled, other filters might pass if they are simple.
        // The function signature demands Option<Box<dyn EventFilter + Send + Sync>>.
        // If `plugin_box`'s concrete type implements `EventFilter`, we need to perform that conversion.
        // This is where `Plugin::as_event_filter(self: Box<Self>)` would be useful.
        // Lacking that, we are in a tough spot for a generic solution.

        // The simplest path that might work for some cases (if the concrete type is directly an EventFilter):
        // This relies on the specific plugin's implementation and how it's boxed.
        // This will likely fail for many cases and isn't robust.
        // The core issue is that Box<dyn Plugin> to Box<dyn EventFilter> is not a simple cast.
        // We will remove this function for now as it's not correctly implemented.
        error!("create_event_filter_with_config is not robustly implemented for generic EventFilters after EventLogger changes.");
        None
    }

    /// Create an event filter from a JSON configuration string
    pub fn create_event_filter_from_json(&self, json_config: &str) -> Option<Box<dyn EventFilter + Send + Sync>> {
        match serde_json::from_str::<Map<String, Value>>(json_config) {
            Ok(config_map) => {
                if config_map.len() != 1 {
                    error!("Invalid JSON config: expected a single event filter configuration");
                    return None;
                }
                let (plugin_type, params) = config_map.iter().next().unwrap();
                info!("Creating event filter of type \'{}\' from JSON", plugin_type);
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
        } else if plugin.as_any().downcast_ref::<EventLogger>().is_some() {
            // For EventLogger, we need to create a new instance with the right configuration
            if let Some(config_val) = config {
                let only_active = config_val.get("only_active")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                
                // Get log level from config
                let log_level = config_val.get("log_level")
                    .and_then(Value::as_str)
                    .map(LogLevel::from)
                    .unwrap_or_default();
                
                // Get event types to log if specified
                let event_types = config_val.get("event_types")
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
                
                Some(Box::new(EventLogger::with_config(only_active, log_level, event_types)) as Box<dyn ActionPlugin + Send + Sync>)
            } else {
                // Use default values
                Some(Box::new(EventLogger::new(false)) as Box<dyn ActionPlugin + Send + Sync>)
            }
        } else if plugin.as_any().downcast_ref::<LastfmPlugin>().is_some() {
            // For LastfmPlugin, create a new instance with its configuration
            if let Some(config_val) = config {
                match serde_json::from_value::<LastfmPluginConfig>(config_val.clone()) {
                    Ok(lastfm_config) => {
                        Some(Box::new(LastfmPlugin::new(lastfm_config)) as Box<dyn ActionPlugin + Send + Sync>)
                    }
                    Err(e) => {
                        error!("Failed to parse LastfmPluginConfig for '{}' in create_action_plugin_with_config: {}. Plugin will not be loaded.", name, e);
                        None
                    }
                }
            } else {
                // This case should ideally not be reached if create_with_config for "lastfm" succeeded,
                // as its registration requires configuration.
                error!("'{}' plugin (LastfmPlugin) requires configuration, but none was provided to create_action_plugin_with_config. This indicates an issue.", name);
                None
            }
        } else {
            error!("Plugin '{}' is not a compatible ActionPlugin or is not specifically handled in create_action_plugin_with_config.", name);
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