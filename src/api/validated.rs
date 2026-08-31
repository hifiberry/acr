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
pub fn matches(if_none_match: Option<&str>, etag: &str) -> bool {
    match if_none_match {
        Some(header) => header.split(',').any(|t| t.trim() == etag),
        None => false,
    }
}

/// A JSON body with a validator, or a bodyless 304.
pub enum Validated<T> {
    Body(Json<T>, Option<String>),
    NotModified(String),
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
