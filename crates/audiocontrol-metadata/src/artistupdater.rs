use log::{debug, info, warn};
use acr_types::artist::Artist;
use acr_types::enrichment::{ArtistRef, ArtistSummary, EnrichmentSink};
use acr_types::Identifier;
use crate::library_enricher::{cached_artist_metadata, BatchSender, BATCH_SIZE};
use crate::musicbrainz::{search_mbids_for_artist, MusicBrainzSearchResult};
use crate::ArtistUpdater;
use std::sync::Arc;

/// Looks up MusicBrainz IDs for an artist and returns them if found
/// 
/// This function searches for MusicBrainz IDs associated with the given artist name.
/// 
/// # Arguments
/// * `artist_name` - The name of the artist to look up
/// 
/// # Returns
/// A tuple containing:
/// * `Vec<String>` - Vector of MusicBrainz IDs if found, empty vector otherwise
/// * `bool` - true if this is a partial match (only some artists in a multi-artist name found)
pub fn lookup_artist_mbids(artist_name: &str) -> (Vec<String>, bool) {
    debug!("Looking up MusicBrainz IDs for artist: {}", artist_name);
    
    // Try to retrieve MusicBrainz ID using search_mbids_for_artist function
    // This is now a fully synchronous call since we replaced musicbrainz_rs with direct HTTP
    let search_result = search_mbids_for_artist(artist_name, true, false, true);
    
    match search_result {
        MusicBrainzSearchResult::Found(mbids, _) => {
            debug!("Found {} MusicBrainz ID(s) for artist {}: {:?}", 
                  mbids.len(), artist_name, mbids);
            (mbids, false) // Complete match
        },
        MusicBrainzSearchResult::FoundPartial(mbids, _) => {
            info!("Found {} partial MusicBrainz ID(s) for multi-artist {}: {:?}", 
                  mbids.len(), artist_name, mbids);
            (mbids, true) // Partial match
        },
        MusicBrainzSearchResult::NotFound => {
            info!("No MusicBrainz ID found for artist: {}", artist_name);
            (Vec::new(), false)
        },
        MusicBrainzSearchResult::Error(error) => {
            warn!("Error retrieving MusicBrainz ID for artist {}: {}", artist_name, error);
            (Vec::new(), false)
        }
    }
}

/// Download and cache artist images using the cover art system
/// 
/// This function retrieves artist images using the new artist store module.
/// 
/// # Arguments
/// * `artist` - The artist to update with cover art
/// 
/// # Returns
/// The updated artist with image URLs in metadata
fn update_artist_with_coverart(artist: Artist) -> Artist {
    debug!("Updating artist {} with cover art system", artist.name);
    
    // Use the new artist store to handle cover art
    crate::artist_store::update_artist_with_coverart(artist)
}

