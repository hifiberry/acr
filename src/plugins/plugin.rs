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