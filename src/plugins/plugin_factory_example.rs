/// Example code showing how to use the JSON-based plugin factory
/// 
/// This module contains examples of how to use the PluginFactory
/// to create plugins from JSON configuration.

use crate::plugins::plugin_factory::PluginFactory;
use crate::plugins::event_filters::event_filter::EventFilter;

/// Example of how to use the plugin factory with JSON
pub fn plugin_factory_json_example() {
    // Create a new plugin factory
    let factory = PluginFactory::new();
    
    // Example 1: Create a single plugin from JSON
    let json_config = r#"{
        "event-logger": {
            "only_active": true
        }
    }"#;
    
    if let Some(plugin) = factory.create_from_json(json_config) {
        println!("Created plugin: {} v{}", plugin.name(), plugin.version());
    }
    
    // Example 2: Create an event filter from JSON
    if let Some(filter) = factory.create_event_filter_from_json(json_config) {
        println!("Created event filter: {} v{}", filter.name(), filter.version());
    }
    
    // Example 3: Create multiple plugins from a JSON array
    let json_array = r#"[
        {
            "event-logger": {
                "only_active": false
            }
        },
        {
            "event-logger": {
                "only_active": true
            }
        }
    ]"#;
    
    let plugins = factory.create_plugins_from_json(json_array);
    println!("Created {} plugins from JSON array", plugins.len());
    
    for plugin in plugins {
        println!("Plugin: {} v{}", plugin.name(), plugin.version());
    }
}