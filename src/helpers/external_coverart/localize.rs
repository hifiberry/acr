//! Turning an endpoint's answer into images the daemon can actually serve.
//!
//! An endpoint's image URLs reach three consumers: the client rendering
//! `cover_art_url`, the `ImageGrader`'s metadata fetch, and `artist_store`'s
//! download. A URL on a private network or behind a credential the client
//! does not hold satisfies none of them, which is what this module exists to
//! fix: the daemon takes the bytes, stores them in the image cache, and hands
//! out a URL it serves itself.
//!
//! The `resolve` / `resolve_with` pair mirrors `lookup` / `lookup_with` next
//! door -- a plain entry point, and a seam that lets every branch of the
//! matrix and every failure path be tested without a network or the global
//! image cache.

use std::io::Cursor;

use base64::Engine as _;
use log::{debug, warn};

use crate::constants::API_PREFIX;
use crate::helpers::http_client::new_http_client;
use crate::helpers::image_meta::detect_image_dimensions;
use crate::helpers::imagecache;

use super::config::EndpointConfig;
use super::protocol::{Lookup, Parsed, ParsedImage};

/// Where an image's bytes come from and where they go.
///
/// A trait rather than direct calls so the decision logic can be tested
/// without a network or the global image cache singleton, which `cargo test`
/// shares across parallel threads.
pub trait ImageSource {
    fn fetch(&self, url: &str, headers: &[(&str, &str)], max_bytes: u64) -> Result<Vec<u8>, String>;
    fn store(&self, path: &str, bytes: Vec<u8>, mime: &str, ttl_days: u64) -> Result<(), String>;
    fn exists(&self, path: &str) -> bool;
}

/// The real thing: an HTTP fetch and the image cache.
pub struct RealImageSource;

impl ImageSource for RealImageSource {
    fn fetch(&self, url: &str, headers: &[(&str, &str)], max_bytes: u64) -> Result<Vec<u8>, String> {
        // A short timeout: the slow part of one of these lookups is the
        // endpoint's own thinking, already paid for by the time we have a
        // URL. Fetching the image it named should be quick, and this fetch
        // is holding a concurrency slot while it runs.
        let client = new_http_client(IMAGE_FETCH_TIMEOUT_SECONDS);
        client
            .get_binary_with_headers(url, headers, max_bytes)
            .map(|(bytes, _mime)| bytes)
            .map_err(|e| e.to_string())
    }

    fn store(&self, path: &str, bytes: Vec<u8>, _mime: &str, ttl_days: u64) -> Result<(), String> {
        // `days * 86400` with overflow checks off in the release profile
        // would wrap on an absurd configured value; `saturating_mul` and
        // `checked_add` bound it instead, and a `None` expiry means "does not
        // expire" rather than "already expired".
        let expiry = std::time::SystemTime::now()
            .checked_add(std::time::Duration::from_secs(ttl_days.saturating_mul(24 * 3600)));

        // `store_image_with_expiry` writes the path verbatim. Its sibling
        // `store_image_from_data_with_expiry` instead appends an extension it
        // re-derives from the mime type, which would write `<hash>.png.png`
        // under a URL saying `<hash>.png` -- every localised image a 404.
        // Avoiding it also avoids relying on two extension tables in two
        // modules agreeing: `sniff` is the only thing that decides the
        // extension, and the extension is what `api::imagecache` serves the
        // Content-Type from, so the mime type is already carried by the path.
        imagecache::store_image_with_expiry(path, &bytes, expiry)
    }

    fn exists(&self, path: &str) -> bool {
        imagecache::image_exists(path)
    }
}

/// How long to wait for an image the endpoint has already named.
const IMAGE_FETCH_TIMEOUT_SECONDS: u64 = 30;

/// The image cache directory localised images live under.
const CACHE_DIR: &str = "external";

