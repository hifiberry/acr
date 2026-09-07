//! Artist detail: what this side knows about one artist, by name.
//!
//! This is the route the player daemon's three artist-detail routes
//! (`by-id`, `by-name`, `by-mbid`) call and merge over what they hold, per
//! `doc/specs/2026-09-04-player-metadata-split.md`. The name travels as
//! URL-safe base64, the same convention the cover art routes use, because an
//! artist name can contain characters a path segment cannot.

use acr_types::url_encoding::decode_url_safe;
use acr_types::{Artist, ArtistMeta, Identifier};
use rocket::get;
use rocket::http::Status;
use rocket::serde::json::Json;

/// One artist's metadata, or 404 when this side has nothing for it.
///
/// Reads the attribute cache first, through the same key-spelling helper
/// `library_enricher` reads and writes it with. A cache miss is answered as
/// "nothing yet" unless the caller passes `?lookup=true`, in which case a
/// fresh synchronous lookup runs via `artist_store::update_data_for_artist`
/// — the same provider chain a library load runs, just for one artist, on
/// demand. The player daemon never passes it; a future caller that would
/// rather wait for an answer than get a miss can.
#[get("/artist/<artist_b64>?<lookup>")]
pub fn get_artist(artist_b64: &str, lookup: Option<bool>) -> Result<Json<ArtistMeta>, Status> {
    let name = decode_url_safe(artist_b64).ok_or(Status::NotFound)?;

    if let Some(meta) = crate::library_enricher::cached_artist_metadata(&name) {
        return Ok(Json(meta));
    }

    if lookup.unwrap_or(false) {
        let fresh = Artist {
            id: Identifier::String(name.clone()),
            name: name.clone(),
            is_multi: false,
            metadata: None,
        };
        if let Some(meta) = crate::artist_store::update_data_for_artist(fresh).metadata {
            return Ok(Json(meta));
        }
    }

    Err(Status::NotFound)
}

#[cfg(test)]
mod tests {
    use super::*;
    use acr_types::url_encoding::encode_url_safe;
    use rocket::local::blocking::Client;

    fn test_client() -> Client {
        let rocket = rocket::build().mount("/api", rocket::routes![get_artist]);
        Client::tracked(rocket).unwrap()
    }

    #[test]
    fn an_unknown_artist_detail_is_not_found() {
        crate::test_support::init_test_caches();
        let client = test_client();
        // "bm9ib2R5" is "nobody" base64url-encoded, per the brief's own test.
        assert_eq!(client.get("/api/artist/bm9ib2R5").dispatch().status(), Status::NotFound);
    }

    #[test]
    fn an_undecodable_name_is_not_used_as_a_literal_cache_key() {
        // A 404 here is ambiguous by itself -- decoding failing and decoding
        // succeeding into an uncached name both answer 404. So this plants a
        // cache entry under the raw, undecoded path segment: if decoding
        // were skipped and the segment used verbatim as the artist name,
        // this would be a cache *hit* and the route would answer 200.
        crate::test_support::init_test_caches();
        let raw = "not-valid-base64!!";
        let mut meta = ArtistMeta::new();
        meta.add_genre("should never be reached".to_string());
        acr_store::attributecache::set(&crate::library_enricher::artist_metadata_key(raw), &meta)
            .expect("attribute cache should accept the write");

        let client = test_client();
        assert_eq!(
            client.get(format!("/api/artist/{}", raw)).dispatch().status(),
            Status::NotFound
        );
    }

    #[test]
    fn a_cached_artist_is_served_from_the_attribute_cache() {
        crate::test_support::init_test_caches();
        let name = "Route Cache Test Artist";
        let mut meta = ArtistMeta::new();
        meta.add_genre("test genre".to_string());
        acr_store::attributecache::set(&crate::library_enricher::artist_metadata_key(name), &meta)
            .expect("attribute cache should accept the write");

        let client = test_client();
        let encoded = encode_url_safe(name);
        let response = client.get(format!("/api/artist/{}", encoded)).dispatch();
        assert_eq!(response.status(), Status::Ok);
        let body = response.into_json::<ArtistMeta>().unwrap();
        assert_eq!(body.genres, vec!["test genre".to_string()]);
    }

    // Whether `?lookup=true`'s absence actually skips the provider-chain
    // lookup is deliberately not asserted here. On a cache miss, both "never
    // attempted" and "attempted, provider chain found nothing" answer 404 in
    // this sandbox (no coverart providers are registered outside
    // `main`'s start-up, and the lookup path also reaches
    // `acr_store::settingsdb`'s global, which an unprivileged test cannot
    // point elsewhere the way `artist_store`'s own tests do). Flipping
    // `lookup.unwrap_or(false)` to `unwrap_or(true)` by hand and rerunning
    // this file's tests left every one of them green -- confirmed rather
    // than assumed -- so a test asserting "not attempted by default" here
    // would pass whether or not that is true. What genuinely distinguishes
    // the two branches -- the trip through `artist_store::update_data_for_artist`
    // -- needs a way to observe or mock that call that this crate's other
    // tests don't have either, so it stays untested rather than covered by
    // a check that cannot fail.
}