/// Updates artist data by fetching additional information like MusicBrainz IDs
/// 
/// This function takes an artist and attempts to retrieve and set any missing data
/// such as MusicBrainz IDs.
/// 
/// # Arguments
/// * `artist` - The artist to update
/// 
/// # Returns
/// The updated artist
pub fn update_data_for_artist(mut artist: Artist) -> Artist {
    debug!("Updating data for artist: {}", artist.name);
    
    // Check if the artist already has MusicBrainz IDs set
    let has_mbid = match &artist.metadata {
        Some(meta) => !meta.mbid.is_empty(),
        None => false,
    };
      if !has_mbid {
        debug!("No MusicBrainz ID set for artist {}, attempting to retrieve it", artist.name);
        
        // Use the synchronous function to look up MusicBrainz IDs directly
        // No more need for Tokio runtime since our function is now synchronous
        let (mbids, partial_match) = lookup_artist_mbids(&artist.name);
        let mbid_count = mbids.len();
        
        // Add each MusicBrainz ID to the artist if any were found
        for mbid in mbids {
            artist.add_mbid(mbid);
        }

        // if there is more than one mbid or it was a partial match, it's a multi-artist entry
        if mbid_count > 1 || partial_match {
            artist.is_multi = true; // Mark as multi-artist entry
            artist.clear_metadata(); // Clear metadata for multi-artist entries
            debug!("Cleared metadata for multi-artist entry: {}", artist.name);
        } else if mbid_count > 0 {
            info!("Updated artist '{}' with MusicBrainz data: {} ID(s)", artist.name, mbid_count);
            debug!("Added MusicBrainz ID(s) to artist {}", artist.name);
        }
        
        // Record if this is a partial match in the artist metadata
        if partial_match {
            debug!("Partial match found for multi-artist name: {}", artist.name);
            if let Some(meta) = &mut artist.metadata {
                meta.is_partial_match = true;
            }
        }
    } else {
        debug!("Artist {} already has MusicBrainz ID(s)", artist.name);
    }
    
    // If the artist has MusicBrainz IDs, update from the coverart system
    if artist.metadata.as_ref().is_some_and(|meta| !meta.mbid.is_empty()) {
        debug!("Artist {} has MusicBrainz ID(s), updating with cover art system", artist.name);
        artist = update_artist_with_coverart(artist);
    } else {
        // For artists without MusicBrainz IDs, still try coverart system with artist name only
        debug!("Artist {} has no MusicBrainz ID, trying cover art by name only", artist.name);
        artist = update_artist_with_coverart(artist);
    }
    
    // Update with individual service providers for biography and additional metadata
    // Note: The coverart system handles images, but we need individual services for biography
    
    // Check if we need biography data or genre data
    let needs_biography = artist.metadata.as_ref().is_none_or(|meta| meta.biography.is_none());
    let needs_genres = artist.metadata.as_ref().is_none_or(|meta| meta.genres.is_empty());
    
    if needs_biography || needs_genres {
        debug!("Artist {} needs biography or genre data, calling individual service updaters", artist.name);
        
        // Track what we had before updating
        let had_biography_before = artist.metadata.as_ref().is_some_and(|meta| meta.biography.is_some());
        let genres_count_before = artist.metadata.as_ref().map_or(0, |meta| meta.genres.len());
        
        // Try LastFM first for biography and genres (usually has good data)
        let lastfm_updater = crate::lastfm::LastfmUpdater;
        artist = lastfm_updater.update_artist(artist);
        
        // Check what we got from LastFM
        let has_biography_after_lastfm = artist.metadata.as_ref().is_some_and(|meta| meta.biography.is_some());
        let genres_count_after_lastfm = artist.metadata.as_ref().map_or(0, |meta| meta.genres.len());
        
        if !had_biography_before && has_biography_after_lastfm {
            info!("Downloaded biography for artist '{}' from LastFM", artist.name);
        }
        if genres_count_after_lastfm > genres_count_before {
            let new_genres = genres_count_after_lastfm - genres_count_before;
            info!("Downloaded {} genre(s) for artist '{}' from LastFM", new_genres, artist.name);
        }
        
        // Check what we still need after LastFM
        let still_needs_biography = artist.metadata.as_ref().is_none_or(|meta| meta.biography.is_none());
        let still_needs_genres = artist.metadata.as_ref().is_none_or(|meta| meta.genres.is_empty());
        let has_mbid = artist.metadata.as_ref().is_some_and(|meta| !meta.mbid.is_empty());
        
        // If we still need data and have MusicBrainz ID, try TheAudioDB
        if (still_needs_biography || still_needs_genres) && has_mbid {
            debug!("Artist {} still needs biography or genres and has MBID, trying TheAudioDB", artist.name);
            
            // Track what we have before TheAudioDB
            let had_biography_before_tadb = artist.metadata.as_ref().is_some_and(|meta| meta.biography.is_some());
            let genres_count_before_tadb = artist.metadata.as_ref().map_or(0, |meta| meta.genres.len());
            
            let theaudiodb_updater = crate::theaudiodb::TheAudioDbUpdater;
            artist = theaudiodb_updater.update_artist(artist);
            
            // Check what we got from TheAudioDB
            let has_biography_after_tadb = artist.metadata.as_ref().is_some_and(|meta| meta.biography.is_some());
            let genres_count_after_tadb = artist.metadata.as_ref().map_or(0, |meta| meta.genres.len());
            
            if !had_biography_before_tadb && has_biography_after_tadb {
                info!("Downloaded biography for artist '{}' from TheAudioDB", artist.name);
            }
            if genres_count_after_tadb > genres_count_before_tadb {
                let new_genres = genres_count_after_tadb - genres_count_before_tadb;
                info!("Downloaded {} genre(s) for artist '{}' from TheAudioDB", new_genres, artist.name);
            }
        }
        
        // FanArt.tv updater no longer provides metadata - all image handling is done by CoverartProvider
        if has_mbid {
            debug!("Artist {} has MBID - FanArt.tv images will be handled by CoverartProvider", artist.name);
        }
    } else {
        debug!("Artist {} already has biography and genre data", artist.name);
    }
    
    // Handle artists without MusicBrainz IDs but with existing thumbnails
    if artist.metadata.as_ref().is_some_and(|meta| meta.mbid.is_empty()) {
        // Check if the artist has thumbnail images
        let has_thumbnails = match &artist.metadata {
            Some(meta) => !meta.thumb_url.is_empty(),
            None => false,
        };
        
        if has_thumbnails {
            debug!("Artist {} has thumbnail image(s) but no MusicBrainz ID, skipping updates", artist.name);
        }
    }

    // Store the updated metadata in cache
    if let Some(metadata) = &artist.metadata {
        // Create a cache key using the artist's name
        let cache_key = format!("artist::metadata::{}", artist.name);
        
        // Store the metadata in the attribute cache
        match acr_store::attributecache::set(&cache_key, metadata) {
            Ok(_) => debug!("Stored metadata for artist {} in attribute cache", artist.name),
            Err(e) => warn!("Failed to store metadata for artist {} in attribute cache: {}", artist.name, e),
        }
        
        // If the artist has MusicBrainz IDs, store them separately for faster lookup
        if !metadata.mbid.is_empty() {
            let mbid_key = format!("artist::mbid::{}", artist.name);
            if let Err(e) = acr_store::attributecache::set(&mbid_key, &metadata.mbid) {
                warn!("Failed to store MusicBrainz IDs for artist {} in attribute cache: {}", artist.name, e);
            }
        }
    }
    
    // Return the potentially updated artist
    artist
}

