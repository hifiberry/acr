use std::any::Any;
use std::sync::Weak;

use crate::audiocontrol::AudioController;
use crate::data::PlayerEvent;
use crate::helpers::lastfm::LastfmClient;
use crate::plugins::action_plugin::{ActionPlugin, BaseActionPlugin};
use crate::plugins::plugin::Plugin;
use log::{error, info};
use serde::Deserialize;

#[derive(Debug, Deserialize, Clone)]
pub struct LastfmPluginConfig {
    pub enabled: bool, // This refers to the plugin's internal enabled state, separate from manager
    pub api_key: String,
    pub api_secret: String,
}

pub struct LastfmPlugin {
    base: BaseActionPlugin,
    config: LastfmPluginConfig,
}

impl LastfmPlugin {
    pub fn new(config: LastfmPluginConfig) -> Self {
        Self {
            base: BaseActionPlugin::new("LastfmPlugin"), // Internal name can remain LastfmPlugin
            config,
        }
    }
}

impl Plugin for LastfmPlugin {
    fn name(&self) -> &str {
        self.base.name()
    }

    fn version(&self) -> &str {
        self.base.version()
    }

    fn init(&mut self) -> bool {
        if !self.config.enabled {
            info!("LastfmPlugin is disabled by configuration. Skipping initialization.");
            return true; // Successfully "initialized" as disabled.
        }

        info!("Initializing LastfmPlugin...");

        let init_result = if self.config.api_key.is_empty() || self.config.api_secret.is_empty() {
            info!("LastfmPlugin: API key or secret is empty in plugin configuration. Attempting to use default credentials.");
            LastfmClient::initialize_with_defaults()
        } else {
            LastfmClient::initialize(
                self.config.api_key.clone(),
                self.config.api_secret.clone(),
            )
        };

        match init_result {
            Ok(_) => {
                info!("LastfmPlugin: Last.fm client connection initialized/verified successfully.");
                self.base.init()
            }
            Err(e) => {
                error!("LastfmPlugin: Failed to initialize Last.fm client: {}", e);
                false
            }
        }
    }

    fn shutdown(&mut self) -> bool {
        info!("LastfmPlugin shutdown.");
        self.base.shutdown()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl ActionPlugin for LastfmPlugin {
    fn initialize(&mut self, controller: Weak<AudioController>) {
        self.base.set_controller(controller);
        // No specific controller interaction needed for just initializing the LastfmClient
        info!("LastfmPlugin received controller reference.");
    }

    fn on_event(&mut self, _event: &PlayerEvent, _is_active_player: bool) {
        // Not doing anything with events yet, as per requirements.
        // Scrobbling and now_playing updates would go here.
    }
}
