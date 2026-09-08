//! The metadata side of library enrichment.
//!
//! This side reads a library's lists over HTTP, looks everything up, and posts
//! the results back in batches; everything between the two — which services are
//! asked, in what order, how often, what is cached — is here and invisible to
//! the player side. The player side never calls in.
//!
//! **What used to be here and is not.** Four questions the player half asked
//! through `LibraryEnricher`: the summary already known for an artist, the
//! genres already known for an album, an artist's full metadata, and an
//! artist's image bytes. Each was the main daemon calling the metadata daemon,
//! and the one-way seam forbids that. The first two were already answered
//! `None` over HTTP by contract — they run while a library loads and may not do
//! network I/O — so deleting them cost nothing. The other two were live, and
//! what replaced them is this module's own output: [`ArtistSummary`] now
//! carries the biography, its source and the banner alongside the thumbnails,
//! so the artist detail routes serve what a batch delivered rather than
//! fetching it per request, and the artist image route redirects to
//! `/coverart/artist/<b64>/image`, which is this side's own route and the one
//! the artist lists have always pointed at.

use acr_types::enrichment::*;
use acr_types::ArtistMeta;
use log::{debug, info};
use std::sync::Arc;

/// The cache key an artist's metadata is stored under. One spelling, because a
/// reader that disagrees with the writer silently finds nothing.
pub(crate) fn artist_metadata_key(name: &str) -> String {
    format!("artist::metadata::{}", name)
}

/// Everything already known about an artist, or `None`.
pub(crate) fn cached_artist_metadata(name: &str) -> Option<ArtistMeta> {
    acr_store::attributecache::get(&artist_metadata_key(name))
        .ok()
        .flatten()
}

/// Starting a sweep over one library.
///
/// This trait used to live in `acr_types::enrichment` and carry four more
/// methods, because the player half held an `Arc<dyn LibraryEnricher>` and
/// asked it what was already known about an artist or an album. Those
/// questions were the main daemon calling the metadata daemon, which the
/// one-way seam forbids, and the answers travel in the batch now. What is left
/// is one side of this crate talking to another, kept as a trait only so that
/// [`library_puller`](crate::library_puller) can be tested without starting a
/// sweep that reaches MusicBrainz.
pub trait LibraryEnricher: Send + Sync {
    /// Start enriching a library. Returns at once; results arrive through the sink.
    ///
    /// `generation` is the library generation every batch of this sweep will
    /// name. It is fixed for the life of the sweep: a library that is rebuilt
    /// meanwhile refuses the next batch, which ends the sweep, and the rebuild
    /// asks again for whatever it now needs.
    fn enrich(
        &self,
        player: &str,
        generation: Option<String>,
        artists: Vec<ArtistRef>,
        albums: Vec<AlbumRef>,
        sink: Arc<dyn EnrichmentSink>,
    );
}

/// Enrichment through the in-process updaters.
pub struct InProcessEnricher;

impl LibraryEnricher for InProcessEnricher {
    fn enrich(
        &self,
        player: &str,
        generation: Option<String>,
        artists: Vec<ArtistRef>,
        albums: Vec<AlbumRef>,
        sink: Arc<dyn EnrichmentSink>,
    ) {
        start_sweeps(
            player,
            generation,
            artists,
            albums,
            sink,
            &crate::artistupdater::enrich_artists_in_background,
            &crate::albumupdater::enrich_albums_in_background,
        );
    }
}

/// Start the two sweeps a library needs, on one generation.
///
/// **Both are given the *same* generation, and that is the whole point.** It is
/// what makes them independent: neither sweep's writes move the generation, so
/// neither can make the other's next batch look stale. Only a reload of the
/// library refuses either, which is exactly when both should stop. Give the
/// album sweep a generation of its own and the symptom is oblique — album
/// genres stop after one batch on any library that also has artists.
///
/// The two starters are arguments rather than called directly so that this
/// hand-out can be watched. Each real one spawns a thread that talks to
/// MusicBrainz, which is why the invariant went untested through two rounds:
/// there was no seam between "one generation is chosen" and "the network is
/// reached".
fn start_sweeps(
    player: &str,
    generation: Option<String>,
    artists: Vec<ArtistRef>,
    albums: Vec<AlbumRef>,
    sink: Arc<dyn EnrichmentSink>,
    start_artists: &dyn Fn(String, Option<String>, Vec<ArtistRef>, Arc<dyn EnrichmentSink>),
    start_albums: &dyn Fn(String, Option<String>, Vec<AlbumRef>, Arc<dyn EnrichmentSink>),
) {
    if !artists.is_empty() {
        start_artists(
            player.to_string(),
            generation.clone(),
            artists,
            sink.clone(),
        );
    }
    if !albums.is_empty() {
        start_albums(player.to_string(), generation, albums, sink);
    }
}

