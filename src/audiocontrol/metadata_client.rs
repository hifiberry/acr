//! `MetadataClient` -- the player side's client for the two seams the
//! metadata side answers over HTTP: title-order and artist-split resolution,
//! and library enrichment (artist detail and artist images -- the enrichment
//! nudge that used to be the third is gone with the route it called, see
//! `enrich` below).
//!
//! `main` installs this in place of the in-process implementations from
//! `audiocontrol-metadata` when `services.metadata` names a base URL. With
//! nothing configured, nothing is installed, and every caller falls back to
//! the behaviour a MusicBrainz-disabled, enrichment-less build already has --
//! see `resolver` and `enrichment` in this module's parent.
//!
//! **Not here any more: the Spotify access token.** It was a third seam,
//! `GET /spotify/access_token` against the metadata side, and it is the one
//! the one-way-seam spec singles out: the librespot backend turned every
//! playback command into a Spotify Web API call and fetched its bearer token
//! through here, so a metadata half that was down meant pressing play did
//! nothing. The account now lives in this daemon
//! (`crate::players::librespot::spotify_account`) and the token travels the
//! other way, from `audiocontrol_metadata::core_client::CoreClient` to
//! `GET /api/spotify/access_token` here.
//!
//! **Not here: `PlaybackStateSource`.** It reads `GET /api/player`, a route of
//! the *player* daemon, while this client addresses `services.metadata.url`
//! -- in Phase 2 a different process on a different port entirely.
//! `CoreClient` (`crates/audiocontrol-metadata/src/core_client.rs`) is the
//! metadata side's client for that route. Today, with one process, both base
//! URLs happen to reach the same port, so implementing `PlaybackStateSource`
//! here would compile, pass every test, and only break once the processes
//! split -- see that file's own doc comment for the same warning from the
//! other side.

use acr_http::http_client::{self, HttpClient};
use acr_types::artist_split::split_artist_with_separators;
use acr_types::config::get_service_config;
use acr_types::enrichment::{AlbumRef, ArtistRef, ArtistSummary, EnrichmentSink, LibraryEnricher};
use acr_types::resolver::Resolver;
use acr_types::url_encoding::encode_url_safe;
use acr_types::{ArtistMeta, OrderResult};
use std::sync::Arc;

/// The spec's two configurable timeouts, used when `services.metadata` names
/// neither. Named rather than inlined at the one call site that reads them so
/// that the value a deployment silently gets is greppable next to the two
/// timeouts that are not configurable at all.
const DEFAULT_DETAIL_TIMEOUT_MS: u64 = 1000;
const DEFAULT_RESOLVE_TIMEOUT_MS: u64 = 5000;

/// The fixed timeout for an artist image fetch.
///
/// Not one of the two configured timeouts: the detail timeout is for the
/// small `ArtistMeta` JSON body the same route family serves, and an image
/// download is neither that call nor a resolver round trip. The spec fixes
/// it at 5 s rather than making it configurable.
const IMAGE_TIMEOUT_SECS: u64 = 5;

/// Round a millisecond timeout up to the whole seconds `UreqHttpClient`
/// understands, with a floor of one second.
///
/// `acr_http::http_client::UreqHttpClient::new` takes only whole seconds
/// (`Duration::from_secs`), so a sub-second config value cannot be honoured
/// precisely. `from_secs(0)` would mean either "no timeout" or an instant
/// one, and both are wrong for a caller that asked for *some* bound under a
/// second, so this rounds up rather than down or to zero. The spec's two
/// values -- 1000 ms and 5000 ms -- are exact multiples of a second and round
/// to themselves; nothing in production depends on sub-second precision.
fn ceil_secs(timeout_ms: u64) -> u64 {
    timeout_ms.div_ceil(1000).max(1)
}

/// A client for the metadata side's HTTP API, from the player side.
///
/// Holds one `HttpClient` per timeout tier rather than a single shared one,
/// because `acr_http`'s timeout is fixed at construction and this client
/// serves three different ones in practice: the configured detail and
/// resolve timeouts, and the fixed 5 s image timeout above. Building three
/// small, stateless clients once is simpler than reconstructing one per call.
pub struct MetadataClient {
    base: String,
    detail_client: Box<dyn HttpClient>,
    resolve_client: Box<dyn HttpClient>,
    image_client: Box<dyn HttpClient>,
}

