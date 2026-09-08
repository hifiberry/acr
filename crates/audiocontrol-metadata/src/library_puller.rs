//! The metadata side's discovery loop: find libraries worth enriching, start
//! the sweeps, and post what they find back.
//!
//! Phase 0 had this the other way round — a library that finished loading
//! called into the enricher directly. Over HTTP the player daemon cannot call
//! a function, and making it push the whole library would put the retry, the
//! backlog and the "have I done this already?" bookkeeping on the side that
//! has none of it. So this side pulls: `GET /api/library` for the list,
//! `GET /api/library/<p>` for each player that has a loaded library, and a
//! sweep for each library whose version has moved since the last one.
//!
//! **What tells it to look has changed.** Phase 1 discovered work by polling
//! every 30 s, with `POST /api/enrich/nudge` as an advisory hint the player
//! daemon sent after a load to shorten the wait. Both are gone. The player
//! daemon now announces a load as a `library_changed` event on the stream this
//! side already subscribes to, and that event is what wakes this loop —
//! [`crate::now_playing_ws`] turns one into a [`Wake`]. The route went with the
//! hint, because a route the player daemon calls is the thing this phase exists
//! to remove. The poll stays, demoted to a backstop: see
//! [`LIBRARY_POLL_INTERVAL`].
//!
//! **The event is a doorbell, not a payload.** It carries the player's
//! `library_version` and `library_generation` for a human and for clients
//! reading the stream, and this side reads neither. A [`Wake`] names a player
//! and nothing else, so the tokens are dropped where the frame is parsed and
//! cannot reach the comparison below. There are two independent reasons, and
//! the second is the one that decides it:
//!
//! - The event's `library_version` is the player's raw counter, while
//!   `GET /api/library/<p>` folds the caller's forwarded prefix into the one it
//!   reports, so the two never compare equal — not even with no proxy in the
//!   path, because the folding hashes the empty prefix rather than skipping it.
//!   (The *generation* is not folded on either side, so this half of the
//!   argument does not apply to it.)
//! - The generation says the library *reloaded*, not that its contents changed,
//!   and "have I enriched this already?" is a question only the version
//!   answers. So the generation cannot stand in for the version even though it
//!   would compare cleanly.
//!
//! Getting this wrong would be Phase 1's defect arriving by a new road: a
//! puller that reads a change out of every event re-enriches every library for
//! as long as it runs, and a suite whose fixtures use the same literal on both
//! sides of the comparison cannot see it.
//! `a_burst_of_events_does_not_re_enrich_an_unchanged_library` is the guard.
//!
//! The two tokens the player daemon reports over REST do different jobs, and
//! both are used here:
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
    AlbumRef, Applied, ArtistRef, EnrichmentBatch, EnrichmentError, EnrichmentSink,
    LibraryEnricher,
};
use crossbeam::channel::{unbounded, Receiver, RecvTimeoutError, Sender};
use log::{debug, info, warn};
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// How often every player's library is checked without anything asking for it.
///
/// **A backstop, not the discovery mechanism.** The `library_changed` event is
/// what starts a sweep now, so this interval covers only a change nobody heard
/// about. It used to be two things, and neither still holds:
///
/// - It was how work was *discovered*. Nothing announced a library load, so the
///   only way to learn of one was to ask; at 30 s a finished load waited up to
///   half a minute to be enriched.
/// - It was how a *missed nudge* was recovered, within the same half minute. A
///   nudge was a fire-and-forget POST that could be dropped for any reason and
///   told nobody it had been, so something had to cover it on a schedule.
///
/// What replaced both is stronger than a shorter interval. A load emits an
/// event on a socket this side holds open and reconnects to with backoff, and
/// every (re)connect asks for a full sweep ([`Wake::Everything`]) — so a change
/// made while the socket was down is picked up when it comes back rather than
/// at the next tick. What is left for this interval is the case where an event
/// was neither delivered nor covered by a connect: a bug in either half's event
/// plumbing, or a library that changed without announcing it at all. Ten
/// minutes keeps that recovery while taking the machinery off the common path.
///
/// It is not longer than ten minutes because this is also the only thing that
/// re-enriches a library whose backend reports no version at all — see
/// [`UNVERSIONED_REFETCH_INTERVAL`], which needs a pass to happen before it can
/// decide anything.
pub const LIBRARY_POLL_INTERVAL: Duration = Duration::from_secs(10 * 60);

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