/// Store the bytes and return the URL the daemon serves them at.
///
/// The `Content-Type` a client gets is derived from the file extension by
/// `api::imagecache::detect_content_type`, so the extension is load-bearing
/// and is taken from the bytes rather than from anything the endpoint said.
fn store_locally(
    bytes: Vec<u8>,
    endpoint: &EndpointConfig,
    cache_key: &str,
    index: usize,
    source: &dyn ImageSource,
) -> Option<String> {
    let (extension, mime) = sniff(&bytes)?;

    // Named by the query rather than by the bytes, so a re-lookup of the same
    // track overwrites its own file instead of accumulating a copy per
    // lookup. The index is in the digest because one answer can carry several
    // images.
    let digest = md5::compute(format!("{}::{}", cache_key, index).as_bytes());
    let path = format!(
        "{}/{}/{:x}.{}",
        CACHE_DIR,
        path_segment(&endpoint.name),
        digest,
        extension
    );

    if let Err(e) = source.store(&path, bytes, mime, endpoint.cache_ttl_days) {
        warn!(
            "External cover art '{}': could not store the image locally: {}",
            endpoint.name, e
        );
        return None;
    }

    Some(format!("{}/imagecache/{}", API_PREFIX, path))
}

/// An endpoint name, reduced to something safe to use as one directory name.
///
/// The name comes from `audiocontrol.json`, so it is administrator-controlled
/// rather than attacker-controlled -- but it lands in a path this module
/// writes files to, and `..` or a `/` in it would put those files somewhere
/// other than the image cache. A directory named after the endpoint is worth
/// keeping for anyone inspecting the cache by hand, so the name is reduced
/// rather than hashed.
fn path_segment(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect();
    // `.` and `..` are excluded by the mapping above, but an empty name would
    // collapse the path by one level.
    if cleaned.is_empty() {
        "unnamed".to_string()
    } else {
        cleaned
    }
}

/// The file extension and mime type for these bytes, if they are an image we
/// recognise.
///
/// Sniffing is validation as much as it is naming. A `url` fetch that returns
/// an HTML login page or a JSON error is a likely failure, and storing those
/// bytes would serve clients a broken image from a URL the daemon vouches
/// for.
fn sniff(bytes: &[u8]) -> Option<(&'static str, &'static str)> {
    let mut cursor = Cursor::new(bytes);
    let (_, _, format) = detect_image_dimensions(&mut cursor).ok()?;
    // Uppercased because the sniffer returns "WebP" in mixed case and the
    // others in capitals. Every arm here is a format
    // `api::imagecache::detect_content_type` can serve; refusing one the
    // daemon could serve would be worse than storing it, and
    // `max_image_bytes` already bounds the size.
    match format.to_ascii_uppercase().as_str() {
        "JPEG" => Some(("jpg", "image/jpeg")),
        "PNG" => Some(("png", "image/png")),
        "WEBP" => Some(("webp", "image/webp")),
        "GIF" => Some(("gif", "image/gif")),
        "BMP" => Some(("bmp", "image/bmp")),
        other => {
            debug!("External cover art: unrecognised image format '{}'; refusing it", other);
            None
        }
    }
}

/// Decode an inline image, accepting a missing pad.
fn decode(data: &str) -> Option<Vec<u8>> {
    base64::engine::general_purpose::STANDARD
        .decode(data)
        .or_else(|_| base64::engine::general_purpose::STANDARD_NO_PAD.decode(data))
        .ok()
}

/// What to do with one entry of the endpoint's answer.
fn resolve_image(
    image: ParsedImage,
    endpoint: &EndpointConfig,
    cache_key: &str,
    index: usize,
    source: &dyn ImageSource,
) -> Option<String> {
    // Inline bytes always win: they are already in hand, so using them
    // avoids a second authenticated round trip, and they have no URL to pass
    // through in any case.
    if let Some(data) = image.data {
        let Some(bytes) = decode(&data) else {
            warn!(
                "External cover art '{}': inline image is not valid base64; skipping it",
                endpoint.name
            );
            return None;
        };
        if bytes.len() as u64 > endpoint.max_image_bytes {
            warn!(
                "External cover art '{}': inline image of {} bytes is over the {} byte limit; skipping it",
                endpoint.name,
                bytes.len(),
                endpoint.max_image_bytes
            );
            return None;
        }
        return store_locally(bytes, endpoint, cache_key, index, source);
    }

    let url = image.url?;

    if !endpoint.localize {
        return Some(url);
    }

    let headers: Vec<(&str, &str)> = endpoint
        .headers
        .iter()
        .map(|(name, value)| (name.as_str(), value.as_str()))
        .collect();

    match source.fetch(&url, &headers, endpoint.max_image_bytes) {
        Ok(bytes) => store_locally(bytes, endpoint, cache_key, index, source),
        Err(e) => {
            warn!(
                "External cover art '{}': could not fetch {}: {}",
                endpoint.name, url, e
            );
            None
        }
    }
}

