//! Where the player side finds a Spotify Web API access token, if a source
//! was injected.
//!
//! The librespot backend does not own an OAuth client and does not refresh
//! tokens itself: `main` installs whichever implementation this build has —
//! in Phase 0 the in-process one from `audiocontrol-metadata`, in Phase 1 an
//! HTTP client for the metadata daemon — and the backend asks here before
//! every command that needs one. A build with none installed behaves like a
//! librespot backend with no linked Spotify account: every command that needs
//! a token is refused.

use acr_types::token::AccessTokenSource;
use std::sync::{Arc, OnceLock};

static TOKEN_SOURCE: OnceLock<Arc<dyn AccessTokenSource>> = OnceLock::new();

/// Install the token source. The first call wins; later ones are ignored.
///
/// Set once, before any player starts, and never replaced: for the same
/// reason as the library enricher and the resolver, a caller that already
/// asked one source for a token must not silently start asking another.
pub fn set_token_source(t: Arc<dyn AccessTokenSource>) {
    let _ = TOKEN_SOURCE.set(t);
}

/// The installed token source, or `None` when this build installed none.
pub fn token_source() -> Option<Arc<dyn AccessTokenSource>> {
    // See `enrichment::enricher` for why this is a thread-local override in
    // tests rather than reading straight from the `OnceLock`: the global can
    // be set only once per process, so two tests that each want a different
    // source installed cannot both use it without one depending on the other
    // having not run yet.
    #[cfg(test)]
    if let Some(t) = testing::current() {
        return Some(t);
    }
    TOKEN_SOURCE.get().cloned()
}

/// Installing a token source for the duration of one test.
#[cfg(test)]
pub mod testing {
    use super::*;
    use std::cell::RefCell;

    thread_local! {
        static OVERRIDE: RefCell<Option<Arc<dyn AccessTokenSource>>> = const { RefCell::new(None) };
    }

    pub(super) fn current() -> Option<Arc<dyn AccessTokenSource>> {
        OVERRIDE.with(|slot| slot.borrow().clone())
    }

    /// Removes the override when dropped, so a test cannot leak its token
    /// source into whatever the harness runs next on the same thread.
    pub struct Installed;

    impl Drop for Installed {
        fn drop(&mut self) {
            OVERRIDE.with(|slot| *slot.borrow_mut() = None);
        }
    }

    /// Install `t` for the current thread until the returned guard is dropped.
    #[must_use]
    pub fn install(t: Arc<dyn AccessTokenSource>) -> Installed {
        OVERRIDE.with(|slot| *slot.borrow_mut() = Some(t));
        Installed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixed(Option<&'static str>);

    impl AccessTokenSource for Fixed {
        fn access_token(&self) -> Option<String> {
            self.0.map(|s| s.to_string())
        }
    }

    #[test]
    fn with_nothing_installed_there_is_no_source() {
        assert!(testing::current().is_none());
    }

    #[test]
    fn an_installed_source_is_the_one_asked() {
        // Not a real credential -- an obvious placeholder for the test.
        let _guard = testing::install(Arc::new(Fixed(Some("placeholder-token"))));
        assert_eq!(
            token_source().unwrap().access_token(),
            Some("placeholder-token".to_string())
        );
    }

    /// The second half of the reason for the thread-local: a `OnceLock` would
    /// have kept the first test's source, and this one would read it.
    #[test]
    fn a_second_test_installs_its_own() {
        let _guard = testing::install(Arc::new(Fixed(None)));
        assert_eq!(token_source().unwrap().access_token(), None);
    }

    #[test]
    fn dropping_the_guard_uninstalls_it() {
        {
            let _guard = testing::install(Arc::new(Fixed(Some("placeholder-token"))));
            assert!(token_source().is_some());
        }
        assert!(testing::current().is_none());
    }
}
