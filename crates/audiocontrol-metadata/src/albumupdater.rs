use log::{debug, info, warn};
use std::sync::Arc;
use acr_types::enrichment::{AlbumGenres, AlbumRef, EnrichmentSink};
use crate::library_enricher::{BatchSender, Swept, BATCH_SIZE};
use crate::musicbrainz::GenreLookup;

const CACHE_KEY_PREFIX: &str = "album::genres::";

/// Return the attribute cache key for a given album ID
fn cache_key(album_id: &str) -> String {
    format!("{}{}", CACHE_KEY_PREFIX, album_id)
}

/// Load cached genres for an album from the attribute cache.
/// Returns `Some(genres)` if a cached entry exists (even if empty), `None` if not found.
pub fn load_cached_genres(album_id: &str) -> Option<Vec<String>> {
    match acr_store::attributecache::get::<Vec<String>>(&cache_key(album_id)) {
        Ok(Some(genres)) => Some(genres),
        Ok(None) => None,
        Err(e) => {
            debug!("Error reading album genre cache for {}: {}", album_id, e);
            None
        }
    }
}

/// Persist genres for an album to the attribute cache.
fn store_cached_genres(album_id: &str, genres: &[String]) {
    let genres_vec = genres.to_vec();
    match acr_store::attributecache::set(&cache_key(album_id), &genres_vec) {
        Ok(_) => debug!("Stored genres for album {} in attribute cache", album_id),
        Err(e) => warn!("Failed to store genres for album {} in attribute cache: {}", album_id, e),
    }
}

/// Look up genres for an album from MusicBrainz.
/// Checks attribute cache first; only calls MusicBrainz if not cached.
pub fn fetch_album_genres(album_id: &str, artist: &str, album_name: &str) -> GenreLookup {
    fetch_album_genres_with(
        album_id,
        artist,
        album_name,
        crate::musicbrainz::search_release_group_genres,
    )
}

/// [`fetch_album_genres`] with the provider passed in, so what is written down
/// after an answer and after a failure can be tested without a network.
fn fetch_album_genres_with(
    album_id: &str,
    artist: &str,
    album_name: &str,
    lookup: impl FnOnce(&str, &str) -> GenreLookup,
) -> GenreLookup {
    // Return cached value if present
    if let Some(cached) = load_cached_genres(album_id) {
        debug!("Using cached genres for album '{}': {:?}", album_name, cached);
        return GenreLookup::Answered(cached);
    }

    // Not cached — fetch from MusicBrainz
    let outcome = lookup(artist, album_name);

    match &outcome {
        // An answer is written down whatever it says, empty included: many
        // albums genuinely have no genres in MusicBrainz, and remembering that
        // is the only thing keeping every library load from asking about all of
        // them again at one request per second. No expiry, because the answer is
        // about a release group's genre tags, which change about as often as the
        // release group does; a wrong entry is corrected by removing the row.
        GenreLookup::Answered(genres) => {
            info!(
                "Fetched {} genre(s) from MusicBrainz for album '{}' by '{}'",
                genres.len(),
                album_name,
                artist
            );
            store_cached_genres(album_id, genres);
        }
        // A failure says nothing about the album, so nothing is written down —
        // not even with an expiry. A cache entry exists to stop a question being
        // asked again; the question here has not been answered once, and an
        // unhealthy afternoon at MusicBrainz used to be recorded permanently as
        // "this album has no genres". The cost of writing nothing is that the
        // next sweep asks again, which is exactly what should happen.
        GenreLookup::Unavailable(reason) => {
            warn!(
                "No genre answer from MusicBrainz for album '{}' by '{}': {}. \
                 Nothing cached; it will be looked up again.",
                album_name, artist, reason
            );
        }
    }

    outcome
}

/// What to do about one album before any network request is made.
#[derive(Debug, PartialEq, Eq)]
enum Plan {
    /// Send these genres; they are already known.
    Send(Vec<String>),
    /// Look them up.
    Fetch,
    /// Nothing to look up with. Record that so the next sweep does not get
    /// this far again.
    RecordEmpty,
    /// A lookup already found nothing for this album. Repeating it would be a
    /// MusicBrainz request per album per library load for an answer that is
    /// known to be empty.
    Skip,
}

