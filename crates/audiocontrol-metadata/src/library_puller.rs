//! The metadata side's discovery loop: find libraries worth enriching, start
//! the sweeps, and post what they find back.
//!
//! Phase 0 had this the other way round — a library that finished loading
//! called into the enricher directly. Over HTTP the player daemon cannot call
//! a function, and making it push the whole library would put the retry, the
//! backlog and the "have I done this already?" bookkeeping on the side that
//! has none of it. So this side pulls: `GET /api/library` every
//! [`LIBRARY_POLL_INTERVAL`], `GET /api/library/<p>` for each player that has
//! a loaded library, and a sweep for each library whose version has moved
//! since the last one. `POST /api/enrich/nudge?player=<p>` shortens the wait
//! after a load; a nudge that arrives nowhere costs nothing, because the poll
//! covers it regardless.
//!
//! The two tokens the player daemon reports do different jobs, and both are
//! used here:
//!
//! - `library_version` answers "has this library changed since I last
//!   enriched it?". It is the [`SeenVersions`] bookkeeping below.
//! - `library_generation` is what a batch *names*, so the player daemon can
//!   refuse work computed against a library that has since been rebuilt. Both
//!   sweeps of one pull are given the same one, which is the whole point:
//!   nothing either sweep posts moves the generation, so neither can make the
//!   other stale.

use crate::core_client::{CoreClient, EnrichmentPostError};
use crate::library_enricher::InProcessEnricher;
use acr_types::enrichment::{
    AlbumRef, Applied, EnrichmentBatch, EnrichmentError, EnrichmentSink, LibraryEnricher,
};
use log::{debug, info, warn};
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

/// How often every player's library is checked. The spec's value.
pub const LIBRARY_POLL_INTERVAL: Duration = Duration::from_secs(30);

/// How often a library that reports no version is enriched again anyway.
///
/// A backend that tracks no changes (LMS) gives nothing to compare, so the
/// only honest answer to "has it changed?" is a clock. The spec's value: often
/// enough that a library edited during the day is picked up, rarely enough
/// that a full sweep is not a background load.
pub const UNVERSIONED_REFETCH_INTERVAL: Duration = Duration::from_secs(30 * 60);

/// What has already been enriched, per player.
///
/// **The two tokens compared here are folded the same way, and that is a
/// property of the routes, not of this deployment.** `GET /api/library/<p>`
/// reports a `library_version` that folds in the caller's own
/// `X-Forwarded-Prefix`, because the paths in the lists it validates are built
/// for that prefix. `POST /api/library/<p>/enrichment` folds the version in its
/// 200 and its 409 the same way, with the same request's prefix — so whatever
/// route a caller is on, the version it records as seen is the string its next
/// poll will be handed. Comparing them for equality is therefore right for a
/// proxied caller as well as a direct one.
///
/// This is worth stating because the obvious assumption is wrong. It is not
/// that the two agree "because there is no proxy on loopback":
/// `acr_web::urlprefix::prefix_tag(None)` hashes the *empty string* rather than
/// short-circuiting, so a direct caller's token is tagged too
/// (`d41d8cd9-<raw>`), and an enrichment route that emitted the version raw
/// would match nothing at all — every poll a change, the whole library
/// re-swept every thirty seconds, forever. Nothing here can detect that; the
/// guard is `the_version_a_200_returns_is_what_the_library_route_then_reports`
/// in `src/api/enrichment.rs`, which pins the two routes against each other.
///
/// The **generation** is not folded, on either route, and must not be: it is
/// not a validator for anything a proxy rewrites, and a prefixed one would
/// match nothing the library holds.
#[derive(Default)]
pub struct SeenVersions {
    seen: HashMap<String, Seen>,
}

struct Seen {
    /// The version last enriched, or `None` from a backend that reports none.
    version: Option<String>,
    /// When that was recorded. Only consulted for the `None` case, where
    /// there is nothing else to go on.
    at: Instant,
}

impl SeenVersions {
    /// Note that `player`'s library has been enriched at `version`, as of now.
    pub fn record(&mut self, player: &str, version: Option<String>) {
        self.record_at(player, version, Instant::now());
    }

    /// [`Self::record`] with the clock supplied, so a test can describe a
    /// library last enriched half an hour ago without waiting half an hour.
    pub fn record_at(&mut self, player: &str, version: Option<String>, at: Instant) {
        self.seen.insert(player.to_string(), Seen { version, at });
    }