impl MetadataClient {
    /// `base` is the metadata side's API root -- e.g. `http://127.0.0.1:1080/api`
    /// in Phase 1, where it and the player daemon's own routes share one
    /// process and one port; a different port entirely once the processes
    /// split.
    pub fn new(base: &str, detail_timeout_ms: u64, resolve_timeout_ms: u64) -> Self {
        Self {
            base: base.trim_end_matches('/').to_string(),
            detail_client: http_client::new_http_client(ceil_secs(detail_timeout_ms)),
            resolve_client: http_client::new_http_client(ceil_secs(resolve_timeout_ms)),
            image_client: http_client::new_http_client(IMAGE_TIMEOUT_SECS),
        }
    }

    /// Build a `MetadataClient` from `services.metadata` in the daemon's
    /// configuration, or `None` when that section is absent -- the signal
    /// that this build makes none of the three calls at all and every caller
    /// keeps its offline fallback.
    ///
    /// The default URL, `http://127.0.0.1:1080/api`, is Phase 1's own value:
    /// the metadata routes this client calls are mounted into the same
    /// Rocket instance as the player daemon's own API
    /// (`audiocontrol_metadata::api::routes` merged in by `src/api/server.rs`),
    /// on the port `services.webserver` binds. It becomes a different port
    /// on a separate host only once the processes actually split.
    pub fn from_config(config: &serde_json::Value) -> Option<Self> {
        let m = get_service_config(config, "metadata")?;
        Some(Self::new(
            m.get("url")
                .and_then(|v| v.as_str())
                .unwrap_or("http://127.0.0.1:1080/api"),
            m.get("detail_timeout_ms")
                .and_then(|v| v.as_u64())
                .unwrap_or(DEFAULT_DETAIL_TIMEOUT_MS),
            m.get("resolve_timeout_ms")
                .and_then(|v| v.as_u64())
                .unwrap_or(DEFAULT_RESOLVE_TIMEOUT_MS),
        ))
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base, path)
    }

    /// `GET` `path` as text through `client` and parse it as JSON.
    ///
    /// `None` on any failure: a connection error, a non-2xx status --
    /// `get_text` collapses both into the same `RequestError` variant, so
    /// they cannot be told apart here -- or a body that will not parse.
    fn get_json(&self, client: &dyn HttpClient, path: &str) -> Option<serde_json::Value> {
        let text = client.get_text(&self.url(path)).ok()?;
        serde_json::from_str(&text).ok()
    }
}

impl Resolver for MetadataClient {
    fn title_order(&self, part1: &str, part2: &str) -> OrderResult {
        let path = format!(
            "/resolve/title-order?part1={}&part2={}",
            urlencoding::encode(part1),
            urlencoding::encode(part2)
        );
        match self
            .get_json(self.resolve_client.as_ref(), &path)
            .and_then(|v| v["order"].as_str().map(str::to_string))
            .as_deref()
        {
            Some("artist_song") => OrderResult::ArtistSong,
            Some("song_artist") => OrderResult::SongArtist,
            Some("undecided") => OrderResult::Undecided,
            _ => OrderResult::Unknown,
        }
    }

    fn artist_split(&self, name: &str, separators: &[String]) -> Option<Vec<String>> {
        // One `separator=` per separator. Joining them with any delimiter is
        // wrong for the same reason a separator list cannot be comma-joined:
        // `,` is itself the first of `DEFAULT_ARTIST_SEPARATORS`, so a joined
        // list loses it and mangles the rest. See the route's doc comment.
        let mut path = format!("/resolve/artist-split?name={}", urlencoding::encode(name));
        for separator in separators {
            path.push_str("&separator=");
            path.push_str(&urlencoding::encode(separator));
        }
        match self.get_json(self.resolve_client.as_ref(), &path) {
            Some(v) => serde_json::from_value(v["artists"].clone()).unwrap_or(None),
            None => {
                // Unreachable, or answered nothing usable: fall back to the
                // plain separator split the in-process resolver uses with
                // MusicBrainz disabled, rather than leaving the caller with
                // nothing at all.
                let parts = split_artist_with_separators(name, separators);
                if parts.len() > 1 {
                    Some(parts)
                } else {
                    None
                }
            }
        }
    }
}