/// Turn a parsed answer into the URLs to cache and serve.
pub fn resolve(parsed: Parsed, endpoint: &EndpointConfig, cache_key: &str) -> Lookup {
    resolve_with(parsed, endpoint, cache_key, &RealImageSource)
}

/// [`resolve`], against a given source.
pub fn resolve_with(
    parsed: Parsed,
    endpoint: &EndpointConfig,
    cache_key: &str,
    source: &dyn ImageSource,
) -> Lookup {
    let images = match parsed {
        Parsed::Images(images) => images,
        Parsed::NoArtwork => return Lookup::NoArtwork,
        Parsed::Error => return Lookup::Error,
    };

    let offered = images.len();
    let urls: Vec<String> = images
        .into_iter()
        .enumerate()
        .filter_map(|(index, image)| resolve_image(image, endpoint, cache_key, index, source))
        .collect();

    if urls.is_empty() {
        // The endpoint reported `offered` images and we could serve none of
        // them. That is a fault on our side of the exchange, not a statement
        // that the track has no artwork, so it is cached for an hour rather
        // than for the length of the negative cache.
        warn!(
            "External cover art '{}': none of the {} offered images could be served",
            endpoint.name, offered
        );
        Lookup::Error
    } else {
        Lookup::Found(urls)
    }
}

/// The image cache path a locally served URL names, if it names one.
fn local_path(url: &str) -> Option<&str> {
    url.strip_prefix(API_PREFIX)?.strip_prefix("/imagecache/")
}

/// Drop cached URLs whose files have gone.
///
/// The answer cache and the image cache expire independently in practice -- a
/// cleared cache directory, a manual delete -- and a cached URL naming a file
/// that no longer exists would serve clients a 404 for as long as the answer
/// is kept. A `stat` is nothing next to a lookup that takes half a minute.
///
/// An external URL is kept unexamined: it is not ours to check, and a mixed
/// answer is possible when an endpoint sends one image inline and another as
/// a public URL.
pub fn prune_missing(lookup: Lookup) -> Lookup {
    prune_missing_with(lookup, &RealImageSource)
}

