//! An `action_plugins` entry whose work has moved out of the daemon.
//!
//! `GET /api/plugins/actions` serialises `AudioController::get_action_plugin_info()`,
//! which reads the registered action plugins. The clients that call it ship
//! separately from the daemon, so what an existing configuration reports there
//! cannot change just because the code behind an entry moved: a name that
//! disappears from that list is an API change.
//!
//! So an entry whose work is now done by a worker elsewhere -- `lastfm`, whose
//! scrobbling runs in `audiocontrol-metadata` -- is still registered, by this
//! type, which reports the name and version the plugin used to report and does
//! nothing else. It subscribes to nothing, holds no controller and handles no
//! event; the worker the entry configures is started by `main`.

use crate::audiocontrol::AudioController;
use crate::plugins::action_plugin::ActionPlugin;
use crate::plugins::plugin::Plugin;
use std::any::Any;
use std::sync::Weak;

/// Reports a name and a version, and does nothing.
#[derive(Clone)]
pub struct WorkerDescriptor {
    name: String,
    version: String,
}

impl WorkerDescriptor {
    /// The version is the daemon's own, which is what `BaseActionPlugin` gave
    /// every built-in plugin, so the reported version is unchanged too.
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
        }
    }
}

impl Plugin for WorkerDescriptor {
    fn name(&self) -> &str {
        &self.name
    }

    fn version(&self) -> &str {
        &self.version
    }

    fn init(&mut self) -> bool {
        true
    }

    fn shutdown(&mut self) -> bool {
        true
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl ActionPlugin for WorkerDescriptor {
    fn initialize(&mut self, _controller: Weak<AudioController>) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The two fields the endpoint serialises, and both have to be what the
    /// plugin reported before its work moved out.
    #[test]
    fn it_reports_the_name_it_was_given_and_the_daemon_version() {
        let descriptor = WorkerDescriptor::new("Lastfm");
        assert_eq!(descriptor.name(), "Lastfm");
        assert_eq!(descriptor.version(), env!("CARGO_PKG_VERSION"));
    }
}
