//! Key name/code resolution and the action keymap.
//!
//! This module deliberately does **not** depend on `evdev`: evdev is Linux-only,
//! and keeping it out here is what lets the mapping logic be unit-tested on any
//! platform. Codes are the stable Linux `input-event-codes.h` ABI values.

use crate::inputs::Action;
use log::warn;
use std::collections::HashMap;

/// Key names recognised in `keymap` config, and used to render codes in
/// `audiocontrol_input_devices`. Not exhaustive over input-event-codes.h --
/// it covers keys plausibly found on a media remote or keyboard. Anything
/// missing can still be configured by raw numeric code.
const KEY_NAMES: &[(&str, u16)] = &[
    ("KEY_ESC", 1),
    ("KEY_1", 2), ("KEY_2", 3), ("KEY_3", 4), ("KEY_4", 5), ("KEY_5", 6),
    ("KEY_6", 7), ("KEY_7", 8), ("KEY_8", 9), ("KEY_9", 10), ("KEY_0", 11),
    ("KEY_BACKSPACE", 14),
    ("KEY_TAB", 15),
    ("KEY_ENTER", 28),
    ("KEY_SPACE", 57),
    ("KEY_HOME", 102),
    ("KEY_UP", 103),
    ("KEY_PAGEUP", 104),
    ("KEY_LEFT", 105),
    ("KEY_RIGHT", 106),
    ("KEY_END", 107),
    ("KEY_DOWN", 108),
    ("KEY_PAGEDOWN", 109),
    ("KEY_INSERT", 110),
    ("KEY_DELETE", 111),
    ("KEY_MUTE", 113),
    ("KEY_VOLUMEDOWN", 114),
    ("KEY_VOLUMEUP", 115),
    ("KEY_POWER", 116),
    ("KEY_PAUSE", 119),
    ("KEY_STOP", 128),
    ("KEY_AGAIN", 129),
    ("KEY_MENU", 139),
    ("KEY_BACK", 158),
    ("KEY_FORWARD", 159),
    ("KEY_NEXTSONG", 163),
    ("KEY_PLAYPAUSE", 164),
    ("KEY_PREVIOUSSONG", 165),
    ("KEY_STOPCD", 166),
    ("KEY_RECORD", 167),
    ("KEY_REWIND", 168),
    ("KEY_CONFIG", 171),
    ("KEY_HOMEPAGE", 172),
    ("KEY_REFRESH", 173),
    ("KEY_PLAYCD", 200),
    ("KEY_PAUSECD", 201),
    ("KEY_PLAY", 207),
    ("KEY_FASTFORWARD", 208),
    ("KEY_PRINT", 210),
    ("KEY_SEARCH", 217),
    ("KEY_MEDIA", 226),
    ("KEY_OK", 352),
    ("KEY_SELECT", 353),
    ("KEY_CLEAR", 355),
    ("KEY_OPTION", 357),
    ("KEY_INFO", 358),
    ("KEY_PROGRAM", 362),
    ("KEY_CHANNEL", 363),
    ("KEY_FAVORITES", 364),
    ("KEY_CHANNELUP", 402),
    ("KEY_CHANNELDOWN", 403),
    ("KEY_LAST", 405),
    ("KEY_NEXT", 407),
    ("KEY_RESTART", 408),
    ("KEY_SLOW", 409),
    ("KEY_SHUFFLE", 410),
    ("KEY_PREVIOUS", 412),
];

/// Resolve a `KEY_*` name to its Linux keycode.
pub fn key_code_from_name(name: &str) -> Option<u16> {
    KEY_NAMES
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, c)| *c)
}

/// Render a keycode as its `KEY_*` name, if known.
pub fn key_name_from_code(code: u16) -> Option<&'static str> {
    KEY_NAMES
        .iter()
        .find(|(_, c)| *c == code)
        .map(|(n, _)| *n)
}