/// Build the artist to look up from what the library named it, seeded with
/// whatever is already cached about it.
///
/// The seed matters: `update_data_for_artist` skips the MusicBrainz search for
/// an artist that already has an MBID, and the cover art system reuses images
/// it already has. Starting from an empty `Artist` would make every sweep
/// research and re-download every artist in the library. Before the enrichment
/// seam this metadata arrived inside the `Artist` the library handed over --
/// which the library had itself read from this same cache.
fn artist_to_update(reference: &ArtistRef) -> Artist {
    let metadata = cached_artist_metadata(&reference.name);
    let is_multi = metadata
        .as_ref()
        .is_some_and(|m| m.mbid.len() > 1 || m.is_partial_match);

    Artist {
        id: Identifier::String(reference.id.clone()),
        name: reference.name.clone(),
        is_multi,
        metadata,
    }
}

/// What a library keeps from an updated artist.
///
/// The summary is exactly the fields a library's own lists are built from --
/// the thumbnail URLs among them, because the artist list route serves them.
/// The biography and the source it came from stay on this side, served from
/// here by the artist detail route.
fn summarise(artist: &Artist) -> ArtistSummary {
    let (mbid, genres, thumb_url) = artist
        .metadata
        .as_ref()
        .map(|m| (m.mbid.clone(), m.genres.clone(), m.thumb_url.clone()))
        .unwrap_or_default();

    ArtistSummary {
        name: artist.name.clone(),
        // `update_data_for_artist` sets this flag, and clears the metadata it
        // was derived from when it does, so it is read from the artist rather
        // than recomputed from what is left.
        is_multi: artist.is_multi,
        mbid,
        genres,
        thumb_url,
    }
}

/// Everything the artist sweep reaches outside itself for.
///
/// Production supplies `LiveSweep`. A test supplies its own and can then see
/// what the sweep cost and what it accumulated, neither of which is visible
/// from outside a background thread that talks to the network.
trait Sweep {
    /// Look everything up for one artist and return what is now known.
    fn update(&self, reference: &ArtistRef) -> Artist;
    /// Announce the artist about to be looked up.
    fn starting(&self, artist_name: &str, index: usize, total: usize);
    /// A progress milestone.
    fn milestone(&self, count: usize, total: usize);
    /// Wait, out of politeness to the services just called. Every artist
    /// reaches this: unlike the album sweep, every one of them costs a lookup.
    fn pace(&self);
}

/// The sweep itself: look up, accumulate, flush.
///
/// Stops early, without a final flush, when the library refuses a batch as
/// stale.
fn sweep_artists(artists: Vec<ArtistRef>, io: &dyn Sweep, sender: &mut BatchSender) {
    let total = artists.len();
    let mut batch: Vec<ArtistSummary> = Vec::with_capacity(BATCH_SIZE);

    for (index, reference) in artists.into_iter().enumerate() {
        debug!("Updating metadata for artist: {}", reference.name);
        io.starting(&reference.name, index, total);

        batch.push(summarise(&io.update(&reference)));
        if batch.len() >= BATCH_SIZE && !sender.send(std::mem::take(&mut batch), Vec::new()) {
            return;
        }

        let count = index + 1;
        if count % 10 == 0 || count == total {
            io.milestone(count, total);
        }

        io.pace();
    }

    sender.send(batch, Vec::new());
}

