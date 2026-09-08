use serde::{Deserialize, Serialize};

/// One artist as the player daemon knows it: enough to look it up.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtistRef {
    pub id: String,
    pub name: String,
    /// True when this is an album-artist string the loader *split*, offered
    /// only so the metadata side can say whether that split was right.
    ///
    /// Such a name is not an artist in the library — the library holds the
    /// parts it was split into — so nothing on this side has a thumbnail, a
    /// biography or genres to keep for it, and a full lookup would download an
    /// image no route serves. The sweep answers it with
    /// [`ArtistSummary::split_into`] and nothing else.
    ///
    /// It exists because a split is lossy in one direction: a library that
    /// divided "Emerson, Lake & Palmer" into three holds three artists and no
    /// record of the name as a whole among them, so the whole name has to be
    /// offered separately or the wrong split can never be corrected.
    #[serde(default)]
    pub split_only: bool,
}

impl ArtistRef {
    /// An artist the library actually holds.
    pub fn named(id: String, name: String) -> Self {
        ArtistRef {
            id,
            name,
            split_only: false,
        }
    }

    /// An album-artist string the loader split, offered for the split question
    /// alone. See [`Self::split_only`].
    pub fn split_question(name: String) -> Self {
        ArtistRef {
            id: String::new(),
            name,
            split_only: true,
        }
    }
}

/// One album as the player daemon knows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AlbumRef {
    pub id: String,
    pub name: String,
    pub artist: String,
}

/// What a lookup learned about an artist.
///
/// This used to stop at the summary level a library's *lists* carry, with the
/// biography and the banner left behind on the metadata side for the player
/// daemon to fetch per request. It cannot stop there any more: fetching them
/// per request meant `GET /artist/<b64>` against the metadata daemon, and no
/// route on the metadata daemon may be called by the main daemon. Everything
/// the artist routes serve therefore travels in this batch, which already goes
/// the one direction that is allowed.
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
    /// The artist's banner URLs, exactly as the metadata side stored them.
    ///
    /// Carried for the same reason as [`Self::thumb_url`] and read the same
    /// way: the stored value is a provider's own URL today, so reconstructing
    /// it here is not possible at all.
    #[serde(default)]
    pub banner_url: Vec<String>,
    /// The artist's biography, and where it came from.
    ///
    /// The one field here that no *list* shows. It is carried because the
    /// artist detail routes serve it and the player daemon has no other way to
    /// get it: it used to be fetched per request from the metadata daemon, and
    /// that call is exactly what the one-way seam forbids.
    ///
    /// The two travel together and are applied together. A biography with no
    /// source is a legitimate state — a provider that supplies no attribution —
    /// but a source with no biography attributes nothing, and splitting them
    /// across two sweeps would produce one.
    #[serde(default)]
    pub biography: Option<String>,
    #[serde(default)]
    pub biography_source: Option<String>,
    /// The artists this name splits into, or `None` to make no claim.
    ///
    /// `Some` of one element asserts the name is a single artist and is **not**
    /// the same as `None`: it is what corrects a plain separator split that
    /// wrongly divided a name like "Emerson, Lake & Palmer". A loader splits on
    /// separators alone and cannot know the difference; this is how it finds
    /// out.
    ///
    /// `None` is the answer whenever the metadata side has nothing positive to
    /// say — MusicBrainz disabled, a lookup that found nothing, a name it was
    /// never asked about. That matters because the loader may have split on
    /// *configured* separators this side has never seen, and a claim built from
    /// the default list alone would undo a correct split. Only a positive
    /// answer travels.
    ///
    /// That is a partial defence rather than a complete one: the configured
    /// separator list does not cross the seam, so a name holding both a
    /// configured separator and a built-in one can still be claimed about and
    /// have the operator's split overridden. See
    /// `audiocontrol_metadata::artistsplitter::split_observation`.
    ///
    /// A claim is validated where it is applied, not where it is made:
    /// `data::library`'s merge refuses one whole rather than applying part of a
    /// malformed list, because a partly applied claim leaves the library worse
    /// than no claim at all.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub split_into: Option<Vec<String>>,
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

