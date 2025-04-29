use std::any::Any;

/// Base trait for all plugins
pub trait Plugin {
    /// Get the name of the plugin
    fn name(&self) -> &str;

    /// Get the version of the plugin
    fn version(&self) -> &str;

    /// Initialize the plugin
    /// 
    /// # Returns
    /// 
    /// `true` if initialization was successful, `false` otherwise
    fn init(&mut self) -> bool;

    /// Shutdown the plugin
    /// 
    /// # Returns
    /// 
    /// `true` if shutdown was successful, `false` otherwise
    fn shutdown(&mut self) -> bool;

    /// Get the plugin as Any for downcasting
    fn as_any(&self) -> &dyn Any;
}

/// A base implementation of Plugin that can be used by other plugins
pub struct BasePlugin {
    /// Plugin name
    name: String,
    
    /// Plugin version
    version: String,
}

impl BasePlugin {
    /// Create a new BasePlugin
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
        }
    }
    
    /// Create a new BasePlugin with a specific version
    pub fn with_version(name: &str, version: &str) -> Self {
        Self {
            name: name.to_string(),
            version: version.to_string(),
        }
    }
}

impl Plugin for BasePlugin {
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
        log::info!("Plugin '{}' shutdown", self.name);
        true
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}