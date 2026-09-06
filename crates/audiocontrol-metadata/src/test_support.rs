//! Test-only setup for the process-wide caches that default to a path under
//! `/var/lib/audiocontrol`.
//!
//! `acr_store::imagecache` and `acr_store::attributecache` are both global
//! singletons that an unconfigured process points at a system directory --
//! right for the daemon, uncreatable for an unprivileged test run. Before
//! this crate had its own test binary, the tests that reach these caches
//! shared a process with `src/api/library.rs`'s tests, which called
//! `ImageCache::initialize` with a temp directory as a side effect of
//! something else -- so every test in the shared binary inherited a
//! writable cache by accident. The split gave this crate its own test
//! binary, which removed that accident, so a test that reaches either cache
//! now has to point it at a temp directory itself.
use std::sync::Once;
use tempfile::TempDir;

/// Repoint both global caches at a temporary directory, once.
///
/// `Once` makes this safe to call from every test that touches either
/// cache, however many of them run and in whatever order: the first caller
/// does the work, every later one is a no-op. The `TempDir` is deliberately
/// leaked (`mem::forget`) rather than dropped, because it has to outlive
/// every test that uses the caches and no single test owns that lifetime --
/// the process exiting is what cleans it up, the same trade `artist_store`'s
/// `init_test_settings_db` makes for the settings database.
pub(crate) fn init_test_caches() {
    static INIT: Once = Once::new();

    INIT.call_once(|| {
        let temp_dir = TempDir::new().expect("cache temp dir");
        acr_store::imagecache::ImageCache::initialize(temp_dir.path().join("images"))
            .expect("image cache should initialize");
        acr_store::attributecache::AttributeCache::initialize_global(
            temp_dir.path().join("attributes"),
        )
        .expect("attribute cache should initialize");
        std::mem::forget(temp_dir);
    });
}
