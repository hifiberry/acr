//! The two synchronous questions the player side asks about names: which half
//! of a split title is the artist, and whether an album-artist string names
//! one artist or several. `main` installs a resolver — in Phase 0 the
//! in-process one from `audiocontrol-metadata`, in Phase 1 an HTTP client for
//! the metadata daemon — and every caller asks here instead of reaching for
//! MusicBrainz directly. With no resolver installed, each question has the
//! answer a MusicBrainz-disabled install gives today: `title_order` is always
//! `Unknown`, and `split_album_artist` falls back to a plain separator split.
//!
//! There is deliberately no memo here in front of the resolver.
//! `split_artist_names_with_mbid_lookup` already caches its answer in the
//! attribute cache, keyed on the artist name, with the same expiry and
//! invalidation (`clear`, `remove_by_prefix`) as every other entry there. A
//! second, process-lifetime memo on top would keep answering from that cache
//! entry after it had been invalidated elsewhere, which is a change in
//! today's behaviour, not a preserved one — so it is left out.

use acr_types::artist_split::{split_artist_with_separators, DEFAULT_ARTIST_SEPARATORS};
use acr_types::resolver::Resolver;
use acr_types::OrderResult;
use std::sync::{Arc, OnceLock};

static RESOLVER: OnceLock<Arc<dyn Resolver>> = OnceLock::new();

/// Install the resolver. The first call wins; later ones are ignored.
///
/// Set once, before any player starts, and never replaced, for the same
/// reason as the library enricher: callers that already have an answer from
/// one resolver must not silently start getting answers from another.
pub fn set_resolver(r: Arc<dyn Resolver>) {
    let _ = RESOLVER.set(r);
}

/// The installed resolver, or `None` when this build installed none.
pub fn resolver() -> Option<Arc<dyn Resolver>> {
    // See `enrichment::enricher` for why this is a thread-local override in
    // tests rather than reading straight from the `OnceLock`: the global can
    // be set only once per process, so two tests that each want a different
    // resolver installed cannot both use it without one depending on the
    // other having not run yet.
    #[cfg(test)]
    if let Some(r) = testing::current() {
        return Some(r);
    }
    RESOLVER.get().cloned()
}

/// Which half of a split title is the artist. `Unknown` with no resolver
/// installed, matching a MusicBrainz-disabled install today.
pub fn title_order(part1: &str, part2: &str) -> OrderResult {
    match resolver() {
        Some(r) => r.title_order(part1, part2),
        None => OrderResult::Unknown,
    }
}

/// `None` means one artist. Names without a separator never reach the
/// resolver — there is nothing for it to decide.
pub fn split_album_artist(name: &str, separators: Option<&[String]>) -> Option<Vec<String>> {
    let seps: Vec<String> = separators.map(|s| s.to_vec()).unwrap_or_else(|| {
        DEFAULT_ARTIST_SEPARATORS
            .iter()
            .map(|s| s.to_string())
            .collect()
    });
    if !seps.iter().any(|s| name.contains(s.as_str())) {
        return None;
    }
    match resolver() {
        Some(r) => r.artist_split(name, &seps),
        None => {
            let parts = split_artist_with_separators(name, &seps);
            if parts.len() > 1 {
                Some(parts)
            } else {
                None
            }
        }
    }
}

/// Installing a resolver for the duration of one test.
#[cfg(test)]
pub mod testing {
    use super::*;
    use std::cell::RefCell;

    thread_local! {
        static OVERRIDE: RefCell<Option<Arc<dyn Resolver>>> = const { RefCell::new(None) };
    }

    pub(super) fn current() -> Option<Arc<dyn Resolver>> {
        OVERRIDE.with(|slot| slot.borrow().clone())
    }

    /// Removes the override when dropped, so a test cannot leak its resolver
    /// into whatever the harness runs next on the same thread.
    pub struct Installed;

    impl Drop for Installed {
        fn drop(&mut self) {
            OVERRIDE.with(|slot| *slot.borrow_mut() = None);
        }
    }

    /// Install `r` for the current thread until the returned guard is dropped.
    #[must_use]
    pub fn install(r: Arc<dyn Resolver>) -> Installed {
        OVERRIDE.with(|slot| *slot.borrow_mut() = Some(r));
        Installed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn without_a_resolver_the_order_is_unknown() {
        assert_eq!(title_order("a", "b"), OrderResult::Unknown);
    }

    #[test]
    fn without_a_resolver_a_separator_split_is_plain() {
        assert_eq!(
            split_album_artist("A & B", None),
            Some(vec!["A".to_string(), "B".to_string()])
        );
        assert_eq!(split_album_artist("Solo", None), None);
    }

    struct Fixed(OrderResult, Option<Vec<String>>);

    impl Resolver for Fixed {
        fn title_order(&self, _part1: &str, _part2: &str) -> OrderResult {
            self.0.clone()
        }
        fn artist_split(&self, _name: &str, _separators: &[String]) -> Option<Vec<String>> {
            self.1.clone()
        }
    }

    #[test]
    fn an_installed_resolver_is_the_one_asked() {
        let _guard = testing::install(Arc::new(Fixed(OrderResult::SongArtist, None)));
        assert_eq!(title_order("a", "b"), OrderResult::SongArtist);
        // A name with a separator reaches the resolver, whatever it answers.
        assert_eq!(split_album_artist("A & B", None), None);
    }

    /// The second half of the reason for the thread-local: a `OnceLock` would
    /// have kept the first test's resolver, and this one would read it.
    #[test]
    fn a_second_test_installs_its_own() {
        let _guard = testing::install(Arc::new(Fixed(
            OrderResult::ArtistSong,
            Some(vec!["A".to_string(), "B".to_string()]),
        )));
        assert_eq!(title_order("a", "b"), OrderResult::ArtistSong);
        assert_eq!(
            split_album_artist("A & B", None),
            Some(vec!["A".to_string(), "B".to_string()])
        );
    }

    #[test]
    fn dropping_the_guard_uninstalls_it() {
        {
            let _guard = testing::install(Arc::new(Fixed(OrderResult::Undecided, None)));
            assert!(resolver().is_some());
        }
        assert!(testing::current().is_none());
    }

    /// A name without a separator never reaches the resolver: even one
    /// answering `Some` for everything must not be asked.
    #[test]
    fn a_name_without_a_separator_never_reaches_the_resolver() {
        let _guard = testing::install(Arc::new(Fixed(
            OrderResult::Unknown,
            Some(vec!["should not".to_string(), "be seen".to_string()]),
        )));
        assert_eq!(split_album_artist("Solo", None), None);
    }
}
