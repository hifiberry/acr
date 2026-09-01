//! The externally visible API prefix, and rewriting internal paths into it.
//!
//! `acr` builds paths against its own mount point (`/api/...`). A reverse
//! proxy may expose it somewhere else and say so in `X-Forwarded-Prefix`.
//! Everything a client is expected to fetch unmodified passes through here on
//! the way out.

use rocket::request::{FromRequest, Outcome, Request};

use crate::constants::API_PREFIX;
use crate::data::artist::Artist;
use crate::data::song::Song;

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

/// Rewrite an internal API-relative URL to the externally visible API base.
///
/// Returns the URL unchanged when there is no forwarded prefix, when the URL
/// is not under the API prefix (an external provider URL, say), or when it
/// already carries the prefix.
pub fn rewrite_api_relative_url(url: &str, forwarded_prefix: Option<&str>) -> String {
    let Some(prefix) = normalize_forwarded_prefix(forwarded_prefix) else {
        return url.to_string();
    };

    // Already rewritten. Doing it twice yields
    // /api/audiocontrol/audiocontrol/... - not a route the gateway knows, so
    // it answers 401 rather than failing visibly. Several fields now reach
    // this function that may already carry the prefix, so this is a live
    // case rather than a theoretical one.
    if url == prefix
        || url
            .strip_prefix(prefix.as_str())
            .is_some_and(|rest| rest.starts_with('/'))
    {
        return url.to_string();
    }

    if url == API_PREFIX {
        return prefix;
    }

    // Only a whole path segment counts as being under the prefix: "/apifoo"
    // is a different path, not "/api" plus "foo".
    if let Some(suffix) = url.strip_prefix(API_PREFIX) {
        if suffix.starts_with('/') {
            return format!("{}{}", prefix, suffix);
        }
    }

    url.to_string()
}

fn normalize_forwarded_prefix(prefix: Option<&str>) -> Option<String> {
    let raw = prefix?.trim();
    if raw.is_empty() {
        return None;
    }

    let without_trailing = raw.trim_end_matches('/');
    if without_trailing.is_empty() {
        return None;
    }

    let candidate = if without_trailing.starts_with('/') {
        without_trailing.to_string()
    } else {
        format!("/{}", without_trailing)
    };

    // A URL parser removes ASCII tab, CR and LF from its input before
    // parsing, and `trim` only takes them off the ends. So "/\t/evil.com"
    // survives to here, passes the protocol-relative check below, and reaches
    // a browser as "//evil.com" - the very case that check exists to refuse.
    // A query or fragment marker breaks the path a different way: everything
    // after it stops being path, so a prefix of "/api/audiocontrol?x" would
    // swallow the rest of every rewritten path into a query string.
    if candidate
        .chars()
        .any(|c| c.is_ascii_control() || c == '?' || c == '#')
    {
        return None;
    }

    // A value beginning with two slashes is protocol-relative: a browser
    // reading "//example.com/library/mpd/image/album:7" fetches it from
    // example.com, not from this device. A backslash in that position is
    // treated the same way by several browsers, and a scheme has no business
    // in a path prefix at all. Reached directly, this header is whatever the
    // caller sent, so refuse the shape rather than emit paths built from it -
    // returning None here means no rewriting happens and internal paths go
    // out unchanged, which is the same safe behaviour as no header at all.
    if candidate.starts_with("//")
        || candidate.starts_with("/\\")
        || candidate.contains("://")
    {
        return None;
    }

    Some(candidate)
}

/// Rewrite every internal path a song carries.
pub fn rewrite_song_urls(song: &mut Song, forwarded_prefix: Option<&str>) {
    if let Some(cover_art_url) = song.cover_art_url.as_mut() {
        *cover_art_url = rewrite_api_relative_url(cover_art_url, forwarded_prefix);
    }

    // `lyrics_url` lives in the free-form metadata map. Compute the new value
    // first: holding the borrow from `get` across the `insert` does not
    // compile.
    let lyrics_url = match song.metadata.get("lyrics_url") {
        Some(serde_json::Value::String(url)) => {
            Some(rewrite_api_relative_url(url, forwarded_prefix))
        }
        _ => None,
    };
    if let Some(url) = lyrics_url {
        song.metadata
            .insert("lyrics_url".to_string(), serde_json::Value::String(url));
    }
}

