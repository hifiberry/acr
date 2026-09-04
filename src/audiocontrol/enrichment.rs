//! Where the player side finds the library enricher, if one was injected.
//!
//! The player daemon does not construct an enricher and does not know what one
//! does: `main` installs whichever implementation this build has — in Phase 0
//! the in-process one from `audiocontrol-metadata`, in Phase 1 an HTTP client
//! for the metadata daemon — and every library asks here. A build with none
//! installed is a working daemon that serves its library unenriched, which is
//! why every caller handles `None` rather than unwrapping.

use acr_types::enrichment::LibraryEnricher;
use std::sync::{Arc, OnceLock};

static ENRICHER: OnceLock<Arc<dyn LibraryEnricher>> = OnceLock::new();

/// Install the enricher. The first call wins; later ones are ignored.
///
/// Set once, before any player starts, and never replaced: a library that
/// already handed a sink to one enricher would otherwise receive batches from
/// two, and nothing downstream could tell them apart.
pub fn set_enricher(e: Arc<dyn LibraryEnricher>) {
    let _ = ENRICHER.set(e);
}

/// The installed enricher, or `None` when this build installed none.
pub fn enricher() -> Option<Arc<dyn LibraryEnricher>> {
    // A test's enricher takes precedence over the process-wide one. The global
    // is a `OnceLock` on purpose - it is set once at startup and must not be
    // replaceable at runtime - but that makes it useless to a test suite that
    // is one process: the first test to install one would decide what every
    // later test sees, and the result would depend on the order tests happen
    // to run in. A thread-local override is per-test by construction, because
    // the harness gives each test its own thread, so two tests can install
    // different enrichers and neither can see the other's.
    #[cfg(test)]
    if let Some(e) = testing::current() {
        return Some(e);
    }
    ENRICHER.get().cloned()
}

/// Installing an enricher for the duration of one test.
#[cfg(test)]
pub mod testing {
    use super::*;
    use std::cell::RefCell;

    thread_local! {
        static OVERRIDE: RefCell<Option<Arc<dyn LibraryEnricher>>> = const { RefCell::new(None) };
    }

    pub(super) fn current() -> Option<Arc<dyn LibraryEnricher>> {
        OVERRIDE.with(|slot| slot.borrow().clone())
    }

    /// Removes the override when dropped, so a test cannot leak its enricher
    /// into whatever the harness runs next on the same thread.
    pub struct Installed;

    impl Drop for Installed {
        fn drop(&mut self) {
            OVERRIDE.with(|slot| *slot.borrow_mut() = None);
        }
    }

    /// Install `e` for the current thread until the returned guard is dropped.
    #[must_use]
    pub fn install(e: Arc<dyn LibraryEnricher>) -> Installed {
        OVERRIDE.with(|slot| *slot.borrow_mut() = Some(e));
        Installed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use acr_types::enrichment::{AlbumRef, ArtistRef, ArtistSummary, EnrichmentSink};
    use parking_lot::Mutex;

    struct Named(&'static str, Mutex<Vec<String>>);

    impl LibraryEnricher for Named {
        fn artist_summary(&self, name: &str) -> Option<ArtistSummary> {
            Some(ArtistSummary {
                name: name.to_string(),
                mbid: vec![self.0.to_string()],
                ..Default::default()
            })
        }
        fn artist_detail(&self, _name: &str) -> Option<acr_types::ArtistMeta> {
            None
        }
        fn enrich(
            &self,
            player: &str,
            _version: Option<String>,
            _artists: Vec<ArtistRef>,
            _albums: Vec<AlbumRef>,
            _sink: Arc<dyn EnrichmentSink>,
        ) {
            self.1.lock().push(player.to_string());
        }
    }

    #[test]
    fn with_nothing_installed_there_is_no_enricher() {
        // Nothing has been installed on *this* thread. The assertion is about
        // the override, not the global: were this reading a global a test
        // elsewhere had set, it would pass or fail by running order.
        assert!(testing::current().is_none());
    }

    #[test]
    fn an_installed_enricher_is_the_one_found() {
        let _guard = testing::install(Arc::new(Named("first", Mutex::new(vec![]))));
        assert_eq!(
            enricher().unwrap().artist_summary("x").unwrap().mbid,
            vec!["first"]
        );
    }

    /// The second half of the reason for the thread-local: a `OnceLock` would
    /// have kept the first test's enricher, and this one would read it.
    #[test]
    fn a_second_test_installs_its_own() {
        let _guard = testing::install(Arc::new(Named("second", Mutex::new(vec![]))));
        assert_eq!(
            enricher().unwrap().artist_summary("x").unwrap().mbid,
            vec!["second"]
        );
    }

    #[test]
    fn dropping_the_guard_uninstalls_it() {
        {
            let _guard = testing::install(Arc::new(Named("third", Mutex::new(vec![]))));
            assert!(enricher().is_some());
        }
        assert!(testing::current().is_none());
    }
}
