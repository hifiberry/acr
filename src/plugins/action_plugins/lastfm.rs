use std::any::Any;
use std::sync::Weak;
use std::thread;
use std::time::Duration;

use crate::audiocontrol::AudioController;
use crate::data::PlayerEvent;
use crate::helpers::lastfm::LastfmClient;
use crate::plugins::action_plugin::{ActionPlugin, BaseActionPlugin};
use crate::plugins::plugin::Plugin;
use log::{error, info};
use serde::Deserialize;

#[derive(Debug, Deserialize, Clone)]
pub struct LastfmConfig { // Renamed from LastfmPluginConfig
    pub enabled: bool,
    pub api_key: String,
    pub api_secret: String,
}

pub struct Lastfm { // Renamed from LastfmPlugin
    base: BaseActionPlugin,
    config: LastfmConfig, // Updated type
    worker_thread: Option<thread::JoinHandle<()>>,
}

impl Lastfm { // Renamed from LastfmPlugin
    pub fn new(config: LastfmConfig) -> Self { // Updated type
        Self {
            base: BaseActionPlugin::new("Lastfm"), // Renamed plugin identifier
            config,
            worker_thread: None,
        }
    }
}

impl Plugin for Lastfm { // Renamed from LastfmPlugin
    fn name(&self) -> &str {
        self.base.name()
    }

    fn version(&self) -> &str {
        self.base.version()
    }

    fn init(&mut self) -> bool {
        if !self.config.enabled {
            info!("Lastfm is disabled by configuration. Skipping initialization."); // Updated log
            return true;
        }

        info!("Initializing Lastfm..."); // Updated log

        let init_result = if self.config.api_key.is_empty() || self.config.api_secret.is_empty() {
            info!("Lastfm: API key or secret is empty in plugin configuration. Attempting to use default credentials."); // Updated log
            LastfmClient::initialize_with_defaults()
        } else {
            LastfmClient::initialize(
                self.config.api_key.clone(),
                self.config.api_secret.clone(),
            )
        };

        match init_result {
            Ok(_) => {
                info!("Lastfm: Last.fm client connection initialized/verified successfully."); // Updated log

                let plugin_name = self.name().to_string();
                let handle = thread::spawn(move || {
                    info!("Lastfm background thread started for plugin: {}", plugin_name); // Updated log
                    loop {
                        thread::sleep(Duration::from_secs(1));
                    }
                });
                self.worker_thread = Some(handle);

                self.base.init()
            }
            Err(e) => {
                error!("Lastfm: Failed to initialize Last.fm client: {}", e); // Updated log
                false
            }
        }
    }

    fn shutdown(&mut self) -> bool {
        info!("Lastfm shutdown."); // Updated log
        self.base.shutdown()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl ActionPlugin for Lastfm { // Renamed from LastfmPlugin
    fn initialize(&mut self, controller: Weak<AudioController>) {
        self.base.set_controller(controller);
        info!("Lastfm received controller reference."); // Updated log
    }

    fn on_event(&mut self, _event: &PlayerEvent, _is_active_player: bool) {
    }
}