/// Rewrite the internal entries of a thumbnail list in place.
///
/// The list mixes internal paths with external provider URLs (last.fm,
/// theaudiodb). Anything not under the API prefix comes back unchanged, so
/// external entries need no special case here.
pub fn rewrite_thumb_urls(thumb_urls: &mut [String], forwarded_prefix: Option<&str>) {
    for url in thumb_urls.iter_mut() {
        *url = rewrite_api_relative_url(url, forwarded_prefix);
    }
}

/// Rewrite an artist's image URLs in place.
///
/// `banner_url` goes through the same pass as `thumb_url`. Every writer of it
/// supplies an absolute provider URL today, which the rewrite leaves alone, so
/// this changes nothing now - but it is emitted in artist responses, and this
/// is the one place a future internal banner path would otherwise be missed.
pub fn rewrite_artist_thumb_urls(artist: &mut Artist, forwarded_prefix: Option<&str>) {
    if let Some(meta) = artist.metadata.as_mut() {
        rewrite_thumb_urls(&mut meta.thumb_url, forwarded_prefix);
        rewrite_thumb_urls(&mut meta.banner_url, forwarded_prefix);
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
    use crate::data::metadata::ArtistMeta;
    use crate::data::Identifier;

    const PREFIX: Option<&str> = Some("/api/audiocontrol");

    #[test]
    fn without_a_prefix_the_url_is_untouched() {
        assert_eq!(
            rewrite_api_relative_url("/api/library/mpd/image/album:7", None),
            "/api/library/mpd/image/album:7"
        );
    }

    #[test]
    fn an_empty_prefix_counts_as_absent() {
        assert_eq!(rewrite_api_relative_url("/api/x", Some("")), "/api/x");
        assert_eq!(rewrite_api_relative_url("/api/x", Some("   ")), "/api/x");
        assert_eq!(rewrite_api_relative_url("/api/x", Some("/")), "/api/x");
    }

    #[test]
    fn an_internal_path_gains_the_prefix() {
        assert_eq!(
            rewrite_api_relative_url("/api/library/mpd/image/album:7", PREFIX),
            "/api/audiocontrol/library/mpd/image/album:7"
        );
    }

    #[test]
    fn a_prefix_without_a_leading_slash_gets_one() {
        assert_eq!(
            rewrite_api_relative_url("/api/x", Some("api/audiocontrol")),
            "/api/audiocontrol/x"
        );
    }

    #[test]
    fn a_trailing_slash_on_the_prefix_is_dropped() {
        assert_eq!(
            rewrite_api_relative_url("/api/x", Some("/api/audiocontrol/")),
            "/api/audiocontrol/x"
        );
    }

    #[test]
    fn the_bare_api_root_becomes_the_bare_prefix() {
        assert_eq!(rewrite_api_relative_url("/api", PREFIX), "/api/audiocontrol");
    }

    #[test]
    fn a_protocol_relative_prefix_is_refused() {
        // "//evil.com/library/..." is not a path - a browser fetches it from
        // evil.com. Refusing the prefix leaves the internal path alone, which
        // is the same outcome as no header at all.
        assert_eq!(
            rewrite_api_relative_url("/api/library/mpd/image/album:7", Some("//evil.com")),
            "/api/library/mpd/image/album:7"
        );
        // A trailing slash must not turn one leading slash into two either.
        assert_eq!(
            rewrite_api_relative_url("/api/x", Some("//evil.com/")),
            "/api/x"
        );
    }

    #[test]
    fn a_prefix_smuggling_a_url_stripped_character_is_refused() {
        // A browser removes ASCII tab, CR and LF before parsing, so
        // "/<tab>/evil.com" would arrive as "//evil.com" - the
        // protocol-relative case, smuggled past the check for it.
        assert_eq!(rewrite_api_relative_url("/api/x", Some("/\t/evil.com")), "/api/x");
        assert_eq!(rewrite_api_relative_url("/api/x", Some("/\r\n/evil.com")), "/api/x");
        // The same trick against the backslash form.
        assert_eq!(rewrite_api_relative_url("/api/x", Some("/\t\\evil.com")), "/api/x");
    }

    #[test]
    fn a_prefix_carrying_a_query_or_fragment_is_refused() {
        // Everything after ? or # stops being part of the path, so the rest
        // of every rewritten path would be swallowed rather than fetched.
        assert_eq!(
            rewrite_api_relative_url("/api/x", Some("/api/audiocontrol?x")),
            "/api/x"
        );
        assert_eq!(
            rewrite_api_relative_url("/api/x", Some("/api/audiocontrol#x")),
            "/api/x"
        );
    }

    #[test]
    fn a_backslash_prefix_is_refused() {
        // Several browsers treat "/\" the way they treat "//".
        assert_eq!(rewrite_api_relative_url("/api/x", Some("/\\evil.com")), "/api/x");
    }

    #[test]
    fn a_prefix_carrying_a_scheme_is_refused() {
        assert_eq!(rewrite_api_relative_url("/api/x", Some("http://evil.com")), "/api/x");
        assert_eq!(rewrite_api_relative_url("/api/x", Some("/a://b")), "/api/x");
    }

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
    fn an_external_url_is_untouched() {
        let url = "https://lastfm.freetls.fastly.net/i/u/300x300/abc.png";
        assert_eq!(rewrite_api_relative_url(url, PREFIX), url);
    }

    #[test]
    fn a_path_outside_the_api_is_untouched() {
        assert_eq!(rewrite_api_relative_url("/static/logo.png", PREFIX), "/static/logo.png");
    }

    #[test]
    fn rewriting_is_idempotent() {
        // Doing it twice must not yield /api/audiocontrol/audiocontrol/...,
        // which the auth gateway answers 401 to.
        let once = rewrite_api_relative_url("/api/library/mpd/image/album:7", PREFIX);
        let twice = rewrite_api_relative_url(&once, PREFIX);
        assert_eq!(once, twice);
        assert_eq!(twice, "/api/audiocontrol/library/mpd/image/album:7");
    }

    #[test]
    fn the_bare_prefix_is_left_alone() {
        assert_eq!(rewrite_api_relative_url("/api/audiocontrol", PREFIX), "/api/audiocontrol");
    }

    #[test]
    fn a_path_that_merely_starts_with_the_letters_api_is_untouched() {
        // "/apifoo" is not "/api" + "foo"; only a whole segment counts.
        assert_eq!(rewrite_api_relative_url("/apifoo", PREFIX), "/apifoo");
    }

    fn song_with(cover: Option<&str>, lyrics: Option<&str>) -> Song {
        let mut song = Song::default();
        song.cover_art_url = cover.map(ToOwned::to_owned);
        if let Some(lyrics) = lyrics {
            song.metadata.insert(
                "lyrics_url".to_string(),
                serde_json::Value::String(lyrics.to_string()),
            );
        }
        song
    }

    #[test]
    fn a_song_gets_both_of_its_paths_rewritten() {
        let mut song = song_with(
            Some("/api/library/mpd/image/album:7"),
            Some("/api/lyrics/mpd/dHJhY2s"),
        );
        rewrite_song_urls(&mut song, PREFIX);
        assert_eq!(
            song.cover_art_url.as_deref(),
            Some("/api/audiocontrol/library/mpd/image/album:7")
        );
        assert_eq!(
            song.metadata.get("lyrics_url").and_then(|v| v.as_str()),
            Some("/api/audiocontrol/lyrics/mpd/dHJhY2s")
        );
    }

    #[test]
    fn a_song_without_a_prefix_is_untouched() {
        let mut song = song_with(
            Some("/api/library/mpd/image/album:7"),
            Some("/api/lyrics/mpd/dHJhY2s"),
        );
        rewrite_song_urls(&mut song, None);
        assert_eq!(song.cover_art_url.as_deref(), Some("/api/library/mpd/image/album:7"));
        assert_eq!(
            song.metadata.get("lyrics_url").and_then(|v| v.as_str()),
            Some("/api/lyrics/mpd/dHJhY2s")
        );
    }

    #[test]
    fn a_song_missing_those_fields_is_handled() {
        let mut song = song_with(None, None);
        rewrite_song_urls(&mut song, PREFIX);
        assert!(song.cover_art_url.is_none());
        assert!(!song.metadata.contains_key("lyrics_url"));
    }

    #[test]
    fn a_non_string_lyrics_url_is_left_alone() {
        let mut song = Song::default();
        song.metadata
            .insert("lyrics_url".to_string(), serde_json::Value::Bool(true));
        rewrite_song_urls(&mut song, PREFIX);
        assert_eq!(song.metadata.get("lyrics_url"), Some(&serde_json::Value::Bool(true)));
    }

    #[test]
    fn a_thumb_list_rewrites_internal_entries_and_keeps_external_ones() {
        let mut thumbs = vec![
            "/api/coverart/artist/YWJj/image".to_string(),
            "https://example.com/artist.png".to_string(),
        ];
        rewrite_thumb_urls(&mut thumbs, PREFIX);
        assert_eq!(thumbs[0], "/api/audiocontrol/coverart/artist/YWJj/image");
        assert_eq!(thumbs[1], "https://example.com/artist.png");
    }

    fn artist_with_thumbs(thumbs: Vec<String>) -> Artist {
        let mut meta = ArtistMeta::new();
        meta.thumb_url = thumbs;
        Artist {
            id: Identifier::Numeric(1),
            name: "Test Artist".to_string(),
            is_multi: false,
            metadata: Some(meta),
        }
    }

    #[test]
    fn an_artists_thumbs_are_rewritten() {
        let mut artist = artist_with_thumbs(vec!["/api/coverart/artist/YWJj/image".to_string()]);
        rewrite_artist_thumb_urls(&mut artist, PREFIX);
        assert_eq!(
            artist.metadata.as_ref().unwrap().thumb_url[0],
            "/api/audiocontrol/coverart/artist/YWJj/image"
        );
    }

    #[test]
    fn an_artist_without_metadata_is_handled() {
        let mut artist = artist_with_thumbs(Vec::new());
        artist.metadata = None;
        rewrite_artist_thumb_urls(&mut artist, PREFIX);
        assert!(artist.metadata.is_none());
    }

    #[test]
    fn an_absolute_banner_url_survives_the_rewrite() {
        // Every writer of banner_url supplies an absolute provider URL today.
        // It passes through the same call as thumb_url, and must come back
        // untouched.
        let mut artist = artist_with_thumbs(Vec::new());
        artist.metadata.as_mut().unwrap().banner_url =
            vec!["https://example.com/artist-banner.png".to_string()];
        rewrite_artist_thumb_urls(&mut artist, PREFIX);
        assert_eq!(
            artist.metadata.as_ref().unwrap().banner_url[0],
            "https://example.com/artist-banner.png"
        );
    }

    #[test]
    fn an_internal_banner_url_gains_the_prefix() {
        let mut artist = artist_with_thumbs(Vec::new());
        artist.metadata.as_mut().unwrap().banner_url =
            vec!["/api/coverart/artist/YWJj/banner".to_string()];
        rewrite_artist_thumb_urls(&mut artist, PREFIX);
        assert_eq!(
            artist.metadata.as_ref().unwrap().banner_url[0],
            "/api/audiocontrol/coverart/artist/YWJj/banner"
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
