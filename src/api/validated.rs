use rocket::http::{Header, Status};
use rocket::request::Request;
use rocket::response::{self, Responder, Response};
use rocket::serde::json::Json;
use serde::Serialize;

/// Build the weak validator for a list endpoint at a given library version.
///
/// Weak deliberately: the version tracks semantic change, not bytes, so two
/// responses at one version are equivalent but not guaranteed byte-identical
/// across a restart. The endpoint name is for debuggability only - ETags are
/// scoped per URL and could not collide regardless.
pub fn weak_etag(kind: &str, token: &str) -> String {
    format!("W/\"{}-{}\"", kind, token)
}

/// Whether the client's `If-None-Match` names this validator.
///
/// A bare `*` (RFC 9110 §13.1.2) means "any current representation" - for a
/// GET that always matches, since we only reach here once a validator for a
/// current representation exists. Treat only an exact `*` token as the
/// wildcard: a `*` embedded in a real tag, e.g. `W/"albums-*-1"`, is just
/// that tag's content and must still be compared by equality.
pub fn matches(if_none_match: Option<&str>, etag: &str) -> bool {
    match if_none_match {
        Some(header) => header.split(',').any(|t| {
            let t = t.trim();
            t == "*" || t == etag
        }),
        None => false,
    }
}

/// A JSON body with a validator, or a bodyless 304.
pub enum Validated<T> {
    Body(Json<T>, Option<String>),
    NotModified(String),
}

/// Fast-path check: does the client's `If-None-Match` already match this
/// version, before any response body has been built?
///
/// Returns `Some` when it does, so the caller can return a 304 immediately
/// without ever constructing the data that goes into a 200. Returns `None`
/// when the body must still be built - either because the token names a real
/// change, no token was sent, or `version` is `None` (the backend does not
/// track changes, so nothing can ever short-circuit here).
///
/// Because `Validated::NotModified` carries no body, this is generic over any
/// `T` without needing one in hand - the caller can call this before it has
/// anything to serialize.
///
/// Safety: this makes the same comparison `validated` would make on a miss
/// path, just earlier. It does not weaken the read-version-before-data rule
/// documented at each call site - callers are expected to read `version`
/// first, then call this, then touch the data only if it returns `None`. A
/// `Some` here means the client's token equals a version read moments ago,
/// with no data access in between, so it can only be an accurate 304.
pub fn not_modified<T>(
    kind: &str,
    version: &Option<String>,
    if_none_match: Option<&str>,
) -> Option<Validated<T>> {
    let token = version.as_ref()?;
    let etag = weak_etag(kind, token);
    matches(if_none_match, &etag).then(|| Validated::NotModified(etag))
}

/// Decide which of the two to send.
///
/// `version: None` means the backend does not track changes; no validator is
/// emitted, and the client's `If-None-Match` is ignored rather than honoured
/// against a value the daemon cannot stand behind.
pub fn validated<T: Serialize>(
    body: T,
    kind: &str,
    token: Option<String>,
    if_none_match: Option<&str>,
) -> Validated<T> {
    let Some(token) = token else {
        return Validated::Body(Json(body), None);
    };

    let etag = weak_etag(kind, &token);
    if matches(if_none_match, &etag) {
        Validated::NotModified(etag)
    } else {
        Validated::Body(Json(body), Some(etag))
    }
}