/// Decide what one album needs, from what is cached about it and what there is
/// to search with. Separated from the sweep because it is the whole of the
/// policy that keeps the sweep off the network.
fn plan(cached: Option<Vec<String>>, artist: &str, album_name: &str) -> Plan {
    match cached {
        Some(genres) if genres.is_empty() => Plan::Skip,
        Some(genres) => Plan::Send(genres),
        None if artist.is_empty() || album_name.is_empty() => Plan::RecordEmpty,
        None => Plan::Fetch,
    }
}

/// Everything the album sweep reaches outside itself for.
///
/// Production supplies `LiveSweep`; a test supplies its own and can then see
/// what the sweep would have paid for. Pacing and milestones are in here on
/// purpose: they are invisible from the outside, they are what the sweep costs
/// on a warm cache, and they have been changed by accident before.
trait Sweep {
    fn cached_genres(&self, album_id: &str) -> Option<Vec<String>>;
    /// Record that an album cannot be looked up at all.
    fn record_no_genres(&self, album_id: &str);
    fn fetch_genres(&self, album_id: &str, artist: &str, album_name: &str) -> GenreLookup;
    /// Announce the album about to be considered. Every album reaches this.
    fn starting(&self, album_name: &str, index: usize, total: usize);
    /// A progress milestone. Only an album that cost a request reaches this.
    fn milestone(&self, count: usize, total: usize, updated: usize);
    /// Wait, out of politeness to the service just called. Only an album that
    /// cost a request reaches this: a sweep over a fully cached library must
    /// cost nothing, and at fifty milliseconds an album a large library would
    /// otherwise spend minutes sleeping between cache hits.
    fn pace(&self);
}

/// What one album sweep got through: what every sweep reports, plus how many
/// lookups got no answer.
///
/// The second number is not the same as "found nothing". An album MusicBrainz
/// has no genres for is answered and remembered; a lookup that failed is
/// neither, and the sweep that ran while the service was unhealthy has to be
/// able to say so rather than reporting a quiet zero.
struct SweptAlbums {
    swept: Swept,
    /// Lookups that got no answer at all, so nothing was written down about
    /// them and the next sweep will ask again.
    unanswered: usize,
}

/// The sweep itself: decide, ask, accumulate, flush.
///
/// Reports how many albums it had something to say about, how many lookups got
/// no answer, and whether it stopped early because the library refused a batch
/// as stale - in which case there is no final flush.
fn sweep_albums(albums: Vec<AlbumRef>, io: &dyn Sweep, sender: &mut BatchSender) -> SweptAlbums {
    let total = albums.len();
    let mut batch: Vec<AlbumGenres> = Vec::with_capacity(BATCH_SIZE);
    let mut updated = 0usize;
    let mut unanswered = 0usize;

    for (index, album) in albums.into_iter().enumerate() {
        let AlbumRef { id: album_id, name: album_name, artist } = album;
        io.starting(&album_name, index, total);

        // Anything reached by `continue` below cost no request, so it reaches
        // neither the milestone nor the pacing at the end of the loop.
        match plan(io.cached_genres(&album_id), &artist, &album_name) {
            Plan::Skip => {
                debug!("Skipping album '{}' — nothing to look up", album_name);
                continue;
            }
            Plan::RecordEmpty => {
                // Record that this album cannot be looked up, so the next
                // sweep reaches Plan::Skip instead of getting here again.
                io.record_no_genres(&album_id);
                continue;
            }
            Plan::Send(genres) => {
                updated += 1;
                if !accumulate(&mut batch, AlbumGenres { id: album_id, genres }, sender) {
                    return SweptAlbums {
                        swept: Swept { reported: updated, stopped_after: Some(index + 1) },
                        unanswered,
                    };
                }
                continue;
            }
            Plan::Fetch => match io.fetch_genres(&album_id, &artist, &album_name) {
                // An empty answer is not sent: an empty list never overwrites
                // anything, so the entry would be work for the library and no
                // change. `fetch_genres` has already cached the emptiness,
                // which is what keeps the next sweep from asking again.
                GenreLookup::Answered(genres) if genres.is_empty() => {}
                GenreLookup::Answered(genres) => {
                    updated += 1;
                    if !accumulate(&mut batch, AlbumGenres { id: album_id, genres }, sender) {
                        return SweptAlbums {
                            swept: Swept { reported: updated, stopped_after: Some(index + 1) },
                            unanswered,
                        };
                    }
                }
                // No answer, so nothing to send and nothing written down. The
                // album stays unknown and the next sweep asks again, which is
                // the whole difference between this and an empty answer.
                GenreLookup::Unavailable(reason) => {
                    unanswered += 1;
                    debug!("No genre answer for album '{}': {}", album_name, reason);
                }
            }
        }

        let count = index + 1;
        if count % 50 == 0 || count == total {
            io.milestone(count, total, updated);
        }
        io.pace();
    }

    if !sender.send(Vec::new(), batch) {
        return SweptAlbums {
            swept: Swept { reported: updated, stopped_after: Some(total) },
            unanswered,
        };
    }
    SweptAlbums {
        swept: Swept { reported: updated, stopped_after: None },
        unanswered,
    }
}

