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
/// lists carry. Biography and images stay with the metadata side.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtistSummary {
    pub name: String,
    #[serde(default)]
    pub mbid: Vec<String>,
    #[serde(default)]
    pub is_multi: bool,
    #[serde(default)]
    pub genres: Vec<String>,
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

    #[test]
    fn a_batch_round_trips_through_json_with_absent_fields_defaulting() {
        let b: EnrichmentBatch =
            serde_json::from_str(r#"{"albums":[{"id":"1"}]}"#).unwrap();
        assert_eq!(b.library_version, None);
        assert!(b.artists.is_empty());
        assert!(b.albums[0].genres.is_empty());
    }
}
