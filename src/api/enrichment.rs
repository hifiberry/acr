//! Where an outside lookup hands a player's library what it learned about the
//! library's artists and albums.
//!
//! The merge rules are the library's — `data::library::apply_batch`, shared by
//! every backend — and the staleness check is the backend's. This route only
//! resolves the player, finds its enrichment sink, and turns the sink's answer
//! into a status code.
use crate::AudioController;
use acr_types::enrichment::{Applied, EnrichmentBatch, EnrichmentError};
use rocket::http::Status;
use rocket::response::status::Custom;
use rocket::serde::json::Json;
use rocket::{post, State};
use std::sync::Arc;

/// A 404 body naming what was not found.
fn not_found(what: &str) -> Custom<Json<serde_json::Value>> {
    Custom(
        Status::NotFound,
        Json(serde_json::json!({ "error": format!("no such {}", what) })),
    )
}

/// Merge one batch of enrichment results into a player's library.
///
/// A batch that names `library_generation` is claiming to have been computed
/// against that generation of the library. If the library has been reloaded
/// since, the batch describes albums and artists that may no longer exist and
/// is refused with 409 rather than merged.
///
/// The 409 carries *both* tokens, because a caller that gets one needs both: the
/// generation to recompute against, and the version for the "seen" bookkeeping
/// it does against `GET /api/library/<p>`.
#[post("/library/<player_name>/enrichment", data = "<batch>")]
pub fn apply_enrichment(
    player_name: &str,
    batch: Json<EnrichmentBatch>,
    controller: &State<Arc<AudioController>>,
) -> Result<Json<Applied>, Custom<Json<serde_json::Value>>> {
    let Some(player) = controller.get_player_by_name(player_name) else {
        return Err(not_found("player"));
    };
    let Some(library) = player.read().get_library() else {
        return Err(not_found("library"));
    };
    let sink = library.as_enrichment_sink().ok_or_else(|| not_found("library"))?;

    match sink.apply(batch.into_inner()) {
        Ok(applied) => Ok(Json(applied)),
        Err(EnrichmentError::Stale { current_generation }) => Err(Custom(
            Status::Conflict,
            Json(serde_json::json!({
                "library_generation": current_generation,
                "library_version": library.library_version(),
            })),
        )),
        Err(EnrichmentError::NoSuchLibrary) => Err(not_found("library")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::library::{
        apply_batch, check_generation, LibraryError, LibraryInterface, LibraryVersion,
    };
    use crate::data::{
        Album, Artist, Identifier, LoopMode, PlaybackState, PlayerCapabilitySet, PlayerCommand,
        Song, Track,
    };
    use crate::players::PlayerController;
    use acr_types::enrichment::EnrichmentSink;
    use parking_lot::{Mutex, RwLock};
    use rocket::http::ContentType;
    use rocket::local::blocking::Client;
    use std::collections::HashMap;

    fn test_album(id: u64, name: &str, artist: &str) -> Album {
        Album {
            id: Identifier::Numeric(id),
            name: name.to_string(),
            artists: Arc::new(Mutex::new(vec![artist.to_string()])),
            artists_flat: None,
            release_date: None,
            tracks: Arc::new(Mutex::new(Vec::new())),
            cover_art: None,
            uri: None,
            genres: Vec::new(),
        }
    }

    fn test_artist(name: &str) -> Artist {
        Artist {
            id: Identifier::String(name.to_string()),
            name: name.to_string(),
            is_multi: false,
            metadata: None,
        }
    }

    /// A library with maps a test can build and read, wired to the *shared*
    /// version counter, staleness check and merge — `LibraryVersion`,
    /// `check_generation` and `apply_batch` — so it refuses and merges a batch
    /// exactly as MPD does rather than by a re-implementation of its own.
    /// `MPDLibrary` cannot be used here: it holds its maps privately and a real
    /// `refresh_library` needs a live MPD. What MPD's own `apply` does is
    /// covered by `players::mpd::library`'s tests; what this covers is the
    /// route.
    #[derive(Clone)]
    struct TestLibrary {
        albums: Arc<RwLock<HashMap<String, Album>>>,
        artists: Arc<RwLock<HashMap<String, Artist>>>,
        version: LibraryVersion,
    }

    impl TestLibrary {
        fn with(albums: Vec<Album>, artists: Vec<Artist>) -> Self {
            TestLibrary {
                albums: Arc::new(RwLock::new(
                    albums.into_iter().map(|a| (a.name.clone(), a)).collect(),
                )),
                artists: Arc::new(RwLock::new(
                    artists.into_iter().map(|a| (a.name.clone(), a)).collect(),
                )),
                version: LibraryVersion::new(),
            }
        }
    }

    impl EnrichmentSink for TestLibrary {
        fn apply(
            &self,
            batch: acr_types::enrichment::EnrichmentBatch,
        ) -> Result<Applied, EnrichmentError> {
            check_generation(&batch, self.library_generation())?;
            let (mut applied, changed) = apply_batch(&self.albums, &self.artists, &batch);
            if changed {
                self.version.bump();
            }
            applied.library_version = self.library_version();
            Ok(applied)
        }
    }

    impl LibraryInterface for TestLibrary {
        fn new() -> Self {
            TestLibrary::with(Vec::new(), Vec::new())
        }
        fn is_loaded(&self) -> bool {
            true
        }
        fn refresh_library(&self) -> Result<(), LibraryError> {
            Ok(())
        }
        fn get_albums(&self) -> Vec<Album> {
            self.albums.read().values().cloned().collect()
        }
        fn get_artists(&self) -> Vec<Artist> {
            self.artists.read().values().cloned().collect()
        }
        fn get_album_by_artist_and_name(&self, _artist: &str, _album: &str) -> Option<Album> {
            None
        }
        fn get_album_by_id(&self, _id: &Identifier) -> Option<Album> {
            None
        }
        fn get_artist_by_name(&self, name: &str) -> Option<Artist> {
            self.artists.read().get(name).cloned()
        }
        fn get_albums_by_artist_id(&self, _artist_id: &Identifier) -> Vec<Album> {
            Vec::new()
        }
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
        fn get_image(&self, _identifier: String) -> Option<(Vec<u8>, String)> {
            None
        }
        fn library_version(&self) -> Option<String> {
            Some(self.version.token())
        }
        fn library_generation(&self) -> Option<String> {
            Some(self.version.generation_token())
        }
        fn as_enrichment_sink(&self) -> Option<&dyn EnrichmentSink> {
            Some(self)
        }
    }

    /// The smallest player that owns a library, named "mpd" so the paths in
    /// these tests read as a real deployment's.
    struct LibraryPlayer(TestLibrary);

    impl PlayerController for LibraryPlayer {
        fn get_capabilities(&self) -> PlayerCapabilitySet {
            PlayerCapabilitySet::empty()
        }
        fn get_song(&self) -> Option<Song> {
            None
        }
        fn get_queue(&self) -> Vec<Track> {
            Vec::new()
        }
        fn get_loop_mode(&self) -> LoopMode {
            LoopMode::None
        }
        fn get_playback_state(&self) -> PlaybackState {
            PlaybackState::Stopped
        }
        fn get_position(&self) -> Option<f64> {
            None
        }
        fn get_shuffle(&self) -> bool {
            false
        }
        fn get_player_name(&self) -> String {
            "mpd".to_string()
        }
        fn get_player_id(&self) -> String {
            "mpd".to_string()
        }
        fn get_last_seen(&self) -> Option<std::time::SystemTime> {
            None
        }
        fn send_command(&self, _command: PlayerCommand) -> bool {
            true
        }
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
        fn start(&self) -> bool {
            true
        }
        fn stop(&self) -> bool {
            true
        }
        fn get_library(&self) -> Option<Box<dyn LibraryInterface>> {
            Some(Box::new(self.0.clone()))
        }
    }

    /// A Rocket client with one player whose library holds the given albums and
    /// artists, plus a handle on that library so a test can read its tokens and
    /// its maps. The handle shares the player's maps and counters.
    fn client_with_library(albums: Vec<Album>, artists: Vec<Artist>) -> (Client, TestLibrary) {
        let library = TestLibrary::with(albums, artists);
        let mut controller = AudioController::new();
        controller.add_controller(Box::new(LibraryPlayer(library.clone())));
        let rocket = rocket::build()
            .manage(Arc::new(controller))
            .mount("/api", rocket::routes![apply_enrichment]);
        (
            Client::tracked(rocket).expect("rocket should launch"),
            library,
        )
    }

    fn post<'c>(
        client: &'c Client,
        path: &str,
        body: String,
    ) -> rocket::local::blocking::LocalResponse<'c> {
        client
            .post(path.to_string())
            .header(ContentType::JSON)
            .body(body)
            .dispatch()
    }

    #[test]
    fn a_batch_is_applied_and_the_new_version_returned() {
        let (client, lib) = client_with_library(
            vec![test_album(1, "Abbey Road", "The Beatles")],
            vec![],
        );
        let (generation, version) = (
            lib.library_generation().unwrap(),
            lib.library_version().unwrap(),
        );

        let r = post(
            &client,
            "/api/library/mpd/enrichment",
            format!(
                r#"{{"library_generation":"{}","albums":[{{"id":"1","genres":["rock"]}}]}}"#,
                generation
            ),
        );
        assert_eq!(r.status(), Status::Ok);

        let body = r.into_json::<serde_json::Value>().unwrap();
        assert_eq!(body["albums"], 1);
        assert_ne!(
            body["library_version"], version,
            "the caller is handed the version its own batch produced"
        );
        assert_eq!(
            body["library_version"],
            serde_json::Value::from(lib.library_version()),
            "and it is the library's current one"
        );
        assert_eq!(lib.albums.read()["Abbey Road"].genres, vec!["rock"]);
    }

    /// A batch naming no generation makes no claim, and is applied. This is the
    /// backend-reports-`None` case (LMS today) reaching the route.
    #[test]
    fn a_batch_naming_no_generation_is_applied() {
        let (client, lib) = client_with_library(
            vec![test_album(1, "Abbey Road", "The Beatles")],
            vec![],
        );

        let r = post(
            &client,
            "/api/library/mpd/enrichment",
            r#"{"albums":[{"id":"1","genres":["rock"]}]}"#.to_string(),
        );
        assert_eq!(r.status(), Status::Ok);
        assert_eq!(lib.albums.read()["Abbey Road"].genres, vec!["rock"]);
    }

    /// Both halves of a sweep reach the library through the route, one after the
    /// other, naming the same generation — because a merge does not move it.
    /// This is the regression the generation exists for: before it, the album
    /// half was refused because the artist half had bumped the version, and
    /// album genres stopped after one batch on any library that had artists.
    #[test]
    fn artists_and_albums_from_one_sweep_are_both_applied() {
        let (client, lib) = client_with_library(
            vec![test_album(1, "Abbey Road", "The Beatles")],
            vec![test_artist("The Beatles")],
        );
        let generation = lib.library_generation().unwrap();

        for body in [
            format!(
                r#"{{"library_generation":"{}","artists":[{{"name":"The Beatles","genres":["rock"]}}]}}"#,
                generation
            ),
            format!(
                r#"{{"library_generation":"{}","albums":[{{"id":"1","genres":["rock"]}}]}}"#,
                generation
            ),
        ] {
            let r = post(&client, "/api/library/mpd/enrichment", body);
            assert_eq!(
                r.status(),
                Status::Ok,
                "neither half of a sweep may be refused because of the other"
            );
        }

        assert_eq!(lib.albums.read()["Abbey Road"].genres, vec!["rock"]);
        assert_eq!(
            lib.artists.read()["The Beatles"]
                .metadata
                .as_ref()
                .unwrap()
                .genres,
            vec!["rock"]
        );
    }

    #[test]
    fn a_stale_batch_is_a_conflict_carrying_the_current_generation() {
        let (client, lib) = client_with_library(
            vec![test_album(1, "Abbey Road", "The Beatles")],
            vec![],
        );

        let r = post(
            &client,
            "/api/library/mpd/enrichment",
            r#"{"library_generation":"old","albums":[{"id":"1","genres":["rock"]}]}"#.to_string(),
        );
        assert_eq!(r.status(), Status::Conflict);

        let body = r.into_json::<serde_json::Value>().unwrap();
        assert_eq!(body["library_generation"], lib.library_generation().unwrap());
        assert_eq!(
            body["library_version"],
            lib.library_version().unwrap(),
            "the caller needs the version too: it re-pulls the library and \
             records what it has seen"
        );
        assert!(
            lib.albums.read()["Abbey Road"].genres.is_empty(),
            "and nothing was merged"
        );
    }

    /// The refusal on a real reload, not just on a token that was never
    /// current: the caller holds the generation it computed against, the
    /// library is rebuilt in the meantime, and the batch it posts afterwards is
    /// refused and told the new generation.
    #[test]
    fn a_batch_computed_before_a_reload_is_refused_by_the_route() {
        let (client, lib) = client_with_library(
            vec![test_album(1, "Abbey Road", "The Beatles")],
            vec![],
        );
        let held = lib.library_generation().unwrap();

        // What `refresh_library` does where it clears the maps.
        lib.version.bump_generation();

        let r = post(
            &client,
            "/api/library/mpd/enrichment",
            format!(
                r#"{{"library_generation":"{}","albums":[{{"id":"1","genres":["rock"]}}]}}"#,
                held
            ),
        );
        assert_eq!(r.status(), Status::Conflict);
        let body = r.into_json::<serde_json::Value>().unwrap();
        assert_eq!(body["library_generation"], lib.library_generation().unwrap());
        assert_ne!(body["library_generation"], held);
        assert!(lib.albums.read()["Abbey Road"].genres.is_empty());
    }

    /// A batch naming a field the route does not know is refused, not parsed
    /// with the field dropped.
    ///
    /// `library_version` is the one that matters: it is the *other* token in
    /// this exchange, so it is what a caller writes by mistake, and every
    /// field of a batch is optional. Without the refusal this body would parse
    /// into a batch claiming no generation at all, which the route applies
    /// unchecked — the staleness check defeated by a typo. `doc/api.md`
    /// documents the 422; this is what holds it to it.
    #[test]
    fn a_batch_naming_an_unknown_field_is_refused_rather_than_applied() {
        let (client, lib) = client_with_library(
            vec![test_album(1, "Abbey Road", "The Beatles")],
            vec![],
        );

        let r = post(
            &client,
            "/api/library/mpd/enrichment",
            r#"{"library_version":"whatever","albums":[{"id":"1","genres":["rock"]}]}"#.to_string(),
        );
        assert_eq!(r.status(), Status::UnprocessableEntity);
        assert!(
            lib.albums.read()["Abbey Road"].genres.is_empty(),
            "and nothing was merged from a batch that claimed nothing"
        );
    }

    #[test]
    fn an_unknown_player_is_not_found() {
        let (client, _lib) = client_with_library(vec![], vec![]);
        let r = post(&client, "/api/library/nope/enrichment", "{}".to_string());
        assert_eq!(r.status(), Status::NotFound);
    }

    /// A player that has no library at all: the route must not answer 200 for a
    /// batch nothing could have merged.
    #[test]
    fn a_player_without_a_library_is_not_found() {
        let config = serde_json::json!({"players": [{"generic": {"name": "test"}}]});
        let controller =
            AudioController::from_json(&config).expect("the configuration should build a controller");
        let rocket = rocket::build()
            .manage(controller)
            .mount("/api", rocket::routes![apply_enrichment]);
        let client = Client::tracked(rocket).expect("rocket should launch");

        let r = post(&client, "/api/library/test/enrichment", "{}".to_string());
        assert_eq!(r.status(), Status::NotFound);
    }
}
