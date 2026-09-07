use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// One artist as the player daemon knows it: enough to look it up.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtistRef {
    pub id: String,
    pub name: String,
}

/// One album as the player daemon knows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AlbumRef {
    pub id: String,
    pub name: String,
    pub artist: String,
}

/// What a lookup learned about an artist, at the summary level the library
/// lists carry. The biography stays with the metadata side.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtistSummary {
    pub name: String,
    #[serde(default)]
    pub mbid: Vec<String>,
    #[serde(default)]
    pub is_multi: bool,
    #[serde(default)]
    pub genres: Vec<String>,
    /// The artist's thumbnail URLs, exactly as the metadata side stored them.
    ///
    /// The artist *list* route serialises this field, and its presence is how
    /// a client knows an image exists at all: the metadata side writes a URL
    /// only when a lookup found one, so an artist without an image serves an
    /// empty list. That makes it part of what a library's lists are built
    /// from, and so part of the summary.
    ///
    /// It is carried rather than rebuilt from the artist's name because the
    /// stored value is not always the daemon's own cover art URL — a
    /// provider's own URLs reach the same field — and reconstructing it would
    /// mean reproducing every writer of it.
    ///
    /// Absent on the wire means "nothing to say", not "no images": a peer that
    /// predates the field must not be read as clearing what a library holds.
    #[serde(default)]
    pub thumb_url: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AlbumGenres {
    pub id: String,
    #[serde(default)]
    pub genres: Vec<String>,
}