/// What the puller has been asked to look at.
///
/// **Neither variant carries a version or a generation, and that is the point.**
/// The `library_changed` event that produces a [`Wake::Library`] does carry
/// both, and they are dropped where the frame is parsed rather than passed
/// along: a token that never reaches this side cannot be compared against the
/// one `GET /api/library/<p>` reports, which is the mistake the module comment
/// describes. What a wake says is "look at this player", and the answer to
/// "has it changed?" is always read from the route.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Wake {
    /// Every player the daemon lists.
    ///
    /// What a (re)connect to the event socket asks for. A gap in the stream
    /// means events were missed and there is no way to learn which, so the only
    /// honest recovery is to look at everything.
    Everything,
    /// One player, named by a `library_changed` event.
    Library(String),
}

/// The sending end of a puller's wake channel.
///
/// Held by the event subscriber, which is the only thing that wakes a puller
/// now. It is a value passed from one to the other rather than the process-wide
/// handle the nudge route needed: with no route to serve, there is no caller
/// left that cannot be handed the channel it wants to send on, and a value that
/// is passed cannot be raced for by two tests in one binary the way a static
/// can.
#[derive(Clone)]
pub struct Wakes(Sender<Wake>);

impl Wakes {
    /// A `library_changed` event arrived for `player`.
    pub fn library_changed(&self, player: &str) {
        self.send(Wake::Library(player.to_string()));
    }

    /// The event socket has just (re)connected, so the puller should look at
    /// everything once.
    ///
    /// The seed that covers a gap, and the counterpart of the now-playing seed
    /// the same connect performs. On the very first connect it asks for a sweep
    /// the puller's opening pass has just done; that costs one `GET` per
    /// library and is guarded by [`SeenVersions::changed`] like any other pass.
    pub fn reconnected(&self) {
        self.send(Wake::Everything);
    }

    /// Advisory in both directions: a puller whose loop has ended is not an
    /// error here, and nothing upstream waits for a wake to be acted on.
    fn send(&self, wake: Wake) {
        match self.0.send(wake.clone()) {
            Ok(()) => debug!("library puller woken: {:?}", wake),
            Err(_) => debug!(
                "library wake dropped ({:?}): the puller's loop has ended",
                wake
            ),
        }
    }
}

/// A wake channel: the handle the subscriber holds and the end a puller reads.
///
/// Public so a test can hold the receiver and observe exactly what the
/// subscriber sends, without a puller running at all.
pub fn wake_channel() -> (Wakes, Receiver<Wake>) {
    let (tx, rx) = unbounded();
    (Wakes(tx), rx)
}

/// Start the puller on a thread of its own, and hand back the [`Wakes`] the
/// event subscriber sends on.
///
/// The handle must be kept: dropping it leaves the puller with nothing that can
/// wake it and only [`LIBRARY_POLL_INTERVAL`] to work from.
///
/// `poll` is [`LIBRARY_POLL_INTERVAL`] in production. It is a parameter so a
/// deployment that wants a different backstop has one place to change it — and
/// so the tests of the backstop itself can ask for one that has elapsed by the
/// time they look, which is the one property no event can stand in for.
#[must_use = "dropping the Wakes leaves the puller with no event trigger at all"]
pub fn start(core: Arc<CoreClient>, poll: Duration) -> Wakes {
    let (wakes, rx) = wake_channel();
    let seen = Arc::new(Mutex::new(SeenVersions::default()));
    let enricher: Arc<dyn LibraryEnricher> = Arc::new(InProcessEnricher);

    std::thread::spawn(move || run(core, poll, rx, seen, enricher));
    info!(
        "library puller started; libraries are discovered from library_changed events, \
         with a backstop sweep every {}s",
        poll.as_secs()
    );
    wakes
}

