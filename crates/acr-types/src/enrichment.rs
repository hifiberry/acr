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
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnrichmentBatch {
    pub library_version: Option<String>,
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
    /// The batch was computed against a version that is no longer current.
    Stale { current: Option<String> },
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
    fn enrich(
        &self,
        player: &str,
        version: Option<String>,
        artists: Vec<ArtistRef>,
        albums: Vec<AlbumRef>,
        sink: Arc<dyn EnrichmentSink>,
    );
}

/// Merge one album's genres the way the in-library updater does: an empty
/// list never clears, an identical list is not a change.
pub fn merge_genres(target: &mut Vec<String>, incoming: &[String]) -> bool {
    if incoming.is_empty() || target.as_slice() == incoming {
        return false;
    }
    *target = incoming.to_vec();
    true
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
            _version: Option<String>,
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
        assert_eq!(b.library_version, None);
        assert!(b.artists.is_empty());
        assert!(b.albums[0].genres.is_empty());
    }
}
