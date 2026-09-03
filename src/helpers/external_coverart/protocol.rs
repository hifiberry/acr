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
/// `width`, `height`, `size_bytes` and `format` are accepted and ignored.
/// The first three because the `ImageGrader` needs dimensions the daemon
/// measured itself; `format` because the daemon sniffs the bytes it stores,
/// and a label it does not need is a label that can disagree with the bytes.
/// They are part of the contract so a later change can use them without a
/// protocol revision.
#[derive(Debug, Deserialize)]
struct WireImage {
    url: Option<String>,
    /// The image itself, base64-encoded.
    ///
    /// The preferred channel for an endpoint whose images are not publicly
    /// fetchable: it costs one round trip instead of two, needs no second
    /// authentication, and works even when the image host is unreachable
    /// from the daemon as well.
    data: Option<String>,
}

#[derive(Debug, Deserialize)]
struct WireResponse {
    images: Vec<WireImage>,
}

/// One usable image from a response, before the daemon decides how to serve
/// it.
///
/// `data` is still base64 here. Decoding belongs to localisation, which is
/// where a failure means something different: an entry with neither field is
/// the service reporting nothing (an answer), while content that cannot be
/// turned into a servable image is a fault on our side (an error). Keeping
/// them apart is what keeps the two cache lifetimes honest.
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedImage {
    pub url: Option<String>,
    pub data: Option<String>,
}

/// A classified response, before localisation.
#[derive(Debug, Clone, PartialEq)]
pub enum Parsed {
    /// Entries the endpoint offered, in the order it ranked them.
    Images(Vec<ParsedImage>),
    /// The service looked and there is none.
    NoArtwork,
    /// The service could not be reached, refused, or did not answer in the
    /// contract's shape.
    Error,
}

impl Parsed {
    /// Treat every entry as a URL to pass straight through.
    ///
    /// This is the behaviour for an endpoint that has not asked for
    /// localisation and delivers its images by URL. Inline bytes have no URL
    /// to pass through, so they are dropped here; `localize::resolve` is what
    /// gives them one.
    pub fn into_lookup(self) -> Lookup {
        match self {
            Parsed::Images(images) => {
                let urls: Vec<String> = images.into_iter().filter_map(|image| image.url).collect();
                if urls.is_empty() {
                    Lookup::NoArtwork
                } else {
                    Lookup::Found(urls)
                }
            }
            Parsed::NoArtwork => Lookup::NoArtwork,
            Parsed::Error => Lookup::Error,
        }
    }
}

/// Classify a 2xx response body.
///
/// A body that does not parse is `Error`, not `NoArtwork`: a service that
/// answers in the wrong shape has told us nothing about the track.
pub fn parse_response(body: &serde_json::Value) -> Parsed {
    let Ok(response) = serde_json::from_value::<WireResponse>(body.clone()) else {
        return Parsed::Error;
    };

    let clean = |value: Option<String>| {
        value
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
    };

    let images: Vec<ParsedImage> = response
        .images
        .into_iter()
        .map(|image| ParsedImage {
            url: clean(image.url),
            data: clean(image.data),
        })
        .filter(|image| image.url.is_some() || image.data.is_some())
        .collect();

    if images.is_empty() {
        // The service answered in the right shape with nothing usable.
        // Re-asking would produce the same list, so this is an answer.
        Parsed::NoArtwork
    } else {
        Parsed::Images(images)
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
            parse_response(&body).into_lookup(),
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
            Parsed::NoArtwork
        );
    }

    /// A body that does not speak the contract is a fault in the service, not
    /// a statement that the track has no artwork.
    #[test]
    fn a_body_without_an_images_field_is_an_error() {
        assert_eq!(
            parse_response(&serde_json::json!({ "result": "ok" })),
            Parsed::Error
        );
    }

    #[test]
    fn an_image_without_a_url_is_skipped() {
        let body = serde_json::json!({
            "images": [{ "width": 1000 }, { "url": "https://img.example/a.jpg" }]
        });
        assert_eq!(
            parse_response(&body).into_lookup(),
            Lookup::Found(vec!["https://img.example/a.jpg".to_string()])
        );
    }

    #[test]
    fn an_inline_image_is_parsed_without_being_decoded() {
        let body = serde_json::json!({
            "images": [{ "data": "aGVsbG8=" }]
        });

        let Parsed::Images(images) = parse_response(&body) else {
            panic!("an inline image is an answer");
        };
        assert_eq!(images.len(), 1);
        assert_eq!(images[0].data.as_deref(), Some("aGVsbG8="));
        assert_eq!(images[0].url, None);
    }

    #[test]
    fn a_url_and_inline_image_both_survive_parsing() {
        let body = serde_json::json!({
            "images": [{ "url": "https://img.example/a.jpg", "data": "aGVsbG8=" }]
        });

        let Parsed::Images(images) = parse_response(&body) else {
            panic!("an answer");
        };
        assert_eq!(images[0].url.as_deref(), Some("https://img.example/a.jpg"));
        assert_eq!(images[0].data.as_deref(), Some("aGVsbG8="));
    }

    /// An entry with neither field is the service listing nothing usable --
    /// the same case the previous increment already classified as an answer,
    /// not a fault. Re-asking would return the same list.
    #[test]
    fn an_entry_with_neither_url_nor_data_is_skipped() {
        let body = serde_json::json!({
            "images": [{ "width": 1000 }, { "url": "https://img.example/a.jpg" }]
        });

        let Parsed::Images(images) = parse_response(&body) else {
            panic!("an answer");
        };
        assert_eq!(images.len(), 1);
        assert_eq!(images[0].url.as_deref(), Some("https://img.example/a.jpg"));
    }

    #[test]
    fn a_list_of_only_empty_entries_is_no_artwork() {
        let body = serde_json::json!({ "images": [{ "width": 1000 }, { "url": "", "data": "  " }] });
        assert_eq!(parse_response(&body), Parsed::NoArtwork);
    }

    /// Until the localiser exists, the pass-through conversion is the whole
    /// behaviour: URLs go out as the endpoint gave them, and inline data has
    /// nowhere to go yet.
    #[test]
    fn into_lookup_passes_urls_through_and_drops_inline_data() {
        let parsed = Parsed::Images(vec![
            ParsedImage { url: Some("https://img.example/a.jpg".into()), data: None },
            ParsedImage { url: None, data: Some("aGVsbG8=".into()) },
        ]);

        assert_eq!(
            parsed.into_lookup(),
            Lookup::Found(vec!["https://img.example/a.jpg".to_string()])
        );
    }

    #[test]
    fn into_lookup_carries_the_other_two_classes_unchanged() {
        assert_eq!(Parsed::NoArtwork.into_lookup(), Lookup::NoArtwork);
        assert_eq!(Parsed::Error.into_lookup(), Lookup::Error);
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
