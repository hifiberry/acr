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

        let mut sender = BatchSender::new(sink, version);
        let mut batch: Vec<AlbumGenres> = Vec::with_capacity(BATCH_SIZE);
        let mut updated = 0usize;

        for (index, album) in albums.into_iter().enumerate() {
            let AlbumRef { id: album_id, name: album_name, artist } = album;

            let _ = acr_store::backgroundjobs::update_job(
                &job_id,
                Some(format!("Processing: {}", album_name)),
                Some(index),
                Some(total),
            );

            // What to send for this album, if anything. `None` means the album
            // is not mentioned in a batch at all, which is not the same as
            // sending an empty list: an empty list never overwrites anything,
            // so an entry carrying one is work for the library and no change.
            let found = match plan(load_cached_genres(&album_id), &artist, &album_name) {
                Plan::Send(genres) => Some(genres),
                Plan::Skip => {
                    debug!("Skipping album '{}' — nothing to look up", album_name);
                    None
                }
                Plan::RecordEmpty => {
                    // Record that this album cannot be looked up, so the next
                    // load reaches Plan::Skip instead of getting here again.
                    store_cached_genres(&album_id, &[]);
                    None
                }
                Plan::Fetch => {
                    let genres = fetch_album_genres(&album_id, &artist, &album_name);
                    if genres.is_empty() {
                        None
                    } else {
                        Some(genres)
                    }
                }
            };

            if let Some(genres) = found {
                batch.push(AlbumGenres { id: album_id, genres });
                updated += 1;
                if batch.len() >= BATCH_SIZE && !sender.send(Vec::new(), std::mem::take(&mut batch)) {
                    let _ = acr_store::backgroundjobs::complete_job(&job_id);
                    return;
                }
            }

            let count = index + 1;
            if count % 50 == 0 || count == total {
                info!("Album genre update: {}/{} processed, {} updated", count, total, updated);
                let _ = acr_store::backgroundjobs::update_job(
                    &job_id,
                    Some(format!("Processed {}/{} albums", count, total)),
                    Some(count),
                    Some(total),
                );
            }

            // Rate limiting: MusicBrainz allows 1 req/sec; the ratelimit helper handles
            // per-request limiting but we add a small sleep to be polite.
            std::thread::sleep(std::time::Duration::from_millis(50));
        }

        sender.send(Vec::new(), batch);

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

    #[test]
    fn the_cache_key_names_the_album_id() {
        assert_eq!(cache_key("42"), "album::genres::42");
    }
}
