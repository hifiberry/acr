//! Builds input sources from configuration.
//!
//! With a single input type this is a `match` on the config key, mirroring
//! `players::player_factory`. A dynamic registration API buys nothing until
//! there is a second type.

use crate::inputs::keyboard::{KeyboardConfig, KeyboardInput};
use crate::inputs::InputController;
use log::{info, warn};

/// Build the configured input sources.
///
/// An **absent** `inputs` section yields the default set (keyboard enabled),
/// which is what makes an upgrade restore remote support with no user action. A
/// **present** section is taken literally: it lists exactly the sources to run,
/// so `"inputs": {}` means none. Keys starting with `_` are treated as commented
/// out, per the convention in `player_factory.rs:38`.
pub fn build_inputs(config: &serde_json::Value) -> Vec<Box<dyn InputController>> {
    let inputs_config = config.get("inputs");

    // No inputs section at all: use defaults.
    let Some(obj) = inputs_config.and_then(|v| v.as_object()) else {
        let cfg = KeyboardConfig::from_config(None);
        return if cfg.enable {
            vec![Box::new(KeyboardInput::new(cfg))]
        } else {
            vec![]
        };
    };

    let mut result: Vec<Box<dyn InputController>> = Vec::new();
    for (key, value) in obj {
        if key.starts_with('_') {
            info!("inputs: skipping commented-out entry '{}'", key);
            continue;
        }
        match key.as_str() {
            "keyboard" => {
                let cfg = KeyboardConfig::from_config(Some(value));
                if !cfg.enable {
                    info!("inputs: keyboard is disabled in configuration");
                    continue;
                }
                result.push(Box::new(KeyboardInput::new(cfg)));
            }
            other => warn!("inputs: unknown input type '{}', ignoring", other),
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_absent_inputs_section_still_builds_keyboard() {
        // audiocontrol2 shipped the keyboard controller enabled by default.
        let inputs = build_inputs(&json!({}));
        assert_eq!(inputs.len(), 1);
        assert_eq!(inputs[0].name(), "keyboard");
    }

    #[test]
    fn test_disabled_keyboard_not_built() {
        let cfg = json!({ "inputs": { "keyboard": { "enable": false } } });
        assert_eq!(build_inputs(&cfg).len(), 0);
    }

    #[test]
    fn test_underscore_prefix_is_commented_out() {
        // Matches the convention in player_factory.rs:38.
        let cfg = json!({ "inputs": { "_keyboard": { "enable": true } } });
        assert_eq!(build_inputs(&cfg).len(), 0);
    }

    #[test]
    fn test_unknown_input_type_skipped() {
        let cfg = json!({ "inputs": { "telepathy": { "enable": true } } });
        assert_eq!(build_inputs(&cfg).len(), 0);
    }
}