/// Add one entry to the batch, flushing it if it is now full.
///
/// Returns `false` when the library refused the flush and the sweep should stop.
fn accumulate(batch: &mut Vec<AlbumGenres>, entry: AlbumGenres, sender: &mut BatchSender) -> bool {
    batch.push(entry);
    if batch.len() < BATCH_SIZE {
        return true;
    }
    sender.send(Vec::new(), std::mem::take(batch))
}

/// The sweep's real world: the attribute cache, MusicBrainz, the background
/// job and the clock.
struct LiveSweep {
    job_id: String,
}

impl Sweep for LiveSweep {
    fn cached_genres(&self, album_id: &str) -> Option<Vec<String>> {
        load_cached_genres(album_id)
    }

    /// An album with no artist or no name cannot be searched for at all, which
    /// is a fact about the library entry rather than about MusicBrainz — no
    /// request was made and none could fail — so it is written down the same
    /// way a genuine empty answer is.
    fn record_no_genres(&self, album_id: &str) {
        store_cached_genres(album_id, &[]);
    }

    fn fetch_genres(&self, album_id: &str, artist: &str, album_name: &str) -> GenreLookup {
        fetch_album_genres(album_id, artist, album_name)
    }

    fn starting(&self, album_name: &str, index: usize, total: usize) {
        let _ = acr_store::backgroundjobs::update_job(
            &self.job_id,
            Some(format!("Processing: {}", album_name)),
            Some(index),
            Some(total),
        );
    }

    fn milestone(&self, count: usize, total: usize, updated: usize) {
        info!("Album genre update: {}/{} processed, {} updated", count, total, updated);
        let _ = acr_store::backgroundjobs::update_job(
            &self.job_id,
            Some(format!("Processed {}/{} albums", count, total)),
            Some(count),
            Some(total),
        );
    }