    /// Whether `player`'s library is worth enriching at `version`.
    ///
    /// A library never seen before always is. A library reporting a version
    /// is, exactly when that version differs from the one last enriched. A
    /// library reporting none is, once [`UNVERSIONED_REFETCH_INTERVAL`] has
    /// passed — there is nothing to compare, so the clock is the only signal
    /// left.
    pub fn changed(&self, player: &str, version: &Option<String>) -> bool {
        let Some(seen) = self.seen.get(player) else {
            return true;
        };
        match version {
            Some(current) => seen.version.as_deref() != Some(current.as_str()),
            None => seen.at.elapsed() >= UNVERSIONED_REFETCH_INTERVAL,
        }
    }

    /// Drop what is known about `player`, so the next pass enriches it again.
    ///
    /// What a refusal calls. A 409 says the library was rebuilt under the
    /// sweep, and a rebuild is exactly when the work has to be redone — but
    /// the version recorded before the sweep started may still be the one the
    /// library reports, so comparing versions would conclude nothing had
    /// changed and the enrichment would be lost until something else moved it.
    pub fn forget(&mut self, player: &str) {
        self.seen.remove(player);
    }
}

/// Where a nudge is delivered, when a puller is running to receive it.
///
/// A process-wide handle because [`nudge`] is called from a Rocket route,
/// which has no puller in hand and should not have to be given one to accept a
/// hint it is allowed to drop. `None` means no puller has started; the route
/// still answers 202, because the periodic poll covers what the nudge would
/// have.
fn nudges() -> &'static Mutex<Option<Sender<String>>> {
    static NUDGES: OnceLock<Mutex<Option<Sender<String>>>> = OnceLock::new();
    NUDGES.get_or_init(|| Mutex::new(None))
}

/// Ask the running puller to look at one player's library now, rather than at
/// its next poll.
///
/// Advisory in every direction: with no puller running, or with one whose loop
/// has ended, this does nothing and says so at debug. Nothing upstream should
/// treat a nudge as a promise — `POST /api/enrich/nudge` answers 202 either
/// way, which is the honest code for "accepted for consideration".
pub fn nudge(player: &str) {
    match nudges().lock().as_ref() {
        Some(tx) if tx.send(player.to_string()).is_ok() => {
            debug!("enrichment nudge for player '{}' handed to the puller", player)
        }
        Some(_) => debug!(
            "enrichment nudge for player '{}' dropped: the puller's loop has ended",
            player
        ),
        None => debug!(
            "enrichment nudge for player '{}' dropped: no library puller is running",
            player
        ),
    }
}

/// Start the puller on a thread of its own, and arm [`nudge`].
///
/// `poll` is [`LIBRARY_POLL_INTERVAL`] in production; it is a parameter so a
/// deployment that wants a slower poll has one place to change, not so tests
/// can shorten it — a test that depends on the interval elapsing is a test
/// that depends on a clock.
pub fn start(core: Arc<CoreClient>, poll: Duration) {
    let (tx, rx) = std::sync::mpsc::channel();
    *nudges().lock() = Some(tx);
    let seen = Arc::new(Mutex::new(SeenVersions::default()));
    let enricher: Arc<dyn LibraryEnricher> = Arc::new(InProcessEnricher);

    std::thread::spawn(move || run(core, poll, rx, seen, enricher));
    info!(
        "library puller started; polling every {}s",
        poll.as_secs()
    );
}

/// What the next wake of the loop should look at.
enum Due {
    /// Every player the daemon lists. What a tick means.
    All,
    /// Only these, because they were nudged.
    Named(Vec<String>),
}

/// The loop. Extracted from [`start`] so that its body is a plain function
/// over its inputs rather than a closure that can only be reached by spawning
/// a thread.
fn run(
    core: Arc<CoreClient>,
    poll: Duration,
    nudges: Receiver<String>,
    seen: Arc<Mutex<SeenVersions>>,
    enricher: Arc<dyn LibraryEnricher>,
) {
    // The first pass is a full one and happens immediately: a daemon that has
    // just started should not wait a poll interval before looking.
    let mut due = Due::All;
    loop {
        match due {
            Due::All => sweep_all(&core, &seen, enricher.as_ref()),
            Due::Named(players) => {
                for player in players {
                    pull(&core, &seen, enricher.as_ref(), &player);
                }
            }
        }

        due = match nudges.recv_timeout(poll) {
            Ok(player) => {
                // Coalesce whatever else arrived while we were working: a
                // library load can nudge several times, and each pull is
                // guarded by `changed` anyway.
                let mut players = vec![player];
                while let Ok(more) = nudges.try_recv() {
                    if !players.contains(&more) {
                        players.push(more);
                    }
                }
                Due::Named(players)
            }
            Err(RecvTimeoutError::Timeout) => Due::All,
            Err(RecvTimeoutError::Disconnected) => {
                // Only reachable if another `start` replaced the sender this
                // loop was listening on. Ending is right: two loops polling
                // one daemon would each undo the other's bookkeeping.
                info!("library puller stopping: its nudge channel was replaced");
                return;
            }
        };
    }
}