/// How many results accumulate before a batch is sent.
///
/// The trade is between how long a client waits to see a lookup and how often
/// every client's cached list is invalidated: a library bumps its version once
/// per batch that changed something, so sending one result at a time would
/// invalidate every cached list once per artist.
pub const BATCH_SIZE: usize = 50;

/// Accumulates results and hands them to a library, one batch at a time.
///
/// Both updaters send through this rather than each keeping its own copy of
/// what to do about the version and about a refusal.
pub struct BatchSender {
    sink: Arc<dyn EnrichmentSink>,
    /// The library generation every batch of this sweep names, or `None` when
    /// the caller supplied none.
    ///
    /// Fixed for the life of the sweep, and never adopted from a reply: a
    /// generation moves only when the library is reloaded, and a reload is
    /// precisely the thing this sweep must stop for rather than follow. The
    /// version in a reply is a different token with a different job — the
    /// caller's "seen" bookkeeping — and taking it from a reply is what used to
    /// be needed to survive the sweep's own version bumps.
    generation: Option<String>,
}

/// What a sweep got through, so its caller can log and complete its background
/// job truthfully rather than reporting a total it did not reach.
pub(crate) struct Swept {
    /// How many entries the sweep had something to say about.
    pub(crate) reported: usize,
    /// `Some(n)` when the library refused a batch and the sweep stopped after
    /// considering `n` entries; `None` when it ran to the end.
    pub(crate) stopped_after: Option<usize>,
}

impl BatchSender {
    pub fn new(sink: Arc<dyn EnrichmentSink>, generation: Option<String>) -> Self {
        BatchSender { sink, generation }
    }

