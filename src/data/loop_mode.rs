/// Loop mode enumeration for playback
use serde::{Serialize, Deserialize};
use strum_macros::EnumString;

// Parsing accepts the wire names (`no`, `song`, `playlist`) that Display and
// serde emit as well as the variant names, in any case: a client has to be
// able to send back the value an event or a state query just gave it.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, EnumString)]
#[serde(rename_all = "lowercase")]
#[strum(ascii_case_insensitive)]
#[derive(Default)]
pub enum LoopMode {
    /// No loop
    #[serde(rename = "no")]
    #[strum(serialize = "no", serialize = "none", serialize = "off")]
    #[default]
    None,
    /// Loop current track/song
    #[serde(rename = "song")]
    #[strum(serialize = "song", serialize = "track")]
    Track,
    /// Loop entire playlist
    #[serde(rename = "playlist")]
    Playlist,
}


impl std::fmt::Display for LoopMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Return the value as a string for backwards compatibility
        match self {
            LoopMode::None => write!(f, "no"),
            LoopMode::Track => write!(f, "song"),
            LoopMode::Playlist => write!(f, "playlist"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn every_mode_parses_back_from_its_own_display_form() {
        for mode in [LoopMode::None, LoopMode::Track, LoopMode::Playlist] {
            assert_eq!(LoopMode::from_str(&mode.to_string()), Ok(mode));
        }
    }

    #[test]
    fn display_and_serde_agree_on_the_wire_names() {
        for (mode, wire) in [
            (LoopMode::None, "no"),
            (LoopMode::Track, "song"),
            (LoopMode::Playlist, "playlist"),
        ] {
            assert_eq!(mode.to_string(), wire);
            assert_eq!(serde_json::to_string(&mode).unwrap(), format!("\"{wire}\""));
            assert_eq!(serde_json::from_str::<LoopMode>(&format!("\"{wire}\"")).unwrap(), mode);
        }
    }

    #[test]
    fn variant_names_and_any_case_still_parse() {
        assert_eq!(LoopMode::from_str("None"), Ok(LoopMode::None));
        assert_eq!(LoopMode::from_str("Track"), Ok(LoopMode::Track));
        assert_eq!(LoopMode::from_str("PLAYLIST"), Ok(LoopMode::Playlist));
        assert_eq!(LoopMode::from_str("Song"), Ok(LoopMode::Track));
        assert!(LoopMode::from_str("sometimes").is_err());
    }
}
