//! The externally visible API prefix, and rewriting internal paths into it.
//!
//! `acr` builds paths against its own mount point (`/api/...`). A reverse
//! proxy may expose it somewhere else and say so in `X-Forwarded-Prefix`.
//! Everything a client is expected to fetch unmodified passes through here on
//! the way out.

use rocket::request::{FromRequest, Outcome, Request};

pub use acr_types::urlprefix::{
    rewrite_api_relative_url, rewrite_artist_thumb_urls, rewrite_song_urls, rewrite_thumb_urls,
};
use acr_types::urlprefix::normalize_forwarded_prefix;

/// The externally visible API base, as reported by a reverse proxy.
///
/// Absent when the client reached audiocontrol directly, in which case no
/// rewriting happens and paths go out in their internal form.
#[derive(Debug, Clone)]
pub struct ForwardedPrefix(pub Option<String>);

impl ForwardedPrefix {
    /// The prefix as the rewriting functions want it.
    ///
    /// Handlers call this rather than reaching into the field, so the type's
    /// shape can change without touching every call site.
    pub fn as_deref(&self) -> Option<&str> {
        self.0.as_deref()
    }

    /// The prefix as an owned value, for holding across a connection rather
    /// than for the length of one request.
    pub fn into_inner(self) -> Option<String> {
        self.0
    }
}

#[rocket::async_trait]
impl<'r> FromRequest<'r> for ForwardedPrefix {
    type Error = ();

    async fn from_request(request: &'r Request<'_>) -> Outcome<Self, Self::Error> {
        let prefix = request
            .headers()
            .get_one("X-Forwarded-Prefix")
            .map(ToOwned::to_owned);
        Outcome::Success(ForwardedPrefix(prefix))
    }
}

/// How many hex digits of the prefix digest a version token carries.
const PREFIX_TAG_LEN: usize = 8;

/// A short, stable tag naming the externally visible prefix a client is on.
///
/// Hashed rather than interpolated: on direct access the header is whatever
/// the caller sent, and a `"` in it would corrupt the `W/"..."` ETag the token
/// ends up inside. The absent case hashes the empty string - a value no
/// normalized prefix can produce - so a client on the direct route gets a tag
/// that is stable across requests and distinct from every proxied one.
fn prefix_tag(forwarded_prefix: Option<&str>) -> String {
    let normalized = normalize_forwarded_prefix(forwarded_prefix).unwrap_or_default();
    let digest = format!("{:x}", md5::compute(normalized.as_bytes()));
    digest[..PREFIX_TAG_LEN].to_string()
}

/// Fold the client's prefix into a library version token.
///
/// The paths in a list body depend on the request's prefix, so two different
/// bodies share one URL. A validator built from library state alone would name
/// both, and the origin could answer 304 for a representation the client does
/// not hold - leaving it with paths that are wrong for its route, every one of
/// which answers 200 with the web interface's index.html.
///
/// The same token is reported as `library_version`, which clients poll instead
/// of revalidating each list: if that one omitted the prefix, a client whose
/// route changed would poll, see no change, and never re-fetch.
///
/// The result stays opaque and still changes whenever the contents change.
/// `None` in means `None` out: a backend that does not track changes must emit
/// no token at all.
pub fn prefixed_library_version(
    library_version: Option<String>,
    forwarded_prefix: Option<&str>,
) -> Option<String> {
    library_version.map(|version| format!("{}-{}", prefix_tag(forwarded_prefix), version))
}

#[cfg(test)]
mod tests {
    use super::*;

    const PREFIX: Option<&str> = Some("/api/audiocontrol");

    #[test]
    fn a_refused_prefix_tags_the_version_like_no_prefix() {
        // A refused prefix serves un-rewritten paths, so it must validate as
        // the direct route does rather than as a route of its own.
        assert_eq!(
            prefixed_library_version(Some("a3f9c1d2-42".to_string()), Some("//evil.com")),
            prefixed_library_version(Some("a3f9c1d2-42".to_string()), None)
        );
    }

    #[test]
    fn a_version_token_differs_between_prefixes() {
        let direct = prefixed_library_version(Some("42".to_string()), None).unwrap();
        let proxied = prefixed_library_version(Some("42".to_string()), PREFIX).unwrap();
        let elsewhere =
            prefixed_library_version(Some("42".to_string()), Some("/music")).unwrap();
        assert_ne!(direct, proxied);
        assert_ne!(direct, elsewhere);
        assert_ne!(proxied, elsewhere);
    }

    #[test]
    fn a_version_token_is_stable_for_one_prefix() {
        assert_eq!(
            prefixed_library_version(Some("42".to_string()), None),
            prefixed_library_version(Some("42".to_string()), None)
        );
        assert_eq!(
            prefixed_library_version(Some("42".to_string()), PREFIX),
            prefixed_library_version(Some("42".to_string()), PREFIX)
        );
    }

    #[test]
    fn a_version_token_still_changes_with_the_contents() {
        assert_ne!(
            prefixed_library_version(Some("42".to_string()), PREFIX),
            prefixed_library_version(Some("43".to_string()), PREFIX)
        );
    }

    #[test]
    fn equivalent_prefixes_yield_one_version_token() {
        // Normalization happens before hashing, so the trailing slash and the
        // missing leading one do not split one client's cache in two.
        let canonical = prefixed_library_version(Some("42".to_string()), Some("/api/audiocontrol"));
        assert_eq!(
            prefixed_library_version(Some("42".to_string()), Some("/api/audiocontrol/")),
            canonical
        );
        assert_eq!(
            prefixed_library_version(Some("42".to_string()), Some("api/audiocontrol")),
            canonical
        );
        // An empty prefix counts as absent, and must agree with absent.
        assert_eq!(
            prefixed_library_version(Some("42".to_string()), Some("  ")),
            prefixed_library_version(Some("42".to_string()), None)
        );
    }

    #[test]
    fn a_hostile_prefix_cannot_break_out_of_the_etag() {
        // Direct on port 1080 the header is attacker-supplied. The digest is
        // hex, so nothing the caller writes reaches the W/"..." header.
        let token = prefixed_library_version(
            Some("42".to_string()),
            Some("/x\", W/\"albums-anything"),
        )
        .unwrap();
        assert!(!token.contains('"'));
        assert!(token
            .split('-')
            .next()
            .unwrap()
            .chars()
            .all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn a_backend_without_a_version_gets_no_token() {
        assert_eq!(prefixed_library_version(None, PREFIX), None);
        assert_eq!(prefixed_library_version(None, None), None);
    }
}
