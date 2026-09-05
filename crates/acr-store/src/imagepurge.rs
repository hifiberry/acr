use std::sync::atomic::{AtomicBool, Ordering};

use log::{debug, info, warn};

/// Name of the file recording which ladder the stored variants belong to.
const LADDER_MARKER: &str = ".variant-ladder";

static PURGE_RUNNING: AtomicBool = AtomicBool::new(false);

/// Held for the duration of a walk; clears the flag on drop, including on panic.
struct RunningGuard;

impl Drop for RunningGuard {
    fn drop(&mut self) {
        PURGE_RUNNING.store(false, Ordering::SeqCst);
    }
}

fn try_acquire() -> Option<RunningGuard> {
    PURGE_RUNNING
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .ok()
        .map(|_| RunningGuard)
}

/// Delete variants at sizes the ladder no longer offers, on a background thread.
///
/// This used to run inside `ImageCache::initialize`, before the API server bound
/// its listener, and it held the global image-cache mutex while it walked. On a
/// real upgrade the daemon answered nothing for about a minute and nginx served
/// 502s throughout.
pub fn purge_retired_in_background() {
    std::thread::spawn(move || {
        let Some(_guard) = try_acquire() else {
            debug!("Not starting a variant purge: one is already running");
            return;
        };

        // Take what the walk needs and release the lock immediately. Holding it
        // across the walk would block every image request for the duration -
        // the same fault, merely moved off the startup path.
        // Take the guard only long enough to copy the path out of it.
        let (base_path, marker_path) = {
            let cache = crate::imagecache::get_image_cache();
            let base = cache.base_path().clone();
            let marker = base.join(LADDER_MARKER);
            (base, marker)
        };

        if !base_path.exists() {
            return;
        }

        let current = acr_images::imageresize::ladder_fingerprint();
        let stored = std::fs::read_to_string(&marker_path).ok();

        match stored.as_deref().map(str::trim) {
            Some(previous) if previous == current => return,
            Some(previous) => {
                let offered = acr_images::imageresize::sizes();
                let retired: Vec<u32> = previous
                    .split('-')
                    .filter_map(|s| s.parse::<u32>().ok())
                    .filter(|s| !offered.contains(s))
                    .collect();

                if retired.is_empty() {
                    info!(
                        "Image size ladder changed from {} to {}; no sizes retired, keeping every variant",
                        previous, current
                    );
                } else {
                    info!(
                        "Image size ladder changed from {} to {}; purging variants at retired sizes {:?}",
                        previous, current, retired
                    );
                    let job_id = "imagecache_purge".to_string();
                    let _ = crate::backgroundjobs::register_job(
                        job_id.clone(),
                        "Image Variant Purge".to_string(),
                    );
                    // Deliberately NOT `get_image_cache().purge_retired_variants(..)`:
                    // that holds the global cache mutex for the whole walk, which
                    // is the fault this task exists to remove. A local ImageCache
                    // over the same directory owns no shared state - the metadata
                    // removal it performs goes to the attribute cache, which has
                    // its own lock and is taken per entry.
                    let walker = crate::imagecache::ImageCache::with_directory(&base_path);
                    let removed = walker.purge_retired_variants(offered);
                    match removed {
                        Ok(n) => info!("Purged {} variant(s) at retired sizes", n),
                        Err(e) => {
                            warn!("Variant purge failed, leaving the marker for a retry: {}", e);
                            let _ = crate::backgroundjobs::complete_job(&job_id);
                            return;
                        }
                    }
                    let _ = crate::backgroundjobs::complete_job(&job_id);
                }
            }
            None => {}
        }

        // Written only after a successful walk: a daemon killed mid-purge retries
        // on the next start. Leftover files at retired sizes are wasted disk, not
        // stale content - snap_to_rung cannot produce a retired size, so nothing
        // serves them.
        if let Err(e) = std::fs::write(&marker_path, &current) {
            warn!("Failed to record the image size ladder: {}", e);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_guard_admits_one_walk_at_a_time() {
        PURGE_RUNNING.store(false, std::sync::atomic::Ordering::SeqCst);
        let first = try_acquire();
        assert!(first.is_some(), "the first caller must get the guard");
        assert!(try_acquire().is_none(), "a second concurrent walk must be refused");
        drop(first);
        assert!(try_acquire().is_some(), "the guard must be released on drop");
    }
}
