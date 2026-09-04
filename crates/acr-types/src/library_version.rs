//! The change counter behind the library ETags.
//!
//! A library hands clones of one counter to the background updaters that
//! mutate it, so a bump from any of them is visible to the endpoints that
//! serve the validator. Those updaters live in a different crate from the
//! libraries they enrich, which is why the counter is here rather than beside
//! either of them.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// A counter that increases whenever a library's contents change.
///
/// Cloning shares the counter: the library keeps one handle and hands clones to
/// the background updaters that mutate it, so a bump from any of them is visible
/// to the endpoints that serve the validator.
#[derive(Debug, Clone)]
pub struct LibraryVersion {
    /// Distinguishes this counter from every other one that has ever existed.
    ///
    /// Without it the counter resets to 0 on restart and climbs back through
    /// values it already issued, so a client could be told 304 for content that
    /// changed. This is a random half, defending across restarts, plus a
    /// process-monotonic sequence appended below, defending within a process:
    /// two independently constructed counters in one process cannot share a
    /// nonce even in the (already astronomically unlikely) case that the
    /// random halves collide.
    nonce: String,
    counter: Arc<AtomicU64>,
}

impl Default for LibraryVersion {
    fn default() -> Self {
        Self::new()
    }
}

/// Process-wide sequence handed out to each `LibraryVersion::new()` call, so
/// two counters constructed in the same process are guaranteed a distinct
/// nonce regardless of what `rand::random` returns. This only has to make
/// each call to `new()` observe a value no other call observes - it is not
/// read or written anywhere near the request-serving hot path that the
/// version counter itself is on - so ordering weaker than the counter's own
/// `SeqCst` is fine here.
static NONCE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

impl LibraryVersion {
    pub fn new() -> Self {
        let sequence = NONCE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        Self {
            nonce: format!("{:08x}-{:x}", rand::random::<u32>(), sequence),
            counter: Arc::new(AtomicU64::new(0)),
        }
    }

    /// The current value.
    pub fn get(&self) -> u64 {
        self.counter.load(Ordering::SeqCst)
    }

    /// Record that the library changed.
    ///
    /// Called as close to the mutation as the code allows. A mutation that does
    /// not bump serves stale data to every client holding a cached list, and no
    /// test can detect that, so proximity to the write is the only guard.
    pub fn bump(&self) {
        self.counter.fetch_add(1, Ordering::SeqCst);
    }

    /// The opaque validator for the library's current contents.
    ///
    /// Compare for equality only. It is not ordered and carries no arithmetic
    /// meaning; a caller that stripped the nonce to compare counters would
    /// reintroduce the stale-serve this design removes.
    pub fn token(&self) -> String {
        format!("{}-{}", self.nonce, self.get())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_version_starts_at_zero() {
        assert_eq!(LibraryVersion::new().get(), 0);
    }

    #[test]
    fn bumping_increases_the_version() {
        let v = LibraryVersion::new();
        v.bump();
        assert_eq!(v.get(), 1);
        v.bump();
        assert_eq!(v.get(), 2);
    }

    #[test]
    fn clones_share_one_counter() {
        // The updaters hold clones; a bump through one must be visible through
        // the library's own handle, or the ETag would not move.
        let v = LibraryVersion::new();
        let handed_to_an_updater = v.clone();
        handed_to_an_updater.bump();
        assert_eq!(v.get(), 1);
    }

    #[test]
    fn the_token_combines_the_nonce_and_the_counter() {
        let v = LibraryVersion::new();
        let first = v.token();
        v.bump();
        let second = v.token();

        assert_ne!(first, second, "a bump must change the token");
        let (nonce_a, count_a) = first.rsplit_once('-').unwrap();
        let (nonce_b, count_b) = second.rsplit_once('-').unwrap();
        assert_eq!(nonce_a, nonce_b, "the nonce is stable within one counter");
        assert_eq!(count_a, "0");
        assert_eq!(count_b, "1");
    }

    #[test]
    fn two_counters_never_share_a_nonce() {
        // This is the property that makes a restart safe: a fresh counter must
        // not reissue a token an earlier one already handed out.
        let a = LibraryVersion::new();
        let b = LibraryVersion::new();
        assert_ne!(a.token(), b.token(), "a new counter must not reuse a token");
    }

    #[test]
    fn a_clone_reports_the_same_token() {
        let v = LibraryVersion::new();
        let handed_out = v.clone();
        handed_out.bump();
        assert_eq!(v.token(), handed_out.token());
    }
}
