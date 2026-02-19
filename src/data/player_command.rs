/// Player commands that can be sent to media players
use serde::{Serialize, Deserialize};
use strum_macros::EnumString;
use super::LoopMode;

/// Metadata for tracks being added to the queue
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct QueueTrackMetadata {
    pub metadata: std::collections::HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, EnumString)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum PlayerCommand {
    /// Simple playback commands
    #[serde(rename = "play")]
    #[default]
    Play,

    #[serde(rename = "pause")]
    Pause,

    #[serde(rename = "playpause")]
    PlayPause,

    #[serde(rename = "stop")]
    Stop,

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
    
    /// Queue commands
    #[serde(rename = "queue_tracks")]
    QueueTracks {
        /// Track URIs to add to the queue
        uris: Vec<String>,
        /// Whether to insert at beginning (true) or append at end (false)
        insert_at_beginning: bool,
        /// Optional metadata for each URI (title and cover art URL)
        #[serde(default)]
        metadata: Vec<Option<QueueTrackMetadata>>,
    },
      #[serde(rename = "remove_track")]
    RemoveTrack(usize), // Changed from String to usize for position-based removal
    
    #[serde(rename = "clear_queue")]
    ClearQueue,
    
    #[serde(rename = "play_queue_index")]
    PlayQueueIndex(usize), // Play specific track in the queue by its index
}


impl std::fmt::Display for PlayerCommand {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PlayerCommand::Play => write!(f, "play"),
            PlayerCommand::Pause => write!(f, "pause"),
            PlayerCommand::PlayPause => write!(f, "playpause"),
            PlayerCommand::Stop => write!(f, "stop"),
            PlayerCommand::Next => write!(f, "next"),
            PlayerCommand::Previous => write!(f, "previous"),
            PlayerCommand::SetLoopMode(mode) => write!(f, "set_loop:{}", mode),
            PlayerCommand::Seek(position) => write!(f, "seek:{}", position),
            PlayerCommand::SetRandom(enabled) => write!(f, "set_random:{}", if *enabled { "on" } else { "off" }),
            PlayerCommand::Kill => write!(f, "kill"),
            PlayerCommand::QueueTracks { insert_at_beginning, .. } => {
                if *insert_at_beginning {
                    write!(f, "queue_tracks_beginning")
                } else {
                    write!(f, "queue_tracks_end")
                }
            },            PlayerCommand::RemoveTrack(position) => write!(f, "remove_track:{}", position),
            PlayerCommand::ClearQueue => write!(f, "clear_queue"),
            PlayerCommand::PlayQueueIndex(index) => write!(f, "play_queue_index:{}", index),
        }
    }
}