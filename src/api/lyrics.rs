use crate::AudioController;
use crate::helpers::lyrics::{LyricsLookup, LyricsContent};
use rocket::serde::json::Json;
use rocket::{get, post, State};
use std::sync::Arc;
use rocket::response::status::Custom;
use rocket::http::Status;
use serde::{Deserialize, Serialize};

/// Request structure for lyrics lookup by metadata
#[derive(Deserialize)]
pub struct LyricsRequest {
    /// Artist name (required)
    pub artist: String,
    /// Song title (required)  
    pub title: String,
    /// Optional song length in seconds for better matching
    pub duration: Option<f64>,
    /// Optional album name for better matching
    pub album: Option<String>,
}

/// Response structure for lyrics
#[derive(Serialize)]
pub struct LyricsResponse {
    /// Whether lyrics were found
    pub found: bool,
    /// The lyrics content
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lyrics: Option<LyricsContentResponse>,
    /// Error message if lyrics could not be retrieved
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Serializable version of LyricsContent
#[derive(Serialize)]
#[serde(tag = "type")]
pub enum LyricsContentResponse {
    #[serde(rename = "plain")]
    PlainText { text: String },
    #[serde(rename = "timed")]
    Timed { lyrics: Vec<TimedLyricResponse> },
}

/// Serializable version of TimedLyric
#[derive(Serialize)]
pub struct TimedLyricResponse {
    /// Timestamp in seconds
    pub timestamp: f64,
    /// Lyrics text (can be empty for timing-only lines)
    pub text: String,
}

impl From<LyricsContent> for LyricsContentResponse {
    fn from(content: LyricsContent) -> Self {
        match content {
            LyricsContent::PlainText(text) => LyricsContentResponse::PlainText { text },
            LyricsContent::Timed(timed_lyrics) => LyricsContentResponse::Timed {
                lyrics: timed_lyrics
                    .into_iter()
                    .map(|lyric| TimedLyricResponse {
                        timestamp: lyric.timestamp,
                        text: lyric.text,
                    })
                    .collect(),
            },
        }
    }
}

/// Get lyrics by song ID (for songs in the MPD database)
/// 
/// GET /api/lyrics/<provider>/<song_id>
#[get("/<provider>/<song_id>")]
pub fn get_lyrics_by_id(
    provider: &str,
    song_id: &str,
    controller: &State<Arc<AudioController>>
) -> Result<Json<LyricsResponse>, Custom<String>> {
    let audio_controller = controller.inner();
    
    // Validate provider
    if provider != "mpd" {
        return Err(Custom(
            Status::BadRequest,
            format!("Unsupported lyrics provider: {}. Currently supported: mpd", provider),
        ));
    }
    
    // Find MPD controller to get lyrics
    let controllers = audio_controller.list_controllers();
    
    for ctrl_lock in controllers {
        let ctrl = ctrl_lock.read();
        // Check if this is an MPD controller with library support
        if ctrl.get_player_name().to_lowercase().contains("mpd") {
            if let Some(library) = ctrl.get_library() {
                // Cast to MPDLibrary to access lyrics methods
                if let Some(mpd_library) = library.as_any().downcast_ref::<crate::players::mpd::library::MPDLibrary>() {
                    // Try to decode the song_id as a base64-encoded file path first
                    match crate::helpers::url_encoding::decode_url_safe(song_id) {
                        Some(decoded_path) => {
                            // Use the decoded file path to get lyrics
                            match mpd_library.get_lyrics_by_url(&decoded_path) {
                                Ok(lyrics) => {
                                    return Ok(Json(LyricsResponse {
                                        found: true,
                                        lyrics: Some(lyrics.into()),
                                        error: None,
                                    }));
                                }
                                Err(crate::helpers::lyrics::LyricsError::NotFound) => {
                                    return Ok(Json(LyricsResponse {
                                        found: false,
                                        lyrics: None,
                                        error: Some("Lyrics not found for this song".to_string()),
                                    }));
                                }
                                Err(e) => {
                                    return Err(Custom(
                                        Status::InternalServerError,
                                        format!("Error retrieving lyrics: {}", e),
                                    ));
                                }
                            }
                        }
                        None => {
                            // If decoding fails, fall back to treating it as a literal song ID
                            match mpd_library.get_lyrics_by_id(song_id) {
                                Ok(lyrics) => {
                                    return Ok(Json(LyricsResponse {
                                        found: true,
                                        lyrics: Some(lyrics.into()),
                                        error: None,
                                    }));
                                }
                                Err(crate::helpers::lyrics::LyricsError::NotFound) => {
                                    return Ok(Json(LyricsResponse {
                                        found: false,
                                        lyrics: None,
                                        error: Some("Lyrics not found for this song".to_string()),
                                    }));
                                }
                                Err(e) => {
                                    return Err(Custom(
                                        Status::InternalServerError,
                                        format!("Error retrieving lyrics: {}", e),
                                    ));
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    Err(Custom(
        Status::NotFound,
        "No MPD player with library support found".to_string(),
    ))
}

/// Get lyrics by artist, title, and optional metadata
///
/// POST /api/lyrics/<provider>
#[post("/<provider>", data = "<request>")]
pub fn get_lyrics_by_metadata(
    provider: &str,
    request: Json<LyricsRequest>,
    controller: &State<Arc<AudioController>>
) -> Result<Json<LyricsResponse>, Custom<String>> {
    let audio_controller = controller.inner();
    let request = request.into_inner();

    // Validate provider
    if provider != "mpd" {
        return Err(Custom(
            Status::BadRequest,
            format!("Unsupported lyrics provider: {}. Currently supported: mpd", provider),
        ));
    }

    // Create lyrics lookup from request
    let mut lookup = LyricsLookup::new(request.artist, request.title);

    if let Some(duration) = request.duration {
        lookup = lookup.with_duration(duration);
    }

    if let Some(album) = request.album {
        lookup = lookup.with_album(album);
    }

    // Find MPD controller to get lyrics
    let controllers = audio_controller.list_controllers();

    for ctrl_lock in controllers {
        let ctrl = ctrl_lock.read();
        // Check if this is an MPD controller with library support
        if ctrl.get_player_name().to_lowercase().contains("mpd") {
            if let Some(library) = ctrl.get_library() {
                // Cast to MPDLibrary to access lyrics methods
                if let Some(mpd_library) = library.as_any().downcast_ref::<crate::players::mpd::library::MPDLibrary>() {
                    match mpd_library.get_lyrics_by_metadata(&lookup) {
                        Ok(lyrics) => {
                            return Ok(Json(LyricsResponse {
                                found: true,
                                lyrics: Some(lyrics.into()),
                                error: None,
                            }));
                        }
                        Err(crate::helpers::lyrics::LyricsError::NotFound) => {
                            return Ok(Json(LyricsResponse {
                                found: false,
                                lyrics: None,
                                error: Some("Lyrics not found for this song".to_string()),
                            }));
                        }
                        Err(e) => {
                            return Err(Custom(
                                Status::InternalServerError,
                                format!("Error retrieving lyrics: {}", e),
                            ));
                        }
                    }
                }
            }
        }
    }
    
    Err(Custom(
        Status::NotFound,
        "No MPD player with library support found".to_string(),
    ))
}