/// One pass over every player the daemon lists.
fn sweep_all(core: &Arc<CoreClient>, seen: &Arc<Mutex<SeenVersions>>, enricher: &dyn LibraryEnricher) {
    let players = match core.libraries() {
        Ok(players) => players,
        Err(e) => {
            // Debug, not warn: this fires on a schedule rather than on an
            // event, so a player daemon that is down would otherwise write a
            // warning every thirty seconds for as long as it stays down.
            debug!("the player daemon's library list is not readable: {}", e);
            return;
        }
    };

    for player in players {
        if !player.has_library || !player.is_loaded {
            continue;
        }
        pull(core, seen, enricher, &player.player_name);
    }
}

/// Consider one player's library, and enrich it if it has moved.
fn pull(
    core: &Arc<CoreClient>,
    seen: &Arc<Mutex<SeenVersions>>,
    enricher: &dyn LibraryEnricher,
    player: &str,
) {
    let detail = match core.library(player) {
        Ok(detail) => detail,
        Err(e) => {
            debug!("library status for '{}' is not readable: {}", player, e);
            return;
        }
    };
    if !detail.has_library || !detail.is_loaded {
        debug!("player '{}' has no loaded library to enrich", player);
        return;
    }
    if !seen.lock().changed(player, &detail.library_version) {
        debug!(
            "player '{}' is at the version already enriched; nothing to do",
            player
        );
        return;
    }

    let artists = match core.artists(player) {
        Ok(artists) => artists,
        Err(e) => {
            debug!("artists for '{}' are not readable: {}", player, e);
            return;
        }
    };
    // Only albums with nothing already recorded are worth a lookup. An album
    // that carries genres has either been enriched or came tagged, and either
    // way an empty answer would not replace them.
    let albums: Vec<AlbumRef> = match core.albums(player) {
        Ok(albums) => albums
            .into_iter()
            .filter(|a| a.genres.is_empty())
            .map(|a| a.album)
            .collect(),
        Err(e) => {
            debug!("albums for '{}' are not readable: {}", player, e);
            return;
        }
    };

    // Recorded before the sweeps start, not after. Two reasons, and the second
    // is the one that bites: a sweep runs for as long as its lookups take, and
    // recording afterwards would leave every poll in between seeing a change
    // and starting a second pair of sweeps over the same library. And a sweep
    // that finishes quickly — a small library answered entirely from cache —
    // would have already recorded the *newer* version its own merge produced,
    // which a record here would then overwrite with the older one.
    seen.lock()
        .record(player, detail.library_version.clone());

    if artists.is_empty() && albums.is_empty() {
        // Nothing to ask about, so nothing is posted: an empty batch cannot
        // move the version, and the round trip would be paid once per
        // unchanged library per poll. The version just recorded is the one the
        // library reports, so it is not pulled again until it really moves.
        debug!("player '{}' has nothing left to enrich", player);
        return;
    }

    info!(
        "enriching '{}': {} artist(s), {} album(s) without genres, generation {:?}",
        player,
        artists.len(),
        albums.len(),
        detail.library_generation
    );

    let sink = Arc::new(HttpEnrichmentSink {
        core: core.clone(),
        player: player.to_string(),
        seen: seen.clone(),
    });
    // One generation for both sweeps. `LibraryEnricher::enrich` is what
    // guarantees that, and it is the same implementation the in-process path
    // used, so the property is not restated here where it could drift.
    enricher.enrich(player, detail.library_generation, artists, albums, sink);
}

/// A library reachable only over HTTP: batches go to
/// `POST /api/library/<p>/enrichment`.
///
/// The sink is what the sweeps hold, so this is also where the answer to a
/// batch is turned back into bookkeeping — the version a 200 reports becomes
/// the seen version, and a refusal drops the entry so the next pass starts
/// over.
pub struct HttpEnrichmentSink {
    core: Arc<CoreClient>,
    player: String,
    seen: Arc<Mutex<SeenVersions>>,
}

impl HttpEnrichmentSink {
    pub fn new(core: Arc<CoreClient>, player: String, seen: Arc<Mutex<SeenVersions>>) -> Self {
        HttpEnrichmentSink { core, player, seen }
    }
}