/// What the next pass of the loop should look at.
enum Due {
    /// Every player the daemon lists. What a backstop tick and a
    /// [`Wake::Everything`] both mean.
    All,
    /// Only these, because an event named them.
    Named(Vec<String>),
}

/// Fold `first` and everything already queued behind it into one pass.
///
/// A library load emits one event, but a reconnect and an event can arrive
/// together, and several players can load at once. [`Wake::Everything`]
/// absorbs the rest: a pass over every player already includes each named one,
/// and doing both would pull the same libraries twice for nothing.
fn coalesced(first: Wake, wakes: &Receiver<Wake>) -> Due {
    let mut players: Vec<String> = Vec::new();
    let mut everything = false;
    for wake in std::iter::once(first).chain(wakes.try_iter()) {
        match wake {
            Wake::Everything => everything = true,
            Wake::Library(player) => {
                if !players.contains(&player) {
                    players.push(player);
                }
            }
        }
    }
    if everything {
        Due::All
    } else {
        Due::Named(players)
    }
}

/// The loop. Extracted from [`start`] so that its body is a plain function
/// over its inputs rather than a closure that can only be reached by spawning
/// a thread.
fn run(
    core: Arc<CoreClient>,
    poll: Duration,
    wakes: Receiver<Wake>,
    seen: Arc<Mutex<SeenVersions>>,
    enricher: Arc<dyn LibraryEnricher>,
) {
    // The first pass is a full one and happens immediately: a daemon that has
    // just started should not wait for an event or a tick before looking. It is
    // also what covers a library that finished loading before this side
    // subscribed to anything.
    let mut due = Due::All;
    let mut deaf = false;
    loop {
        match due {
            Due::All => sweep_all(&core, &seen, enricher.as_ref()),
            Due::Named(players) => {
                for player in players {
                    pull(&core, &seen, enricher.as_ref(), &player);
                }
            }
        }

        due = match wakes.recv_timeout(poll) {
            Ok(wake) => coalesced(wake, &wakes),
            Err(RecvTimeoutError::Timeout) => Due::All,
            Err(RecvTimeoutError::Disconnected) => {
                // Every sender is gone, so nothing can wake this loop again --
                // the event subscriber's thread has ended, which happens when
                // the daemon is shutting down. The loop keeps going on the
                // backstop rather than returning: the backstop is exactly what
                // covers "no events are arriving", and a puller that stopped
                // here would leave a running daemon with no enrichment at all
                // and nothing in the log to say why. `recv_timeout` returns
                // immediately once disconnected, so the wait has to be taken
                // here instead.
                if !deaf {
                    // Expected during shutdown -- the subscriber's thread ends
                    // and takes the sender with it -- and a fault at any other
                    // time. Only the second is worth waking an operator for.
                    if crate::startup::stop_was_requested() {
                        debug!("the library puller's wake channel closed during shutdown");
                    } else {
                        warn!(
                            "the library puller can no longer be woken by library_changed \
                             events; falling back to a sweep every {}s",
                            poll.as_secs()
                        );
                    }
                    deaf = true;
                }
                std::thread::sleep(poll);
                Due::All
            }
        };
    }
}

/// One pass over every player the daemon lists.
fn sweep_all(core: &Arc<CoreClient>, seen: &Arc<Mutex<SeenVersions>>, enricher: &dyn LibraryEnricher) {
    let players = match core.libraries() {
        Ok(players) => players,
        Err(e) => {
            // Debug, not warn: a pass also happens on a schedule rather than
            // only on an event, so a player daemon that is down would otherwise
            // write a warning for as long as it stays down.
            debug!("the player daemon's library list is not readable: {}", e);
            return;
        }
    };

    for player in players {
        // The list's own answer, and this guard is not the one in `pull`. A
        // player the list reports unloaded is not asked about at all, which is
        // what keeps a full sweep from costing one `GET /api/library/<p>` per
        // library that has nothing to offer. `pull`'s guard is what protects a
        // library named directly by an event, where no list was consulted;
        // each is covered by a test of its own.
        if !player.has_library || !player.is_loaded {
            continue;
        }
        pull(core, seen, enricher, &player.player_name);
    }
}

