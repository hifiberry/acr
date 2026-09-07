//! The metadata side of library enrichment.
//!
//! A library hands over a list of what it has and a sink to answer through;
//! everything between the two — which services are asked, in what order, how
//! often, what is cached — is here and invisible to the player side. Phase 1
//! replaces `InProcessEnricher` with an HTTP client and this module's other
//! half, the batching in `BatchSender`, with the same batches over the wire.
//! The updaters themselves do not change again.

use acr_types::enrichment::*;
use acr_types::ArtistMeta;
use log::{debug, info, warn};
use std::sync::Arc;

/// What an artist image cached under `path` is served as.
///
/// Inferred from the extension, exactly as the MPD library did before this
/// moved: an unrecognised extension is served as JPEG rather than refused,
/// because the store only ever writes files it fetched as images and a client
/// that got a 404 here would show a broken artist instead of a picture.
fn mime_type_for(path: &str) -> String {
    if path.ends_with(".jpg") || path.ends_with(".jpeg") {
        "image/jpeg".to_string()
    } else if path.ends_with(".png") {
        "image/png".to_string()
    } else if path.ends_with(".webp") {
        "image/webp".to_string()
    } else {
        "image/jpeg".to_string() // Default to JPEG
    }
}

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

/// Enrichment through the in-process updaters.
pub struct InProcessEnricher;

impl LibraryEnricher for InProcessEnricher {
    fn artist_summary(&self, name: &str) -> Option<ArtistSummary> {
        let meta = cached_artist_metadata(name)?;
        Some(ArtistSummary {
            name: name.to_string(),
            // More than one MusicBrainz ID, or a lookup that matched only part
            // of the name, means the name covers several artists. The libraries
            // used to derive this themselves from the same cached metadata.
            is_multi: meta.mbid.len() > 1 || meta.is_partial_match,
            mbid: meta.mbid,
            genres: meta.genres,
            // Carried as stored. The artist list route serves this field, and
            // it holds a URL only for an artist an image was actually found
            // for, so an empty list is meaningful rather than missing.
            thumb_url: meta.thumb_url,
        })
    }

    fn artist_detail(&self, name: &str) -> Option<ArtistMeta> {
        cached_artist_metadata(name)
    }

    /// Moved here verbatim from the MPD library's `get_artist_cover`: the same
    /// two store calls in the same order, the same "download only if nothing
    /// was cached", and the same MIME inference. A cached file that cannot be
    /// read is still followed by the download attempt, as it was.
    fn artist_image(&self, name: &str) -> Option<(Vec<u8>, String)> {
        debug!("Getting artist cover for: {}", name);

        // Use the artist store to get the cached image path
        if let Some(cache_path) = crate::artist_store::get_artist_cached_image(name) {
            debug!("Found cached artist image at: {}", cache_path);

            // Read the image data from the cache file
            if let Ok(image_data) = std::fs::read(&cache_path) {
                let mime_type = mime_type_for(&cache_path);
                debug!(
                    "Successfully loaded artist image for {}: {} bytes, MIME: {}",
                    name,
                    image_data.len(),
                    mime_type
                );
                return Some((image_data, mime_type));
            } else {
                warn!("Failed to read cached artist image from: {}", cache_path);
            }
        }

        // If no cached image found, try to download one
        if let Some(cache_path) = crate::artist_store::get_or_download_artist_image(name) {
            debug!("Downloaded new artist image at: {}", cache_path);

            // Read the newly downloaded image
            if let Ok(image_data) = std::fs::read(&cache_path) {
                let mime_type = mime_type_for(&cache_path);
                debug!(
                    "Successfully loaded downloaded artist image for {}: {} bytes, MIME: {}",
                    name,
                    image_data.len(),
                    mime_type
                );
                return Some((image_data, mime_type));
            } else {
                warn!("Failed to read downloaded artist image from: {}", cache_path);
            }
        }

        debug!("No artist cover found for: {}", name);
        None
    }

    fn album_genres(&self, album_id: &str) -> Option<Vec<String>> {
        crate::albumupdater::load_cached_genres(album_id)
    }

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

    /// The artist image route serves whatever this returns, so the mapping is
    /// pinned: it is the one the MPD library did inline before `artist_image`
    /// existed, extension by extension, including the JPEG it falls back to.
    #[test]
    fn the_artist_image_mime_type_comes_from_the_extension() {
        assert_eq!(mime_type_for("/cache/a.jpg"), "image/jpeg");
        assert_eq!(mime_type_for("/cache/a.jpeg"), "image/jpeg");
        assert_eq!(mime_type_for("/cache/a.png"), "image/png");
        assert_eq!(mime_type_for("/cache/a.webp"), "image/webp");
        assert_eq!(mime_type_for("/cache/a.gif"), "image/jpeg");
        assert_eq!(mime_type_for("/cache/no-extension"), "image/jpeg");
    }

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
            vec![ArtistRef {
                id: "7".to_string(),
                name: "Pink Floyd".to_string(),
            }],
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