/// Render a keycode for display: its `KEY_*` name if known, else the decimal
/// number as a string. Unlike [`key_name_from_code`], this never drops a
/// code -- raw numeric codes are a supported escape hatch in `resolve_key`,
/// and status output must not silently omit devices' unnamed keys.
pub fn key_display_name(code: u16) -> String {
    key_name_from_code(code)
        .map(|n| n.to_string())
        .unwrap_or_else(|| code.to_string())
}

/// Resolve a config keymap key: a `KEY_*` name, or a raw numeric code as an
/// escape hatch for remotes emitting codes with no name in `KEY_NAMES`.
fn resolve_key(key: &str) -> Option<u16> {
    key_code_from_name(key).or_else(|| key.parse::<u16>().ok())
}

/// Maps Linux keycodes to actions.
#[derive(Debug, Clone, PartialEq)]
pub struct KeyMap {
    map: HashMap<u16, Action>,
}

impl KeyMap {
    /// The built-in default map.
    ///
    /// This is audiocontrol2's table minus its two broken entries (248 is
    /// `KEY_MICMUTE`, 19 is `KEY_R` -- neither is a play or pause key), plus the
    /// real Linux media keycodes. It is a superset of what actually worked in
    /// audiocontrol2, so no working button regresses.
    pub fn default_map() -> Self {
        let mut map = HashMap::new();
        // From audiocontrol2's default code table.
        map.insert(115, Action::VolumeUp);    // KEY_VOLUMEUP
        map.insert(114, Action::VolumeDown);  // KEY_VOLUMEDOWN
        map.insert(113, Action::Mute);        // KEY_MUTE
        map.insert(105, Action::Previous);    // KEY_LEFT
        map.insert(106, Action::Next);        // KEY_RIGHT
        map.insert(103, Action::Previous);    // KEY_UP
        map.insert(108, Action::Next);        // KEY_DOWN
        map.insert(28, Action::PlayPause);    // KEY_ENTER
        // Real media keys, absent from audiocontrol2.
        map.insert(164, Action::PlayPause);   // KEY_PLAYPAUSE
        map.insert(207, Action::Play);        // KEY_PLAY
        map.insert(119, Action::Pause);       // KEY_PAUSE
        map.insert(163, Action::Next);        // KEY_NEXTSONG
        map.insert(165, Action::Previous);    // KEY_PREVIOUSSONG
        map.insert(166, Action::Stop);        // KEY_STOPCD
        KeyMap { map }
    }

    /// Build from the `keymap` config value.
    ///
    /// A present `keymap` **replaces** the default map rather than merging with
    /// it -- merging would make it impossible to unmap a key. This matches
    /// audiocontrol2. Unresolvable keys and unknown actions are warned and
    /// skipped; they never fail startup.
    pub fn from_config(value: Option<&serde_json::Value>) -> Self {
        let Some(obj) = value.and_then(|v| v.as_object()) else {
            return Self::default_map();
        };

        let mut map = HashMap::new();
        for (key, action_value) in obj {
            let Some(code) = resolve_key(key) else {
                warn!("keyboard: ignoring unknown key '{}' in keymap", key);
                continue;
            };
            let Some(action_str) = action_value.as_str() else {
                warn!("keyboard: ignoring non-string action for key '{}'", key);
                continue;
            };
            let Some(action) = Action::from_action_str(action_str) else {
                warn!(
                    "keyboard: ignoring unknown action '{}' for key '{}'",
                    action_str, key
                );
                continue;
            };
            map.insert(code, action);
        }
        KeyMap { map }
    }

    /// The action bound to a keycode, if any.
    pub fn get(&self, code: u16) -> Option<Action> {
        self.map.get(&code).copied()
    }

    /// All mapped keycodes. Used for device capability matching.
    pub fn codes(&self) -> Vec<u16> {
        self.map.keys().copied().collect()
    }

    /// Number of mapped keys.
    pub fn len(&self) -> usize {
        self.map.len()
    }