// `LibraryEnricher` used to live here, next to the sink, because both halves
// named it: the metadata side implemented it and the player side called it
// through an injected `Arc<dyn LibraryEnricher>`. The one-way seam ended that.
// A trait the player half calls is, by construction, the player half calling
// the metadata daemon, so the injection point and every call site went with
// the client that answered them, and what is left — starting a sweep — is the
// metadata daemon talking to itself. It lives in
// `audiocontrol_metadata::library_enricher` now.
//
// `EnrichmentSink` stays. It is the seam that runs the allowed way round: the
// metadata side calls it, over `POST /api/library/<p>/enrichment`, and the
// player half's library implements it.

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
    use std::sync::Arc;

    /// A sink that accepts everything, present only so the trait is exercised
    /// as a trait object.
    struct Accepts;

    impl EnrichmentSink for Accepts {
        fn apply(&self, _batch: EnrichmentBatch) -> Result<Applied, EnrichmentError> {
            Ok(Applied::default())
        }
    }

    /// The sink is only ever held as `Arc<dyn EnrichmentSink>`, so the trait
    /// has to stay object-safe. A method that broke that — a generic
    /// parameter, `self` by value, a return type mentioning `Self` — would
    /// still compile here and fail at every hand-out site instead, with an
    /// error naming the caller rather than the trait. This coercion puts the
    /// failure next to the definition.
    #[test]
    fn the_sink_trait_is_object_safe() {
        let s: Arc<dyn EnrichmentSink> = Arc::new(Accepts);
        assert!(s.apply(EnrichmentBatch::default()).is_ok());
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

    /// The detail fields survive the wire, and an absent one is `None` rather
    /// than a parse failure.
    ///
    /// These carry what the artist detail routes used to fetch per request
    /// from the metadata daemon. If they did not cross, the routes would serve
    /// an artist with no biography at all and every test of those routes that
    /// builds its own `ArtistMeta` would still pass — the failure would be
    /// visible only on a device.
    #[test]
    fn the_artist_detail_fields_cross_the_wire() {
        let bare: ArtistSummary = serde_json::from_str(r#"{"name":"Bowie"}"#).unwrap();
        assert_eq!(bare.biography, None);
        assert_eq!(bare.biography_source, None);
        assert!(bare.banner_url.is_empty());

        let full: ArtistSummary = serde_json::from_str(
            r#"{"name":"Bowie","biography":"Born in Brixton.",
                "biography_source":"LastFM",
                "banner_url":["https://example/banner.jpg"]}"#,
        )
        .unwrap();
        assert_eq!(full.biography.as_deref(), Some("Born in Brixton."));
        assert_eq!(full.biography_source.as_deref(), Some("LastFM"));
        assert_eq!(full.banner_url, vec!["https://example/banner.jpg"]);

        for summary in [bare, full] {
            let round_tripped: ArtistSummary =
                serde_json::from_str(&serde_json::to_string(&summary).unwrap()).unwrap();
            assert_eq!(round_tripped, summary);
        }
    }

    /// The three states of the split claim have to survive the wire, and the
    /// two that are easy to confuse are the ones that matter: absent means
    /// "no claim", and a one-element list means "this is one artist". A peer
    /// that predates the field sends neither, and must be read as making no
    /// claim rather than as asserting an empty split.
    #[test]
    fn the_three_states_of_a_split_claim_survive_the_wire() {
        let absent: ArtistSummary = serde_json::from_str(r#"{"name":"Bowie"}"#).unwrap();
        assert_eq!(absent.split_into, None, "a peer that predates the field");

        let one: ArtistSummary = serde_json::from_str(
            r#"{"name":"Emerson, Lake & Palmer","split_into":["Emerson, Lake & Palmer"]}"#,
        )
        .unwrap();
        assert_eq!(
            one.split_into,
            Some(vec!["Emerson, Lake & Palmer".to_string()]),
            "one element is an assertion, not an absence"
        );

        let several: ArtistSummary =
            serde_json::from_str(r#"{"name":"Alpha and Beta","split_into":["Alpha","Beta"]}"#)
                .unwrap();
        assert_eq!(
            several.split_into,
            Some(vec!["Alpha".to_string(), "Beta".to_string()])
        );

        for summary in [absent.clone(), one, several] {
            let round_tripped: ArtistSummary =
                serde_json::from_str(&serde_json::to_string(&summary).unwrap()).unwrap();
            assert_eq!(round_tripped, summary);
        }

        assert!(
            !serde_json::to_string(&absent).unwrap().contains("split_into"),
            "and no claim is not serialised at all, so an older peer sees nothing new"
        );
    }

    /// A reference built for the split question alone is distinguishable from
    /// one naming an artist the library holds. The sweep branches on it: a
    /// `split_only` name has no artist behind it, so a full lookup would
    /// download an image no route serves.
    #[test]
    fn a_split_question_is_distinguishable_from_an_artist_the_library_holds() {
        let held = ArtistRef::named("7".to_string(), "Pink Floyd".to_string());
        assert!(!held.split_only);

        let question = ArtistRef::split_question("Emerson, Lake & Palmer".to_string());
        assert!(question.split_only);
        assert_eq!(question.name, "Emerson, Lake & Palmer");
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
