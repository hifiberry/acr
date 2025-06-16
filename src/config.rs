// Configuration utilities for ACR
// 
// This module provides utilities for reading configuration values with backward compatibility
// support for the migration from top-level service configuration to the new "services" subtree.

use log::debug;

/// Helper function to get service configuration with backward compatibility
/// 
/// This function first tries to find the service in the new "services" structure,
/// then falls back to the old top-level structure for backward compatibility.
/// 
/// # Arguments
/// * `config` - The configuration JSON object
/// * `service_name` - The name of the service to look up (e.g., "spotify", "lastfm", etc.)
/// 
/// # Returns
/// * `Option<&serde_json::Value>` - The service configuration if found, None otherwise
/// 
/// # Example
/// ```rust
/// use serde_json::json;
/// use acr::config::get_service_config;
/// 
/// // For a config with new structure:
/// let config = json!({
///   "services": {
///     "spotify": { "enable": true }
///   }
/// });
/// 
/// if let Some(spotify_config) = get_service_config(&config, "spotify") {
///     assert_eq!(spotify_config["enable"], true);
/// }
/// 
/// // For old structure (backward compatibility):
/// let old_config = json!({
///   "spotify": { "enable": false }
/// });
/// 
/// if let Some(spotify_config) = get_service_config(&old_config, "spotify") {
///     assert_eq!(spotify_config["enable"], false);
/// }
/// ```
pub fn get_service_config<'a>(config: &'a serde_json::Value, service_name: &str) -> Option<&'a serde_json::Value> {
    // First, try to find the service in the new "services" structure
    if let Some(services) = config.get("services") {
        if let Some(service_config) = services.get(service_name) {
            debug!("Found {} configuration in services section", service_name);
            return Some(service_config);
        }
    }
    
    // Fall back to the old top-level structure for backward compatibility
    if let Some(service_config) = config.get(service_name) {
        debug!("Found {} configuration at top level (legacy structure)", service_name);
        return Some(service_config);
    }
    
    // Service configuration not found
    debug!("No {} configuration found in either services section or top level", service_name);
    None
}
