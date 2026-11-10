use rocket::serde::json::Json;
use rocket::{get};
use rocket::response::status::Custom;
use rocket::http::Status;
use serde::Serialize;
use crate::theaudiodb;

/// Response structure for TheAudioDB lookup
#[derive(Serialize)]
pub struct TheAudioDbResponse {
    mbid: String,
    success: bool,
    data: Option<serde_json::Value>,
    error: Option<String>,
}

/// API endpoint to lookup artist information from TheAudioDB by MusicBrainz ID
/// 
/// This endpoint is primarily used for integration testing to verify that the
/// TheAudioDB module is working correctly.
/// 
/// # Path Parameters
/// * `mbid` - MusicBrainz ID of the artist to look up
/// 
/// # Returns
/// * 200 OK with artist data if found
/// * 400 Bad Request if `mbid` is not a MusicBrainz ID
/// * 404 Not Found if artist not found
/// * 503 Service Unavailable if TheAudioDB is disabled
/// * 500 Internal Server Error for other errors
#[get("/audiodb/mbid/<mbid>")]
pub fn lookup_artist_by_mbid(mbid: String) -> Result<Json<TheAudioDbResponse>, Custom<Json<TheAudioDbResponse>>> {
    // Refused before anything else looks at it: the value is interpolated
    // into the upstream URL and into cache keys, and only an MBID belongs in
    // either.
    if !crate::musicbrainz::is_mbid(&mbid) {
        return Err(Custom(
            Status::BadRequest,
            Json(TheAudioDbResponse {
                mbid,
                success: false,
                data: None,
                error: Some("Not a valid MusicBrainz ID".to_string()),
            })
        ));
    }

    // Check if TheAudioDB is enabled
    if !theaudiodb::is_enabled() {
        return Err(Custom(
            Status::ServiceUnavailable,
            Json(TheAudioDbResponse {
                mbid: mbid.clone(),
                success: false,
                data: None,
                error: Some("TheAudioDB lookups are disabled".to_string()),
            })
        ));
    }

    // Perform the lookup
    match theaudiodb::lookup_theaudiodb_by_mbid(&mbid) {
        Ok(artist_data) => {
            Ok(Json(TheAudioDbResponse {
                mbid,
                success: true,
                data: Some(artist_data),
                error: None,
            }))
        }
        Err(e) => {
            // Check if it's a "not found" error
            if e.contains("No artist found") {
                Err(Custom(
                    Status::NotFound,
                    Json(TheAudioDbResponse {
                        mbid,
                        success: false,
                        data: None,
                        error: Some(e),
                    })
                ))
            } else {
                // Other errors (API key, network, etc.)
                Err(Custom(
                    Status::InternalServerError,
                    Json(TheAudioDbResponse {
                        mbid,
                        success: false,
                        data: None,
                        error: Some(e),
                    })
                ))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Anything that is not an MBID is refused before it can reach the
    /// upstream URL or a cache key, whether or not lookups are enabled.
    #[test]
    fn a_path_segment_that_is_not_an_mbid_is_a_bad_request() {
        for bad in ["x", "../../search.php?s=a", "5b11f4ce-a62d-471e-81fc-a69a8278c7da&x=1"] {
            match lookup_artist_by_mbid(bad.to_string()) {
                Err(err) => assert_eq!(err.0, Status::BadRequest, "for {:?}", bad),
                Ok(_) => panic!("{:?} must be refused", bad),
            }
        }
    }
}
