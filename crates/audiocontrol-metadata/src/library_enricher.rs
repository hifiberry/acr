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
        })
    }

    fn artist_detail(&self, name: &str) -> Option<ArtistMeta> {
        cached_artist_metadata(name)
    }

    fn enrich(
        &self,
        player: &str,
        version: Option<String>,
        artists: Vec<ArtistRef>,
        albums: Vec<AlbumRef>,
        sink: Arc<dyn EnrichmentSink>,
    ) {
        if !artists.is_empty() {
            crate::artistupdater::enrich_artists_in_background(
                player.to_string(),
                version.clone(),
                artists,
                sink.clone(),
            );
        }
        if !albums.is_empty() {
            crate::albumupdater::enrich_albums_in_background(player.to_string(), version, albums, sink);
        }
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
    /// The version the next batch names, or `None` when the caller supplied
    /// none. It is only ever adopted from a reply when one was supplied in the
    /// first place: an in-process caller passes `None` because two updaters run
    /// against one version counter and would read each other's bumps as a
    /// reload, and taking a version back from the first reply would put it
    /// straight back into that state.
    version: Option<String>,
    versioned: bool,
}

impl BatchSender {
    pub fn new(sink: Arc<dyn EnrichmentSink>, version: Option<String>) -> Self {
        BatchSender {
            sink,
            versioned: version.is_some(),
            version,
        }
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
            library_version: self.version.clone(),
            artists,
            albums,
        };
        match self.sink.apply(batch) {
            Ok(applied) => {
                debug!(
                    "Applied {} artist(s) and {} album(s)",
                    applied.artists, applied.albums
                );
                if self.versioned {
                    self.version = applied.library_version;
                }
                true
            }
            Err(EnrichmentError::Stale { current }) => {
                info!(
                    "Library moved on while enriching (now {:?}); stopping, the reload asks again",
                    current
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
            mbid: vec![],
            is_multi: false,
            genres: vec![],
        }
    }

    #[test]
    fn an_empty_batch_is_not_sent() {
        let sink = Recording::new(vec![]);
        let mut sender = BatchSender::new(sink.clone(), None);

        assert!(sender.send(vec![], vec![]));
        assert!(sink.batches.lock().is_empty());
    }

    /// A caller that named no version keeps naming none, whatever the library
    /// hands back: adopting a version here is what would make the second batch
    /// of two concurrent updaters look stale.
    #[test]
    fn an_unversioned_sender_never_adopts_a_version() {
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
        assert_eq!(batches[1].library_version, None);
    }

    /// A caller that did name one follows the library forward, so its next
    /// batch names the version its own last batch produced.
    #[test]
    fn a_versioned_sender_follows_the_library_forward() {
        let sink = Recording::new(vec![Ok(Applied {
            artists: 1,
            albums: 0,
            library_version: Some("v2".to_string()),
        })]);
        let mut sender = BatchSender::new(sink.clone(), Some("v1".to_string()));

        assert!(sender.send(vec![summary("a")], vec![]));
        assert!(sender.send(vec![summary("b")], vec![]));

        let batches = sink.batches.lock();
        assert_eq!(batches[0].library_version, Some("v1".to_string()));
        assert_eq!(batches[1].library_version, Some("v2".to_string()));
    }

    /// A refusal ends the sweep. Carrying on would spend a MusicBrainz request
    /// per artist on a library that will not take the answers.
    #[test]
    fn a_stale_refusal_stops_the_sweep() {
        let sink = Recording::new(vec![Err(EnrichmentError::Stale {
            current: Some("v9".to_string()),
        })]);
        let mut sender = BatchSender::new(sink.clone(), Some("v1".to_string()));

        assert!(!sender.send(vec![summary("a")], vec![]));
    }
}