/// The sweep's real world: the metadata providers, the background job and the
/// clock.
struct LiveSweep {
    job_id: String,
}

impl Sweep for LiveSweep {
    fn update(&self, reference: &ArtistRef) -> Artist {
        let had_mbid =
            cached_artist_metadata(&reference.name).is_some_and(|m| !m.mbid.is_empty());

        let updated = update_data_for_artist(artist_to_update(reference));

        let has_mbid_now = updated
            .metadata
            .as_ref()
            .is_some_and(|m| !m.mbid.is_empty());
        if has_mbid_now && !had_mbid {
            info!("Adding MusicBrainz ID(s) to artist {}", reference.name);
        }

        updated
    }

    fn starting(&self, artist_name: &str, index: usize, total: usize) {
        if let Err(e) = acr_store::backgroundjobs::update_job(
            &self.job_id,
            Some(format!("Processing artist: {}", artist_name)),
            Some(index),
            Some(total),
        ) {
            warn!("Failed to update background job progress: {}", e);
        }
    }

    fn milestone(&self, count: usize, total: usize) {
        info!("Processed {}/{} artists for metadata", count, total);
        if let Err(e) = acr_store::backgroundjobs::update_job(
            &self.job_id,
            Some(format!("Processed {}/{} artists", count, total)),
            Some(count),
            Some(total),
        ) {
            warn!("Failed to update background job milestone: {}", e);
        }
    }

