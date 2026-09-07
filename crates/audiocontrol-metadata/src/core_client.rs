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
use acr_types::enrichment::{
    AlbumRef, Applied, ArtistRef, EnrichmentBatch, EnrichmentError,
};
use acr_types::now_playing::{PlaybackStateSource, SongInformationSink, SplitterObservationSink};
use acr_types::token::AccessTokenSource;
use acr_types::url_encoding::encode_url_safe;
use acr_types::{OrderResult, PlaybackState, PlayerSource, Song};
use parking_lot::Mutex;
use serde::Deserialize;
use std::time::{Duration, Instant};

/// How long a fetched Spotify access token is reused before asking again.
///
/// Not the token's own expiry -- the player daemon refreshes before handing
/// one out -- but a bound on two things: how often this side asks, and how
/// long a stale answer survives an account being unlinked in between. A token
/// that outlived that fails at Spotify with its own 401 regardless, so the TTL
/// is about traffic, not correctness.
const SPOTIFY_TOKEN_TTL: Duration = Duration::from_secs(60);

/// One player as `GET /library` lists it.
///
/// Only the three fields the puller filters on are read; the route also serves
/// `player_id` and `supports_delete`, which are no business of this side.
#[derive(Debug, Clone, Deserialize)]
pub struct LibraryPlayer {
    pub player_name: String,
    #[serde(default)]
    pub has_library: bool,
    #[serde(default)]
    pub is_loaded: bool,
}

/// What `GET /library/<p>` says about one player's library.
///
/// The two tokens do different jobs and are both carried. `library_version`
/// moves on every change a client can observe, this side's own merges
/// included, and is what the "have I enriched this already?" comparison uses.
/// `library_generation` moves only when the library is rebuilt, and is what a
/// batch names so the player daemon can refuse work computed against a library
/// that no longer exists. Either may be absent, from a backend that tracks
/// neither.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct LibraryDetail {
    #[serde(default)]
    pub has_library: bool,
    #[serde(default)]
    pub is_loaded: bool,
    #[serde(default)]
    pub library_version: Option<String>,
    #[serde(default)]
    pub library_generation: Option<String>,
}

/// One album as `GET /library/<p>/albums` lists it.
///
/// `genres` comes along because it is what decides whether the album is worth
/// looking up at all; the caller filters on it rather than this client, so a
/// second caller with a different rule does not have to work around one baked
/// in here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibraryAlbum {
    pub album: AlbumRef,
    pub genres: Vec<String>,
}

/// Why a batch did not merge.
///
/// [`EnrichmentError`] alone cannot say this: its two variants are both the
/// *library* speaking, and a network that dropped the request is a different
/// thing that must not be logged as "the library is gone". The distinction is
/// the caller's to collapse — [`EnrichmentSink::apply`] has only the two
/// variants to return — but it should collapse it knowing which happened.
///
/// [`EnrichmentSink::apply`]: acr_types::enrichment::EnrichmentSink::apply
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EnrichmentPostError {
    /// The library refused the batch: a 409 (it has since reloaded) or a 404
    /// (there is no such player, or it has no library).
    Refused(EnrichmentError),
    /// The exchange did not happen, or its answer made no sense: a transport
    /// failure, an unparseable body, or a status the route does not document.
    Failed(String),
}

impl std::fmt::Display for EnrichmentPostError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EnrichmentPostError::Refused(EnrichmentError::Stale { current_generation }) => write!(
                f,
                "the library was reloaded while this batch was computed (generation is now {:?})",
                current_generation
            ),
            EnrichmentPostError::Refused(EnrichmentError::NoSuchLibrary) => {
                write!(f, "there is no such player, or it has no library")
            }
            EnrichmentPostError::Failed(reason) => write!(f, "{}", reason),
        }
    }
}

