use log::{debug, info, warn};
use std::sync::Arc;
use acr_types::enrichment::{AlbumGenres, AlbumRef, EnrichmentSink};
use crate::library_enricher::{BatchSender, BATCH_SIZE};

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
/// Stores the result (even an empty list) in the cache so we don't retry.
pub fn fetch_album_genres(album_id: &str, artist: &str, album_name: &str) -> Vec<String> {
    // Return cached value if present
    if let Some(cached) = load_cached_genres(album_id) {
        debug!("Using cached genres for album '{}': {:?}", album_name, cached);
        return cached;
    }

    // Not cached — fetch from MusicBrainz
    let genres = crate::musicbrainz::search_release_group_genres(artist, album_name);

    info!(
        "Fetched {} genre(s) from MusicBrainz for album '{}' by '{}'",
        genres.len(),
        album_name,
        artist
    );

    // Cache the result (including empty results to avoid repeated lookups)
    store_cached_genres(album_id, &genres);

    genres
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
    fn fetch_genres(&self, album_id: &str, artist: &str, album_name: &str) -> Vec<String>;
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

/// The sweep itself: decide, ask, accumulate, flush.
///
/// Returns how many albums it had something to say about. Stops early, without
/// a final flush, when the library refuses a batch as stale.
fn sweep_albums(albums: Vec<AlbumRef>, io: &dyn Sweep, sender: &mut BatchSender) -> usize {
    let total = albums.len();
    let mut batch: Vec<AlbumGenres> = Vec::with_capacity(BATCH_SIZE);
    let mut updated = 0usize;

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
                    return updated;
                }
                continue;
            }
            Plan::Fetch => {
                let genres = io.fetch_genres(&album_id, &artist, &album_name);
                // An empty answer is not sent: an empty list never overwrites
                // anything, so the entry would be work for the library and no
                // change. `fetch_genres` has already cached the emptiness.
                if !genres.is_empty() {
                    updated += 1;
                    if !accumulate(&mut batch, AlbumGenres { id: album_id, genres }, sender) {
                        return updated;
                    }
                }
            }
        }

        let count = index + 1;
        if count % 50 == 0 || count == total {
            io.milestone(count, total, updated);
        }
        io.pace();
    }

    sender.send(Vec::new(), batch);
    updated
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

    fn record_no_genres(&self, album_id: &str) {
        store_cached_genres(album_id, &[]);
    }

    fn fetch_genres(&self, album_id: &str, artist: &str, album_name: &str) -> Vec<String> {
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
    version: Option<String>,
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
        let mut sender = BatchSender::new(sink, version);
        let updated = sweep_albums(albums, &io, &mut sender);

        info!("Album genre update complete: {}/{} albums updated", updated, total);
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
                seen: Mutex::new(Log::default()),
            }
        }
    }

    impl Sweep for FakeSweep {
        fn cached_genres(&self, album_id: &str) -> Option<Vec<String>> {
            self.cached.iter().find(|(id, _)| id == album_id).map(|(_, g)| g.clone())
        }
        fn record_no_genres(&self, album_id: &str) {
            self.seen.lock().recorded_empty.push(album_id.to_string());
        }
        fn fetch_genres(&self, album_id: &str, _artist: &str, _album: &str) -> Vec<String> {
            self.seen.lock().fetched.push(album_id.to_string());
            self.findable
                .iter()
                .find(|(id, _)| id == album_id)
                .map(|(_, g)| g.clone())
                .unwrap_or_default()
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
            seen: Mutex::new(Log::default()),
        };
        let sink = Arc::new(Recording::default());
        let mut sender = BatchSender::new(sink.clone(), None);

        let updated = sweep_albums(albums, &io, &mut sender);

        let seen = io.seen.lock();
        assert_eq!(updated, 120);
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

        let updated = sweep_albums(albums, &io, &mut sender);

        let seen = io.seen.lock();
        assert_eq!(updated, 0);
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
                Err(EnrichmentError::Stale { current: None })
            }
        }

        let albums: Vec<AlbumRef> = (0..120).map(|i| album(&i.to_string())).collect();
        let cached: Vec<(String, Vec<String>)> = (0..120)
            .map(|i| (i.to_string(), vec!["rock".to_string()]))
            .collect();
        let io = FakeSweep {
            cached,
            findable: Vec::new(),
            seen: Mutex::new(Log::default()),
        };
        let mut sender = BatchSender::new(Arc::new(Refusing), Some("v1".to_string()));

        let updated = sweep_albums(albums, &io, &mut sender);

        assert_eq!(updated, BATCH_SIZE, "it stopped at the first flush");
        assert_eq!(
            io.seen.lock().started.len(),
            BATCH_SIZE,
            "the albums after the refusal were never considered"
        );
    }

    #[test]
    fn the_cache_key_names_the_album_id() {
        assert_eq!(cache_key("42"), "album::genres::42");
    }
}