    fn pace(&self) {
        // Sleep between updates to avoid overwhelming external services
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

/// Look up metadata for a library's artists in the background, sending what is
/// found back in batches.
///
/// Returns at once. The sweep is sequential and paced: the services behind
/// `update_data_for_artist` are rate limited, and a library being enriched is
/// a library already serving requests.
pub fn enrich_artists_in_background(
    player: String,
    version: Option<String>,
    artists: Vec<ArtistRef>,
    sink: Arc<dyn EnrichmentSink>,
) {
    debug!("Starting background thread to update artist metadata for {}", player);

    use std::thread;
    thread::spawn(move || {
        let job_id = "artist_metadata_update".to_string();
        let job_name = "Artist Metadata Update".to_string();

        // Register the background job
        if let Err(e) = acr_store::backgroundjobs::register_job(job_id.clone(), job_name) {
            warn!("Failed to register background job: {}", e);
            return;
        }

        info!("Artist metadata update thread started");

        let total = artists.len();
        info!("Processing metadata for {} artists", total);

        // Update the job with total count
        if let Err(e) = acr_store::backgroundjobs::update_job(
            &job_id,
            Some(format!("Starting metadata update for {} artists", total)),
            Some(0),
            Some(total)
        ) {
            warn!("Failed to update background job: {}", e);
        }

        let io = LiveSweep { job_id: job_id.clone() };
        let mut sender = BatchSender::new(sink, version);
        sweep_artists(artists, &io, &mut sender);

        info!("Artist metadata update process completed");

        // Complete and remove the background job
        if let Err(e) = acr_store::backgroundjobs::complete_job(&job_id) {
            warn!("Failed to complete background job: {}", e);
        }
    });

    info!("Background artist metadata update initiated");
}

#[cfg(test)]
mod tests {
    use super::*;
    use acr_types::enrichment::{Applied, EnrichmentBatch, EnrichmentError};
    use acr_types::metadata::ArtistMeta;
    use parking_lot::Mutex;

    fn artist(name: &str, metadata: Option<ArtistMeta>, is_multi: bool) -> Artist {
        Artist {
            id: Identifier::Numeric(1),
            name: name.to_string(),
            is_multi,
            metadata,
        }
    }

    /// A summary carries exactly the fields a library's lists are built from,
    /// and this asserts the whole of it rather than the fields it remembers to
    /// name: a field silently dropped here is a field silently missing from
    /// every artist list, which is how the thumbnails were lost once already.
    /// The biography is set on the fixture and has nowhere to go — the type
    /// has no such field, which is the guarantee that it stays on this side.
    #[test]
    fn a_summary_carries_the_fields_the_library_lists_are_built_from() {
        let mut meta = ArtistMeta::new();
        meta.add_mbid("mbid-1".to_string());
        meta.add_genre("rock".to_string());
        meta.biography = Some("A long story".to_string());
        meta.biography_source = Some("TheAudioDB".to_string());
        // Both shapes this field takes: the daemon's own cover art URL, and a
        // provider's, which is never rewritten and so must survive verbatim.
        meta.add_thumb_url("/api/coverart/artist/YWJj/image".to_string());
        meta.add_thumb_url("https://example.com/artist.png".to_string());

        assert_eq!(
            summarise(&artist("Radiohead", Some(meta), false)),
            ArtistSummary {
                name: "Radiohead".to_string(),
                mbid: vec!["mbid-1".to_string()],
                is_multi: false,
                genres: vec!["rock".to_string()],
                thumb_url: vec![
                    "/api/coverart/artist/YWJj/image".to_string(),
                    "https://example.com/artist.png".to_string(),
                ],
            }
        );
    }

    /// A name that turns out to cover several artists has its metadata cleared
    /// by `update_data_for_artist`, so the flag is the only thing left saying
    /// so. Recomputing it from the metadata that is gone would lose it.
    #[test]
    fn a_multi_artist_stays_multi_even_with_its_metadata_cleared() {
        let summary = summarise(&artist("Simon & Garfunkel", None, true));

        assert!(summary.is_multi);
        assert!(summary.mbid.is_empty());
        assert!(summary.genres.is_empty());
        assert!(summary.thumb_url.is_empty());
    }

    /// A sweep world with no clock and no network.
    struct FakeSweep {
        seen: Mutex<Log>,
    }

    #[derive(Default)]
    struct Log {
        started: Vec<String>,
        milestones: Vec<(usize, usize)>,
        paced: usize,
    }

    impl FakeSweep {
        fn new() -> Self {
            FakeSweep { seen: Mutex::new(Log::default()) }
        }
    }

    impl Sweep for FakeSweep {
        fn update(&self, reference: &ArtistRef) -> Artist {
            let mut meta = ArtistMeta::new();
            meta.add_mbid(format!("mbid-{}", reference.id));
            artist(&reference.name, Some(meta), false)
        }
        fn starting(&self, artist_name: &str, _index: usize, _total: usize) {
            self.seen.lock().started.push(artist_name.to_string());
        }
        fn milestone(&self, count: usize, total: usize) {
            self.seen.lock().milestones.push((count, total));
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

    fn refs(count: usize) -> Vec<ArtistRef> {
        (0..count)
            .map(|i| ArtistRef { id: i.to_string(), name: format!("Artist {}", i) })
            .collect()
    }

    /// Results accumulate and flush at the batch boundary, not one per artist:
    /// a library bumps its version once per batch, so flushing per artist
    /// would invalidate every client's cached list once per artist.
    #[test]
    fn results_accumulate_and_flush_at_the_batch_boundary() {
        let io = FakeSweep::new();
        let sink = Arc::new(Recording::default());
        let mut sender = BatchSender::new(sink.clone(), None);

        sweep_artists(refs(120), &io, &mut sender);

        let batches = sink.0.lock();
        let sizes: Vec<usize> = batches.iter().map(|b| b.artists.len()).collect();
        assert_eq!(sizes, vec![BATCH_SIZE, BATCH_SIZE, 20], "two full batches, then the rest");
        assert_eq!(batches[0].artists[0].name, "Artist 0", "and in the order they were swept");
        assert_eq!(batches[0].artists[0].mbid, vec!["mbid-0"], "carrying what was found");
        assert_eq!(batches[2].artists[19].name, "Artist 119");
    }

    /// Every artist costs a lookup, so every artist is paced — the asymmetry
    /// with the album sweep, where a cached answer costs nothing, is
    /// deliberate. Milestones stay on their every-tenth schedule.
    #[test]
    fn every_artist_is_paced_and_the_milestones_keep_their_schedule() {
        let io = FakeSweep::new();
        let sink = Arc::new(Recording::default());
        let mut sender = BatchSender::new(sink, None);

        sweep_artists(refs(25), &io, &mut sender);

        let seen = io.seen.lock();
        assert_eq!(seen.started.len(), 25);
        assert_eq!(seen.paced, 25);
        assert_eq!(seen.milestones, vec![(10, 25), (20, 25), (25, 25)]);
    }

    /// A refusal stops the sweep where it stands rather than looking up the
    /// rest for a library that will not take the answers.
    #[test]
    fn a_refused_batch_stops_the_sweep() {
        struct Refusing;
        impl EnrichmentSink for Refusing {
            fn apply(&self, _batch: EnrichmentBatch) -> Result<Applied, EnrichmentError> {
                Err(EnrichmentError::Stale { current: None })
            }
        }

        let io = FakeSweep::new();
        let mut sender = BatchSender::new(Arc::new(Refusing), Some("v1".to_string()));

        sweep_artists(refs(120), &io, &mut sender);

        assert_eq!(
            io.seen.lock().started.len(),
            BATCH_SIZE,
            "the artists after the refusal were never looked up"
        );
    }
}
