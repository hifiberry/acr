use crate::AudioController;
use rocket::serde::json::Json;
use rocket::{get, State};
use std::sync::Arc;

/// Response struct for listing active action plugins
#[derive(serde::Serialize)]
pub struct ActionPluginsResponse {
    plugins: Vec<PluginInfo>
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

#[cfg(test)]
mod tests {
    use super::*;
    use rocket::local::blocking::Client;

    /// The `action_plugins` array as the shipped configuration has it, with
    /// Last.fm enabled and last.
    fn config_with_lastfm() -> serde_json::Value {
        serde_json::json!({
            "services": {},
            "action_plugins": [
                { "active-monitor": { "enabled": true } },
                { "event-logger": { "enabled": true, "log_level": "info" } },
                {
                    "lastfm": {
                        "enabled": true,
                        "api_key": "",
                        "api_secret": "",
                        "scrobble": true
                    }
                }
            ]
        })
    }

    fn plugins_payload(config: &serde_json::Value) -> serde_json::Value {
        let controller =
            AudioController::from_json(config).expect("the configuration should build a controller");
        let rocket = rocket::build()
            .manage(controller)
            .mount("/api", rocket::routes![list_action_plugins]);
        let client = Client::tracked(rocket).expect("rocket should launch");

        let response = client.get("/api/plugins/actions").dispatch();
        assert_eq!(response.status(), rocket::http::Status::Ok);
        serde_json::from_str(&response.into_string().expect("a body")).expect("valid JSON")
    }

    /// The contract this endpoint has with clients that ship separately from
    /// the daemon: an `action_plugins` entry that is listed today is still
    /// listed, under the same name, with the same version, in the same
    /// position -- whatever moved behind it. Last.fm's scrobbling now runs as a
    /// worker in the metadata crate rather than as an action plugin, and this
    /// payload must not show it.
    #[test]
    fn a_lastfm_configuration_reports_the_same_plugin_list_as_before() {
        assert_eq!(
            plugins_payload(&config_with_lastfm()),
            serde_json::json!({
                "plugins": [
                    { "name": "ActiveMonitor", "version": env!("CARGO_PKG_VERSION") },
                    { "name": "EventLogger", "version": env!("CARGO_PKG_VERSION") },
                    { "name": "Lastfm", "version": env!("CARGO_PKG_VERSION") },
                ]
            })
        );
    }

    /// Position, specifically: the list follows the configuration array, so an
    /// entry whose work moved out of the daemon cannot be quietly appended at
    /// the end instead.
    #[test]
    fn the_list_follows_the_order_of_the_configuration() {
        let config = serde_json::json!({
            "services": {},
            "action_plugins": [
                {
                    "lastfm": {
                        "enabled": true,
                        "api_key": "",
                        "api_secret": "",
                        "scrobble": true
                    }
                },
                { "active-monitor": { "enabled": true } }
            ]
        });

        let payload = plugins_payload(&config);
        let names: Vec<&str> = payload["plugins"]
            .as_array()
            .expect("an array")
            .iter()
            .map(|entry| entry["name"].as_str().expect("a name"))
            .collect();
        assert_eq!(names, vec!["Lastfm", "ActiveMonitor"]);
    }

    /// `enabled: false` never kept the entry out of this list -- the plugin was
    /// created either way and only declined to do anything -- so the disabled
    /// case has to keep reporting it too.
    #[test]
    fn a_disabled_lastfm_entry_is_still_listed() {
        let config = serde_json::json!({
            "services": {},
            "action_plugins": [
                {
                    "lastfm": {
                        "enabled": false,
                        "api_key": "",
                        "api_secret": ""
                    }
                }
            ]
        });

        assert_eq!(
            plugins_payload(&config),
            serde_json::json!({
                "plugins": [
                    { "name": "Lastfm", "version": env!("CARGO_PKG_VERSION") },
                ]
            })
        );
    }

    /// An entry missing the keys the worker needs produced no plugin before,
    /// and so appears in no list now either.
    #[test]
    fn an_unusable_lastfm_entry_is_not_listed() {
        let config = serde_json::json!({
            "services": {},
            "action_plugins": [
                { "lastfm": { "enabled": true } }
            ]
        });

        assert_eq!(
            plugins_payload(&config),
            serde_json::json!({ "plugins": [] })
        );
    }
}