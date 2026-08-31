//! REST API for per-station artist/title splitting.
//!
//! Radio streams announce a single combined title. The server splits it into
//! artist and song, guessing the order from MusicBrainz and learning per
//! station. The guess can be wrong, and on a device without internet access it
//! cannot be made at all — so the order and separator can also be set outright,
//! per station, and a set value wins over anything guessed or learned.

use crate::helpers::songsplitmanager::SplitterState;
use crate::helpers::songtitlesplitter::OrderResult;
use crate::players::mpd::MPDPlayerController;
use crate::AudioController;
use rocket::http::Status;
use rocket::response::status::Custom;
use rocket::serde::json::Json;
use rocket::{delete, get, post, State};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Separators the splitter recognises.
const SUPPORTED_SEPARATORS: [char; 3] = ['-', '/', ':'];

/// Wire name of an order.
fn order_name(order: &OrderResult) -> &'static str {
    match order {
        OrderResult::ArtistSong => "artist_song",
        OrderResult::SongArtist => "song_artist",
        OrderResult::Unknown => "unknown",
        OrderResult::Undecided => "undecided",
    }
}

/// Parse the wire name of an order that a client may set.
///
/// `unknown` and `undecided` are outcomes of detection rather than readings of
/// a title, so they are reported but cannot be set.
fn parse_order(value: &str) -> Result<OrderResult, String> {
    match value {
        "artist_song" => Ok(OrderResult::ArtistSong),
        "song_artist" => Ok(OrderResult::SongArtist),
        other => Err(format!(
            "unknown order '{}', expected artist_song or song_artist",
            other
        )),
    }
}

/// Parse a separator, which must be exactly one of the supported characters.
fn parse_separator(value: &str) -> Result<char, String> {
    let mut chars = value.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) if SUPPORTED_SEPARATORS.contains(&c) => Ok(c),
        _ => Err(format!(
            "unsupported separator '{}', expected one of {:?}",
            value, SUPPORTED_SEPARATORS
        )),
    }
}

/// One station's splitting state.
#[derive(Serialize)]
pub struct SplitterResponse {
    /// Stream URL the splitter belongs to
    pub station: String,
    /// Order set explicitly, if any
    pub order: Option<String>,
    /// Separator set explicitly, if any
    pub separator: Option<String>,
    /// Order established by learning, if any
    pub learned_order: Option<String>,
    /// Separator established by learning, if any
    pub learned_separator: Option<String>,
    /// Titles read as "artist - song"
    pub artist_song_count: u32,
    /// Titles read as "song - artist"
    pub song_artist_count: u32,
    /// Lookups that found neither reading
    pub unknown_count: u32,
    /// Lookups that found both readings
    pub undecided_count: u32,
}

impl From<SplitterState> for SplitterResponse {
    fn from(state: SplitterState) -> Self {
        SplitterResponse {
            station: state.id,
            order: state.forced_order.as_ref().map(|o| order_name(o).to_string()),
            separator: state.forced_separator.map(|c| c.to_string()),
            learned_order: state.learned_order.as_ref().map(|o| order_name(o).to_string()),
            learned_separator: state.learned_separator.map(|c| c.to_string()),
            artist_song_count: state.artist_song_count,
            song_artist_count: state.song_artist_count,
            unknown_count: state.unknown_count,
            undecided_count: state.undecided_count,
        }
    }
}

/// All stations with a splitter.
#[derive(Serialize)]
pub struct SplittersResponse {
    pub player_name: String,
    pub count: usize,
    pub splitters: Vec<SplitterResponse>,
}

/// Body of a request setting a station's split.
///
/// Both fields are replaced together, so an omitted field clears that setting
/// and returns the station to guessing.
#[derive(Deserialize)]
pub struct SetSplitterRequest {
    #[serde(default)]
    pub order: Option<String>,
    #[serde(default)]
    pub separator: Option<String>,
}

/// Find an MPD controller by player name.
///
/// Splitting is MPD-only: it is the backend that plays radio streams and the
/// only one holding a splitter manager.
fn with_mpd_controller<T>(
    controller: &State<Arc<AudioController>>,
    player_name: &str,
    f: impl FnOnce(&MPDPlayerController) -> T,
) -> Result<T, Custom<String>> {
    for ctrl_lock in controller.inner().list_controllers() {
        let ctrl = ctrl_lock.read();
        if ctrl.get_player_name() != player_name {
            continue;
        }
        return match ctrl.as_any().downcast_ref::<MPDPlayerController>() {
            Some(mpd) => Ok(f(mpd)),
            None => Err(Custom(
                Status::BadRequest,
                format!("Player '{}' does not split stream titles", player_name),
            )),
        };
    }

    Err(Custom(
        Status::NotFound,
        format!("Player '{}' not found", player_name),
    ))
}

