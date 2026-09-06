//! `CoreClient` -- the metadata side's client for the player daemon.
//!
//! The now-playing seam is not one-directional. `song_information` is the
//! push half: an enrichment result handed to `POST
//! /player/<name>/song-information`. `current_player` and `now_playing` are
//! the pull half: what the Last.fm worker asks, every 30 s, because a
//! scrobble is timed from how long a track really played and a single missed
//! `StateChanged` would otherwise leave its timer running against a paused
//! player for the rest of the track.
//!
//! `PlaybackStateSource` lives here and not on `MetadataClient`: `GET
//! /player` is a *player daemon* route, while `MetadataClient` addresses
//! `services.metadata.url`, which in Phase 2 is the other daemon and has no
//! `/player`.
use acr_http::http_client;
use acr_types::now_playing::{PlaybackStateSource, SongInformationSink};
use acr_types::{PlaybackState, PlayerSource, Song};

/// A client for the player daemon's HTTP API, from the metadata side.
///
/// `base_url` is the player daemon's API root (e.g. `http://127.0.0.1:1080/api`
/// in Phase 1, where both daemons share a process but not a call stack).
pub struct CoreClient {
    base: String,
    timeout_secs: u64,
}

impl CoreClient {
    pub fn new(base_url: &str) -> Self {
        Self {
            base: base_url.trim_end_matches('/').to_string(),
            timeout_secs: 5,
        }
    }

    /// Push a partial `Song` to `POST /player/<name>/song-information` and
    /// report whether the player's stored song changed.
    ///
    /// The route answers `{"success": bool, "applied": bool}`. `success` is
    /// checked, not just `applied`: a response that reports the request
    /// itself as unsuccessful (player not found, an unusable partial) is a
    /// client-visible error, not a silently-swallowed "not applied".
    pub fn song_information(&self, source: &PlayerSource, partial: &Song) -> Result<bool, String> {
        let url = format!(
            "{}/player/{}/song-information",
            self.base,
            urlencoding::encode(&source.player_name)
        );
        let payload = serde_json::to_value(partial).map_err(|e| e.to_string())?;
        let client = http_client::new_http_client(self.timeout_secs);
        let response = client
            .post_json_value(&url, payload)
            .map_err(|e| e.to_string())?;

        let success = response
            .get("success")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if !success {
            return Err(response
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("the player daemon reported the request as unsuccessful")
                .to_string());
        }

        Ok(response
            .get("applied")
            .and_then(|v| v.as_bool())
            .unwrap_or(false))
    }

    /// The active player's state, read from `GET /player`. The fallible core
    /// behind `PlaybackStateSource` below.
    pub fn current_player(&self) -> Result<PlaybackState, String> {
        let client = http_client::new_http_client(self.timeout_secs);
        let v = client
            .get_json_with_headers(&format!("{}/player", self.base), &[])
            .map_err(|e| e.to_string())?;
        serde_json::from_value(v["state"].clone()).map_err(|e| e.to_string())
    }

    /// What the player is currently playing, read from `GET /now-playing`.
    /// `Ok(None)` is a player with no current song (idle or stopped), not an
    /// error.
    pub fn now_playing(&self) -> Result<Option<(PlayerSource, Song)>, String> {
        let client = http_client::new_http_client(self.timeout_secs);
        let v = client
            .get_json_with_headers(&format!("{}/now-playing", self.base), &[])
            .map_err(|e| e.to_string())?;
        let Some(song) = v.get("song").filter(|s| !s.is_null()) else {
            return Ok(None);
        };
        let song: Song = serde_json::from_value(song.clone()).map_err(|e| e.to_string())?;
        let name = v["player"]["name"].as_str().unwrap_or_default().to_string();
        let id = v["player"]["id"].as_str().unwrap_or_default().to_string();
        Ok(Some((PlayerSource::new(name, id), song)))
    }
}

impl SongInformationSink for CoreClient {
    /// `false` on any error -- unreachable player, a malformed response, or
    /// one that reports itself unsuccessful -- logged at warn level.
    ///
    /// This seam changes at most once per song, not once a second, so one
    /// warning per song change while the player is unreachable is
    /// information rather than noise: a per-minute rate limiter would be
    /// extra state with its own failure modes and its own test for a
    /// scenario this frequency does not create.
    fn apply(&self, source: &PlayerSource, partial: &Song) -> bool {
        match self.song_information(source, partial) {
            Ok(applied) => applied,
            Err(e) => {
                log::warn!(
                    "song information not delivered to the player daemon: {}",
                    e
                );
                false
            }
        }
    }
}