/// A client for the player daemon's HTTP API, from the metadata side.
///
/// `base_url` is the player daemon's API root (e.g. `http://127.0.0.1:1080/api`
/// in Phase 1, where both daemons share a process but not a call stack).
pub struct CoreClient {
    base: String,
    timeout_secs: u64,
    /// The last Spotify access token read from the player daemon, and when.
    spotify_token: Mutex<Option<(Option<String>, Instant)>>,
}

impl CoreClient {
    pub fn new(base_url: &str) -> Self {
        Self {
            base: base_url.trim_end_matches('/').to_string(),
            timeout_secs: 5,
            spotify_token: Mutex::new(None),
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

    /// Report an observed title order to a station's splitter, through
    /// `POST /player/<name>/splitter/<station>/observation`.
    ///
    /// `station` is the un-encoded stream URL; this method applies the same
    /// URL-safe base64 encoding `<station>` uses everywhere else in that
    /// API, and the player daemon's route reverses it. The route feeds only
    /// what the station has *learned* — an order a user set explicitly is
    /// never touched by this call, whatever it reports.
    pub fn splitter_observation(
        &self,
        player_name: &str,
        station: &str,
        order: OrderResult,
    ) -> Result<(), String> {
        let url = format!(
            "{}/player/{}/splitter/{}/observation",
            self.base,
            urlencoding::encode(player_name),
            encode_url_safe(station)
        );
        let payload = serde_json::json!({ "order": crate::api::resolve::order_name(order) });
        let client = http_client::new_http_client(self.timeout_secs);
        client
            .post_json_value(&url, payload)
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    /// The player daemon's version string, read from `GET /version`.
    ///
    /// The cheapest route the daemon serves that takes no parameters and
    /// touches no player, which is what makes it the liveness probe
    /// `startup::start_after_core_is_listening` waits on. The version itself
    /// is only logged; nothing branches on it, and nothing should -- this
    /// side's compatibility with the player daemon is a property of the
    /// routes it calls, not of a number.
    pub fn version(&self) -> Result<String, String> {
        let v = self.get("/version")?;
        v.get("version")
            .and_then(|s| s.as_str())
            .map(str::to_string)
            .ok_or_else(|| format!("no version in the answer to GET /version: {}", v))
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

    /// The Spotify bearer token, from the player daemon that owns the account.
    ///
    /// Cached for 60 s: this bounds both how often it is fetched and how long
    /// a token survives an account being unlinked. A stale token fails at
    /// Spotify with its own 401, so the TTL is about traffic, not correctness.
    ///
    /// This is connection ④ of the one-way seam, and the direction is the
    /// whole point. Before the account moved, the player daemon called the
    /// metadata daemon for this token on every playback command, so a metadata
    /// half that was down stopped playback. Now the account is in the player
    /// daemon and this side asks *it* -- and getting no answer costs only
    /// cover art and favourites, which is what the metadata half is allowed to
    /// cost.
    ///
    /// `GET /spotify/access_token` answers 404 specifically when no account is
    /// linked, but `get_text` collapses every non-2xx status into the same
    /// error with the code discarded, so a 404 is indistinguishable here from
    /// the player daemon being unreachable. Sniffing `"404"` out of an error
    /// string would work today and break the moment its wording changed, so
    /// every failure is treated identically: answer `None` and leave whatever
    /// is cached untouched. The 60 s TTL already bounds how long an unlink
    /// takes to be noticed.
    pub fn spotify_access_token(&self) -> Option<String> {
        let mut guard = self.spotify_token.lock();
        if let Some((answer, fetched_at)) = guard.as_ref() {
            if fetched_at.elapsed() < SPOTIFY_TOKEN_TTL {
                // Including a cached `None`: "there is no account linked" is
                // an answer worth remembering for the TTL, not a reason to ask
                // again on the next call.
                return answer.clone();
            }
        }

        let client = http_client::new_http_client(self.timeout_secs);
        match client.get_text(&format!("{}/spotify/access_token", self.base)) {
            Ok(token) => {
                // A blank body is not a token. The route cannot send one --
                // it answers 404 with no account linked -- but a proxy in
                // between could, and an empty bearer would be cached for a
                // minute and rejected by Spotify for every call in it.
                let token = token.trim().to_string();
                if token.is_empty() {
                    return None;
                }
                *guard = Some((Some(token.clone()), Instant::now()));
                Some(token)
            }
            Err(_) => {
                // Cache the absence for the same TTL. A device with no
                // Spotify account linked is the common case, and every
                // favourites operation and every cover art lookup asks --
                // `is_enabled()` is `access_token().is_some()`. Without this
                // each of those is a fresh HTTP GET with a fresh client, and
                // once the halves are two processes each one waits out the
                // full timeout when the player daemon is slow.
                *guard = Some((None, Instant::now()));
                None
            }
        }
    }

    fn get(&self, path: &str) -> Result<serde_json::Value, String> {
        let client = http_client::new_http_client(self.timeout_secs);
        client
            .get_json_with_headers(&format!("{}{}", self.base, path), &[])
            .map_err(|e| e.to_string())
    }

    /// Every player the daemon knows, with whether it has a library and
    /// whether that library has finished loading. `GET /library`.
    pub fn libraries(&self) -> Result<Vec<LibraryPlayer>, String> {
        let v = self.get("/library")?;
        serde_json::from_value(v["players"].clone()).map_err(|e| e.to_string())
    }

    /// One player's library status and its two tokens. `GET /library/<p>`.
    ///
    /// A player that exists but has no library is answered 404 by the route,
    /// which arrives here as an error rather than as a `has_library: false`
    /// body — so a caller must not read `Err` as "no such player".
    pub fn library(&self, player: &str) -> Result<LibraryDetail, String> {
        let v = self.get(&format!("/library/{}", urlencoding::encode(player)))?;
        serde_json::from_value(v).map_err(|e| e.to_string())
    }

    /// The library's artists, as much of each as a lookup needs.
    /// `GET /library/<p>/artists`.
    ///
    /// No `If-None-Match` is sent, and no ETag is kept. Not an oversight: the
    /// only caller fetches this list *because* the library version moved, and
    /// the ETag the route emits is built from that same version
    /// (`acr_web::validated`), so a conditional request from here could only
    /// ever be answered 200. Where the version is absent — the backend tracks
    /// no changes — the route emits no validator at all and ignores
    /// `If-None-Match` outright, so there is nothing to condition on there
    /// either. A stored ETag would be state that can only be stale, and the
    /// 304 branch it justified could not be reached.
    pub fn artists(&self, player: &str) -> Result<Vec<ArtistRef>, String> {
        let v = self.get(&format!("/library/{}/artists", urlencoding::encode(player)))?;
        let Some(artists) = v.get("artists").and_then(|a| a.as_array()) else {
            // The route omits the key when the library holds no artists.
            return Ok(Vec::new());
        };
        Ok(artists
            .iter()
            .filter_map(|a| {
                Some(ArtistRef {
                    id: a.get("id")?.as_str()?.to_string(),
                    name: a.get("name")?.as_str()?.to_string(),
                })
            })
            .collect())
    }

    /// The library's albums with the genres each already carries.
    /// `GET /library/<p>/albums`. See [`Self::artists`] for why no ETag is sent.
    pub fn albums(&self, player: &str) -> Result<Vec<LibraryAlbum>, String> {
        let v = self.get(&format!("/library/{}/albums", urlencoding::encode(player)))?;
        let Some(albums) = v.get("albums").and_then(|a| a.as_array()) else {
            return Ok(Vec::new());
        };
        Ok(albums
            .iter()
            .filter_map(|a| {
                Some(LibraryAlbum {
                    album: AlbumRef {
                        id: a.get("id")?.as_str()?.to_string(),
                        name: a.get("name")?.as_str()?.to_string(),
                        // The lookup searches on one artist name. An album
                        // with none is still worth carrying: the sweep records
                        // that it cannot be looked up, so the next sweep does
                        // not consider it again.
                        artist: a
                            .get("artists")
                            .and_then(|v| v.as_array())
                            .and_then(|v| v.first())
                            .and_then(|v| v.as_str())
                            .unwrap_or_default()
                            .to_string(),
                    },
                    // Absent when empty, which is exactly the case the caller
                    // is looking for.
                    genres: a
                        .get("genres")
                        .and_then(|g| g.as_array())
                        .map(|g| {
                            g.iter()
                                .filter_map(|s| s.as_str().map(str::to_string))
                                .collect()
                        })
                        .unwrap_or_default(),
                })
            })
            .collect())
    }

    /// Hand one batch of results to a player's library.
    /// `POST /library/<p>/enrichment`.
    ///
    /// The three answers the route documents are all read here rather than
    /// collapsed into "it worked or it didn't":
    ///
    /// - 200 carries what merged and the library version the caller should now
    ///   treat as seen — its own merge moved that version, and reading it back
    ///   is what stops the next poll seeing its own write as a change.
    /// - 409 carries the generation the library is on now. The batch was
    ///   computed against one that is gone.
    /// - 404 is no such player, or a player with no library.
    ///
    /// This is the first producer of [`EnrichmentError::NoSuchLibrary`] in the
    /// codebase: Phase 0 defined it for exactly this 404 and no in-process sink
    /// could ever construct one.
    pub fn enrichment(
        &self,
        player: &str,
        batch: &EnrichmentBatch,
    ) -> Result<Applied, EnrichmentPostError> {
        let url = format!(
            "{}/library/{}/enrichment",
            self.base,
            urlencoding::encode(player)
        );
        let payload =
            serde_json::to_value(batch).map_err(|e| EnrichmentPostError::Failed(e.to_string()))?;
        let client = http_client::new_http_client(self.timeout_secs);
        // `post_json_status`, not `post_json_value`: the latter turns a 409
        // into an error string with the body discarded, and the body is where
        // the generation is.
        let (status, body) = client
            .post_json_status(&url, payload)
            .map_err(|e| EnrichmentPostError::Failed(e.to_string()))?;

        match status {
            200 => serde_json::from_value(body)
                .map_err(|e| EnrichmentPostError::Failed(e.to_string())),
            409 => Err(EnrichmentPostError::Refused(EnrichmentError::Stale {
                current_generation: body
                    .get("library_generation")
                    .and_then(|g| g.as_str())
                    .map(str::to_string),
            })),
            404 => Err(EnrichmentPostError::Refused(EnrichmentError::NoSuchLibrary)),
            other => Err(EnrichmentPostError::Failed(format!(
                "the enrichment route answered {} with {}",
                other, body
            ))),
        }
    }
}

impl AccessTokenSource for CoreClient {
    /// The same token [`CoreClient::spotify_access_token`] returns.
    ///
    /// The trait is what `crate::spotify` holds this client behind, so the
    /// providers need not know where the token comes from -- and it is the
    /// same trait the player side used for the seam in the other direction,
    /// now with its only implementor on this side.
    fn access_token(&self) -> Option<String> {
        self.spotify_access_token()
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

impl SplitterObservationSink for CoreClient {
    /// `false` on any error, logged at warn level — same rate reasoning as
    /// `SongInformationSink::apply` above: this changes at most once per
    /// disagreeing track, not once a second.
    fn record_order_observation(&self, player_name: &str, station: &str, order: OrderResult) -> bool {
        match self.splitter_observation(player_name, station, order) {
            Ok(()) => true,
            Err(e) => {
                log::warn!(
                    "title-order observation not delivered to the player daemon: {}",
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

    /// The token is fetched once and reused for the TTL. Two calls, one
    /// request: without the cache every provider lookup would ask the player
    /// daemon again.
    #[test]
    fn the_spotify_token_is_cached() {
        use crate::external_coverart::stub_server::Canned;
        let server = StubServer::queued(vec![Canned::bytes(
            200,
            "text/plain",
            b"placeholder-token".to_vec(),
        )]);
        let client = CoreClient::new(&server.base_url());

        assert_eq!(
            client.spotify_access_token().as_deref(),
            Some("placeholder-token")
        );
        assert_eq!(
            client.spotify_access_token().as_deref(),
            Some("placeholder-token")
        );

        assert_eq!(
            server.requests().len(),
            1,
            "the second read should come from the cache"
        );
        assert!(
            server.requests()[0].starts_with("GET /spotify/access_token HTTP/1.1"),
            "unexpected request line: {}",
            server.requests()[0]
        );
    }

    /// No account linked is a 404, and an unreachable player daemon is a
    /// transport failure. Both answer `None`, which every caller reads as
    /// "contribute nothing".
    #[test]
    fn no_account_and_no_daemon_both_answer_no_token() {
        let no_account = StubServer::serving(404, "");
        assert_eq!(CoreClient::new(&no_account.base_url()).spotify_access_token(), None);

        let dead = CoreClient::new("http://127.0.0.1:1/api");
        assert_eq!(dead.spotify_access_token(), None);
    }

    /// A blank body is not a token: caching one would hand an empty bearer to
    /// every Spotify call for the next minute.
    #[test]
    fn a_blank_body_is_not_a_token() {
        let blank = StubServer::queued(vec![
            crate::external_coverart::stub_server::Canned::bytes(200, "text/plain", b"  \n".to_vec()),
        ]);
        assert_eq!(CoreClient::new(&blank.base_url()).spotify_access_token(), None);
    }

    /// `StubServer` now records the body along with the headers (see its
    /// own test in `stub_server.rs`), so the partial is checked directly:
    /// the exact JSON sent, not a proxy for it.
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
        let body = requests[0]
            .split_once("\r\n\r\n")
            .map(|(_, body)| body)
            .expect("a body after the headers");
        // Exact equality, not `contains`: this also pins that no field other
        // than `title` rode along -- `Song`'s other fields all serialize
        // away when `None`/empty, so a partial that leaked an unset field
        // would fail this rather than only a `contains` check for `title`.
        assert_eq!(body, serde_json::to_string(&partial).unwrap());
        assert_eq!(body, r#"{"title":"Nemo"}"#);
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

    #[test]
    fn a_splitter_observation_posts_the_order_to_the_encoded_station() {
        let server = StubServer::serving(200, r#"{"station":"http://stream.example/radio"}"#);
        let client = CoreClient::new(&server.base_url());

        assert_eq!(
            client.splitter_observation("mpd", "http://stream.example/radio", OrderResult::SongArtist),
            Ok(())
        );

        let requests = server.requests();
        assert_eq!(requests.len(), 1, "exactly one request should be sent");
        let expected_path = format!(
            "POST /player/mpd/splitter/{}/observation HTTP/1.1",
            encode_url_safe("http://stream.example/radio")
        );
        assert!(
            requests[0].starts_with(&expected_path),
            "unexpected request line: {} (expected to start with {})",
            requests[0],
            expected_path
        );
        let body = requests[0]
            .split_once("\r\n\r\n")
            .map(|(_, body)| body)
            .expect("a body after the headers");
        assert_eq!(body, r#"{"order":"song_artist"}"#);
    }

    /// `SplitterObservationSink::record_order_observation` is the trait
    /// method the correction worker actually calls; it must collapse a
    /// transport error into `false` rather than panicking or propagating.
    #[test]
    fn an_unreachable_player_daemon_answers_false_for_an_observation() {
        let dead = CoreClient::new("http://127.0.0.1:1/api");
        assert!(!SplitterObservationSink::record_order_observation(
            &dead,
            "mpd",
            "http://stream.example/radio",
            OrderResult::SongArtist
        ));
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
