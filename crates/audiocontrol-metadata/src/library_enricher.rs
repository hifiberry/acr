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