impl LibraryEnricher for MetadataClient {
    /// Always `None`.
    ///
    /// The trait's own doc comment (`crates/acr-types/src/enrichment.rs`)
    /// says this is called once per artist *while a library loads* and must
    /// not do network I/O. This client has nothing local to answer from --
    /// its only source is the metadata side, over the network -- so the only
    /// contract-respecting answer is "nothing yet". Genres and the rest
    /// arrive later, through `enrich`'s batch callback.
    fn artist_summary(&self, _name: &str) -> Option<ArtistSummary> {
        None
    }

    fn artist_detail(&self, name: &str) -> Option<ArtistMeta> {
        let path = format!("/artist/{}", encode_url_safe(name));
        self.get_json(self.detail_client.as_ref(), &path)
            .and_then(|v| serde_json::from_value(v).ok())
    }

    fn artist_image(&self, name: &str) -> Option<(Vec<u8>, String)> {
        let path = format!("/coverart/artist/{}/image", encode_url_safe(name));
        self.image_client.get_binary(&self.url(&path)).ok()
    }

    /// Always `None`, for the same reason as `artist_summary`: called once
    /// per album while a library loads and must not do network I/O. `Some`
    /// and `None` are documented as different answers there, and `None` --
    /// "nothing stored yet", not "looked up and found nothing" -- is the
    /// correct one for a client with no local answer at all. Genres arrive
    /// through the enrichment batch, the same as artist genres do.
    fn album_genres(&self, _album_id: &str) -> Option<Vec<String>> {
        None
    }

