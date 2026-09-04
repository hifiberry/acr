use rocket::http::{ContentType, Header, Status};
use rocket::request::Request;
use rocket::response::{self, Responder, Response};
use std::io::Cursor;

/// Album art addressed by album id does not change under that id, so a year is honest.
pub const IMMUTABLE_CACHE: &str = "public, max-age=31536000, immutable";

/// Artist images can be replaced by the user, so promising immutability would
/// strand the replacement behind every client cache in the field.
pub const REVALIDATE_DAILY_CACHE: &str = "public, max-age=86400";

/// A strong ETag over the bytes actually served.
///
/// Hashing the response rather than reading stored metadata keeps this correct for
/// images that never passed through the cache, and costs well under a millisecond
/// even for a 243 KB original.
pub fn etag_for(data: &[u8]) -> String {
    format!("\"{:x}\"", md5::compute(data))
}

/// An image response, or a 304 when the client already has it.
pub enum ImageReply {
    Image {
        data: Vec<u8>,
        content_type: ContentType,
        cache_control: &'static str,
        etag: String,
    },
    NotModified {
        etag: String,
        cache_control: &'static str,
    },
}

/// Build the reply, honouring `If-None-Match`.
pub fn reply(
    data: Vec<u8>,
    mime: &str,
    cache_control: &'static str,
    if_none_match: Option<&str>,
) -> ImageReply {
    let etag = etag_for(&data);

    if let Some(candidate) = if_none_match {
        if candidate.split(',').any(|t| t.trim() == etag) {
            return ImageReply::NotModified { etag, cache_control };
        }
    }

    let media_type = mime.split('/').next().unwrap_or("application").to_string();
    let media_subtype = mime.split('/').nth(1).unwrap_or("octet-stream").to_string();

    ImageReply::Image {
        data,
        content_type: ContentType::new(media_type, media_subtype),
        cache_control,
        etag,
    }
}

impl<'r> Responder<'r, 'static> for ImageReply {
    fn respond_to(self, _: &'r Request<'_>) -> response::Result<'static> {
        match self {
            ImageReply::Image { data, content_type, cache_control, etag } => Response::build()
                .header(content_type)
                .header(Header::new("Cache-Control", cache_control))
                .header(Header::new("ETag", etag))
                .sized_body(data.len(), Cursor::new(data))
                .ok(),
            ImageReply::NotModified { etag, cache_control } => Response::build()
                .status(Status::NotModified)
                .header(Header::new("Cache-Control", cache_control))
                .header(Header::new("ETag", etag))
                .ok(),
        }
    }
}

/// The client's `If-None-Match` header, if any.
pub struct IfNoneMatch<'r>(pub Option<&'r str>);

#[rocket::async_trait]
impl<'r> rocket::request::FromRequest<'r> for IfNoneMatch<'r> {
    type Error = std::convert::Infallible;

    async fn from_request(req: &'r Request<'_>) -> rocket::request::Outcome<Self, Self::Error> {
        rocket::request::Outcome::Success(IfNoneMatch(req.headers().get_one("If-None-Match")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rocket::get;
    use rocket::local::blocking::Client;
    use rocket::routes;

    #[test]
    fn etag_is_stable_and_content_dependent() {
        assert_eq!(etag_for(b"abc"), etag_for(b"abc"));
        assert_ne!(etag_for(b"abc"), etag_for(b"abd"));
        assert!(etag_for(b"abc").starts_with('"'));
        assert!(etag_for(b"abc").ends_with('"'));
    }

    #[test]
    fn matching_if_none_match_yields_not_modified() {
        let data = b"some image bytes".to_vec();
        let tag = etag_for(&data);
        let reply = reply(data, "image/jpeg", IMMUTABLE_CACHE, Some(&tag));
        assert!(matches!(reply, ImageReply::NotModified { .. }));
    }

    #[test]
    fn absent_or_stale_if_none_match_yields_the_image() {
        let data = b"some image bytes".to_vec();
        assert!(matches!(
            reply(data.clone(), "image/jpeg", IMMUTABLE_CACHE, None),
            ImageReply::Image { .. }
        ));
        assert!(matches!(
            reply(data, "image/jpeg", IMMUTABLE_CACHE, Some("\"stale\"")),
            ImageReply::Image { .. }
        ));
    }

    #[test]
    fn matching_etag_among_several_if_none_match_tags_yields_not_modified() {
        let data = b"some image bytes".to_vec();
        let tag = etag_for(&data);
        let header = format!("\"someothertag\", {}", tag);
        assert!(matches!(
            reply(data, "image/jpeg", IMMUTABLE_CACHE, Some(&header)),
            ImageReply::NotModified { .. }
        ));
    }

    #[test]
    fn no_matching_tag_among_several_if_none_match_tags_yields_the_image() {
        let data = b"some image bytes".to_vec();
        assert!(matches!(
            reply(
                data,
                "image/jpeg",
                IMMUTABLE_CACHE,
                Some("\"tag-one\", \"tag-two\"")
            ),
            ImageReply::Image { .. }
        ));
    }

    #[get("/test-image-reply")]
    fn test_image_route() -> ImageReply {
        reply(
            b"some image bytes".to_vec(),
            "image/jpeg",
            IMMUTABLE_CACHE,
            None,
        )
    }

    #[get("/test-not-modified-reply")]
    fn test_not_modified_route() -> ImageReply {
        let data = b"some image bytes".to_vec();
        let tag = etag_for(&data);
        reply(data, "image/jpeg", IMMUTABLE_CACHE, Some(&tag))
    }

    #[test]
    fn image_arm_responds_with_status_headers_and_body() {
        let rocket = rocket::build().mount("/", routes![test_image_route]);
        let client = Client::tracked(rocket).expect("valid rocket instance");
        let response = client.get("/test-image-reply").dispatch();

        assert_eq!(response.status(), Status::Ok);
        assert_eq!(response.content_type(), Some(ContentType::JPEG));
        assert_eq!(
            response.headers().get_one("Cache-Control"),
            Some(IMMUTABLE_CACHE)
        );
        let expected_etag = etag_for(b"some image bytes");
        assert_eq!(
            response.headers().get_one("ETag"),
            Some(expected_etag.as_str())
        );
        let body = response.into_bytes().expect("a body");
        assert_eq!(body, b"some image bytes".to_vec());
    }

    #[test]
    fn not_modified_arm_responds_with_status_headers_and_no_body() {
        let rocket = rocket::build().mount("/", routes![test_not_modified_route]);
        let client = Client::tracked(rocket).expect("valid rocket instance");
        let response = client.get("/test-not-modified-reply").dispatch();

        assert_eq!(response.status(), Status::NotModified);
        assert_eq!(
            response.headers().get_one("Cache-Control"),
            Some(IMMUTABLE_CACHE)
        );
        let expected_etag = etag_for(b"some image bytes");
        assert_eq!(
            response.headers().get_one("ETag"),
            Some(expected_etag.as_str())
        );
        // A 304 must carry no body at all, not merely an empty one.
        assert!(response.into_bytes().is_none());
    }
}