/// Consider one player's library, and enrich it if it has moved.
///
/// The entry point for a [`Wake::Library`] as well as for each player of a
/// sweep, and the `is_loaded` check below is the only thing standing between a
/// half-loaded library and an enrichment pass on that path. That case is not
/// hypothetical: a load is what emits `library_changed`, and both backends emit
/// one as the rebuild *starts*, when the library reports `is_loaded: false`.
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

    let mut artists = match core.artists(player) {
        Ok(artists) => artists,
        Err(e) => {
            debug!("artists for '{}' are not readable: {}", player, e);
            return;
        }
    };
    let listed = match core.albums(player) {
        Ok(albums) => albums,
        Err(e) => {
            debug!("albums for '{}' are not readable: {}", player, e);
            return;
        }
    };

    // The album-artist strings the library split, added to the artist list so
    // the split can be corrected. Two filters, and each is load-bearing:
    //
    // - A name the library *does* hold as an artist is already in `artists`, and
    //   its own summary carries the split claim. Adding it again would put two
    //   summaries for one name in one batch, and the second would overwrite the
    //   first's metadata with nothing.
    // - Everything else is offered `split_only`, so the sweep asks what it
    //   splits into and nothing else. There is no artist over there under that
    //   name to hold a thumbnail or a biography, so a full lookup would download
    //   an image no route can serve.
    //
    // The albums list is read before the genre filter below: an album that came
    // tagged with genres needs no lookup and still has an artist string that may
    // have been split wrongly.
    {
        let held: std::collections::HashSet<&str> =
            artists.iter().map(|a| a.name.as_str()).collect();
        let mut questions: Vec<String> = Vec::new();
        for album in &listed {
            let Some(name) = album.album_artist.as_deref() else {
                continue;
            };
            if name.is_empty() || held.contains(name) || questions.iter().any(|q| q == name) {
                continue;
            }
            questions.push(name.to_string());
        }
        artists.extend(questions.into_iter().map(ArtistRef::split_question));
    }

    // Only albums with nothing already recorded are worth a lookup. An album
    // that carries genres has either been enriched or came tagged, and either
    // way an empty answer would not replace them.
    let albums: Vec<AlbumRef> = listed
        .into_iter()
        .filter(|a| a.genres.is_empty())
        .map(|a| a.album)
        .collect();

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

    /// An album-artist string the player daemon split is offered to the sweep as
    /// its own question, and it is the only thing that can be: the artist list
    /// holds "Emerson", "Lake" and "Palmer", and those cannot be turned back
    /// into the name they came from. Without this the rejoin half of the
    /// correction has no way of ever being asked about.
    ///
    /// The album here also carries genres, so it is dropped from the *album*
    /// list -- which is why the strings are collected before that filter.
    #[test]
    fn a_split_album_artist_becomes_a_question_of_its_own() {
        let body = r#"{"players":[{"player_name":"mpd","player_id":"mpd","has_library":true,"is_loaded":true,"supports_delete":false}],
             "player_name":"mpd","has_library":true,"is_loaded":true,
             "library_version":"v1","library_generation":"g1",
             "count":1,
             "artists":[{"name":"Emerson","id":"1","is_multi":false,"album_count":1,"thumb_url":[]},
                        {"name":"Lake","id":"2","is_multi":false,"album_count":1,"thumb_url":[]},
                        {"name":"Palmer","id":"3","is_multi":false,"album_count":1,"thumb_url":[]}],
             "albums":[{"id":"12","name":"Trilogy","artists":["Emerson","Lake","Palmer"],
                        "album_artist":"Emerson, Lake & Palmer","genres":["progressive rock"],
                        "tracks_count":0,"cover_art":null,"uri":null}]}"#;
        let server = StubServer::serving(200, body);
        let seen = Arc::new(Mutex::new(SeenVersions::default()));
        let enricher = RecordingEnricher::new(false);

        sweep_all(&client(&server), &seen, enricher.as_ref());

        let calls = enricher.calls.lock();
        assert_eq!(calls.len(), 1);
        let questions: Vec<&ArtistRef> =
            calls[0].artists.iter().filter(|a| a.split_only).collect();
        assert_eq!(
            questions.len(),
            1,
            "one split string to ask about, got {:?}",
            calls[0].artists
        );
        assert_eq!(questions[0].name, "Emerson, Lake & Palmer");
        assert!(
            calls[0].albums.is_empty(),
            "and the album itself needs no genre lookup"
        );
    }

    /// An album whose artist list is the whole recorded string was never split,
    /// so the string is already in the artist list and asking again would put
    /// two summaries for one name in one batch.
    #[test]
    fn an_unsplit_album_artist_is_not_asked_about_twice() {
        let body = r#"{"players":[{"player_name":"mpd","player_id":"mpd","has_library":true,"is_loaded":true,"supports_delete":false}],
             "player_name":"mpd","has_library":true,"is_loaded":true,
             "library_version":"v1","library_generation":"g1",
             "count":1,
             "artists":[{"name":"Alpha and Beta","id":"1","is_multi":false,"album_count":1,"thumb_url":[]}],
             "albums":[{"id":"12","name":"Together","artists":["Alpha and Beta"],
                        "album_artist":"Alpha and Beta",
                        "tracks_count":0,"cover_art":null,"uri":null}]}"#;
        let server = StubServer::serving(200, body);
        let seen = Arc::new(Mutex::new(SeenVersions::default()));
        let enricher = RecordingEnricher::new(false);

        sweep_all(&client(&server), &seen, enricher.as_ref());

        let calls = enricher.calls.lock();
        assert_eq!(
            calls[0].artists.len(),
            1,
            "the name is asked about once, as the artist it is: {:?}",
            calls[0].artists
        );
        assert!(!calls[0].artists[0].split_only);
    }

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
    /// **The list's guard specifically, and there are two.** The detail body
    /// queued after the list reports a library that is loaded and has an artist
    /// and an album to enrich, so a `sweep_all` that ignored what the *list*
    /// said would pull it and sweep it. Deleting `pull`'s guard instead leaves
    /// this test green — `an_event_naming_a_library_that_is_not_loaded_is_not_swept`
    /// is what fails then. One test covering both at once passed with either
    /// guard deleted, which is how they came to be two.
    #[test]
    fn a_player_the_list_reports_unloaded_is_not_even_asked_about() {
        let list = r#"{"players":[{"player_name":"mpd","has_library":true,"is_loaded":false}]}"#;
        let server = StubServer::queued(vec![
            Canned::json(200, list),
            Canned::json(200, &versioned()),
        ]);
        let seen = Arc::new(Mutex::new(SeenVersions::default()));
        let enricher = RecordingEnricher::new(false);

        sweep_all(&client(&server), &seen, enricher.as_ref());

        assert!(
            enricher.calls.lock().is_empty(),
            "a library the list reports unloaded must not be swept"
        );
        assert_eq!(
            server.requests().len(),
            1,
            "and must not even be asked about: {:?}",
            server.requests()
        );
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

    // --- the event, and the loop it wakes ---------------------------------

    /// Run the loop on a thread with a wake channel of the test's own.
    ///
    /// No process-wide state is involved: the channel is a value now, not a
    /// static, so several of these run side by side in one binary without
    /// racing each other for the sender. The nudge tests had to be limited to
    /// one per binary for exactly that reason.
    ///
    /// The thread outlives the test. It is a loop with no exit, as it is in
    /// production; when the returned handle is dropped it falls back to its
    /// backstop and keeps sweeping its own stub server, which nothing else
    /// looks at.
    fn puller(core: Arc<CoreClient>, poll: Duration, enricher: Arc<RecordingEnricher>) -> Wakes {
        let (wakes, rx) = wake_channel();
        let seen = Arc::new(Mutex::new(SeenVersions::default()));
        let swept: Arc<dyn LibraryEnricher> = enricher;
        std::thread::spawn(move || run(core, poll, rx, seen, swept));
        wakes
    }

    /// The players each recorded sweep was for, in order.
    fn swept(enricher: &RecordingEnricher) -> Vec<String> {
        enricher
            .calls
            .lock()
            .iter()
            .map(|c| c.player.clone())
            .collect()
    }

    /// An event pulls the library it names, at the production backstop
    /// interval. Nothing waits for a tick: at ten minutes, a pull that happens
    /// is a pull the event caused.
    #[test]
    fn an_event_pulls_the_library_it_names() {
        // The opening pass happens as soon as the loop starts, so the stub
        // reports nothing to enrich until the test is ready.
        let server = StubServer::serving(200, r#"{"players":[]}"#);
        let enricher = RecordingEnricher::new(false);
        let wakes = puller(client(&server), LIBRARY_POLL_INTERVAL, enricher.clone());

        wait_until(|| !server.requests().is_empty());
        server.set_queue(vec![Canned::json(200, &versioned())]);

        wakes.library_changed("mpd");

        wait_until(|| !swept(&enricher).is_empty());
        assert_eq!(swept(&enricher), vec!["mpd"]);
    }

    /// **The ruling this task turns on.** The event is a doorbell: five of them
    /// for a library that has not changed produce no second sweep, because the
    /// answer to "has this changed?" is read from `GET /api/library/<p>` and
    /// never from the event. A consumer that compared the event's own
    /// `library_version` against the route's would find a change every time and
    /// re-enrich the whole library on every event, forever — Phase 1's defect,
    /// arriving by a new road.
    ///
    /// The sentinel is what makes this a test rather than a hope. Proving "no
    /// second sweep" by waiting proves nothing, so the last event names a
    /// library that *must* be swept: one loop consumes the wakes in order, so
    /// once its sweep is recorded every event before it has been acted on — and
    /// a re-enrichment of mpd would sit in the list ahead of it.
    #[test]
    fn a_burst_of_events_does_not_re_enrich_an_unchanged_library() {
        let server = StubServer::serving(200, &versioned());
        let enricher = RecordingEnricher::new(false);
        let wakes = puller(client(&server), LIBRARY_POLL_INTERVAL, enricher.clone());

        // The opening pass sweeps mpd once. That is the state the burst must
        // not add to.
        wait_until(|| swept(&enricher) == vec!["mpd"]);

        for _ in 0..5 {
            wakes.library_changed("mpd");
        }
        // Never listed, so never seen, so it is swept: the sentinel.
        wakes.library_changed("other");

        wait_until(|| swept(&enricher).contains(&"other".to_string()));
        assert_eq!(
            swept(&enricher),
            vec!["mpd", "other"],
            "five events for an unchanged library must add nothing"
        );
    }

    /// The `is_loaded` guard on the path where it is the only one there is.
    ///
    /// An event names a player directly, so no list is consulted and
    /// `sweep_all`'s check is not in the way. This is not a corner case: a
    /// library load is what emits `library_changed`, and both backends emit one
    /// as the rebuild *starts*, with `is_loaded` false for the whole of it. The
    /// event that finds work is the second one, sent once the load has finished
    /// — the sentinel here stands in for it, so this proves the wake path works
    /// rather than only that nothing happened.
    #[test]
    fn an_event_naming_a_library_that_is_not_loaded_is_not_swept() {
        let unloaded = r#"{"player_name":"mpd","has_library":true,"is_loaded":false,
            "library_version":"v1","library_generation":"g1",
            "artists":[{"name":"The Beatles","id":"7"}],
            "albums":[{"id":"12","name":"Abbey Road","artists":["The Beatles"]}]}"#;
        let server = StubServer::queued(vec![
            // The opening pass: nothing listed.
            Canned::json(200, r#"{"players":[]}"#),
            // The event for mpd: a library mid-rebuild, with work in it that a
            // missing guard would happily sweep.
            Canned::json(200, unloaded),
            // Everything the sentinel asks for. Repeated, being last.
            Canned::json(200, &versioned()),
        ]);
        let enricher = RecordingEnricher::new(false);
        let wakes = puller(client(&server), LIBRARY_POLL_INTERVAL, enricher.clone());

        wait_until(|| !server.requests().is_empty());
        wakes.library_changed("mpd");
        wakes.library_changed("other");

        wait_until(|| swept(&enricher).contains(&"other".to_string()));
        assert_eq!(
            swept(&enricher),
            vec!["other"],
            "a library that has not finished loading must not be swept"
        );
    }

    /// The property the poll exists for, and the one the demotion must not
    /// lose: a change nobody heard about is still picked up.
    ///
    /// No wake is delivered at all. The library appears only after the opening
    /// pass has been and gone, so the sweep that finds it can only be a
    /// backstop tick. The interval is short because the assertion is on the
    /// sweep having happened and not on how long it took — `wait_until` bounds
    /// the failure, and nothing here measures elapsed time.
    #[test]
    fn a_missed_event_is_still_recovered_by_the_backstop() {
        let server = StubServer::serving(200, r#"{"players":[]}"#);
        let enricher = RecordingEnricher::new(false);
        let _wakes = puller(
            client(&server),
            Duration::from_millis(50),
            enricher.clone(),
        );

        // The opening pass finds nothing, so what follows cannot be it.
        wait_until(|| !server.requests().is_empty());
        server.set_queue(vec![Canned::json(200, &versioned())]);

        wait_until(|| swept(&enricher).contains(&"mpd".to_string()));
    }

    /// The subscriber's thread ending must not end the puller with it.
    ///
    /// Its wake channel disconnects, which this loop used to treat as a reason
    /// to stop -- there, it could only mean "another `start` replaced my
    /// sender". Here it means "nothing will ever wake me again", and stopping
    /// would leave a running daemon with no enrichment at all and nothing in the
    /// log to say why. The backstop is exactly what covers that, so the loop
    /// keeps sweeping: the handle is dropped before the library appears, so the
    /// sweep that finds it happens with no sender left in existence.
    #[test]
    fn a_puller_nothing_can_wake_any_more_keeps_sweeping() {
        let server = StubServer::serving(200, r#"{"players":[]}"#);
        let enricher = RecordingEnricher::new(false);
        let wakes = puller(client(&server), Duration::from_millis(50), enricher.clone());

        wait_until(|| !server.requests().is_empty());
        drop(wakes);
        server.set_queue(vec![Canned::json(200, &versioned())]);

        wait_until(|| swept(&enricher).contains(&"mpd".to_string()));
    }

    /// A reconnect and a burst of events can arrive together, and
    /// [`Wake::Everything`] absorbs the named players rather than the other way
    /// round: a pass over every library already covers each named one, while
    /// the reverse would turn a reconnect — the only thing that recovers an
    /// event missed while the socket was down — into a pull of whatever
    /// happened to be queued beside it.
    #[test]
    fn a_reconnect_in_a_burst_of_events_sweeps_everything() {
        let (wakes, rx) = wake_channel();

        wakes.library_changed("mpd");
        wakes.reconnected();
        wakes.library_changed("lms");
        let first = rx.recv().expect("a wake to have been queued");
        assert!(
            matches!(coalesced(first, &rx), Due::All),
            "a reconnect asks for everything, whatever else is queued with it"
        );

        // ... and with no reconnect among them, only the players named, each
        // once however many times it arrived.
        wakes.library_changed("mpd");
        wakes.library_changed("lms");
        wakes.library_changed("mpd");
        let first = rx.recv().expect("a wake to have been queued");
        match coalesced(first, &rx) {
            Due::Named(players) => assert_eq!(players, vec!["mpd", "lms"]),
            Due::All => panic!("no reconnect arrived, so no full sweep should be due"),
        }
    }

    /// A wake nothing is listening for is dropped rather than an error. The
    /// subscriber holds this handle for the life of the process and must not
    /// have to know whether a puller is still running.
    #[test]
    fn a_wake_with_no_puller_listening_is_dropped() {
        let (wakes, rx) = wake_channel();
        drop(rx);
        wakes.library_changed("mpd");
        wakes.reconnected();
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
}