    /// Whether the map has no entries.
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inputs::Action;
    use serde_json::json;

    #[test]
    fn test_name_to_code() {
        assert_eq!(key_code_from_name("KEY_VOLUMEUP"), Some(115));
        assert_eq!(key_code_from_name("KEY_PLAYPAUSE"), Some(164));
        assert_eq!(key_code_from_name("KEY_NOT_A_REAL_KEY"), None);
    }

    #[test]
    fn test_code_to_name() {
        assert_eq!(key_name_from_code(115), Some("KEY_VOLUMEUP"));
        assert_eq!(key_name_from_code(60000), None);
    }

    #[test]
    fn test_display_name_known_code() {
        assert_eq!(key_display_name(115), "KEY_VOLUMEUP");
    }

    #[test]
    fn test_display_name_unnamed_code_falls_back_to_number() {
        assert_eq!(key_display_name(190), "190");
    }

    #[test]
    fn test_default_map_matches_spec() {
        let m = KeyMap::default_map();
        assert_eq!(m.get(115), Some(Action::VolumeUp));
        assert_eq!(m.get(114), Some(Action::VolumeDown));
        assert_eq!(m.get(113), Some(Action::Mute));
        assert_eq!(m.get(105), Some(Action::Previous));
        assert_eq!(m.get(106), Some(Action::Next));
        assert_eq!(m.get(103), Some(Action::Previous));
        assert_eq!(m.get(108), Some(Action::Next));
        assert_eq!(m.get(28), Some(Action::PlayPause));
        assert_eq!(m.get(164), Some(Action::PlayPause));
        assert_eq!(m.get(207), Some(Action::Play));
        assert_eq!(m.get(119), Some(Action::Pause));
        assert_eq!(m.get(163), Some(Action::Next));
        assert_eq!(m.get(165), Some(Action::Previous));
        assert_eq!(m.get(166), Some(Action::Stop));
        assert_eq!(m.len(), 14);
    }

    /// The two audiocontrol2 defects: 248 is KEY_MICMUTE and 19 is KEY_R.
    #[test]
    fn test_default_map_drops_broken_ac2_codes() {
        let m = KeyMap::default_map();
        assert_eq!(m.get(248), None);
        assert_eq!(m.get(19), None);
    }

    #[test]
    fn test_from_config_absent_gives_default() {
        assert_eq!(KeyMap::from_config(None), KeyMap::default_map());
    }

    #[test]
    fn test_from_config_replaces_not_merges() {
        let cfg = json!({ "KEY_ENTER": "playpause" });
        let m = KeyMap::from_config(Some(&cfg));
        assert_eq!(m.get(28), Some(Action::PlayPause));
        // KEY_VOLUMEUP was in the default map but must be gone: replace, not merge.
        assert_eq!(m.get(115), None);
        assert_eq!(m.len(), 1);
    }

    #[test]
    fn test_from_config_numeric_fallback() {
        let cfg = json!({ "190": "next" });
        let m = KeyMap::from_config(Some(&cfg));
        assert_eq!(m.get(190), Some(Action::Next));
    }

    #[test]
    fn test_from_config_skips_bad_entries() {
        let cfg = json!({
            "KEY_VOLUMEUP": "volume_up",
            "KEY_BOGUS": "next",          // unresolvable key name
            "KEY_ENTER": "fly_to_moon",   // unknown action
            "99999999": "next"            // out of u16 range
        });
        let m = KeyMap::from_config(Some(&cfg));
        assert_eq!(m.get(115), Some(Action::VolumeUp));
        assert_eq!(m.len(), 1);
    }

    #[test]
    fn test_codes_lists_all_mapped() {
        let cfg = json!({ "KEY_VOLUMEUP": "volume_up", "KEY_ENTER": "playpause" });
        let mut codes = KeyMap::from_config(Some(&cfg)).codes();
        codes.sort();
        assert_eq!(codes, vec![28, 115]);
    }
}