/// Decode a base64url-encoded station URL.
fn decode_station(station: &str) -> Result<String, Custom<String>> {
    crate::helpers::url_encoding::decode_url_safe(station).ok_or_else(|| {
        Custom(
            Status::BadRequest,
            "Station must be a URL-safe base64 encoded stream URL".to_string(),
        )
    })
}

/// List every station this player currently holds a splitter for.
#[get("/player/<player_name>/splitters")]
pub fn list_splitters(
    player_name: &str,
    controller: &State<Arc<AudioController>>,
) -> Result<Json<SplittersResponse>, Custom<String>> {
    let states = with_mpd_controller(controller, player_name, |mpd| mpd.get_all_splitter_states())?;

    Ok(Json(SplittersResponse {
        player_name: player_name.to_string(),
        count: states.len(),
        splitters: states.into_iter().map(SplitterResponse::from).collect(),
    }))
}

/// Report one station's splitting state.
#[get("/player/<player_name>/splitter/<station>")]
pub fn get_splitter(
    player_name: &str,
    station: &str,
    controller: &State<Arc<AudioController>>,
) -> Result<Json<SplitterResponse>, Custom<String>> {
    let url = decode_station(station)?;
    let state = with_mpd_controller(controller, player_name, |mpd| mpd.get_splitter_state(&url))?;

    match state {
        Some(state) => Ok(Json(SplitterResponse::from(state))),
        None => Err(Custom(
            Status::NotFound,
            format!("No splitter for station '{}'", url),
        )),
    }
}

/// Set — or clear — a station's order and separator.
#[post("/player/<player_name>/splitter/<station>", data = "<request>")]
pub fn set_splitter(
    player_name: &str,
    station: &str,
    request: Json<SetSplitterRequest>,
    controller: &State<Arc<AudioController>>,
) -> Result<Json<SplitterResponse>, Custom<String>> {
    let url = decode_station(station)?;

    let order = match &request.order {
        Some(value) => Some(parse_order(value).map_err(|e| Custom(Status::BadRequest, e))?),
        None => None,
    };
    let separator = match &request.separator {
        Some(value) => Some(parse_separator(value).map_err(|e| Custom(Status::BadRequest, e))?),
        None => None,
    };

    let outcome = with_mpd_controller(controller, player_name, |mpd| {
        mpd.set_splitter_forced(&url, order, separator)
            .map(|state| (state, mpd.save_title_splitter(&url)))
    })?;

    match outcome {
        Some((state, Ok(()))) => Ok(Json(SplitterResponse::from(state))),
        Some((_, Err(e))) => Err(Custom(
            Status::InternalServerError,
            format!("Setting applied but could not be saved: {}", e),
        )),
        None => Err(Custom(
            Status::InsufficientStorage,
            "No splitter slot available for this station".to_string(),
        )),
    }
}

/// Forget a station's splitter, discarding both what was learned and what was set.
#[delete("/player/<player_name>/splitter/<station>")]
pub fn delete_splitter(
    player_name: &str,
    station: &str,
    controller: &State<Arc<AudioController>>,
) -> Result<Status, Custom<String>> {
    let url = decode_station(station)?;
    let removed = with_mpd_controller(controller, player_name, |mpd| mpd.remove_title_splitter(&url))?;

    if removed {
        Ok(Status::NoContent)
    } else {
        Err(Custom(
            Status::NotFound,
            format!("No splitter for station '{}'", url),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::helpers::songtitlesplitter::OrderResult;

    #[test]
    fn the_two_settable_orders_parse() {
        assert_eq!(parse_order("artist_song"), Ok(OrderResult::ArtistSong));
        assert_eq!(parse_order("song_artist"), Ok(OrderResult::SongArtist));
    }

    /// `unknown` and `undecided` are outcomes of detection, not readings of a
    /// title. Accepting them would set a station to an order that cannot split.
    #[test]
    fn detection_outcomes_are_not_settable_orders() {
        assert!(parse_order("unknown").is_err());
        assert!(parse_order("undecided").is_err());
    }

    #[test]
    fn an_unrecognised_order_is_rejected() {
        assert!(parse_order("").is_err());
        assert!(parse_order("artist-song").is_err());
    }

    #[test]
    fn order_names_round_trip() {
        for order in [OrderResult::ArtistSong, OrderResult::SongArtist] {
            assert_eq!(parse_order(order_name(&order)), Ok(order));
        }
    }

    /// The splitter only ever looks for these three characters, so accepting
    /// anything else would store a setting that silently never applies.
    #[test]
    fn only_the_supported_separators_are_accepted() {
        assert_eq!(parse_separator("-"), Ok('-'));
        assert_eq!(parse_separator("/"), Ok('/'));
        assert_eq!(parse_separator(":"), Ok(':'));
        assert!(parse_separator("|").is_err());
    }

    #[test]
    fn a_separator_must_be_a_single_character() {
        assert!(parse_separator("").is_err());
        assert!(parse_separator(" - ").is_err());
    }
}