impl<'r, T: Serialize> Responder<'r, 'static> for Validated<T> {
    fn respond_to(self, req: &'r Request<'_>) -> response::Result<'static> {
        match self {
            Validated::Body(json, etag) => {
                let mut build = Response::build_from(json.respond_to(req)?);
                if let Some(etag) = etag {
                    build.header(Header::new("ETag", etag));
                }
                build.ok()
            }
            Validated::NotModified(etag) => Response::build()
                .status(Status::NotModified)
                .header(Header::new("ETag", etag))
                .ok(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tag_is_weak_and_names_its_endpoint() {
        assert_eq!(weak_etag("albums", "a3f9c1d2-42"), "W/\"albums-a3f9c1d2-42\"");
        assert_eq!(weak_etag("artists", "a3f9c1d2-0"), "W/\"artists-a3f9c1d2-0\"");
    }

    #[test]
    fn a_matching_tag_matches() {
        assert!(matches(Some("W/\"albums-a3f9c1d2-42\""), "W/\"albums-a3f9c1d2-42\""));
    }

    #[test]
    fn a_different_version_does_not_match() {
        assert!(!matches(Some("W/\"albums-a3f9c1d2-41\""), "W/\"albums-a3f9c1d2-42\""));
    }

    #[test]
    fn an_absent_header_does_not_match() {
        assert!(!matches(None, "W/\"albums-a3f9c1d2-42\""));
    }

    #[test]
    fn a_list_of_tags_matches_on_any_member() {
        assert!(matches(Some("W/\"albums-a3f9c1d2-1\", W/\"albums-a3f9c1d2-42\""), "W/\"albums-a3f9c1d2-42\""));
    }

    #[test]
    fn a_bare_wildcard_matches_any_validator() {
        assert!(matches(Some("*"), "W/\"albums-a3f9c1d2-42\""));
        // Whitespace around it must still count as the bare wildcard.
        assert!(matches(Some(" * "), "W/\"albums-a3f9c1d2-42\""));
    }

    #[test]
    fn a_wildcard_embedded_in_a_real_tag_is_not_the_wildcard() {
        // "*" only means "any representation" as its own complete token -
        // here it's just part of the tag's content, so it must compare
        // literally like any other tag and not match a different etag.
        assert!(!matches(Some("W/\"albums-*-1\""), "W/\"albums-a3f9c1d2-42\""));
        assert!(matches(Some("W/\"albums-*-1\""), "W/\"albums-*-1\""));
    }

    #[test]
    fn a_backend_without_a_version_gets_a_plain_body() {
        // version: None means the backend does not track changes, so no
        // validator may be emitted even if the client sent one.
        let reply = validated(vec![1, 2, 3], "albums", None, Some("W/\"albums-a3f9c1d2-42\""));
        assert!(matches!(reply, Validated::Body(_, None)));
    }

    #[test]
    fn a_matching_request_is_not_modified() {
        let reply = validated(vec![1, 2, 3], "albums", Some("a3f9c1d2-42".to_string()), Some("W/\"albums-a3f9c1d2-42\""));
        assert!(matches!(reply, Validated::NotModified(_)));
    }

    #[test]
    fn not_modified_fast_path_matches_without_a_body() {
        let version = Some("a3f9c1d2-42".to_string());
        let reply: Option<Validated<Vec<u32>>> =
            not_modified("albums", &version, Some("W/\"albums-a3f9c1d2-42\""));
        assert!(matches!(reply, Some(Validated::NotModified(_))));
    }

    #[test]
    fn not_modified_fast_path_defers_on_a_different_token() {
        let version = Some("a3f9c1d2-42".to_string());
        let reply: Option<Validated<Vec<u32>>> =
            not_modified("albums", &version, Some("W/\"albums-a3f9c1d2-41\""));
        assert!(reply.is_none());
    }

    #[test]
    fn not_modified_fast_path_defers_when_no_token_was_sent() {
        let version = Some("a3f9c1d2-42".to_string());
        let reply: Option<Validated<Vec<u32>>> = not_modified("albums", &version, None);
        assert!(reply.is_none());
    }

    #[test]
    fn not_modified_fast_path_defers_when_the_backend_has_no_version() {
        let reply: Option<Validated<Vec<u32>>> =
            not_modified("albums", &None, Some("W/\"albums-a3f9c1d2-42\""));
        assert!(reply.is_none());
    }

    use rocket::local::blocking::Client;
    use rocket::http::ContentType;

    #[rocket::get("/list")]
    fn body_route() -> Validated<Vec<u32>> {
        validated(vec![1, 2, 3], "albums", Some("a3f9c1d2-42".to_string()), None)
    }

    #[rocket::get("/nomod")]
    fn not_modified_route() -> Validated<Vec<u32>> {
        validated(vec![1, 2, 3], "albums", Some("a3f9c1d2-42".to_string()), Some("W/\"albums-a3f9c1d2-42\""))
    }

    #[test]
    fn a_body_response_carries_the_tag_and_the_json() {
        let rocket = rocket::build().mount("/", rocket::routes![body_route]);
        let client = Client::tracked(rocket).unwrap();
        let response = client.get("/list").dispatch();
        assert_eq!(response.status(), Status::Ok);
        assert_eq!(response.content_type(), Some(ContentType::JSON));
        assert_eq!(response.headers().get_one("ETag"), Some("W/\"albums-a3f9c1d2-42\""));
        assert_eq!(response.into_string().unwrap(), "[1,2,3]");
    }

    #[test]
    fn a_not_modified_response_carries_no_body() {
        let rocket = rocket::build().mount("/", rocket::routes![not_modified_route]);
        let client = Client::tracked(rocket).unwrap();
        let response = client.get("/nomod").dispatch();
        assert_eq!(response.status(), Status::NotModified);
        assert_eq!(response.headers().get_one("ETag"), Some("W/\"albums-a3f9c1d2-42\""));
        assert!(response.into_bytes().is_none(), "a 304 must not carry a body");
    }
}
