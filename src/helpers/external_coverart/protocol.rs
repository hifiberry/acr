//! The JSON contract an external cover art endpoint speaks, and how each
//! kind of answer is cached.

use serde::{Deserialize, Serialize};

use super::config::{EndpointConfig, ERROR_TTL_SECONDS};

/// What an endpoint said, as three distinct classes.
///
/// The distinction is the reason this type exists. "There is no artwork for
/// this track" and "the service is down" look alike at the call site and must
/// not be cached alike: the first is an answer worth keeping for weeks, the
/// second is a fault that must be retried soon.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Lookup {
    /// Artwork, in the order the service ranked it.
    Found(Vec<String>),
    /// The service looked and there is none.
    NoArtwork,
    /// The service could not be reached, refused, or did not answer in the
    /// contract's shape.
    Error,
}

impl Lookup {
    /// The URLs to show. An error yields none: it is cached so the daemon
    /// stops asking, never so it can be displayed.
    pub fn urls(&self) -> Vec<String> {
        match self {
            Lookup::Found(urls) => urls.clone(),
            Lookup::NoArtwork | Lookup::Error => Vec::new(),
        }
    }
}

/// One image as an endpoint reports it.
///
/// `width`, `height`, `size_bytes` and `format` are accepted and ignored:
/// `CoverartProvider` deals in URLs, and the `ImageGrader` needs dimensions
/// the daemon measured itself. They are part of the contract so a later
/// change can use them without a protocol revision.
#[derive(Debug, Deserialize)]
struct WireImage {
    url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct WireResponse {
    images: Vec<WireImage>,
}

/// Classify a 2xx response body.
///
/// A body that does not parse is `Error`, not `NoArtwork`: a service that
/// answers in the wrong shape has told us nothing about the track.
pub fn parse_response(body: &serde_json::Value) -> Lookup {
    let Ok(response) = serde_json::from_value::<WireResponse>(body.clone()) else {
        return Lookup::Error;
    };

    let urls: Vec<String> = response
        .images
        .into_iter()
        .filter_map(|image| image.url)
        .map(|url| url.trim().to_string())
        .filter(|url| !url.is_empty())
        .collect();

    if urls.is_empty() {
        // The service answered in the right shape with nothing usable.
        // Re-asking would produce the same list, so this is an answer.
        Lookup::NoArtwork
    } else {
        Lookup::Found(urls)
    }
}

/// How long this answer is worth keeping.
pub fn ttl_seconds(lookup: &Lookup, endpoint: &EndpointConfig) -> u64 {
    const DAY: u64 = 24 * 3600;
    match lookup {
        Lookup::Found(_) => endpoint.cache_ttl_days * DAY,
        Lookup::NoArtwork => endpoint.negative_cache_ttl_days * DAY,
        Lookup::Error => ERROR_TTL_SECONDS,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::config::Trigger;
    use crate::helpers::coverart::CoverartMethod;

    fn endpoint() -> EndpointConfig {
        EndpointConfig {
            name: "llm".to_string(),
            display_name: "AI Lookup".to_string(),
            url: "https://x.example/c".to_string(),
            methods: vec![CoverartMethod::Song],
            headers: Default::default(),
            timeout_seconds: 45,
            trigger: Trigger::Fallback,
            cache_ttl_days: 30,
            negative_cache_ttl_days: 7,
            max_concurrent: 1,
            localize: false,
            max_image_bytes: 8 * 1024 * 1024,
        }
    }

    #[test]
    fn images_are_read_in_order() {
        let body = serde_json::json!({
            "images": [
                { "url": "https://img.example/a.jpg", "width": 1000, "height": 1000 },
                { "url": "https://img.example/b.jpg" }
            ]
        });
        assert_eq!(
            parse_response(&body),
            Lookup::Found(vec![
                "https://img.example/a.jpg".to_string(),
                "https://img.example/b.jpg".to_string()
            ])
        );
    }

    /// An empty list is the service saying it looked and there is nothing.
    /// That is an answer, and a different one from a failure.
    #[test]
    fn an_empty_image_list_is_no_artwork() {
        assert_eq!(
            parse_response(&serde_json::json!({ "images": [] })),
            Lookup::NoArtwork
        );
    }

    /// A body that does not speak the contract is a fault in the service, not
    /// a statement that the track has no artwork.
    #[test]
    fn a_body_without_an_images_field_is_an_error() {
        assert_eq!(
            parse_response(&serde_json::json!({ "result": "ok" })),
            Lookup::Error
        );
    }

    #[test]
    fn an_image_without_a_url_is_skipped() {
        let body = serde_json::json!({
            "images": [{ "width": 1000 }, { "url": "https://img.example/a.jpg" }]
        });
        assert_eq!(
            parse_response(&body),
            Lookup::Found(vec!["https://img.example/a.jpg".to_string()])
        );
    }

    /// If every entry was unusable the service answered, but with nothing we
    /// can show. Cached as an answer, not as a fault: re-asking would return
    /// the same malformed list.
    #[test]
    fn a_list_of_only_unusable_entries_is_no_artwork() {
        let body = serde_json::json!({ "images": [{ "width": 1000 }, { "url": "" }] });
        assert_eq!(parse_response(&body), Lookup::NoArtwork);
    }

    /// The three classes are cached for very different lengths, which is the
    /// point of distinguishing them: an outage must not blank a track for the
    /// weeks a real answer is kept.
    #[test]
    fn each_class_has_its_own_ttl() {
        let endpoint = endpoint();
        assert_eq!(
            ttl_seconds(&Lookup::Found(vec!["https://img.example/a.jpg".into()]), &endpoint),
            30 * 24 * 3600
        );
        assert_eq!(ttl_seconds(&Lookup::NoArtwork, &endpoint), 7 * 24 * 3600);
        assert_eq!(ttl_seconds(&Lookup::Error, &endpoint), ERROR_TTL_SECONDS);
    }

    /// An error is cached only so the daemon stops hammering a broken
    /// service; it must never be shown as an answer.
    #[test]
    fn only_found_yields_urls() {
        assert_eq!(
            Lookup::Found(vec!["https://img.example/a.jpg".into()]).urls(),
            vec!["https://img.example/a.jpg".to_string()]
        );
        assert!(Lookup::NoArtwork.urls().is_empty());
        assert!(Lookup::Error.urls().is_empty());
    }

    /// Cached entries outlive a daemon restart, so the representation has to
    /// survive a round trip through the attribute cache.
    #[test]
    fn a_lookup_round_trips_through_serde() {
        for lookup in [
            Lookup::Found(vec!["https://img.example/a.jpg".into()]),
            Lookup::NoArtwork,
            Lookup::Error,
        ] {
            let encoded = serde_json::to_string(&lookup).expect("serialises");
            let decoded: Lookup = serde_json::from_str(&encoded).expect("deserialises");
            assert_eq!(decoded, lookup);
        }
    }
}
