use log::{debug, info, warn};
use std::sync::Arc;
use parking_lot::RwLock;
use std::collections::HashMap;
use crate::data::album::Album;

const CACHE_KEY_PREFIX: &str = "album::genres::";

/// Return the attribute cache key for a given album ID
fn cache_key(album_id: &str) -> String {
    format!("{}{}", CACHE_KEY_PREFIX, album_id)
}

/// Load cached genres for an album from the attribute cache.
/// Returns `Some(genres)` if a cached entry exists (even if empty), `None` if not found.
pub fn load_cached_genres(album_id: &str) -> Option<Vec<String>> {
    match crate::helpers::attributecache::get::<Vec<String>>(&cache_key(album_id)) {
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
    match crate::helpers::attributecache::set(&cache_key(album_id), &genres_vec) {
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
    let genres = crate::helpers::musicbrainz::search_release_group_genres(artist, album_name);

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

/// Write genres and record that the library changed.
///
/// The write and the bump live together deliberately: a mutation that does not
/// move the version serves stale lists to every client holding a cached copy,
/// and no test can catch that. Keeping them in one function is the only thing
/// that makes forgetting hard.
///
/// `version` is `None` for backends that do not track a library version at all
/// (currently LMS): there is nothing to bump, and no client is ever handed a
/// validator for those lists, so silently doing nothing is correct rather than
/// a gap.
fn set_genres(
    target: &mut Vec<String>,
    genres: Vec<String>,
    version: Option<&crate::data::library::LibraryVersion>,
) {
    if genres.is_empty() {
        return;
    }
    *target = genres;
    if let Some(version) = version {
        version.bump();
    }
}

/// Start a background thread to update genre tags for all albums in the library.
///
/// For each album that has no genres, fetches genres from MusicBrainz and stores
/// them in the album struct and in the attribute cache.
pub fn update_library_albums_genres_in_background(
    albums_collection: Arc<RwLock<HashMap<String, Album>>>,
    version: Option<crate::data::library::LibraryVersion>,
) {
    debug!("Starting background thread to update album genres");

    std::thread::spawn(move || {
        let job_id = "album_genre_update".to_string();
        let job_name = "Album Genre Update".to_string();

        if let Err(e) = crate::helpers::backgroundjobs::register_job(job_id.clone(), job_name) {
            warn!("Failed to register album genre background job: {}", e);
            return;
        }

        info!("Album genre update thread started");

        // Collect albums that need genre lookup
        let albums_snapshot: Vec<(String, String, Vec<String>)> = {
            let map = albums_collection.read();
            map.values()
                .filter(|a| a.genres.is_empty())
                .map(|a| {
                    let id = a.id.to_string();
                    let name = a.name.clone();
                    let artists = a.artists.lock().clone();
                    (id, name, artists)
                })
                .collect()
        };

        let total = albums_snapshot.len();
        info!("Updating genres for {} albums without genre tags", total);

        let _ = crate::helpers::backgroundjobs::update_job(
            &job_id,
            Some(format!("Starting genre update for {} albums", total)),
            Some(0),
            Some(total),
        );

        let mut updated = 0usize;

        for (index, (album_id, album_name, artists)) in albums_snapshot.into_iter().enumerate() {
            let artist = artists.first().cloned().unwrap_or_default();

            let _ = crate::helpers::backgroundjobs::update_job(
                &job_id,
                Some(format!("Processing: {}", album_name)),
                Some(index),
                Some(total),
            );

            // Skip if already cached with empty result (avoid repeated API calls)
            if let Some(cached) = load_cached_genres(&album_id) {
                if cached.is_empty() {
                    debug!("Skipping album '{}' — cached empty result", album_name);
                    continue;
                }
                // Has cached genres — apply them to the album
                let mut map = albums_collection.write();
                if let Some(album) = map.get_mut(&album_name) {
                    if album.genres.is_empty() {
                        set_genres(&mut album.genres, cached, version.as_ref());
                        updated += 1;
                    }
                }
                continue;
            }

            if artist.is_empty() || album_name.is_empty() {
                store_cached_genres(&album_id, &[]);
                continue;
            }

            let genres = fetch_album_genres(&album_id, &artist, &album_name);

            if !genres.is_empty() {
                let mut map = albums_collection.write();
                if let Some(album) = map.get_mut(&album_name) {
                    // Mirror the cached-genres branch's guard above: the
                    // snapshot this loop iterates was taken before this
                    // fetch started, so another sweep may have already
                    // populated this album's genres while the MusicBrainz
                    // lookup was in flight. Without the guard this would
                    // overwrite genres that are already set - and, now that
                    // writes bump the library version, invalidate every
                    // client's cache for a write that changed nothing.
                    if album.genres.is_empty() {
                        set_genres(&mut album.genres, genres, version.as_ref());
                        updated += 1;
                    }
                }
            }

            let count = index + 1;
            if count % 50 == 0 || count == total {
                info!("Album genre update: {}/{} processed, {} updated", count, total, updated);
                let _ = crate::helpers::backgroundjobs::update_job(
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

        info!("Album genre update complete: {}/{} albums updated", updated, total);
        let _ = crate::helpers::backgroundjobs::complete_job(&job_id);
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::library::LibraryVersion;

    #[test]
    fn writing_genres_stores_them_and_bumps_the_version() {
        let version = LibraryVersion::new();
        let mut target: Vec<String> = Vec::new();
        set_genres(&mut target, vec!["rock".to_string()], Some(&version));
        assert_eq!(target, vec!["rock".to_string()]);
        assert_eq!(version.get(), 1, "a genre write must move the version");
    }

    #[test]
    fn writing_nothing_leaves_both_alone() {
        let version = LibraryVersion::new();
        let mut target = vec!["existing".to_string()];
        set_genres(&mut target, Vec::new(), Some(&version));
        assert_eq!(target, vec!["existing".to_string()], "an empty write must not clear");
        assert_eq!(version.get(), 0, "an empty write is not a change");
    }

    #[test]
    fn writing_genres_with_no_version_still_stores_them() {
        // The LMS path: no LibraryVersion is tracked, so `version` is `None`.
        // The write must still happen; there is simply nothing to bump.
        let mut target: Vec<String> = Vec::new();
        set_genres(&mut target, vec!["rock".to_string()], None);
        assert_eq!(target, vec!["rock".to_string()]);
    }
}
