/// Player commands that can be sent to media players
use serde::{Serialize, Deserialize};
use strum_macros::EnumString;
use super::LoopMode;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, EnumString)]
#[serde(rename_all = "lowercase")]
pub enum PlayerCommand {
    /// Simple playback commands
    #[serde(rename = "play")]
    Play,

    #[serde(rename = "pause")]
    Pause,

    #[serde(rename = "playpause")]
    PlayPause,

    #[serde(rename = "next")]
    Next,

    #[serde(rename = "previous")]
    Previous,

    /// Commands with additional arguments
    #[serde(rename = "set_loop")]
    SetLoopMode(LoopMode),

    #[serde(rename = "seek")]
    Seek(f64),

    #[serde(rename = "set_random")]
    SetRandom(bool),
}

impl Default for PlayerCommand {
    fn default() -> Self {
        PlayerCommand::Play
    }
}

impl std::fmt::Display for PlayerCommand {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PlayerCommand::Play => write!(f, "play"),
            PlayerCommand::Pause => write!(f, "pause"),
            PlayerCommand::PlayPause => write!(f, "playpause"),
            PlayerCommand::Next => write!(f, "next"),
            PlayerCommand::Previous => write!(f, "previous"),
            PlayerCommand::SetLoopMode(mode) => write!(f, "set_loop:{}", mode),
            PlayerCommand::Seek(position) => write!(f, "seek:{}", position),
            PlayerCommand::SetRandom(enabled) => write!(f, "set_random:{}", if *enabled { "on" } else { "off" }),
        }
    }
}