/// [`prune_missing`], against a given source.
pub fn prune_missing_with(lookup: Lookup, source: &dyn ImageSource) -> Lookup {
    let Lookup::Found(urls) = lookup else {
        return lookup;
    };

    let surviving: Vec<String> = urls
        .into_iter()
        .filter(|url| match local_path(url) {
            Some(path) => {
                let present = source.exists(path);
                if !present {
                    debug!("External cover art: cached image {} has gone; dropping it", path);
                }
                present
            }
            None => true,
        })
        .collect();

    if surviving.is_empty() {
        Lookup::Error
    } else {
        Lookup::Found(surviving)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::helpers::coverart::CoverartMethod;
    use crate::helpers::external_coverart::config::Trigger;
    use parking_lot::Mutex;
    use std::collections::HashMap;

    fn endpoint(localize: bool) -> EndpointConfig {
        EndpointConfig {
            name: "llm".to_string(),
            display_name: "AI Lookup".to_string(),
            url: "https://x.example/c".to_string(),
            methods: vec![CoverartMethod::Song],
            headers: HashMap::from([("Authorization".to_string(), "Bearer sekrit".to_string())]),
            timeout_seconds: 45,
            trigger: Trigger::Fallback,
            cache_ttl_days: 30,
            negative_cache_ttl_days: 7,
            max_concurrent: 1,
            localize,
            max_image_bytes: 8 * 1024 * 1024,
        }
    }

    /// A 1x1 PNG, so a test needs no fixture file.
    fn tiny_png() -> Vec<u8> {
        vec![
            0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48,
            0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00,
            0x00, 0x1F, 0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41, 0x54, 0x78,
            0x9C, 0x63, 0x00, 0x01, 0x00, 0x00, 0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00,
            0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
        ]
    }

    #[derive(Default)]
    struct FakeSource {
        fetched: Mutex<Vec<(String, Vec<(String, String)>, u64)>>,
        stored: Mutex<Vec<(String, usize, String, u64)>>,
        fetch_result: Mutex<Option<Result<Vec<u8>, String>>>,
        store_fails: Mutex<bool>,
        missing: Mutex<Vec<String>>,
    }

    impl FakeSource {
        fn returning(bytes: Vec<u8>) -> Self {
            let source = Self::default();
            *source.fetch_result.lock() = Some(Ok(bytes));
            source
        }

        fn failing(message: &str) -> Self {
            let source = Self::default();
            *source.fetch_result.lock() = Some(Err(message.to_string()));
            source
        }

        fn stored_paths(&self) -> Vec<String> {
            self.stored.lock().iter().map(|entry| entry.0.clone()).collect()
        }
    }

    impl ImageSource for FakeSource {
        fn fetch(&self, url: &str, headers: &[(&str, &str)], max_bytes: u64) -> Result<Vec<u8>, String> {
            self.fetched.lock().push((
                url.to_string(),
                headers.iter().map(|(n, v)| (n.to_string(), v.to_string())).collect(),
                max_bytes,
            ));
            self.fetch_result
                .lock()
                .clone()
                .unwrap_or_else(|| Err("no fetch configured".to_string()))
        }

        fn store(&self, path: &str, bytes: Vec<u8>, mime: &str, ttl_days: u64) -> Result<(), String> {
            if *self.store_fails.lock() {
                return Err("disk full".to_string());
            }
            self.stored.lock().push((path.to_string(), bytes.len(), mime.to_string(), ttl_days));
            Ok(())
        }

        fn exists(&self, path: &str) -> bool {
            !self.missing.lock().iter().any(|missing| missing == path)
        }
    }

    fn key() -> String {
        "coverart::external::llm::song|artist|title".to_string()
    }

    // --- the matrix ---------------------------------------------------

    /// Bytes have no URL to pass through, so an inline image is stored
    /// regardless of the flag. `localize` governs URL images only.
    #[test]
    fn an_inline_image_is_stored_even_when_localize_is_off() {
        let source = FakeSource::default();
        let parsed = Parsed::Images(vec![ParsedImage {
            url: None,
            data: Some(base64_of(&tiny_png())),
        }]);

        let lookup = resolve_with(parsed, &endpoint(false), &key(), &source);

        let Lookup::Found(urls) = lookup else { panic!("an inline image is artwork") };
        assert_eq!(urls.len(), 1);
        assert!(
            urls[0].starts_with("/api/imagecache/external/llm/"),
            "expected a locally served URL, got {}",
            urls[0]
        );
        assert!(urls[0].ends_with(".png"), "the extension comes from the bytes: {}", urls[0]);
        assert_eq!(source.stored_paths().len(), 1);
        assert!(source.fetched.lock().is_empty(), "inline bytes need no fetch");
    }

    #[test]
    fn a_url_image_passes_through_when_localize_is_off() {
        let source = FakeSource::default();
        let parsed = Parsed::Images(vec![ParsedImage {
            url: Some("https://img.example/a.jpg".to_string()),
            data: None,
        }]);

        assert_eq!(
            resolve_with(parsed, &endpoint(false), &key(), &source),
            Lookup::Found(vec!["https://img.example/a.jpg".to_string()])
        );
        assert!(source.fetched.lock().is_empty());
        assert!(source.stored_paths().is_empty());
    }

    /// The point of the whole increment: the credential that authorised the
    /// lookup has to authorise the image fetch too.
    #[test]
    fn a_url_image_is_fetched_with_the_endpoint_headers_when_localize_is_on() {
        let source = FakeSource::returning(tiny_png());
        let parsed = Parsed::Images(vec![ParsedImage {
            url: Some("https://img.example/a.jpg".to_string()),
            data: None,
        }]);

        let lookup = resolve_with(parsed, &endpoint(true), &key(), &source);

        let Lookup::Found(urls) = lookup else { panic!("artwork") };
        assert!(urls[0].starts_with("/api/imagecache/external/llm/"));

        let fetched = source.fetched.lock();
        assert_eq!(fetched.len(), 1);
        assert_eq!(fetched[0].0, "https://img.example/a.jpg");
        assert!(
            fetched[0].1.iter().any(|(n, v)| n == "Authorization" && v == "Bearer sekrit"),
            "the endpoint's credential must be on the image fetch"
        );
        assert_eq!(fetched[0].2, 8 * 1024 * 1024, "the fetch is bounded by max_image_bytes");
    }

    /// Already in hand beats a second authenticated round trip.
    #[test]
    fn inline_data_wins_when_both_are_present() {
        let source = FakeSource::returning(tiny_png());
        let parsed = Parsed::Images(vec![ParsedImage {
            url: Some("https://img.example/a.jpg".to_string()),
            data: Some(base64_of(&tiny_png())),
        }]);

        resolve_with(parsed, &endpoint(true), &key(), &source);

        assert!(source.fetched.lock().is_empty(), "data was available; no fetch should happen");
        assert_eq!(source.stored_paths().len(), 1);
    }

    // --- paths and lifetime -------------------------------------------

    /// A re-lookup of the same query must overwrite its own file rather than
    /// accumulate a copy per lookup.
    #[test]
    fn the_same_query_stores_to_the_same_path() {
        let first = FakeSource::default();
        let second = FakeSource::default();
        let image = || {
            Parsed::Images(vec![ParsedImage { url: None, data: Some(base64_of(&tiny_png())) }])
        };

        resolve_with(image(), &endpoint(false), &key(), &first);
        resolve_with(image(), &endpoint(false), &key(), &second);

        assert_eq!(first.stored_paths(), second.stored_paths());
    }

    #[test]
    fn different_queries_store_to_different_paths() {
        let source = FakeSource::default();
        let image = || {
            Parsed::Images(vec![ParsedImage { url: None, data: Some(base64_of(&tiny_png())) }])
        };

        resolve_with(image(), &endpoint(false), "key-a", &source);
        resolve_with(image(), &endpoint(false), "key-b", &source);

        let paths = source.stored_paths();
        assert_ne!(paths[0], paths[1]);
    }

    #[test]
    fn two_images_in_one_answer_store_to_different_paths() {
        let source = FakeSource::default();
        let parsed = Parsed::Images(vec![
            ParsedImage { url: None, data: Some(base64_of(&tiny_png())) },
            ParsedImage { url: None, data: Some(base64_of(&tiny_png())) },
        ]);

        let lookup = resolve_with(parsed, &endpoint(false), &key(), &source);

        let Lookup::Found(urls) = lookup else { panic!("artwork") };
        assert_eq!(urls.len(), 2);
        assert_ne!(urls[0], urls[1], "the index has to reach the path");
    }

    /// The file must not outlive the cache entry that names it, or a client
    /// gets a 404 for an answer the daemon still believes in.
    #[test]
    fn the_stored_file_expires_with_the_answer() {
        let source = FakeSource::default();
        let parsed = Parsed::Images(vec![ParsedImage {
            url: None,
            data: Some(base64_of(&tiny_png())),
        }]);

        resolve_with(parsed, &endpoint(false), &key(), &source);

        assert_eq!(source.stored.lock()[0].3, 30, "the endpoint's cache_ttl_days");
    }

    #[test]
    fn the_stored_mime_type_comes_from_the_bytes() {
        let source = FakeSource::default();
        let parsed = Parsed::Images(vec![ParsedImage {
            url: None,
            data: Some(base64_of(&tiny_png())),
        }]);

        resolve_with(parsed, &endpoint(false), &key(), &source);

        assert_eq!(source.stored.lock()[0].2, "image/png");
    }

    // --- failures -----------------------------------------------------

    /// A fetch that returns a login page or a JSON error is a real failure
    /// mode. Without sniffing, those bytes get stored and served to clients
    /// as a broken image.
    #[test]
    fn bytes_that_are_not_an_image_are_refused() {
        let source = FakeSource::returning(b"<html>Please log in</html>".to_vec());
        let parsed = Parsed::Images(vec![ParsedImage {
            url: Some("https://img.example/a.jpg".to_string()),
            data: None,
        }]);

        assert_eq!(resolve_with(parsed, &endpoint(true), &key(), &source), Lookup::Error);
        assert!(source.stored_paths().is_empty(), "nothing unrecognised may be written");
    }

    #[test]
    fn undecodable_inline_data_is_refused() {
        let source = FakeSource::default();
        let parsed = Parsed::Images(vec![ParsedImage {
            url: None,
            data: Some("this is not base64 !!!".to_string()),
        }]);

        assert_eq!(resolve_with(parsed, &endpoint(false), &key(), &source), Lookup::Error);
    }

    /// An endpoint that omits padding is accepted rather than reported as a
    /// fault; the encoding is unambiguous either way.
    #[test]
    fn unpadded_inline_data_is_accepted() {
        let source = FakeSource::default();
        let padded = base64_of(&tiny_png());
        let unpadded = padded.trim_end_matches('=').to_string();
        let parsed = Parsed::Images(vec![ParsedImage { url: None, data: Some(unpadded) }]);

        assert!(matches!(
            resolve_with(parsed, &endpoint(false), &key(), &source),
            Lookup::Found(_)
        ));
    }

    #[test]
    fn an_inline_image_over_the_cap_is_refused() {
        let source = FakeSource::default();
        let mut config = endpoint(false);
        config.max_image_bytes = 10;
        let parsed = Parsed::Images(vec![ParsedImage {
            url: None,
            data: Some(base64_of(&tiny_png())),
        }]);

        assert_eq!(resolve_with(parsed, &config, &key(), &source), Lookup::Error);
        assert!(source.stored_paths().is_empty());
    }

    #[test]
    fn a_failed_fetch_is_refused() {
        let source = FakeSource::failing("connection refused");
        let parsed = Parsed::Images(vec![ParsedImage {
            url: Some("https://img.example/a.jpg".to_string()),
            data: None,
        }]);

        assert_eq!(resolve_with(parsed, &endpoint(true), &key(), &source), Lookup::Error);
    }

    #[test]
    fn a_failed_store_is_refused() {
        let source = FakeSource::default();
        *source.store_fails.lock() = true;
        let parsed = Parsed::Images(vec![ParsedImage {
            url: None,
            data: Some(base64_of(&tiny_png())),
        }]);

        assert_eq!(resolve_with(parsed, &endpoint(false), &key(), &source), Lookup::Error);
    }

    /// One bad image among several is dropped, not fatal: the others are
    /// still artwork worth showing.
    #[test]
    fn a_partial_failure_keeps_the_images_that_worked() {
        let source = FakeSource::default();
        let parsed = Parsed::Images(vec![
            ParsedImage { url: None, data: Some("not base64 !!!".to_string()) },
            ParsedImage { url: None, data: Some(base64_of(&tiny_png())) },
        ]);

        let Lookup::Found(urls) = resolve_with(parsed, &endpoint(false), &key(), &source) else {
            panic!("the surviving image is still artwork")
        };
        assert_eq!(urls.len(), 1);
    }

    /// The distinction that keeps a service outage from blanking a track for
    /// a week: the endpoint *did* report artwork, so failing to get it is a
    /// fault on our side, retried within the hour -- not a statement that
    /// this track has no cover.
    #[test]
    fn losing_every_image_is_an_error_not_an_absence() {
        let source = FakeSource::failing("connection refused");
        let parsed = Parsed::Images(vec![ParsedImage {
            url: Some("https://img.example/a.jpg".to_string()),
            data: None,
        }]);

        let lookup = resolve_with(parsed, &endpoint(true), &key(), &source);
        assert_eq!(lookup, Lookup::Error);
        assert_ne!(lookup, Lookup::NoArtwork);
    }

    #[test]
    fn the_other_two_classes_pass_through_unchanged() {
        let source = FakeSource::default();
        assert_eq!(
            resolve_with(Parsed::NoArtwork, &endpoint(true), &key(), &source),
            Lookup::NoArtwork
        );
        assert_eq!(
            resolve_with(Parsed::Error, &endpoint(true), &key(), &source),
            Lookup::Error
        );
    }

    // --- pruning ------------------------------------------------------

    /// The two caches expire independently in practice -- a cleared cache
    /// directory, a manual delete -- so a cached local URL whose file is
    /// gone has to read back as a miss rather than serve a 404 until the
    /// TTL runs out.
    #[test]
    fn a_cached_url_whose_file_is_gone_is_pruned() {
        let source = FakeSource::default();
        source.missing.lock().push("external/llm/deadbeef.png".to_string());

        let lookup = Lookup::Found(vec!["/api/imagecache/external/llm/deadbeef.png".to_string()]);

        assert_eq!(prune_missing_with(lookup, &source), Lookup::Error);
    }

    #[test]
    fn a_cached_url_whose_file_exists_survives() {
        let source = FakeSource::default();
        let lookup = Lookup::Found(vec!["/api/imagecache/external/llm/alive.png".to_string()]);

        assert_eq!(prune_missing_with(lookup.clone(), &source), lookup);
    }

    /// A mixed answer is possible: one image inline, another a public URL.
    /// An external URL is not ours to check, so it is kept unexamined.
    #[test]
    fn an_external_url_is_kept_without_being_checked() {
        let source = FakeSource::default();
        source.missing.lock().push("external/llm/gone.png".to_string());

        let lookup = Lookup::Found(vec![
            "/api/imagecache/external/llm/gone.png".to_string(),
            "https://img.example/a.jpg".to_string(),
        ]);

        assert_eq!(
            prune_missing_with(lookup, &source),
            Lookup::Found(vec!["https://img.example/a.jpg".to_string()])
        );
    }

    /// The name is administrator-controlled, not attacker-controlled, but it
    /// reaches a path this module writes to. A traversal in it would put
    /// files outside the image cache entirely.
    #[test]
    fn an_endpoint_name_cannot_escape_the_cache_directory() {
        let source = FakeSource::default();
        let mut config = endpoint(false);
        config.name = "../../etc/cron.d".to_string();
        let parsed = Parsed::Images(vec![ParsedImage {
            url: None,
            data: Some(base64_of(&tiny_png())),
        }]);

        let Lookup::Found(urls) = resolve_with(parsed, &config, &key(), &source) else {
            panic!("artwork")
        };

        let stored = source.stored_paths();
        assert_eq!(stored.len(), 1);
        assert!(
            !stored[0].contains(".."),
            "the stored path must not contain a traversal: {}",
            stored[0]
        );
        assert!(
            stored[0].starts_with("external/"),
            "the stored path must stay under the cache directory: {}",
            stored[0]
        );
        assert!(
            !urls[0].contains(".."),
            "the served URL must not contain a traversal: {}",
            urls[0]
        );
    }

    #[test]
    fn an_endpoint_name_of_only_punctuation_still_yields_a_directory() {
        let source = FakeSource::default();
        let mut config = endpoint(false);
        config.name = "///".to_string();
        let parsed = Parsed::Images(vec![ParsedImage {
            url: None,
            data: Some(base64_of(&tiny_png())),
        }]);

        resolve_with(parsed, &config, &key(), &source);

        let stored = source.stored_paths();
        assert_eq!(stored.len(), 1);
        // Three separators would otherwise collapse into `external//<hash>`.
        assert!(stored[0].starts_with("external/___/"), "got {}", stored[0]);
    }

    #[test]
    fn pruning_leaves_the_other_classes_alone() {
        let source = FakeSource::default();
        assert_eq!(prune_missing_with(Lookup::NoArtwork, &source), Lookup::NoArtwork);
        assert_eq!(prune_missing_with(Lookup::Error, &source), Lookup::Error);
    }

    fn base64_of(bytes: &[u8]) -> String {
        use base64::Engine as _;
        base64::engine::general_purpose::STANDARD.encode(bytes)
    }
}
