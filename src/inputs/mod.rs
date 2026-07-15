//! Generic input layer.
//!
//! Input sources translate hardware events into abstract [`Action`]s, which an
//! `ActionSink` dispatches to the volume control and the active player. Adding a
//! new source (rotary encoder, IR receiver) means emitting `Action`s; no new
//! dispatch code is required.

pub mod keyboard;
pub mod dispatch;
pub mod registry;

pub use dispatch::ActionSink;

use crate::audiocontrol::audiocontrol::AudioController;
use dispatch::GlobalActionTarget;
use log::{error, info, warn};
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use std::sync::{Arc, Weak};

/// Errors an input source can fail to start with.
#[derive(Debug, thiserror::Error)]
pub enum InputError {
    #[error("permission denied opening {path}: add the 'audiocontrol' user to the 'input' group")]
    PermissionDenied { path: String },

    #[error("i/o error on {path}: {message}")]
    Io { path: String, message: String },
}

/// An input source: hardware that produces [`Action`]s.
pub trait InputController: Send {
    /// Stable identifier, used as the config key and in the status API.
    fn name(&self) -> &str;

    /// Begin producing actions into `sink`.
    ///
    /// Must not block: long-running work belongs on its own thread.
    fn start(&mut self, sink: ActionSink) -> Result<(), InputError>;

    /// Stop producing actions.
    fn stop(&mut self);

    /// Status for `GET /api/inputs`.
    fn status(&self) -> serde_json::Value;
}

/// The started input sources, kept so `GET /api/inputs` can report status.
static INPUTS: Lazy<Mutex<Vec<Box<dyn InputController>>>> =
    Lazy::new(|| Mutex::new(Vec::new()));

/// Build and start the configured input sources.
///
/// Must be called after the volume control and the `AudioController` singleton
/// are initialised, so the first keypress can act. Never fails: an input
/// problem must not stop audio.
pub fn init_inputs(config: &serde_json::Value, controller: Weak<AudioController>) {
    let mut inputs = registry::build_inputs(config);
    if inputs.is_empty() {
        info!("inputs: no input sources configured");
        return;
    }

    let target = Arc::new(GlobalActionTarget::new(controller));

    for input in inputs.iter_mut() {
        let step = match input.name() {
            "keyboard" => keyboard::KeyboardConfig::from_config(
                config.get("inputs").and_then(|v| v.get("keyboard")),
            )
            .volume_step,
            _ => keyboard::DEFAULT_VOLUME_STEP,
        };
        let sink = ActionSink::new(target.clone(), step);
        match input.start(sink) {
            Ok(()) => info!("inputs: started '{}'", input.name()),
            Err(e @ InputError::PermissionDenied { .. }) => {
                error!("inputs: could not start '{}': {}", input.name(), e)
            }
            Err(e) => warn!("inputs: could not start '{}': {}", input.name(), e),
        }
    }

    INPUTS.lock().extend(inputs);
}

/// Status of all input sources, for `GET /api/inputs`.
pub fn inputs_status() -> serde_json::Value {
    let inputs = INPUTS.lock();
    let entries: Vec<serde_json::Value> = inputs
        .iter()
        .map(|i| serde_json::json!({ "name": i.name(), "status": i.status() }))
        .collect();
    serde_json::json!({ "inputs": entries })
}

/// An abstract control action produced by an input source.
///
/// The string forms are the ones audiocontrol2 used in its code tables, so old
/// configurations port over unchanged. `Stop` is new.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Action {
    VolumeUp,
    VolumeDown,
    Mute,
    Play,
    Pause,
    PlayPause,
    Stop,
    Next,
    Previous,
}

impl Action {
    /// Parse a config action string. Returns `None` for anything unrecognised.
    pub fn from_action_str(s: &str) -> Option<Action> {
        match s {
            "volume_up" => Some(Action::VolumeUp),
            "volume_down" => Some(Action::VolumeDown),
            "mute" => Some(Action::Mute),
            "play" => Some(Action::Play),
            "pause" => Some(Action::Pause),
            "playpause" => Some(Action::PlayPause),
            "stop" => Some(Action::Stop),
            "next" => Some(Action::Next),
            "previous" => Some(Action::Previous),
            _ => None,
        }
    }

    /// The canonical config string for this action.
    pub fn as_str(&self) -> &'static str {
        match self {
            Action::VolumeUp => "volume_up",
            Action::VolumeDown => "volume_down",
            Action::Mute => "mute",
            Action::Play => "play",
            Action::Pause => "pause",
            Action::PlayPause => "playpause",
            Action::Stop => "stop",
            Action::Next => "next",
            Action::Previous => "previous",
        }
    }

    /// Whether this action should fire on key autorepeat (evdev value 2).
    ///
    /// Only volume actions repeat: holding volume-up should ramp, but holding
    /// `next` must not skip thirty tracks.
    pub fn repeats_on_hold(&self) -> bool {
        matches!(self, Action::VolumeUp | Action::VolumeDown)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_action_from_str() {
        assert_eq!(Action::from_action_str("volume_up"), Some(Action::VolumeUp));
        assert_eq!(Action::from_action_str("playpause"), Some(Action::PlayPause));
        assert_eq!(Action::from_action_str("stop"), Some(Action::Stop));
        assert_eq!(Action::from_action_str("nonsense"), None);
        // Case-sensitive: config uses lowercase, matching audiocontrol2.
        assert_eq!(Action::from_action_str("Volume_Up"), None);
    }

    #[test]
    fn test_action_round_trips() {
        for a in [
            Action::VolumeUp, Action::VolumeDown, Action::Mute,
            Action::Play, Action::Pause, Action::PlayPause,
            Action::Stop, Action::Next, Action::Previous,
        ] {
            assert_eq!(Action::from_action_str(a.as_str()), Some(a));
        }
    }

    #[test]
    fn test_only_volume_repeats_on_hold() {
        assert!(Action::VolumeUp.repeats_on_hold());
        assert!(Action::VolumeDown.repeats_on_hold());
        assert!(!Action::Mute.repeats_on_hold());
        assert!(!Action::Next.repeats_on_hold());
        assert!(!Action::PlayPause.repeats_on_hold());
        assert!(!Action::Play.repeats_on_hold());
        assert!(!Action::Pause.repeats_on_hold());
        assert!(!Action::Stop.repeats_on_hold());
        assert!(!Action::Previous.repeats_on_hold());
    }
}