    /// Send one batch.
    ///
    /// Returns `false` when the library refused it because it has since
    /// reloaded: the caller stops, and the reload's own request asks again for
    /// whatever it now needs. An empty batch is not sent at all.
    pub fn send(&mut self, artists: Vec<ArtistSummary>, albums: Vec<AlbumGenres>) -> bool {
        if artists.is_empty() && albums.is_empty() {
            return true;
        }
        let batch = EnrichmentBatch {
            library_generation: self.generation.clone(),
            artists,
            albums,
        };
        match self.sink.apply(batch) {
            Ok(applied) => {
                debug!(
                    "Applied {} artist(s) and {} album(s)",
                    applied.artists, applied.albums
                );
                true
            }
            Err(EnrichmentError::Stale { current_generation }) => {
                info!(
                    "Library reloaded while enriching (generation now {:?}); stopping, the reload asks again",
                    current_generation
                );
                false
            }
            Err(EnrichmentError::NoSuchLibrary) => {
                info!("The library being enriched is gone; stopping");
                false
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use parking_lot::Mutex;

    struct Recording {
        batches: Mutex<Vec<EnrichmentBatch>>,
        reply: Mutex<Vec<Result<Applied, EnrichmentError>>>,
    }

    impl Recording {
        fn new(reply: Vec<Result<Applied, EnrichmentError>>) -> Arc<Self> {
            Arc::new(Recording {
                batches: Mutex::new(Vec::new()),
                reply: Mutex::new(reply),
            })
        }
    }

    impl EnrichmentSink for Recording {
        fn apply(&self, batch: EnrichmentBatch) -> Result<Applied, EnrichmentError> {
            self.batches.lock().push(batch);
            let mut replies = self.reply.lock();
            if replies.is_empty() {
                Ok(Applied::default())
            } else {
                replies.remove(0)
            }
        }
    }

    fn summary(name: &str) -> ArtistSummary {
        ArtistSummary {
            name: name.to_string(),
            ..Default::default()
        }
    }

    // The MIME mapping this module used to hold went with `artist_image`. The
    // one that survives is `api::coverart::serve_artist_image_file`'s, which
    // has always answered the client-facing route and now answers the
    // redirected one too.

    #[test]
    fn an_empty_batch_is_not_sent() {
        let sink = Recording::new(vec![]);
        let mut sender = BatchSender::new(sink.clone(), None);

        assert!(sender.send(vec![], vec![]));
        assert!(sink.batches.lock().is_empty());
    }

    /// A caller that named no generation keeps naming none, whatever the
    /// library hands back.
    #[test]
    fn a_sender_with_no_generation_never_names_one() {
        let sink = Recording::new(vec![Ok(Applied {
            artists: 1,
            albums: 0,
            library_version: Some("v2".to_string()),
        })]);
        let mut sender = BatchSender::new(sink.clone(), None);

        assert!(sender.send(vec![summary("a")], vec![]));
        assert!(sender.send(vec![summary("b")], vec![]));

        let batches = sink.batches.lock();
        assert_eq!(batches.len(), 2);
        assert_eq!(batches[1].library_generation, None);
    }

    /// The regression this whole design exists for. Both sweeps run against one
    /// library and one generation; neither may stop the other, and a version
    /// coming back in a reply must not be taken for the next batch's
    /// generation. Before the generation, the album sweep's first batch was
    /// refused because the artist sweep had already bumped the version — so the
    /// sink here answers each batch with a *different* version, the way a real
    /// library does.
    #[test]
    fn a_sweep_over_a_library_with_both_artists_and_albums_names_one_generation() {
        let sink = Recording::new(vec![
            Ok(Applied {
                artists: 1,
                albums: 0,
                library_version: Some("v2".to_string()),
            }),
            Ok(Applied {
                artists: 0,
                albums: 1,
                library_version: Some("v3".to_string()),
            }),
        ]);
        let generation = Some("g1".to_string());
        let mut sender = BatchSender::new(sink.clone(), generation.clone());

        assert!(sender.send(vec![summary("Pink Floyd")], vec![]));
        assert!(sender.send(
            vec![],
            vec![AlbumGenres {
                id: "1".to_string(),
                genres: vec!["rock".to_string()],
            }]
        ));

        let batches = sink.batches.lock();
        assert_eq!(batches.len(), 2, "both sweeps' batches reached the library");
        assert!(
            batches.iter().all(|b| b.library_generation == generation),
            "the generation a sweep was started with is the generation every \
             one of its batches names, whatever version the library reports \
             back: got {:?}",
            batches
                .iter()
                .map(|b| b.library_generation.clone())
                .collect::<Vec<_>>()
        );
    }

    /// What the test above does *not* cover, because it builds one
    /// `BatchSender` by hand: that the fan-out hands the two sweeps one
    /// generation rather than two. Both halves must be watched at the point
    /// they are started, or a change giving the album sweep a generation of its
    /// own passes the whole suite.
    #[test]
    fn both_sweeps_are_started_on_the_one_generation_they_were_given() {
        let started: Mutex<Vec<(&'static str, Option<String>)>> = Mutex::new(Vec::new());
        let generation = Some("g1".to_string());

        start_sweeps(
            "mpd",
            generation.clone(),
            vec![ArtistRef::named("7".to_string(), "Pink Floyd".to_string())],
            vec![AlbumRef {
                id: "1".to_string(),
                name: "Animals".to_string(),
                artist: "Pink Floyd".to_string(),
            }],
            Recording::new(vec![]),
            &|_, g, _, _| started.lock().push(("artists", g)),
            &|_, g, _, _| started.lock().push(("albums", g)),
        );

        let started = started.lock();
        assert_eq!(
            started.len(),
            2,
            "a library with both needs both sweeps: {:?}",
            started
        );
        assert_eq!(
            started[0].1, started[1].1,
            "the two sweeps must run against one generation, or each makes the \
             other's next batch look stale: {:?}",
            started
        );
        assert_eq!(started[0].1, generation, "and it is the one handed in");
    }

    /// The other half of the fan-out: nothing to sweep starts no sweep. A
    /// thread, a background job and a MusicBrainz sweep over an empty list is
    /// all cost and no answer.
    #[test]
    fn a_sweep_is_not_started_for_a_list_that_is_empty() {
        let started: Mutex<Vec<&'static str>> = Mutex::new(Vec::new());

        start_sweeps(
            "mpd",
            None,
            Vec::new(),
            vec![AlbumRef {
                id: "1".to_string(),
                name: "Animals".to_string(),
                artist: "Pink Floyd".to_string(),
            }],
            Recording::new(vec![]),
            &|_, _, _, _| started.lock().push("artists"),
            &|_, _, _, _| started.lock().push("albums"),
        );

        assert_eq!(*started.lock(), vec!["albums"]);
    }

    /// A refusal ends the sweep. Carrying on would spend a MusicBrainz request
    /// per artist on a library that will not take the answers.
    #[test]
    fn a_stale_refusal_stops_the_sweep() {
        let sink = Recording::new(vec![Err(EnrichmentError::Stale {
            current_generation: Some("g9".to_string()),
        })]);
        let mut sender = BatchSender::new(sink.clone(), Some("g1".to_string()));

        assert!(!sender.send(vec![summary("a")], vec![]));
    }
}
