use crate::AudioController;
use rocket::serde::json::Json;
use rocket::{get, State};
use std::sync::Arc;

/// Response struct for listing active action plugins
#[derive(serde::Serialize)]
pub struct ActionPluginsResponse {
    plugins: Vec<PluginInfo>
}

/// Response struct for listing active event filters
#[derive(serde::Serialize)]
pub struct EventFiltersResponse {
    filters: Vec<PluginInfo>
}

/// Information about a plugin for the API response
#[derive(serde::Serialize)]
pub struct PluginInfo {
    name: String,
    version: String,
}

/// List all active action plugins
#[get("/plugins/actions")]
pub fn list_action_plugins(controller: &State<Arc<AudioController>>) -> Json<ActionPluginsResponse> {
    // Get plugin info from controller
    let plugins_info = controller.get_action_plugin_info()
        .into_iter()
        .map(|(name, version)| PluginInfo { name, version })
        .collect();

    Json(ActionPluginsResponse {
        plugins: plugins_info,
    })
}

/// List all active event filters
#[get("/plugins/event-filters")]
pub fn list_event_filters(controller: &State<Arc<AudioController>>) -> Json<EventFiltersResponse> {
    // Get filter info from controller
    let filters_info = controller.get_event_filter_info()
        .into_iter()
        .map(|(name, version)| PluginInfo { name, version })
        .collect();

    Json(EventFiltersResponse {
        filters: filters_info,
    })
}