    /// Does nothing, and makes no request.
    ///
    /// This used to `POST /enrich/nudge?player=` to ask the metadata side to
    /// look at a library that had just finished loading. **That route no longer
    /// exists, and this call is the reason it could be deleted.** Nothing on the
    /// metadata daemon may be called by this one after the one-way seam, so the
    /// announcement travels the other way: the load emits a `library_changed`
    /// event on `/api/events`, a route this daemon already serves and the
    /// metadata side already subscribes to. A nudge was advisory in both
    /// directions, so what is lost by not sending it is a round trip.
    ///
    /// Every argument is unused, `sink` included. Results have never come back
    /// through the sink on this path -- they arrive at
    /// `POST /api/library/<p>/enrichment`, which this client does not call -- and
    /// the generation the metadata side computes against is whatever its own
    /// pull reads, not whatever this call saw a moment earlier. `sink` is
    /// dropped explicitly so the omission reads as deliberate rather than
    /// forgotten.
    ///
    /// The method stays only because `LibraryEnricher` requires it; it goes with
    /// the rest of this client when the last seam call from this daemon goes.
    fn enrich(
        &self,
        _player: &str,
        _generation: Option<String>,
        _artists: Vec<ArtistRef>,
        _albums: Vec<AlbumRef>,
        sink: Arc<dyn EnrichmentSink>,
    ) {
        drop(sink);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use acr_types::enrichment::{Applied, EnrichmentBatch, EnrichmentError};
    use parking_lot::Mutex;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    /// A stub HTTP server that answers a status and body a closure picks
    /// from the request path, then keeps listening. Binds port 0 and reports
    /// back the port actually assigned, so tests need no fixed port and run
    /// unprivileged.
    struct Stub {
        port: u16,
    }

    impl Stub {
        fn base(&self) -> String {
            format!("http://127.0.0.1:{}/api", self.port)
        }
    }

    fn stub<F>(handler: F) -> Stub
    where
        F: Fn(&str) -> (u16, &'static str) + Send + Sync + 'static,
    {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a free local port");
        let port = listener.local_addr().expect("a bound address").port();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let mut request = Vec::new();
                let mut buf = [0u8; 4096];
                // Read until the header terminator. None of these paths
                // send a body worth reading past it.
                loop {
                    let Ok(n) = stream.read(&mut buf) else { break };
                    if n == 0 {
                        break;
                    }
                    request.extend_from_slice(&buf[..n]);
                    if request.windows(4).any(|w| w == b"\r\n\r\n") {
                        break;
                    }
                }
                let request = String::from_utf8_lossy(&request);
                let path = request
                    .lines()
                    .next()
                    .and_then(|line| line.split_whitespace().nth(1))
                    .unwrap_or("/")
                    .to_string();
                let (status, body) = handler(&path);
                let reason = if (200..300).contains(&status) { "OK" } else { "Error" };
                let head = format!(
                    "HTTP/1.1 {} {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    status,
                    reason,
                    body.len()
                );
                let _ = stream.write_all(head.as_bytes());
                let _ = stream.write_all(body.as_bytes());
                let _ = stream.flush();
            }
        });
        Stub { port }
    }

    /// An `EnrichmentSink` that is never called -- `enrich` must not touch
    /// it, and a call here would panic to prove the point.
    struct UnusedSink;

    impl EnrichmentSink for UnusedSink {
        fn apply(&self, _batch: EnrichmentBatch) -> Result<Applied, EnrichmentError> {
            panic!("MetadataClient::enrich must not call the sink it was handed");
        }
    }

    #[test]
    fn title_order_maps_the_answer_and_an_unreachable_side_is_unknown() {
        let server = stub(|path| {
            if path.starts_with("/api/resolve/title-order") {
                (200, r#"{"order":"song_artist"}"#)
            } else {
                (404, "")
            }
        });
        let client = MetadataClient::new(&server.base(), 1000, 5000);
        assert_eq!(client.title_order("a", "b"), OrderResult::SongArtist);

        let dead = MetadataClient::new("http://127.0.0.1:1/api", 1000, 100);
        assert_eq!(dead.title_order("a", "b"), OrderResult::Unknown);
    }

    #[test]
    fn artist_split_null_means_one_artist_and_failure_means_plain_split() {
        let server = stub(|_| (200, r#"{"artists":null}"#));
        let client = MetadataClient::new(&server.base(), 1000, 5000);
        assert_eq!(client.artist_split("A & B", &["&".to_string()]), None);

        let dead = MetadataClient::new("http://127.0.0.1:1/api", 1000, 100);
        assert_eq!(
            dead.artist_split("A & B", &[" & ".to_string()]),
            Some(vec!["A".into(), "B".into()])
        );
    }

    /// The regression this route shipped with, caught at the client end: the
    /// separators went out joined by a comma, and `,` is itself the first of
    /// `DEFAULT_ARTIST_SEPARATORS`, so the list could not survive its own
    /// contents. Asserting on the query string rather than the answer, because
    /// the answer comes from a stub -- what has to be right here is what goes
    /// on the wire.
    #[test]
    fn every_separator_crosses_the_wire_as_its_own_parameter() {
        let seen = Arc::new(Mutex::new(String::new()));
        let recorder = seen.clone();
        let server = stub(move |path| {
            *recorder.lock() = path.to_string();
            (200, r#"{"artists":null}"#)
        });

        let defaults: Vec<String> = acr_types::artist_split::DEFAULT_ARTIST_SEPARATORS
            .iter()
            .map(|s| s.to_string())
            .collect();
        let client = MetadataClient::new(&server.base(), 1000, 5000);
        client.artist_split("Simon, Garfunkel", &defaults);

        let path = seen.lock().clone();
        assert_eq!(
            path.matches("&separator=").count(),
            defaults.len(),
            "one parameter per separator, got: {path}"
        );
        // The comma must arrive percent-encoded and alone, not as a delimiter
        // between two other values.
        assert!(
            path.contains("&separator=%2C&"),
            "the comma separator must survive as its own value, got: {path}"
        );
        assert!(
            path.contains("&separator=%20feat%20"),
            "a separator with spaces must survive intact, got: {path}"
        );
    }

    #[test]
    fn from_config_is_none_without_a_metadata_section() {
        assert!(MetadataClient::from_config(&serde_json::json!({"services": {}})).is_none());
    }

    /// `from_config` has to actually thread the configured URL through, not
    /// just report `Some` -- a client built with the wrong base would still
    /// pass the presence check above.
    #[test]
    fn from_config_uses_the_configured_url_and_timeouts() {
        let server = stub(|_| (200, r#"{"order":"artist_song"}"#));
        let cfg = serde_json::json!({
            "services": {
                "metadata": {
                    "url": server.base(),
                    "detail_timeout_ms": 1000,
                    "resolve_timeout_ms": 5000
                }
            }
        });
        let client = MetadataClient::from_config(&cfg).expect("the section is present");
        assert_eq!(client.title_order("a", "b"), OrderResult::ArtistSong);
    }

    /// Both must refuse to do network I/O while a library loads, per the
    /// trait's own contract -- there is no server running in this test at
    /// all, so a call that tried to reach one would hang or error rather
    /// than answer directly.
    #[test]
    fn artist_summary_and_album_genres_never_call_out() {
        let client = MetadataClient::new("http://127.0.0.1:1/api", 1000, 5000);
        assert_eq!(client.artist_summary("Pink Floyd"), None);
        assert_eq!(client.album_genres("some-album-id"), None);
    }

    #[test]
    fn artist_detail_parses_the_body_and_a_failure_is_none() {
        let server = stub(|path| {
            if path.starts_with("/api/artist/") {
                (200, r#"{"mbid":["abc-123"],"biography":"a biography"}"#)
            } else {
                (404, "")
            }
        });
        let client = MetadataClient::new(&server.base(), 1000, 5000);
        let meta = client
            .artist_detail("Pink Floyd")
            .expect("the stub answers a body");
        assert_eq!(meta.mbid, vec!["abc-123".to_string()]);
        assert_eq!(meta.biography.as_deref(), Some("a biography"));

        let dead = MetadataClient::new("http://127.0.0.1:1/api", 1000, 5000);
        assert!(dead.artist_detail("Pink Floyd").is_none());
    }

    #[test]
    fn artist_image_returns_the_body_and_a_failure_is_none() {
        let server = stub(|path| {
            if path.contains("/image") {
                (200, "not-really-a-jpeg")
            } else {
                (404, "")
            }
        });
        let client = MetadataClient::new(&server.base(), 1000, 5000);
        let (bytes, _mime) = client
            .artist_image("Pink Floyd")
            .expect("the stub answers a body");
        assert_eq!(bytes, b"not-really-a-jpeg");

        let dead = MetadataClient::new("http://127.0.0.1:1/api", 1000, 5000);
        assert!(dead.artist_image("Pink Floyd").is_none());
    }

    /// `enrich` calls the metadata side at all any more -- which is the rule the
    /// one-way seam exists to establish, asserted where it can be broken.
    ///
    /// This test used to assert the opposite: that the call POSTed
    /// `/api/enrich/nudge?player=mpd`. That route is gone, and the library load
    /// that used to send the nudge now announces itself as a `library_changed`
    /// event instead. Re-adding any request here would put this daemon back to
    /// calling the metadata daemon, so the assertion is on the request log being
    /// empty rather than on the absence of one particular path.
    #[test]
    fn enrich_makes_no_request_at_all_and_never_touches_the_sink() {
        let seen = Arc::new(Mutex::new(Vec::<String>::new()));
        let recorded = seen.clone();
        let server = stub(move |path| {
            recorded.lock().push(path.to_string());
            (202, "")
        });
        let client = MetadataClient::new(&server.base(), 1000, 5000);
        client.enrich("mpd", None, vec![], vec![], Arc::new(UnusedSink));

        let requests = seen.lock().clone();
        assert!(
            requests.is_empty(),
            "the player daemon must not call the metadata daemon here; it asked for {:?}",
            requests
        );

        let dead = MetadataClient::new("http://127.0.0.1:1/api", 1000, 5000);
        // Must not panic, and must not block, with nothing listening.
        dead.enrich("mpd", None, vec![], vec![], Arc::new(UnusedSink));
    }
}
