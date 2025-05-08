/// Player commands that can be sent to media players
use serde::{Serialize, Deserialize};
use strum_macros::EnumString;
use super::LoopMode;

/// Queue-related commands for managing the playback queue
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum QueueCommand {
    /// Add tracks to the queue
    AddTracks {
        /// Track URIs to add to the queue
        uris: Vec<String>,
        /// Whether to insert at beginning (true) or append at end (false)
        insert_at_beginning: bool,
    },
    
    /// Remove a track from the queue by its index
    RemoveTrack(usize),
    
    /// Clear the entire queue
    Clear,
    
    /// Get the current queue contents
    GetQueue,
}

/// Default implementation for QueueCommand
impl Default for QueueCommand {
    fn default() -> Self {
        QueueCommand::GetQueue
    }
}

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

    /// Kill (forcefully terminate) the player
    #[serde(rename = "kill")]
    Kill,
    
    /// Queue-related command
    #[serde(rename = "queue")]
    Queue(QueueCommand),
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
            PlayerCommand::Kill => write!(f, "kill"),
            PlayerCommand::Queue(cmd) => {
                match cmd {
                    QueueCommand::AddTracks { insert_at_beginning, .. } => {
                        if *insert_at_beginning {
                            write!(f, "queue:add_tracks_beginning")
                        } else {
                            write!(f, "queue:add_tracks_end")
                        }
                    },
                    QueueCommand::RemoveTrack(index) => write!(f, "queue:remove_track:{}", index),
                    QueueCommand::Clear => write!(f, "queue:clear"),
                    QueueCommand::GetQueue => write!(f, "queue:get"),
                }
            },
        }
    }
}