use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use chrono::Datelike;
use log::{debug, info, warn};
use parking_lot::RwLock;

use crate::data::Album;
use crate::helpers::imageresize::prewarm_sizes;

/// How long to wait after a variant that actually cost something.
///
/// The job exists to make the first scroll fast, not to win a race with playback,
/// so the intent is that it spends the clear minority of its wall time working.
/// That is what sets this number: it is not a "short sleep", it is the other side
/// of a duty cycle. One variant costs a decode, a scale and an encode — measured
/// at 300-600ms per album on a Pi 4 with Lanczos3, and a fraction of that since
/// the switch to CatmullRom — against which 20ms was not a pause at all: it left the
/// machine working ~94% of the one to two hours after every library load, which is
/// exactly the window in which a client is scrolling the new library. 250ms brings
/// the working share into the minority for any plausible per-variant cost.
///
/// The pause is per variant rather than per album, so widening the configured
/// pre-warm sizes widens the throttle with it: an album with a cover now costs
/// three decodes, and three pauses to go with them.
///
/// This pause follows work actually performed, not every iteration: the common
/// case by far is an album with no cached cover at all, where
/// `get_or_create_variant` returns `Err` after a cheap directory-existence check
/// and there is nothing to throttle. Against the measured library of 11,318
/// albums, sleeping unconditionally cost roughly 47 minutes of wall time asleep
/// for no reason, versus about 3.8 minutes' worth of real throttling — largely
/// missing the freshly-scanned-library window the job exists to serve.
const PAUSE_BETWEEN_VARIANTS: Duration = Duration::from_millis(250);

/// Re-entrancy guard for the pre-warm walk.
///
/// `backgroundjobs::register_job` does not reject a duplicate id — it just
/// overwrites the existing job's bookkeeping and always returns `Ok`. A fixed
/// job id is therefore a display label, not a lock: without this flag, two
/// library refreshes in quick succession would spawn two threads that both
/// walk `albums_collection` and both call `get_or_create_variant` for the
/// same albums concurrently. Variant writes are atomic, so the result would
/// not be a torn file, but it would be two Pi cores decoding and encoding the
/// same 11,000 covers twice over — against a job whose entire design premise
/// is that it stays out of playback's way.
static PREWARM_RUNNING: AtomicBool = AtomicBool::new(false);

/// Clears `PREWARM_RUNNING` when dropped, so every exit path from the
/// spawned thread releases the guard — an early return, the normal end of
/// the walk, or a panic partway through it.
struct RunningGuard;

impl Drop for RunningGuard {
    fn drop(&mut self) {
        PREWARM_RUNNING.store(false, Ordering::SeqCst);
    }
}

/// Generate the album-grid variants for every album, in the background.
///
/// Every rung in the configured pre-warm sizes is generated for each album,
/// because a client picks its grid size from the display scale and any of
/// them may be the first thing asked for. Variants that already exist are
/// skipped, so a restart mid-walk is cheap and a rescan is nearly free. A
/// process-wide re-entrancy guard — not the fixed job id — is what stops a
/// second library refresh from starting a second, concurrently-writing copy
/// of this walk.
pub fn prewarm_album_variants_in_background(
    albums_collection: Arc<RwLock<HashMap<String, Album>>>,
) {
    std::thread::spawn(move || {
        if PREWARM_RUNNING
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            debug!("Thumbnail pre-warm already running; skipping this request");
            return;
        }
        let _guard = RunningGuard;

        if prewarm_sizes().is_empty() {
            info!("Thumbnail pre-warm disabled: no pre-warm sizes configured");
            return;
        }

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
        info!(
            "Pre-warming {:?}px cover art thumbnails for {} albums",
            prewarm_sizes(), total
        );

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

            let mut covered = true;
            for size in prewarm_sizes().iter().copied() {
                match crate::helpers::imagecache::get_or_create_variant(&base, size) {
                    Ok(_) => {
                        // Only pause after work that actually cost something (a decode,
                        // scale and encode, or at worst a disk read of an existing
                        // variant) — see PAUSE_BETWEEN_VARIANTS for why.
                        std::thread::sleep(PAUSE_BETWEEN_VARIANTS);
                    }
                    // An album with no cached cover is the common case, not an error,
                    // and costs only a cheap directory-existence check — nothing to
                    // throttle, so no pause here. The remaining rungs would fail on
                    // the same missing original, so stop asking for them.
                    Err(e) => {
                        debug!("No {}px thumbnail for {}: {}", size, base, e);
                        covered = false;
                        break;
                    }
                }
            }
            if covered {
                generated += 1;
            }

            if index % 50 == 0 {
                let _ = crate::helpers::backgroundjobs::update_job(
                    &job_id,
                    Some(format!("Generating thumbnails: {}", album_name)),
                    Some(index),
                    Some(total),
                );
            }
        }

        info!(
            "Thumbnail pre-warm finished: {} of {} albums have {:?}px variants",
            generated, total, prewarm_sizes()
        );

        if let Err(e) = crate::helpers::backgroundjobs::complete_job(&job_id) {
            warn!("Failed to complete thumbnail pre-warm job: {}", e);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Exercises the `PREWARM_RUNNING` + `RunningGuard` primitive directly —
    /// not `prewarm_album_variants_in_background` itself, which spawns a real
    /// thread that reaches the live `backgroundjobs` and `imagecache`
    /// singletons and has no injection seam for a unit test. This confirms
    /// the two properties the re-entrancy guard exists for: a second
    /// `compare_exchange` while the flag is held fails immediately (mirroring
    /// the early `return` at the top of the spawned thread), and dropping the
    /// guard releases the flag on every exit path, panics included.
    #[test]
    fn reentrancy_guard_blocks_concurrent_entry_and_releases_on_drop() {
        // `PREWARM_RUNNING` is a single process-wide static; force it back to
        // its idle state first so this test doesn't depend on run order.
        PREWARM_RUNNING.store(false, Ordering::SeqCst);

        // First entry acquires the guard, exactly as the spawned thread does.
        assert!(
            PREWARM_RUNNING
                .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok(),
            "first entry should acquire the flag"
        );

        // A second, concurrent entry must be rejected while the first holds it.
        assert!(
            PREWARM_RUNNING
                .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
                .is_err(),
            "second entry must not acquire the flag while the first is running"
        );

        // Dropping the guard — the normal end of the walk, an early return,
        // or a panic — releases the flag.
        {
            let _guard = RunningGuard;
        }
        assert!(
            !PREWARM_RUNNING.load(Ordering::SeqCst),
            "dropping the guard must release the flag"
        );

        // A fresh entry can now succeed again.
        assert!(
            PREWARM_RUNNING
                .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok(),
            "a later entry should succeed once the flag has been released"
        );

        // Leave the static idle for any other test that runs in this process.
        PREWARM_RUNNING.store(false, Ordering::SeqCst);
    }
}
