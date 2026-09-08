//! Whether an album-artist string names one artist or several -- decided
//! locally, and corrected afterwards.
//!
//! This used to be a question, asked once per album while a library loaded:
//! `GET /resolve/artist-split` on the metadata daemon, a blocking round trip
//! with a 5 s bound, on a load that can cover 200,000 songs. **It was the last
//! call the main daemon made into the metadata daemon**, and with it gone every
//! connection across that seam is opened by the metadata side.
//!
//! What is left is the answer a MusicBrainz-disabled install has always given:
//! a plain separator split. That answer is wrong in both directions for a name
//! containing a separator -- "Emerson, Lake & Palmer" becomes three artists,
//! "Alpha and Beta" stays one -- and it is wrong only until the enrichment
//! sweep says otherwise. The correction travels in the batch that already
//! carries `is_multi`, `mbid` and genres for the same names
//! (`ArtistSummary::split_into`, applied by `data::library::apply_splits`), so
//! the first load meeting a new album artist shows the plain split and the
//! sweep fixes it -- the same eventual consistency genres, images and
//! biographies already have on that screen.
//!
//! **The other question this module used to answer**, gone the same way: which
//! half of a split stream title is the artist. `SongTitleSplitter`
//! (`crate::helpers::songtitlesplitter`) decides that locally --
//! `forced_order`, then a learned `default_order`, then a fixed heuristic --
//! and the metadata daemon reports what it finds afterwards as a per-station
//! observation, which feeds the learned order for later tracks. It does not
//! reach `POST song-information`: that route identifies a song by title and
//! artist, and an order swap disagrees with both by construction.
//!
//! Nothing is installed here any more, so there is no resolver, no setter and
//! no memo in front of either. The `Resolver` trait and `MetadataClient`'s
//! implementation of it survive only until the client itself goes.

use acr_types::artist_split::{split_artist_with_separators, DEFAULT_ARTIST_SEPARATORS};

/// Split an album-artist string on separators alone. `None` means one artist.
///
/// The separators are the player's configured `artist_separator` list where it
/// has one, and `DEFAULT_ARTIST_SEPARATORS` otherwise. The name is checked for
/// a separator first: without one there is nothing to split and the answer is
/// `None` whatever the list contains.
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
    let parts = split_artist_with_separators(name, &seps);
    if parts.len() > 1 {
        Some(parts)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_separator_split_is_plain() {
        assert_eq!(
            split_album_artist("A & B", None),
            Some(vec!["A".to_string(), "B".to_string()])
        );
        assert_eq!(split_album_artist("Solo", None), None);
    }

    /// The two names the whole correction path exists for. Neither answer here
    /// is right, and both are what the loader stores until a batch says
    /// otherwise -- see `data::library`'s split tests, which is where the
    /// correction itself is exercised.
    #[test]
    fn the_plain_split_is_wrong_in_both_directions() {
        assert_eq!(
            split_album_artist("Emerson, Lake & Palmer", None),
            Some(vec![
                "Emerson".to_string(),
                "Lake".to_string(),
                "Palmer".to_string()
            ]),
            "one artist, divided into three"
        );
        assert_eq!(
            split_album_artist("Alpha and Beta", None),
            None,
            "two artists, kept whole: ' and ' is not a default separator"
        );
    }

    /// A configured list is used instead of the defaults, and only it: a name
    /// that splits on a default separator but not on a configured one stays
    /// whole.
    #[test]
    fn a_configured_separator_list_replaces_the_defaults() {
        let pipe = vec!["|".to_string()];
        assert_eq!(
            split_album_artist("Alpha|Beta", Some(&pipe)),
            Some(vec!["Alpha".to_string(), "Beta".to_string()])
        );
        assert_eq!(split_album_artist("Alpha & Beta", Some(&pipe)), None);
    }
}
