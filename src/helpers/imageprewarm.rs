use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use chrono::Datelike;
use log::{debug, info, warn};
use parking_lot::RwLock;

use crate::data::Album;
use crate::helpers::imageresize::GRID_SIZE;

/// How long to wait between albums.
///
/// The job exists to make the first scroll fast, not to win a race with playback.
/// A short sleep keeps a Pi responsive while it works through 11,000 albums.
const PAUSE_BETWEEN_ALBUMS: Duration = Duration::from_millis(20);

/// Generate the album-grid variant for every album, in the background.
///
/// Albums whose variant already exists are skipped, so a restart mid-walk is cheap
/// and a rescan is nearly free. Registering under a fixed job id means a second
/// library refresh cannot start a second copy.
pub fn prewarm_album_variants_in_background(
    albums_collection: Arc<RwLock<HashMap<String, Album>>>,
) {
    std::thread::spawn(move || {
        let job_id = "imagecache_prewarm".to_string();

        if let Err(e) = crate::helpers::backgroundjobs::register_job(
            job_id.clone(),
            "Cover Art Thumbnail Generation".to_string(),
        ) {
            debug!("Not starting thumbnail pre-warm: {}", e);
            return;
        }

        // Snapshot what we need so the library lock is not held while decoding.
        let albums: Vec<(String, String, Option<i32>)> = {
            let map = albums_collection.read();
            map.values()
                .map(|a| {
                    let artist = {
                        let artists = a.artists.lock();
                        artists.first().cloned().unwrap_or_else(|| "Unknown Artist".to_string())
                    };
                    (artist, a.name.clone(), a.release_date.map(|d| d.year()))
                })
                .collect()
        };

        let total = albums.len();
        info!("Pre-warming {}px cover art thumbnails for {} albums", GRID_SIZE, total);

        let _ = crate::helpers::backgroundjobs::update_job(
            &job_id,
            Some(format!("Generating thumbnails for {} albums", total)),
            Some(0),
            Some(total),
        );

        let mut generated = 0usize;

        for (index, (artist, album_name, year)) in albums.into_iter().enumerate() {
            let base = format!(
                "{}/cover",
                crate::helpers::local_coverart::album_cache_key(&artist, &album_name, year)
            );

            match crate::helpers::imagecache::get_or_create_variant(&base, GRID_SIZE) {
                Ok(_) => generated += 1,
                // An album with no cached cover is the common case, not an error.
                Err(e) => debug!("No thumbnail for {}: {}", base, e),
            }

            if index % 50 == 0 {
                let _ = crate::helpers::backgroundjobs::update_job(
                    &job_id,
                    Some(format!("Generating thumbnails: {}", album_name)),
                    Some(index),
                    Some(total),
                );
            }

            std::thread::sleep(PAUSE_BETWEEN_ALBUMS);
        }

        info!("Thumbnail pre-warm finished: {} of {} albums have a {}px variant", generated, total, GRID_SIZE);

        if let Err(e) = crate::helpers::backgroundjobs::complete_job(&job_id) {
            warn!("Failed to complete thumbnail pre-warm job: {}", e);
        }
    });
}