impl EnrichmentSink for HttpEnrichmentSink {
    /// A merge bumps the library's version, so the version the 200 hands back
    /// is recorded as seen: without that, this side would read its own write
    /// as a change on the next poll and enrich the library again, forever.
    ///
    /// Both refusals stop the sweep — that is [`crate::library_enricher::BatchSender`]'s
    /// doing — and both forget what was seen, so the next pass re-pulls.
    fn apply(&self, batch: EnrichmentBatch) -> Result<Applied, EnrichmentError> {
        match self.core.enrichment(&self.player, &batch) {
            Ok(applied) => {
                self.seen
                    .lock()
                    .record(&self.player, applied.library_version.clone());
                Ok(applied)
            }
            Err(EnrichmentPostError::Refused(refusal)) => {
                self.seen.lock().forget(&self.player);
                Err(refusal)
            }
            Err(EnrichmentPostError::Failed(reason)) => {
                // The sink's contract has two error variants, both of them the
                // library speaking, and a network failure is neither. It is
                // reported as `NoSuchLibrary` because that is the variant that
                // means "stop, this library is not reachable" — with the real
                // reason logged here, since the generic line the sweep writes
                // for that variant would otherwise be the only record and it
                // would be wrong.
                warn!(
                    "enrichment batch for '{}' was not delivered: {}",
                    self.player, reason
                );
                self.seen.lock().forget(&self.player);
                Err(EnrichmentError::NoSuchLibrary)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::external_coverart::stub_server::{Canned, StubServer};
    use crate::library_enricher::BatchSender;
    use acr_types::enrichment::{AlbumGenres, ArtistRef, ArtistSummary};
    use acr_types::ArtistMeta;

    /// A library that has one player, one artist and one album, and reports
    /// version `v1` at generation `g1`.
    ///
    /// One body answers all four GETs the puller makes. That is not laziness:
    /// the stub answers requests in arrival order with no idea what was asked,
    /// so a body that satisfies every route makes these tests independent of
    /// the order the puller happens to fetch in, and immune to a stray request
    /// from another test in the same process.
    fn one_of_everything(generation: &str) -> String {
        format!(
            r#"{{"players":[{{"player_name":"mpd","player_id":"mpd","has_library":true,"is_loaded":true,"supports_delete":false}}],
                 "player_name":"mpd","has_library":true,"is_loaded":true,
                 "library_version":"v1"{},
                 "count":1,
                 "artists":[{{"name":"The Beatles","id":"7","is_multi":false,"album_count":1,"thumb_url":[]}}],
                 "albums":[{{"id":"12","name":"Abbey Road","artists":["The Beatles"],"tracks_count":0,"cover_art":null,"uri":null}}]}}"#,
            generation
        )
    }

    fn versioned() -> String {
        one_of_everything(r#","library_generation":"g1""#)
    }

    fn client(server: &StubServer) -> Arc<CoreClient> {
        Arc::new(CoreClient::new(&server.base_url()))
    }

    struct Recorded {
        player: String,
        generation: Option<String>,
        artists: Vec<ArtistRef>,
        albums: Vec<AlbumRef>,
    }

    /// Stands in for the sweeps. Production's enricher spawns two threads that
    /// talk to MusicBrainz; what these tests need to see is what it was
    /// *given*, and — when `post_through_sink` is set — that a batch built
    /// from it reaches the player daemon.
    struct RecordingEnricher {
        calls: Mutex<Vec<Recorded>>,
        post_through_sink: bool,
    }

    impl RecordingEnricher {
        fn new(post_through_sink: bool) -> Arc<Self> {
            Arc::new(RecordingEnricher {
                calls: Mutex::new(Vec::new()),
                post_through_sink,
            })
        }
    }

    impl LibraryEnricher for RecordingEnricher {
        fn artist_summary(&self, _name: &str) -> Option<ArtistSummary> {
            None
        }
        fn artist_detail(&self, _name: &str) -> Option<ArtistMeta> {
            None
        }
        fn artist_image(&self, _name: &str) -> Option<(Vec<u8>, String)> {
            None
        }
        fn album_genres(&self, _album_id: &str) -> Option<Vec<String>> {
            None
        }
        fn enrich(
            &self,
            player: &str,
            generation: Option<String>,
            artists: Vec<ArtistRef>,
            albums: Vec<AlbumRef>,
            sink: Arc<dyn EnrichmentSink>,
        ) {
            if self.post_through_sink {
                let _ = sink.apply(EnrichmentBatch {
                    library_generation: generation.clone(),
                    artists: Vec::new(),
                    albums: albums
                        .iter()
                        .map(|a| AlbumGenres {
                            id: a.id.clone(),
                            genres: vec!["rock".to_string()],
                        })
                        .collect(),
                });
            }
            self.calls.lock().push(Recorded {
                player: player.to_string(),
                generation,
                artists,
                albums,
            });
        }
    }

    /// The two requests the puller makes over one pass that a test needs to
    /// count, without depending on how many GETs precede them.
    fn posts(server: &StubServer) -> Vec<String> {
        server
            .requests()
            .into_iter()
            .filter(|r| r.starts_with("POST "))
            .collect()
    }

    // --- SeenVersions, the pure part ------------------------------------

    #[test]
    fn the_version_returned_by_an_apply_counts_as_seen() {
        let mut seen = SeenVersions::default();
        seen.record("mpd", Some("v1".into()));
        assert!(!seen.changed("mpd", &Some("v1".into())));
        assert!(seen.changed("mpd", &Some("v2".into())));
    }

    #[test]
    fn a_library_never_seen_before_is_always_due() {
        let seen = SeenVersions::default();
        assert!(seen.changed("mpd", &Some("v1".into())));
        assert!(seen.changed("lms", &None));
    }

    #[test]
    fn a_library_without_a_version_is_due_every_thirty_minutes() {
        let mut seen = SeenVersions::default();
        let half_an_hour_ago = Instant::now()
            .checked_sub(Duration::from_secs(31 * 60))
            .expect("the monotonic clock to be older than half an hour");
        seen.record_at("lms", None, half_an_hour_ago);
        assert!(seen.changed("lms", &None));
        seen.record("lms", None);
        assert!(!seen.changed("lms", &None));
    }

    #[test]
    fn forgetting_a_player_makes_it_due_again() {
        let mut seen = SeenVersions::default();
        seen.record("mpd", Some("v1".into()));
        assert!(!seen.changed("mpd", &Some("v1".into())));
        seen.forget("mpd");
        assert!(
            seen.changed("mpd", &Some("v1".into())),
            "a refused sweep must be redone even though the version has not moved"
        );
    }

    // --- one pass over a library ----------------------------------------

    /// Both halves of a sweep are handed the *generation*, not the version,
    /// and one generation covers both. A sweep given the version would refuse
    /// its own second batch the moment the first one merged.
    #[test]
    fn a_pass_hands_the_libraries_generation_to_the_sweeps() {
        let server = StubServer::serving(200, &versioned());
        let seen = Arc::new(Mutex::new(SeenVersions::default()));
        let enricher = RecordingEnricher::new(false);

        sweep_all(&client(&server), &seen, enricher.as_ref());

        let calls = enricher.calls.lock();
        assert_eq!(calls.len(), 1, "one library, one sweep");
        assert_eq!(calls[0].player, "mpd");
        assert_eq!(
            calls[0].generation.as_deref(),
            Some("g1"),
            "the generation, not the version"
        );
        assert_eq!(calls[0].artists.len(), 1);
        assert_eq!(calls[0].artists[0].name, "The Beatles");
        assert_eq!(calls[0].albums.len(), 1);
        assert_eq!(calls[0].albums[0].id, "12");
        assert_eq!(calls[0].albums[0].artist, "The Beatles");
    }

    /// LMS reports no generation, and a batch that names none is what that
    /// backend accepts. The version must not be substituted: the route would
    /// compare it against a generation and refuse every batch.
    #[test]
    fn a_library_reporting_no_generation_gets_batches_naming_none() {
        let server = StubServer::serving(200, &one_of_everything(""));
        let seen = Arc::new(Mutex::new(SeenVersions::default()));
        let enricher = RecordingEnricher::new(false);

        sweep_all(&client(&server), &seen, enricher.as_ref());

        let calls = enricher.calls.lock();
        assert_eq!(calls.len(), 1);
        assert_eq!(
            calls[0].generation, None,
            "no generation was reported, so none is claimed"
        );
    }

    #[test]
    fn a_version_that_has_not_changed_is_not_re_enriched() {
        let server = StubServer::serving(200, &versioned());
        let core = client(&server);
        let seen = Arc::new(Mutex::new(SeenVersions::default()));
        let enricher = RecordingEnricher::new(false);

        sweep_all(&core, &seen, enricher.as_ref());
        sweep_all(&core, &seen, enricher.as_ref());

        assert_eq!(
            enricher.calls.lock().len(),
            1,
            "the second pass saw the same version and had nothing to do"
        );
    }

    /// An album that already carries genres is not worth a lookup, and an
    /// artist list is not filtered here at all — the sweep answers a cached
    /// artist from its own cache without a request.
    #[test]
    fn albums_that_already_have_genres_are_not_swept() {
        let body = r#"{"players":[{"player_name":"mpd","has_library":true,"is_loaded":true}],
            "has_library":true,"is_loaded":true,"library_version":"v1","library_generation":"g1",
            "artists":[],
            "albums":[{"id":"12","name":"Abbey Road","artists":["The Beatles"],"genres":["rock"]},
                      {"id":"13","name":"Let It Be","artists":["The Beatles"]}]}"#;
        let server = StubServer::serving(200, body);
        let seen = Arc::new(Mutex::new(SeenVersions::default()));
        let enricher = RecordingEnricher::new(false);

        sweep_all(&client(&server), &seen, enricher.as_ref());

        let calls = enricher.calls.lock();
        assert_eq!(calls.len(), 1);
        assert_eq!(
            calls[0].albums.iter().map(|a| a.id.as_str()).collect::<Vec<_>>(),
            vec!["13"],
            "only the album with nothing recorded is looked up"
        );
    }

    /// Ruling: an empty batch is not posted. A library with no artists and no
    /// album missing genres has nothing to ask about, and posting nothing
    /// cannot move the version — so the round trip is skipped and the library
    /// is not pulled again at the next pass either.
    #[test]
    fn a_library_with_nothing_to_enrich_posts_no_batch_and_is_not_re_pulled() {
        let body = r#"{"players":[{"player_name":"mpd","has_library":true,"is_loaded":true}],
            "has_library":true,"is_loaded":true,"library_version":"v1","library_generation":"g1",
            "artists":[],
            "albums":[{"id":"12","name":"Abbey Road","artists":["The Beatles"],"genres":["rock"]}]}"#;
        let server = StubServer::serving(200, body);
        let core = client(&server);
        let seen = Arc::new(Mutex::new(SeenVersions::default()));
        let enricher = RecordingEnricher::new(true);

        sweep_all(&core, &seen, enricher.as_ref());
        sweep_all(&core, &seen, enricher.as_ref());

        assert!(
            enricher.calls.lock().is_empty(),
            "there was nothing to sweep"
        );
        assert!(
            posts(&server).is_empty(),
            "and nothing to post: {:?}",
            posts(&server)
        );
        assert!(
            !seen.lock().changed("mpd", &Some("v1".into())),
            "the version the GET reported is still recorded as seen"
        );
    }