    fn pace(&self) {
        // MusicBrainz allows 1 req/sec; the ratelimit helper handles
        // per-request limiting but we add a small sleep to be polite.
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}

/// Look up genres for a library's albums in the background, sending what is
/// found back in batches.
///
/// Returns at once. The caller decides which albums are worth asking about;
/// this sweep asks about every one it is given, in order, paced for the
/// service behind `fetch_album_genres`.
pub fn enrich_albums_in_background(
    player: String,
    generation: Option<String>,
    albums: Vec<AlbumRef>,
    sink: Arc<dyn EnrichmentSink>,
) {
    debug!("Starting background thread to update album genres for {}", player);

    std::thread::spawn(move || {
        let job_id = "album_genre_update".to_string();
        let job_name = "Album Genre Update".to_string();

        if let Err(e) = acr_store::backgroundjobs::register_job(job_id.clone(), job_name) {
            warn!("Failed to register album genre background job: {}", e);
            return;
        }

        info!("Album genre update thread started");

        let total = albums.len();
        info!("Updating genres for {} albums without genre tags", total);

        let _ = acr_store::backgroundjobs::update_job(
            &job_id,
            Some(format!("Starting genre update for {} albums", total)),
            Some(0),
            Some(total),
        );

        let io = LiveSweep { job_id: job_id.clone() };
        let mut sender = BatchSender::new(sink, generation);
        let swept = sweep_albums(albums, &io, &mut sender);

        // What is reported has to be what happened. A sweep that stopped on a
        // refusal used to log the same "complete" line, with a total it never
        // reached; until the staleness check could actually fire, that line
        // could not be wrong.
        match swept.swept.stopped_after {
            Some(reached) => {
                let message = format!(
                    "Album genre update stopped after {} of {}: the library reloaded",
                    reached, total
                );
                info!("{}", message);
                let _ = acr_store::backgroundjobs::update_job(
                    &job_id,
                    Some(message),
                    Some(reached),
                    Some(total),
                );
            }
            None => info!(
                "Album genre update complete: {}/{} albums updated",
                swept.swept.reported, total
            ),
        }
        // A sweep that got no answer for most of what it asked about looks
        // exactly like a sweep over a library with no genres to find, unless it
        // says so. Nothing was written down for these, so they are asked about
        // again next time rather than being lost.
        if swept.unanswered > 0 {
            warn!(
                "{} of {} album genre lookups got no answer from MusicBrainz; \
                 nothing was cached for them and the next sweep will ask again",
                swept.unanswered, total
            );
        }
        // The job is over either way: it ran out of albums or out of library.
        let _ = acr_store::backgroundjobs::complete_job(&job_id);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cached_answer_is_sent_without_a_lookup() {
        assert_eq!(
            plan(Some(vec!["rock".to_string()]), "The Beatles", "Abbey Road"),
            Plan::Send(vec!["rock".to_string()])
        );
    }

    /// The cache records lookups that found nothing, and that record is the
    /// only thing stopping every library load from repeating them.
    #[test]
    fn a_cached_empty_answer_is_not_looked_up_again() {
        assert_eq!(plan(Some(vec![]), "The Beatles", "Abbey Road"), Plan::Skip);
    }

    #[test]
    fn an_album_nothing_is_known_about_is_looked_up() {
        assert_eq!(plan(None, "The Beatles", "Abbey Road"), Plan::Fetch);
    }

    /// A search needs both halves. Without them the album is recorded as
    /// unanswerable rather than searched for with a blank.
    #[test]
    fn an_album_with_no_artist_or_no_name_is_recorded_as_empty() {
        assert_eq!(plan(None, "", "Abbey Road"), Plan::RecordEmpty);
        assert_eq!(plan(None, "The Beatles", ""), Plan::RecordEmpty);
    }

    use acr_types::enrichment::{Applied, EnrichmentBatch, EnrichmentError};
    use parking_lot::Mutex;
    use std::sync::Arc;

    /// A sweep world with no clock, no network and no cache: it records what
    /// the sweep asked of it.
    struct FakeSweep {
        /// Genres the cache answers with, by album id.
        cached: Vec<(String, Vec<String>)>,
        /// Genres a lookup would find, by album id.
        findable: Vec<(String, Vec<String>)>,
        /// Album ids whose lookup gets no answer at all — the provider is down,
        /// or disabled.
        unanswered: Vec<String>,
        seen: Mutex<Log>,
    }

    #[derive(Default)]
    struct Log {
        started: Vec<String>,
        fetched: Vec<String>,
        recorded_empty: Vec<String>,
        milestones: Vec<(usize, usize, usize)>,
        paced: usize,
    }

    impl FakeSweep {
        fn new(cached: Vec<(&str, Vec<&str>)>, findable: Vec<(&str, Vec<&str>)>) -> Self {
            let own = |v: Vec<(&str, Vec<&str>)>| {
                v.into_iter()
                    .map(|(id, g)| (id.to_string(), g.into_iter().map(String::from).collect()))
                    .collect()
            };
            FakeSweep {
                cached: own(cached),
                findable: own(findable),
                unanswered: Vec::new(),
                seen: Mutex::new(Log::default()),
            }
        }

        /// A world where the lookup for these albums gets no answer.
        fn unanswered(mut self, ids: Vec<&str>) -> Self {
            self.unanswered = ids.into_iter().map(String::from).collect();
            self
        }
    }

    impl Sweep for FakeSweep {
        fn cached_genres(&self, album_id: &str) -> Option<Vec<String>> {
            self.cached.iter().find(|(id, _)| id == album_id).map(|(_, g)| g.clone())
        }
        fn record_no_genres(&self, album_id: &str) {
            self.seen.lock().recorded_empty.push(album_id.to_string());
        }
        fn fetch_genres(&self, album_id: &str, _artist: &str, _album: &str) -> GenreLookup {
            self.seen.lock().fetched.push(album_id.to_string());
            if self.unanswered.iter().any(|id| id == album_id) {
                return GenreLookup::Unavailable("503 Service Unavailable".to_string());
            }
            GenreLookup::Answered(
                self.findable
                    .iter()
                    .find(|(id, _)| id == album_id)
                    .map(|(_, g)| g.clone())
                    .unwrap_or_default(),
            )
        }
        fn starting(&self, album_name: &str, _index: usize, _total: usize) {
            self.seen.lock().started.push(album_name.to_string());
        }
        fn milestone(&self, count: usize, total: usize, updated: usize) {
            self.seen.lock().milestones.push((count, total, updated));
        }
        fn pace(&self) {
            self.seen.lock().paced += 1;
        }
    }

    #[derive(Default)]
    struct Recording(Mutex<Vec<EnrichmentBatch>>);

    impl EnrichmentSink for Recording {
        fn apply(&self, batch: EnrichmentBatch) -> Result<Applied, EnrichmentError> {
            self.0.lock().push(batch);
            Ok(Applied::default())
        }
    }

    fn album(id: &str) -> AlbumRef {
        AlbumRef {
            id: id.to_string(),
            name: format!("Album {}", id),
            artist: "The Beatles".to_string(),
        }
    }

    /// The sweep every restart runs: everything already answered. It must cost
    /// nothing — no lookups, and no pacing, which is the expensive half. At
    /// fifty milliseconds an album, pacing a warm cache would put a ten
    /// thousand album library eight minutes behind for no requests at all.
    #[test]
    fn a_warm_cache_costs_no_lookups_and_no_pacing() {
        let albums: Vec<AlbumRef> = (0..120).map(|i| album(&i.to_string())).collect();
        let cached: Vec<(String, Vec<String>)> = (0..120)
            .map(|i| (i.to_string(), vec!["rock".to_string()]))
            .collect();
        let io = FakeSweep {
            cached,
            findable: Vec::new(),
            unanswered: Vec::new(),
            seen: Mutex::new(Log::default()),
        };
        let sink = Arc::new(Recording::default());
        let mut sender = BatchSender::new(sink.clone(), None);

        let swept = sweep_albums(albums, &io, &mut sender);

        let seen = io.seen.lock();
        assert_eq!(swept.swept.reported, 120);
        assert_eq!(swept.swept.stopped_after, None, "nothing refused it");
        assert_eq!(seen.started.len(), 120, "every album is still announced");
        assert!(seen.fetched.is_empty(), "a cached answer must not be looked up");
        assert_eq!(seen.paced, 0, "an album that cost no request must not be paced");
        assert!(
            seen.milestones.is_empty(),
            "milestones report requests, and none were made"
        );
    }

    /// A cached empty answer and an album with nothing to search with are the
    /// other two branches that cost no request, and they must not be paced
    /// either.
    #[test]
    fn albums_that_cannot_be_looked_up_are_not_paced() {
        let mut nameless = album("2");
        nameless.artist = String::new();
        let albums = vec![album("1"), nameless];
        let io = FakeSweep::new(vec![("1", vec![])], vec![]);
        let sink = Arc::new(Recording::default());
        let mut sender = BatchSender::new(sink.clone(), None);

        let swept = sweep_albums(albums, &io, &mut sender);

        let seen = io.seen.lock();
        assert_eq!(swept.swept.reported, 0);
        assert_eq!(seen.paced, 0);
        assert!(seen.milestones.is_empty());
        assert_eq!(seen.recorded_empty, vec!["2"], "the unsearchable album is recorded");
        assert!(sink.0.lock().is_empty(), "nothing to send is not sent");
    }

    /// An album that did cost a lookup is paced, and reaches the milestone on
    /// the same schedule as before — every fiftieth album and the last one.
    #[test]
    fn an_album_that_costs_a_lookup_is_paced_and_reaches_the_milestone() {
        let albums: Vec<AlbumRef> = (0..50).map(|i| album(&i.to_string())).collect();
        let findable: Vec<(&str, Vec<&str>)> = vec![];
        let io = FakeSweep::new(vec![], findable);
        let sink = Arc::new(Recording::default());
        let mut sender = BatchSender::new(sink.clone(), None);

        sweep_albums(albums, &io, &mut sender);

        let seen = io.seen.lock();
        assert_eq!(seen.fetched.len(), 50);
        assert_eq!(seen.paced, 50, "every request is paced");
        assert_eq!(
            seen.milestones,
            vec![(50, 50, 0)],
            "the fiftieth album is also the last, so one milestone covers both"
        );
    }

    /// Results accumulate and flush at the batch boundary, not one per album:
    /// a library bumps its version once per batch, so flushing per album would
    /// invalidate every client's cached list once per album.
    #[test]
    fn results_accumulate_and_flush_at_the_batch_boundary() {
        let albums: Vec<AlbumRef> = (0..120).map(|i| album(&i.to_string())).collect();
        let cached: Vec<(String, Vec<String>)> = (0..120)
            .map(|i| (i.to_string(), vec!["rock".to_string()]))
            .collect();
        let io = FakeSweep {
            cached,
            findable: Vec::new(),
            unanswered: Vec::new(),
            seen: Mutex::new(Log::default()),
        };
        let sink = Arc::new(Recording::default());
        let mut sender = BatchSender::new(sink.clone(), None);

        sweep_albums(albums, &io, &mut sender);

        let batches = sink.0.lock();
        let sizes: Vec<usize> = batches.iter().map(|b| b.albums.len()).collect();
        assert_eq!(sizes, vec![BATCH_SIZE, BATCH_SIZE, 20], "two full batches, then the rest");
        assert_eq!(batches[0].albums[0].id, "0", "and in the order they were swept");
        assert_eq!(batches[2].albums[19].id, "119");
    }

    /// A refusal stops the sweep where it stands: the remaining albums are not
    /// looked up, and the partial batch is not sent to a library that has said
    /// it will not take it.
    #[test]
    fn a_refused_batch_stops_the_sweep() {
        struct Refusing;
        impl EnrichmentSink for Refusing {
            fn apply(&self, _batch: EnrichmentBatch) -> Result<Applied, EnrichmentError> {
                Err(EnrichmentError::Stale {
                    current_generation: None,
                })
            }
        }

        let albums: Vec<AlbumRef> = (0..120).map(|i| album(&i.to_string())).collect();
        let cached: Vec<(String, Vec<String>)> = (0..120)
            .map(|i| (i.to_string(), vec!["rock".to_string()]))
            .collect();
        let io = FakeSweep {
            cached,
            findable: Vec::new(),
            unanswered: Vec::new(),
            seen: Mutex::new(Log::default()),
        };
        let mut sender = BatchSender::new(Arc::new(Refusing), Some("g1".to_string()));

        let swept = sweep_albums(albums, &io, &mut sender);

        assert_eq!(swept.swept.reported, BATCH_SIZE, "it stopped at the first flush");
        assert_eq!(
            swept.swept.stopped_after,
            Some(BATCH_SIZE),
            "and the caller is told where it stopped, not that it finished"
        );
        assert_eq!(
            io.seen.lock().started.len(),
            BATCH_SIZE,
            "the albums after the refusal were never considered"
        );
    }

    /// A lookup that got no answer says nothing about the album. Writing it
    /// down anyway is what turned one afternoon of MusicBrainz answering 503
    /// into albums permanently recorded as having no genres, never asked about
    /// again on any later sweep or restart.
    #[test]
    fn a_lookup_that_got_no_answer_is_not_written_down() {
        crate::test_support::init_test_caches();
        let album_id = "album-whose-lookup-failed";
        let _ = acr_store::attributecache::remove(&cache_key(album_id));

        let outcome = fetch_album_genres_with(album_id, "The Beatles", "Abbey Road", |_, _| {
            GenreLookup::Unavailable("503 Service Unavailable".to_string())
        });

        assert_eq!(
            outcome,
            GenreLookup::Unavailable("503 Service Unavailable".to_string())
        );
        assert_eq!(
            load_cached_genres(album_id),
            None,
            "a failed lookup must leave the album unknown, not recorded as genre-less"
        );
        assert_eq!(
            plan(load_cached_genres(album_id), "The Beatles", "Abbey Road"),
            Plan::Fetch,
            "so the next sweep asks again"
        );
    }

    /// The other half, and the one a fix that simply stopped caching emptiness
    /// would break: many albums really have no genres in MusicBrainz, and
    /// remembering that is the only thing keeping every library load from
    /// re-asking about all of them at one request per second.
    #[test]
    fn a_genuine_empty_answer_is_written_down() {
        crate::test_support::init_test_caches();
        let album_id = "album-with-no-genres-in-musicbrainz";
        let _ = acr_store::attributecache::remove(&cache_key(album_id));

        let outcome = fetch_album_genres_with(album_id, "The Beatles", "Abbey Road", |_, _| {
            GenreLookup::Answered(Vec::new())
        });

        assert_eq!(outcome, GenreLookup::Answered(Vec::new()));
        assert_eq!(
            load_cached_genres(album_id),
            Some(Vec::new()),
            "the provider answered, and an empty answer is a fact worth keeping"
        );
        assert_eq!(
            plan(load_cached_genres(album_id), "The Beatles", "Abbey Road"),
            Plan::Skip,
            "so it is not asked about again"
        );
    }

    #[test]
    fn an_answer_with_genres_is_written_down_and_returned() {
        crate::test_support::init_test_caches();
        let album_id = "album-with-genres";
        let _ = acr_store::attributecache::remove(&cache_key(album_id));

        let outcome = fetch_album_genres_with(album_id, "The Beatles", "Abbey Road", |_, _| {
            GenreLookup::Answered(vec!["rock".to_string()])
        });

        assert_eq!(outcome, GenreLookup::Answered(vec!["rock".to_string()]));
        assert_eq!(load_cached_genres(album_id), Some(vec!["rock".to_string()]));
    }

    #[test]
    fn a_cached_answer_is_not_asked_of_the_provider() {
        crate::test_support::init_test_caches();
        let album_id = "album-already-known";
        store_cached_genres(album_id, &["jazz".to_string()]);

        let mut asked = false;
        let outcome = fetch_album_genres_with(album_id, "Miles Davis", "Kind of Blue", |_, _| {
            asked = true;
            GenreLookup::Unavailable("should not have been reached".to_string())
        });

        assert!(!asked, "a cached answer must not cost a request");
        assert_eq!(outcome, GenreLookup::Answered(vec!["jazz".to_string()]));
    }

    /// A sweep that could not get answers must say so rather than report the
    /// same quiet zero as a sweep over a library with no genres to find: with
    /// nothing written down, those albums are asked about again, and an
    /// operator reading the log is the only one who can tell that from a
    /// library that has nothing to find.
    #[test]
    fn lookups_with_no_answer_are_counted_and_do_not_stop_the_sweep() {
        let albums = vec![album("1"), album("2"), album("3")];
        let io = FakeSweep::new(vec![], vec![("3", vec!["rock"])]).unanswered(vec!["1", "2"]);
        let sink = Arc::new(Recording::default());
        let mut sender = BatchSender::new(sink.clone(), None);

        let swept = sweep_albums(albums, &io, &mut sender);

        assert_eq!(swept.unanswered, 2, "two lookups got no answer");
        assert_eq!(swept.swept.reported, 1, "only the answered album is reported");
        assert_eq!(
            swept.swept.stopped_after, None,
            "an unhealthy provider is not a reason to abandon the sweep"
        );
        assert_eq!(
            io.seen.lock().fetched,
            vec!["1", "2", "3"],
            "every album was still asked about"
        );
        let batches = sink.0.lock();
        assert_eq!(batches.len(), 1, "one flush at the end");
        assert_eq!(
            batches[0].albums.len(),
            1,
            "nothing is sent for an album with no answer"
        );
        assert_eq!(batches[0].albums[0].id, "3");
    }

    #[test]
    fn the_cache_key_names_the_album_id() {
        assert_eq!(cache_key("42"), "album::genres::42");
    }
}
