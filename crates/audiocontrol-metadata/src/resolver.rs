//! The metadata side of the two name questions the player daemon asks:
//! which half of a split title is the artist, and whether an album-artist
//! string names one artist or several.
//!
//! Phase 1 replaces `InProcessResolver` with an HTTP client to the metadata
//! daemon; the two MusicBrainz-backed answers themselves do not change.

use acr_types::resolver::Resolver;
use acr_types::OrderResult;

/// Answers both questions in-process, against this build's own MusicBrainz
/// client and attribute cache.
pub struct InProcessResolver;

impl Resolver for InProcessResolver {
    fn title_order(&self, part1: &str, part2: &str) -> OrderResult {
        crate::title_order::detect_order(part1, part2)
    }

    fn artist_split(&self, name: &str, separators: &[String]) -> Option<Vec<String>> {
        crate::artistsplitter::split_artist_names_with_mbid_lookup(name, false, Some(separators))
    }
}