    /// A player whose library has not finished loading is left alone: its
    /// lists are incomplete, and enriching half a library would record a
    /// version that covers all of it.
    ///
    /// The library described here has an artist and an album to enrich, which
    /// is the point — an empty one would be skipped for having nothing to do
    /// and this test would pass with every `is_loaded` check deleted.
    #[test]
    fn a_library_that_is_not_loaded_is_left_alone() {
        let body = r#"{"players":[{"player_name":"mpd","has_library":true,"is_loaded":false}],
            "has_library":true,"is_loaded":false,"library_version":"v1","library_generation":"g1",
            "artists":[{"name":"The Beatles","id":"7"}],
            "albums":[{"id":"12","name":"Abbey Road","artists":["The Beatles"]}]}"#;
        let server = StubServer::serving(200, body);
        let seen = Arc::new(Mutex::new(SeenVersions::default()));
        let enricher = RecordingEnricher::new(false);

        sweep_all(&client(&server), &seen, enricher.as_ref());

        assert!(enricher.calls.lock().is_empty());
    }

    // --- the batch on the wire -------------------------------------------

    /// The whole chain in one pass: discover, sweep, post. The body is what
    /// the player daemon's route parses, so it is asserted as sent.
    #[test]
    fn a_batch_reaches_the_enrichment_route_naming_the_generation() {
        let server = StubServer::queued(vec![
            Canned::json(200, &versioned()), // GET /library
            Canned::json(200, &versioned()), // GET /library/mpd
            Canned::json(200, &versioned()), // GET /library/mpd/artists
            Canned::json(200, &versioned()), // GET /library/mpd/albums
            Canned::json(200, r#"{"artists":0,"albums":1,"library_version":"v2"}"#),
        ]);
        let seen = Arc::new(Mutex::new(SeenVersions::default()));
        let enricher = RecordingEnricher::new(true);

        sweep_all(&client(&server), &seen, enricher.as_ref());

        let posts = posts(&server);
        assert_eq!(posts.len(), 1, "exactly one batch");
        assert!(
            posts[0].starts_with("POST /library/mpd/enrichment HTTP/1.1"),
            "unexpected request line: {}",
            posts[0]
        );
        let body = posts[0]
            .split_once("\r\n\r\n")
            .map(|(_, body)| body)
            .expect("a body after the headers");
        assert!(
            body.contains(r#""library_generation":"g1""#),
            "the batch must name the generation it was computed against: {}",
            body
        );
        assert!(
            body.contains(r#""id":"12""#),
            "and carry the album it is about: {}",
            body
        );
    }

    /// The same thing as `a_version_that_has_not_changed_is_not_re_enriched`,
    /// but with the tokens the daemon really emits rather than `"v1"`.
    ///
    /// Every other fixture in this module says `"v1"` on both sides of the
    /// comparison, which is exactly why the suite could not see that the
    /// enrichment route was returning a raw version while
    /// `GET /api/library/<p>` returned a prefix-tagged one: the two never
    /// matched, so the library was re-fetched, re-swept and re-posted on every
    /// poll. Here the library reports `d41d8cd9-<counter>` — the tag a caller
    /// with no forwarded prefix gets, since `prefix_tag(None)` hashes the empty
    /// string — the merge moves the counter, and the 200 hands back the moved
    /// token folded the same way. The second pass must then find nothing to do.
    #[test]
    fn a_second_pass_over_a_library_this_puller_just_enriched_does_nothing() {
        let at = |version: &str| {
            format!(
                r#"{{"players":[{{"player_name":"mpd","has_library":true,"is_loaded":true}}],
                     "has_library":true,"is_loaded":true,
                     "library_version":"{}","library_generation":"a3f9c1d2-g0",
                     "artists":[],
                     "albums":[{{"id":"12","name":"Abbey Road","artists":["The Beatles"]}}]}}"#,
                version
            )
        };
        let before = "d41d8cd9-a3f9c1d2-0-42";
        let after = "d41d8cd9-a3f9c1d2-0-43";
        let server = StubServer::queued(vec![
            Canned::json(200, &at(before)), // GET /library
            Canned::json(200, &at(before)), // GET /library/mpd
            Canned::json(200, &at(before)), // GET /library/mpd/artists
            Canned::json(200, &at(before)), // GET /library/mpd/albums
            // The merge bumped the version, and the route hands it back folded
            // for this caller -- the same way the GET below reports it.
            Canned::json(
                200,
                &format!(r#"{{"artists":0,"albums":1,"library_version":"{}"}}"#, after),
            ),
            Canned::json(200, &at(after)), // second pass: GET /library
            Canned::json(200, &at(after)), // second pass: GET /library/mpd
        ]);
        let core = client(&server);
        let seen = Arc::new(Mutex::new(SeenVersions::default()));
        let enricher = RecordingEnricher::new(true);

        sweep_all(&core, &seen, enricher.as_ref());
        sweep_all(&core, &seen, enricher.as_ref());

        assert_eq!(
            enricher.calls.lock().len(),
            1,
            "the second pass saw the version this puller's own merge produced, \
             not a change"
        );
        assert_eq!(posts(&server).len(), 1, "and posted nothing further");
    }

    /// The version a merge produced is the one to compare against next time.
    /// Without this the puller would read its own write as a change and
    /// enrich the same library on every poll for as long as it ran.
    #[test]
    fn the_version_an_apply_returns_becomes_the_seen_version() {
        let server = StubServer::serving(200, r#"{"artists":0,"albums":1,"library_version":"v2"}"#);
        let seen = Arc::new(Mutex::new(SeenVersions::default()));
        seen.lock().record("mpd", Some("v1".into()));
        let sink = HttpEnrichmentSink::new(client(&server), "mpd".into(), seen.clone());

        let applied = sink
            .apply(EnrichmentBatch {
                library_generation: Some("g1".into()),
                artists: vec![ArtistSummary {
                    name: "The Beatles".into(),
                    ..Default::default()
                }],
                albums: Vec::new(),
            })
            .expect("a 200 is an applied batch");

        assert_eq!(applied.library_version.as_deref(), Some("v2"));
        assert!(
            !seen.lock().changed("mpd", &Some("v2".into())),
            "the version the route returned is what the next poll compares against"
        );
        assert!(seen.lock().changed("mpd", &Some("v1".into())));
    }

    /// A 409 is read out of the body, not inferred from the status alone —
    /// the generation it carries is the one the library is on now.
    #[test]
    fn a_refused_batch_is_stale_and_makes_the_next_pass_re_pull() {
        let server = StubServer::serving(
            409,
            r#"{"library_generation":"g2","library_version":"v7"}"#,
        );
        let seen = Arc::new(Mutex::new(SeenVersions::default()));
        seen.lock().record("mpd", Some("v1".into()));
        let sink = HttpEnrichmentSink::new(client(&server), "mpd".into(), seen.clone());

        let refusal = sink
            .apply(EnrichmentBatch {
                library_generation: Some("g1".into()),
                albums: vec![AlbumGenres {
                    id: "12".into(),
                    genres: vec!["rock".into()],
                }],
                ..Default::default()
            })
            .expect_err("a batch computed against a rebuilt library is refused");

        assert_eq!(
            refusal,
            EnrichmentError::Stale {
                current_generation: Some("g2".into())
            }
        );
        assert!(
            seen.lock().changed("mpd", &Some("v1".into())),
            "the refusal drops what was seen, so the next pass pulls the library again"
        );
    }

    /// `NoSuchLibrary` has no in-process producer at all — Phase 0 defined it
    /// for this 404 and nothing could construct one until now. So this is the
    /// first exercise of the whole path: the status is read, the variant is
    /// produced, and the sweep stops on it instead of posting its next batch.
    #[test]
    fn a_404_becomes_no_such_library_and_stops_the_sweep() {
        let server = StubServer::serving(404, r#"{"error":"no such library"}"#);
        let seen = Arc::new(Mutex::new(SeenVersions::default()));
        let sink = Arc::new(HttpEnrichmentSink::new(
            client(&server),
            "gone".into(),
            seen.clone(),
        ));

        let entry = || {
            vec![AlbumGenres {
                id: "12".into(),
                genres: vec!["rock".into()],
            }]
        };
        assert_eq!(
            sink.apply(EnrichmentBatch {
                albums: entry(),
                ..Default::default()
            }),
            Err(EnrichmentError::NoSuchLibrary)
        );

        // And through the sender the sweeps actually use: the first batch is
        // refused, `send` reports the sweep should stop, and a sweep that
        // ignored that would post a second time.
        let mut sender = BatchSender::new(sink, Some("g1".into()));
        assert!(
            !sender.send(Vec::new(), entry()),
            "a 404 must stop the sweep"
        );
        assert_eq!(
            posts(&server).len(),
            2,
            "one direct apply and one through the sender -- and no third, \
             which is what a sweep that kept going would have sent"
        );
    }

    /// A player daemon that cannot be reached is not the library saying no.
    /// The sweep still stops, but the entry is forgotten so the next pass
    /// tries again rather than concluding the library is already enriched.
    #[test]
    fn an_undeliverable_batch_stops_the_sweep_and_is_retried_later() {
        let seen = Arc::new(Mutex::new(SeenVersions::default()));
        seen.lock().record("mpd", Some("v1".into()));
        let sink = HttpEnrichmentSink::new(
            Arc::new(CoreClient::new("http://127.0.0.1:1")),
            "mpd".into(),
            seen.clone(),
        );

        assert_eq!(
            sink.apply(EnrichmentBatch {
                albums: vec![AlbumGenres {
                    id: "12".into(),
                    genres: vec!["rock".into()]
                }],
                ..Default::default()
            }),
            Err(EnrichmentError::NoSuchLibrary)
        );
        assert!(
            seen.lock().changed("mpd", &Some("v1".into())),
            "an undelivered batch must not leave the library looking enriched"
        );
    }

    // --- the nudge --------------------------------------------------------

    /// The nudge, through the route that receives it, against a running
    /// puller whose poll interval is the production one. Nothing here waits
    /// for a tick: at thirty seconds, a pull that happens is a pull the nudge
    /// caused.
    ///
    /// This is the only test in this module that arms the process-wide nudge
    /// channel, and it must stay that way — a second one would race it for
    /// the sender.
    #[test]
    fn a_nudge_pulls_before_the_poll_interval_could_elapse() {
        // The first pass happens as soon as the puller starts, so the stub
        // reports a library with nothing to enrich until the test is ready.
        let empty = r#"{"players":[],"has_library":false,"is_loaded":false}"#;
        let server = StubServer::serving(200, empty);
        let enricher = RecordingEnricher::new(false);
        let seen = Arc::new(Mutex::new(SeenVersions::default()));

        let (tx, rx) = std::sync::mpsc::channel();
        *nudges().lock() = Some(tx);
        let core = client(&server);
        let swept: Arc<dyn LibraryEnricher> = enricher.clone();
        std::thread::spawn(move || run(core, LIBRARY_POLL_INTERVAL, rx, seen, swept));

        // Wait for the puller's opening pass to be over, so that what follows
        // can only be the nudge's doing.
        wait_until(|| !server.requests().is_empty());
        server.set_queue(vec![Canned::json(200, &versioned())]);

        let route = rocket::local::blocking::Client::tracked(
            rocket::build().mount("/api", rocket::routes![crate::api::enrich::nudge]),
        )
        .expect("rocket should launch");
        let response = route.post("/api/enrich/nudge?player=mpd").dispatch();
        assert_eq!(response.status(), rocket::http::Status::Accepted);

        // "a call naming mpd", not "the first call": another test in this
        // binary may post a nudge of its own to the route, and an extra pull
        // it caused must not be able to fail this one.
        wait_until(|| enricher.calls.lock().iter().any(|c| c.player == "mpd"));
    }

    /// Poll a condition rather than sleep for a fixed time: the assertion is
    /// on a transition having happened, and the bound is only there so a
    /// failure is a failure rather than a hang.
    fn wait_until(mut condition: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            if condition() {
                return;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("the puller did not act within ten seconds");
    }

    // `nudge` with no puller running is deliberately *not* covered by a test.
    // The branch is process-wide state: whether it is reached at all depends
    // on whether the nudge test above has already armed the channel, which
    // depends on the order the harness runs them in. A test that passes
    // because it happened to run first proves nothing, so there is no test
    // here claiming otherwise.
}