impl PlaybackStateSource for CoreClient {
    /// `PlaybackState::Unknown` on any error: a timeout, a connection
    /// failure or a body that will not parse. The Last.fm worker reads
    /// `Unknown` as "no news" and keeps whatever state it already had, so
    /// this never invents a state the player did not report.
    ///
    /// Logged at debug, not warn: this is polled every 30 s regardless of
    /// whether anything changed, so a warn here would fire on a schedule
    /// rather than on an event, which is exactly the noise `apply` above is
    /// written to avoid creating on its own seam.
    fn playback_state(&self) -> PlaybackState {
        match self.current_player() {
            Ok(state) => state,
            Err(e) => {
                log::debug!(
                    "playback state not readable from the player daemon: {}",
                    e
                );
                PlaybackState::Unknown
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::external_coverart::stub_server::StubServer;

    /// `StubServer` records requests only through the end of the headers
    /// (see its own doc comment: "these requests carry no body"), so the
    /// partial's presence is checked two ways instead of by reading raw
    /// body bytes the stub does not capture: the response round-trips
    /// `applied` correctly, and the recorded `Content-Length` matches the
    /// exact byte length of the partial actually serialized -- which a
    /// request carrying a different body could not produce.
    #[test]
    fn a_song_information_post_carries_the_partial_and_reads_applied() {
        let server = StubServer::serving(200, r#"{"success":true,"applied":true}"#);
        let client = CoreClient::new(&server.base_url());
        let source = PlayerSource::new("mpd".into(), "mpd:1".into());
        let partial = Song {
            title: Some("Nemo".into()),
            ..Default::default()
        };

        assert_eq!(client.song_information(&source, &partial), Ok(true));

        let requests = server.requests();
        assert_eq!(requests.len(), 1, "exactly one request should be sent");
        assert!(
            requests[0].starts_with("POST /player/mpd/song-information HTTP/1.1"),
            "unexpected request line: {}",
            requests[0]
        );
        let expected_len = serde_json::to_string(&partial).unwrap().len();
        assert!(
            requests[0]
                .to_lowercase()
                .contains(&format!("content-length: {}", expected_len)),
            "request should carry a body sized for the partial: {}",
            requests[0]
        );
    }

    #[test]
    fn an_unreachable_core_is_not_a_panic() {
        let client = CoreClient::new("http://127.0.0.1:1/api");
        let source = PlayerSource::new("mpd".into(), "mpd:1".into());
        assert!(client
            .song_information(&source, &Song::default())
            .is_err());
        assert!(!SongInformationSink::apply(&client, &source, &Song::default()));
    }

    /// Task 1's route emits `{"success": bool, "applied": bool}`, but its
    /// own tests assert only `applied`. This pins `success` from the client
    /// side: a route that stopped emitting it (or started emitting `false`)
    /// must surface as an error here, not as a silently-downgraded "not
    /// applied".
    #[test]
    fn a_response_missing_success_is_treated_as_an_error() {
        let server = StubServer::serving(200, r#"{"applied":true}"#);
        let client = CoreClient::new(&server.base_url());
        let source = PlayerSource::new("mpd".into(), "mpd:1".into());
        assert!(client
            .song_information(&source, &Song::default())
            .is_err());
    }

    #[test]
    fn a_response_reporting_failure_is_an_error_not_a_false_applied() {
        let server = StubServer::serving(
            200,
            r#"{"success":false,"applied":true,"message":"player not found"}"#,
        );
        let client = CoreClient::new(&server.base_url());
        let source = PlayerSource::new("mpd".into(), "mpd:1".into());
        let err = client
            .song_information(&source, &Song::default())
            .expect_err("success: false must not read as Ok");
        assert_eq!(err, "player not found");
    }

    /// The pull half: what the Last.fm worker reconciles against.
    #[test]
    fn the_playback_state_is_read_from_the_current_player_route() {
        let server = StubServer::serving(
            200,
            r#"{"name":"mpd","id":"mpd:1","state":"paused","last_seen":null}"#,
        );
        let client = CoreClient::new(&server.base_url());
        assert_eq!(
            PlaybackStateSource::playback_state(&client),
            PlaybackState::Paused
        );
        assert!(server.requests()[0].starts_with("GET /player HTTP/1.1"));
    }

    /// A worker that cannot reach the player must not conclude "playing": it
    /// would scrobble a track nothing is playing. `Unknown` is what the
    /// trait has no way to refuse, and what the worker already treats as "no
    /// news".
    #[test]
    fn an_unreachable_or_unparseable_answer_is_unknown() {
        let dead = CoreClient::new("http://127.0.0.1:1/api");
        assert_eq!(
            PlaybackStateSource::playback_state(&dead),
            PlaybackState::Unknown
        );

        let nonsense = StubServer::serving(200, r#"{"state":"levitating"}"#);
        let client = CoreClient::new(&nonsense.base_url());
        assert_eq!(
            PlaybackStateSource::playback_state(&client),
            PlaybackState::Unknown
        );
    }

    #[test]
    fn now_playing_reads_the_active_song() {
        let server = StubServer::serving(
            200,
            r#"{"player":{"name":"mpd","id":"mpd:1"},"song":{"title":"Nemo"},"state":"playing","shuffle":false,"loop_mode":"none","position":12.5}"#,
        );
        let client = CoreClient::new(&server.base_url());
        let (source, song) = client
            .now_playing()
            .expect("the request should succeed")
            .expect("a song is playing");
        assert_eq!(source, PlayerSource::new("mpd".into(), "mpd:1".into()));
        assert_eq!(song.title.as_deref(), Some("Nemo"));
    }

    #[test]
    fn now_playing_is_none_when_nothing_is_playing() {
        let server = StubServer::serving(
            200,
            r#"{"player":{"name":"none","id":"none"},"song":null,"state":"unknown","shuffle":false,"loop_mode":"none","position":null}"#,
        );
        let client = CoreClient::new(&server.base_url());
        assert_eq!(client.now_playing().expect("the request should succeed"), None);
    }
}