/// A batch of results for one player's library. This is the JSON body of
/// `POST /api/library/<p>/enrichment` in Phase 1; in Phase 0 it crosses a
/// function call.
///
/// Unknown fields are refused rather than ignored, and that is not tidiness.
/// Every field here is `#[serde(default)]`, so a caller that named the
/// *version* field instead of the generation would parse cleanly, leave
/// `library_generation` at `None` — which means "make no claim" — and have
/// every batch merged unchecked. The failure the generation exists to prevent
/// would arrive through a typo, silently. Refusing the field turns that into a
/// 422 the caller cannot miss.
///
/// The trade is deliberate: adding a field later means an older peer refuses a
/// batch carrying it. Both daemons ship in one package, so the only window is
/// the seconds of an upgrade in which one has restarted and the other has not,
/// and the puller retries — a few refused batches against a silent no-op.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnrichmentBatch {
    /// The library generation this batch was computed against, or `None` to
    /// make no claim.
    ///
    /// Deliberately *not* the library version: a version moves on every merge,
    /// including this batch's own, so two sweeps running against one library
    /// would read each other's bumps as a reload. A generation moves only when
    /// the library is rebuilt, which is the one thing that makes a batch
    /// unmergeable — see `LibraryVersion::bump_generation` on the player side.
    #[serde(default)]
    pub library_generation: Option<String>,
    #[serde(default)]
    pub artists: Vec<ArtistSummary>,
    #[serde(default)]
    pub albums: Vec<AlbumGenres>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Applied {
    pub artists: usize,
    pub albums: usize,
    pub library_version: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EnrichmentError {
    /// The batch was computed against a generation of the library that is no
    /// longer loaded: it was rebuilt in between, and the albums and artists
    /// the batch describes may no longer be there.
    Stale { current_generation: Option<String> },
    NoSuchLibrary,
}

/// Implemented by a library: receives batches and merges them.
pub trait EnrichmentSink: Send + Sync {
    fn apply(&self, batch: EnrichmentBatch) -> Result<Applied, EnrichmentError>;
}

/// Implemented on the metadata side: what a library asks for.
pub trait LibraryEnricher: Send + Sync {
    /// The summary a library shows in its lists, if one is already known.
    /// Called while a library loads, once per artist. Must not do network I/O.
    fn artist_summary(&self, name: &str) -> Option<ArtistSummary>;
    /// Everything known about an artist, for the detail routes.
    /// May take up to the caller's timeout; must not block longer.
    fn artist_detail(&self, name: &str) -> Option<crate::ArtistMeta>;
    /// An artist's image and the MIME type it should be served as.
    ///
    /// This is what `/library/<p>/image/artist:<name>` answers with, and the
    /// pair is served verbatim — the caller does not re-derive the type from
    /// the bytes. Unlike [`Self::artist_summary`] this may reach the network:
    /// the in-process implementation downloads an image the first time one is
    /// asked for. It is therefore only called from a request, never while a
    /// library loads.
    fn artist_image(&self, name: &str) -> Option<(Vec<u8>, String)>;
    /// Genres already known for an album, or `None` when nothing is stored.
    ///
    /// `Some(vec![])` is a real answer and not the same as `None`: it records
    /// a lookup that ran and found no genres, which is what keeps the lookup
    /// from being repeated. Called once per album while a library loads, so
    /// like [`Self::artist_summary`] it must not do network I/O.
    fn album_genres(&self, album_id: &str) -> Option<Vec<String>>;
    /// Start enriching a library. Returns at once; results arrive through the sink.
    ///
    /// `generation` is the library generation every batch of this sweep will
    /// name. It is fixed for the life of the sweep: a library that is rebuilt
    /// meanwhile refuses the next batch, which ends the sweep, and the rebuild
    /// asks again for whatever it now needs.
    fn enrich(
        &self,
        player: &str,
        generation: Option<String>,
        artists: Vec<ArtistRef>,
        albums: Vec<AlbumRef>,
        sink: Arc<dyn EnrichmentSink>,
    );
}

/// Merge one album's genres the way the in-library updater does: an empty
/// list never clears, a list holding the same genres is not a change.
pub fn merge_genres(target: &mut Vec<String>, incoming: &[String]) -> bool {
    if incoming.is_empty() || same_genres(target, incoming) {
        return false;
    }
    *target = incoming.to_vec();
    true
}

/// Whether two genre lists say the same thing.
///
/// Order is not part of what they say: a provider that returns the same genres
/// in a different order on the next sweep is not new information, and treating
/// it as a change costs a library version bump, a fresh ETag for every client
/// and another poll cycle for nothing a client can act on.
///
/// This is a comparison only — the *stored* order is left as it is. Sorting
/// what is stored would change what clients read in `genres`, for no benefit
/// to them.
fn same_genres(stored: &[String], incoming: &[String]) -> bool {
    if stored.len() != incoming.len() {
        return false;
    }
    if stored == incoming {
        return true;
    }
    let mut stored: Vec<&str> = stored.iter().map(String::as_str).collect();
    let mut incoming: Vec<&str> = incoming.iter().map(String::as_str).collect();
    stored.sort_unstable();
    incoming.sort_unstable();
    stored == incoming
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An enricher that knows nothing, present only so the trait is exercised
    /// as a trait object.
    struct Nothing;

    impl LibraryEnricher for Nothing {
        fn artist_summary(&self, _name: &str) -> Option<ArtistSummary> {
            None
        }
        fn artist_detail(&self, _name: &str) -> Option<crate::ArtistMeta> {
            None
        }
        fn artist_image(&self, _name: &str) -> Option<(Vec<u8>, String)> {
            None
        }
        fn album_genres(&self, _album_id: &str) -> Option<Vec<String>> {
            None
        }
        fn enrich(
            &self,
            _player: &str,
            _generation: Option<String>,
            _artists: Vec<ArtistRef>,
            _albums: Vec<AlbumRef>,
            _sink: Arc<dyn EnrichmentSink>,
        ) {
        }
    }

    /// The enricher is only ever held as `Arc<dyn LibraryEnricher>`, so the
    /// trait has to stay object-safe. A method that broke that — a generic
    /// parameter, `self` by value, a return type mentioning `Self` — would
    /// still compile here and fail at every injection site instead, with an
    /// error naming the caller rather than the trait. This coercion puts the
    /// failure next to the definition.
    #[test]
    fn the_trait_is_object_safe() {
        let e: Arc<dyn LibraryEnricher> = Arc::new(Nothing);
        assert!(e.artist_summary("x").is_none());
        assert!(e.artist_detail("x").is_none());
        assert!(e.artist_image("x").is_none());
        assert!(e.album_genres("1").is_none());
    }

    #[test]
    fn an_empty_incoming_list_never_clears() {
        let mut g = vec!["rock".to_string()];
        assert!(!merge_genres(&mut g, &[]));
        assert_eq!(g, vec!["rock"]);
    }

    #[test]
    fn an_identical_list_is_not_a_change() {
        let mut g = vec!["rock".to_string()];
        assert!(!merge_genres(&mut g, &["rock".to_string()]));
    }

    /// The same genres in a different order say nothing new. Counting a
    /// reorder as a change bumps the library version, invalidates every
    /// client's cached list and costs another poll cycle, once per sweep, for
    /// content no client can tell apart.
    #[test]
    fn a_reordering_of_the_same_genres_is_not_a_change() {
        let mut g = vec!["rock".to_string(), "pop".to_string()];
        assert!(!merge_genres(
            &mut g,
            &["pop".to_string(), "rock".to_string()]
        ));
        assert_eq!(
            g,
            vec!["rock", "pop"],
            "and what is stored keeps the order a client already read"
        );
    }

    /// Order-insensitivity must not swallow a genuinely different list, not
    /// even one that differs only in how often a genre appears or in a single
    /// entry among several.
    #[test]
    fn a_genuinely_different_list_is_still_a_change() {
        let mut g = vec!["rock".to_string(), "pop".to_string()];
        assert!(merge_genres(
            &mut g,
            &["pop".to_string(), "folk".to_string()]
        ));
        assert_eq!(g, vec!["pop", "folk"]);

        let mut same_length = vec!["rock".to_string(), "rock".to_string()];
        assert!(merge_genres(
            &mut same_length,
            &["rock".to_string(), "pop".to_string()]
        ));

        let mut longer = vec!["rock".to_string()];
        assert!(merge_genres(
            &mut longer,
            &["rock".to_string(), "pop".to_string()]
        ));
        assert_eq!(longer, vec!["rock", "pop"]);
    }

    #[test]
    fn a_different_list_replaces_and_reports() {
        let mut g = vec![];
        assert!(merge_genres(&mut g, &["jazz".to_string()]));
        assert_eq!(g, vec!["jazz"]);
    }

    /// The field is optional on the wire in both directions: a peer that does
    /// not send it must not fail to parse, and one that does must be understood.
    #[test]
    fn an_artist_summary_thumbnail_is_optional_on_the_wire() {
        let without: ArtistSummary = serde_json::from_str(r#"{"name":"Bowie"}"#).unwrap();
        assert!(without.thumb_url.is_empty());

        let with: ArtistSummary = serde_json::from_str(
            r#"{"name":"Bowie","thumb_url":["/api/coverart/artist/YWJj/image"]}"#,
        )
        .unwrap();
        assert_eq!(with.thumb_url, vec!["/api/coverart/artist/YWJj/image"]);

        let round_tripped: ArtistSummary =
            serde_json::from_str(&serde_json::to_string(&with).unwrap()).unwrap();
        assert_eq!(round_tripped, with);
    }

    #[test]
    fn a_batch_round_trips_through_json_with_absent_fields_defaulting() {
        let b: EnrichmentBatch =
            serde_json::from_str(r#"{"albums":[{"id":"1"}]}"#).unwrap();
        assert_eq!(b.library_generation, None);
        assert!(b.artists.is_empty());
        assert!(b.albums[0].genres.is_empty());
    }

    /// A batch names a *generation*, not a version. The field is what the
    /// player daemon's route reads, and a caller still sending the old name
    /// would otherwise silently make no claim at all and have every batch
    /// applied unchecked.
    #[test]
    fn a_batch_carries_the_generation_it_was_computed_against() {
        let parsed: EnrichmentBatch =
            serde_json::from_str(r#"{"library_generation":"a3f9-g2","albums":[]}"#).unwrap();
        assert_eq!(parsed.library_generation.as_deref(), Some("a3f9-g2"));

        let round_tripped: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&parsed).unwrap()).unwrap();
        assert_eq!(round_tripped["library_generation"], "a3f9-g2");
    }

    /// And a batch that names something else is refused rather than quietly
    /// making no claim at all.
    ///
    /// `library_version` is the field to test with, because it is the one a
    /// caller would plausibly write by mistake: it is the other token in this
    /// exchange, it appears in the 200 and the 409, and every field here
    /// defaults — so without the refusal this body would parse into a batch
    /// with `library_generation: None`, which the route applies unchecked.
    #[test]
    fn a_batch_naming_an_unknown_field_is_refused() {
        let error = serde_json::from_str::<EnrichmentBatch>(
            r#"{"library_version":"a3f9-c7","albums":[]}"#,
        )
        .expect_err("the wrong token must not parse into a batch that claims nothing");
        assert!(
            error.to_string().contains("library_version"),
            "the refusal should name the field that was not understood, got: {}",
            error
        );
    }
}
