//! Where an outside lookup hands the player daemon a better version of the
//! song it is playing. The policy is `apply_song_information`'s; this route
//! only carries the partial to it.
use crate::data::{PlayerSource, Song};
use crate::AudioController;
use rocket::http::Status;
use rocket::response::status::Custom;
use rocket::serde::json::Json;
use rocket::{post, State};
use std::sync::Arc;

/// Response for a song-information update.
#[derive(serde::Serialize)]
pub struct SongInformationResponse {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub applied: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// Accept a partial `Song` from outside the process and hand it to
/// `AudioController::apply_song_information`, which owns the merge policy:
/// cover art replaces only a placeholder, and `liked`, `metadata` and
/// `cover_art_source` provenance are merged. This route resolves the player
/// exactly like `player_event_update` does, and does not reimplement any of
/// that policy itself.
#[post("/player/<player_name>/song-information", data = "<partial>")]
pub fn song_information(
    player_name: &str,
    partial: Json<Song>,
    controller: &State<Arc<AudioController>>,
) -> Result<Json<SongInformationResponse>, Custom<Json<SongInformationResponse>>> {
    let partial = partial.into_inner();

    if partial.title.is_none() && partial.artist.is_none() {
        return Err(Custom(
            Status::BadRequest,
            Json(SongInformationResponse {
                success: false,
                applied: None,
                message: Some("a partial must carry a title or an artist".to_string()),
            }),
        ));
    }

    let Some(player) = controller.get_player_by_name(player_name) else {
        return Err(Custom(
            Status::NotFound,
            Json(SongInformationResponse {
                success: false,
                applied: None,
                message: Some(format!("Player '{}' not found", player_name)),
            }),
        ));
    };

    let source = {
        let player = player.read();
        PlayerSource::new(player.get_player_name(), player.get_player_id())
    };

    let applied = controller.apply_song_information(&source, &partial);

    Ok(Json(SongInformationResponse {
        success: true,
        applied: Some(applied),
        message: None,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rocket::http::{ContentType, Status};
    use rocket::local::blocking::Client;

    /// One generic player named "test", with a current song set through its
    /// `process_api_event("song_changed")` path -- the same way a real
    /// generic-player deployment gets a song into `AudioController` -- and
    /// only this route mounted, since nothing else is under test.
    fn client_with_generic_player_playing(title: &str, artist: &str) -> Client {
        let config = serde_json::json!({"players": [{"generic": {"name": "test"}}]});
        let controller =
            AudioController::from_json(&config).expect("the configuration should build a controller");

        let player = controller
            .get_player_by_name("test")
            .expect("the configured generic player must exist");
        {
            let player = player.read();
            assert!(player.process_api_event(&serde_json::json!({
                "type": "song_changed",
                "song": { "title": title, "artist": artist }
            })));
        }

        let rocket = rocket::build()
            .manage(controller)
            .mount("/api", rocket::routes![song_information]);
        Client::tracked(rocket).expect("rocket should launch")
    }

    #[test]
    fn a_matching_partial_is_applied() {
        let client = client_with_generic_player_playing("Nemo", "Nightwish");
        let r = client
            .post("/api/player/test/song-information")
            .header(ContentType::JSON)
            .body(r#"{"title":"Nemo","artist":"Nightwish","cover_art_url":"http://x/y.jpg"}"#)
            .dispatch();
        assert_eq!(r.status(), Status::Ok);
        assert_eq!(
            r.into_json::<serde_json::Value>().unwrap()["applied"],
            true
        );
    }

    #[test]
    fn a_stale_partial_is_not_applied_but_is_not_an_error() {
        let client = client_with_generic_player_playing("Nemo", "Nightwish");
        let r = client
            .post("/api/player/test/song-information")
            .header(ContentType::JSON)
            .body(r#"{"title":"Other","artist":"Nightwish","cover_art_url":"http://x/y.jpg"}"#)
            .dispatch();
        assert_eq!(r.status(), Status::Ok);
        assert_eq!(
            r.into_json::<serde_json::Value>().unwrap()["applied"],
            false
        );
    }

    #[test]
    fn a_partial_with_neither_title_nor_artist_is_a_bad_request() {
        let client = client_with_generic_player_playing("Nemo", "Nightwish");
        let r = client
            .post("/api/player/test/song-information")
            .header(ContentType::JSON)
            .body(r#"{"cover_art_url":"http://x/y.jpg"}"#)
            .dispatch();
        assert_eq!(r.status(), Status::BadRequest);
    }

    #[test]
    fn an_unknown_player_is_not_found() {
        let client = client_with_generic_player_playing("Nemo", "Nightwish");
        let r = client
            .post("/api/player/nope/song-information")
            .header(ContentType::JSON)
            .body(r#"{"title":"Nemo"}"#)
            .dispatch();
        assert_eq!(r.status(), Status::NotFound);
    }
}
