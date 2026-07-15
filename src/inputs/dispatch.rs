//! Action dispatch: the single translation point from abstract [`Action`]s to
//! volume and player operations.
//!
//! Every input source funnels through here, so a new source (rotary, IR) needs
//! no dispatch code of its own.

use crate::audiocontrol::audiocontrol::AudioController;
use crate::data::PlayerCommand;
use crate::helpers::global_volume;
use crate::inputs::Action;
use log::debug;
use std::sync::{Arc, Weak};

/// The operations an [`ActionSink`] performs. Exists so dispatch can be tested
/// without an initialised volume singleton or a real player.
pub trait ActionTarget: Send + Sync {
    /// Adjust volume by `delta` percentage points. Returns success.
    fn volume_adjust(&self, delta: f64) -> bool;
    /// Toggle mute. Returns success.
    fn volume_toggle_mute(&self) -> bool;
    /// Whether volume control is usable at all.
    fn volume_available(&self) -> bool;
    /// Send a command to the active player. Returns success.
    fn player_command(&self, cmd: PlayerCommand) -> bool;
}

/// The production [`ActionTarget`]: the global volume control and the
/// `AudioController` singleton.
pub struct GlobalActionTarget {
    controller: Weak<AudioController>,
}

impl GlobalActionTarget {
    pub fn new(controller: Weak<AudioController>) -> Self {
        GlobalActionTarget { controller }
    }
}

impl ActionTarget for GlobalActionTarget {
    fn volume_adjust(&self, delta: f64) -> bool {
        global_volume::adjust_volume_percentage(delta)
    }

    fn volume_toggle_mute(&self) -> bool {
        global_volume::toggle_mute()
    }

    fn volume_available(&self) -> bool {
        global_volume::is_volume_control_available()
    }

    fn player_command(&self, cmd: PlayerCommand) -> bool {
        // A dead Weak means shutdown is in progress: drop the command quietly.
        match self.controller.upgrade() {
            Some(controller) => controller.send_command(cmd),
            None => {
                debug!("inputs: dropping command, AudioController is gone");
                false
            }
        }
    }
}

/// Translates [`Action`]s into operations on an [`ActionTarget`].
#[derive(Clone)]
pub struct ActionSink {
    target: Arc<dyn ActionTarget>,
    volume_step: f64,
}

impl ActionSink {
    pub fn new(target: Arc<dyn ActionTarget>, volume_step: f64) -> Self {
        ActionSink { target, volume_step }
    }

    /// Volume percentage points per volume action.
    pub fn volume_step(&self) -> f64 {
        self.volume_step
    }

    /// Perform an action. Returns whether it was carried out.
    ///
    /// Never panics: a missing volume control or a dead controller is a dropped
    /// action, not a failure. Audio playback must survive any input problem.
    pub fn dispatch(&self, action: Action) -> bool {
        match action {
            Action::VolumeUp | Action::VolumeDown | Action::Mute => {
                if !self.target.volume_available() {
                    debug!("inputs: ignoring {}, no volume control", action.as_str());
                    return false;
                }
                match action {
                    Action::VolumeUp => self.target.volume_adjust(self.volume_step),
                    Action::VolumeDown => self.target.volume_adjust(-self.volume_step),
                    _ => self.target.volume_toggle_mute(),
                }
            }
            Action::Play => self.target.player_command(PlayerCommand::Play),
            Action::Pause => self.target.player_command(PlayerCommand::Pause),
            Action::PlayPause => self.target.player_command(PlayerCommand::PlayPause),
            Action::Stop => self.target.player_command(PlayerCommand::Stop),
            Action::Next => self.target.player_command(PlayerCommand::Next),
            Action::Previous => self.target.player_command(PlayerCommand::Previous),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inputs::Action;
    use parking_lot::Mutex as PlMutex;

    #[derive(Default)]
    struct MockTarget {
        adjusts: PlMutex<Vec<f64>>,
        mutes: PlMutex<usize>,
        commands: PlMutex<Vec<PlayerCommand>>,
        available: bool,
    }

    impl MockTarget {
        fn available() -> Arc<Self> {
            Arc::new(MockTarget { available: true, ..Default::default() })
        }
        fn unavailable() -> Arc<Self> {
            Arc::new(MockTarget { available: false, ..Default::default() })
        }
    }

    impl ActionTarget for MockTarget {
        fn volume_adjust(&self, delta: f64) -> bool {
            self.adjusts.lock().push(delta);
            true
        }
        fn volume_toggle_mute(&self) -> bool {
            *self.mutes.lock() += 1;
            true
        }
        fn volume_available(&self) -> bool {
            self.available
        }
        fn player_command(&self, cmd: PlayerCommand) -> bool {
            self.commands.lock().push(cmd);
            true
        }
    }

    #[test]
    fn test_volume_actions_use_step() {
        let t = MockTarget::available();
        let sink = ActionSink::new(t.clone(), 5.0);
        assert!(sink.dispatch(Action::VolumeUp));
        assert!(sink.dispatch(Action::VolumeDown));
        assert_eq!(*t.adjusts.lock(), vec![5.0, -5.0]);
    }

    #[test]
    fn test_custom_volume_step() {
        let t = MockTarget::available();
        let sink = ActionSink::new(t.clone(), 2.5);
        assert!(sink.dispatch(Action::VolumeUp));
        assert_eq!(*t.adjusts.lock(), vec![2.5]);
    }

    #[test]
    fn test_mute_action() {
        let t = MockTarget::available();
        let sink = ActionSink::new(t.clone(), 5.0);
        assert!(sink.dispatch(Action::Mute));
        assert_eq!(*t.mutes.lock(), 1);
    }

    #[test]
    fn test_transport_actions_map_to_player_commands() {
        let t = MockTarget::available();
        let sink = ActionSink::new(t.clone(), 5.0);
        for a in [Action::Play, Action::Pause, Action::PlayPause,
                  Action::Stop, Action::Next, Action::Previous] {
            assert!(sink.dispatch(a));
        }
        assert_eq!(
            *t.commands.lock(),
            vec![
                PlayerCommand::Play,
                PlayerCommand::Pause,
                PlayerCommand::PlayPause,
                PlayerCommand::Stop,
                PlayerCommand::Next,
                PlayerCommand::Previous,
            ]
        );
    }

    /// audiocontrol2 logged "ignoring %s, no volume control" and carried on.
    #[test]
    fn test_volume_actions_dropped_when_unavailable() {
        let t = MockTarget::unavailable();
        let sink = ActionSink::new(t.clone(), 5.0);
        assert!(!sink.dispatch(Action::VolumeUp));
        assert!(!sink.dispatch(Action::Mute));
        assert!(t.adjusts.lock().is_empty());
        assert_eq!(*t.mutes.lock(), 0);
        // Transport still works without volume control.
        assert!(sink.dispatch(Action::Next));
        assert_eq!(*t.commands.lock(), vec![PlayerCommand::Next]);
    }

    /// A dead Weak<AudioController> (shutdown in progress) must not panic.
    #[test]
    fn test_global_target_with_dead_weak_does_not_panic() {
        let target = GlobalActionTarget::new(Weak::new());
        assert!(!target.player_command(PlayerCommand::Next));
    }
}
