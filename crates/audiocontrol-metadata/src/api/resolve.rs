//! Two questions the player daemon used to answer in-process, both driven by
//! MusicBrainz: which half of a split title is the artist, and whether a
//! combined artist string names more than one artist.
//!
//! Both keep their offline fallback: with MusicBrainz lookups disabled (the
//! default until configured), `title_order` answers `unknown` and
//! `artist_split` falls back to a plain separator split, exactly as the
//! in-process callers do today.

use acr_types::OrderResult;
use rocket::get;
use rocket::serde::json::Json;

/// Wire name of an order.
///
/// Copied from `src/api/splitters.rs`'s `order_name` rather than shared: the
/// player crate owns its own `OrderResult` wire mapping for the splitter
/// settings API and must not depend on this crate, so the two mappings are
/// kept in sync by eye. The spelling — `artist_song` / `song_artist` /
/// `unknown` / `undecided` — is the one wire format both sides agree on.
fn order_name(order: OrderResult) -> &'static str {
    match order {
        OrderResult::ArtistSong => "artist_song",
        OrderResult::SongArtist => "song_artist",
        OrderResult::Unknown => "unknown",
        OrderResult::Undecided => "undecided",
    }
}

/// Guess which of two title parts is the artist.
#[get("/resolve/title-order?<part1>&<part2>")]
pub fn title_order(part1: &str, part2: &str) -> Json<serde_json::Value> {
    let order = order_name(crate::title_order::detect_order(part1, part2));
    Json(serde_json::json!({ "order": order }))
}

/// Split a combined artist string into its individual artists, if it names
/// more than one.
///
/// `separators` is a comma-separated list of the separators to try; absent
/// means the module's own defaults.
#[get("/resolve/artist-split?<name>&<separators>")]
pub fn artist_split(name: &str, separators: Option<&str>) -> Json<serde_json::Value> {
    let seps: Option<Vec<String>> = separators
        .map(|s| s.split(',').filter(|p| !p.is_empty()).map(str::to_string).collect());
    let answer =
        crate::artistsplitter::split_artist_names_with_mbid_lookup(name, false, seps.as_deref());
    Json(serde_json::json!({ "artists": answer }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rocket::http::Status;
    use rocket::local::blocking::Client;

    fn test_client() -> Client {
        let rocket = rocket::build().mount("/api", rocket::routes![title_order, artist_split]);
        Client::tracked(rocket).unwrap()
    }

    #[test]
    fn order_name_covers_every_variant_with_the_splitter_apis_spelling() {
        assert_eq!(order_name(OrderResult::ArtistSong), "artist_song");
        assert_eq!(order_name(OrderResult::SongArtist), "song_artist");
        assert_eq!(order_name(OrderResult::Unknown), "unknown");
        assert_eq!(order_name(OrderResult::Undecided), "undecided");
    }

    #[test]
    fn title_order_answers_one_of_four_values() {
        let client = test_client();
        let r = client
            .get("/api/resolve/title-order?part1=The%20Beatles&part2=Hey%20Jude")
            .dispatch();
        assert_eq!(r.status(), Status::Ok);
        let order = r.into_json::<serde_json::Value>().unwrap()["order"]
            .as_str()
            .unwrap()
            .to_string();
        assert!(["artist_song", "song_artist", "unknown", "undecided"].contains(&order.as_str()));
    }

    #[test]
    fn artist_split_without_musicbrainz_is_the_plain_split() {
        crate::test_support::init_test_caches();
        // musicbrainz is disabled in tests (no initialize_from_config ran)
        let client = test_client();
        let r = client.get("/api/resolve/artist-split?name=A%20%26%20B").dispatch();
        assert_eq!(
            r.into_json::<serde_json::Value>().unwrap()["artists"],
            serde_json::json!(["A", "B"])
        );
    }

    #[test]
    fn artist_split_of_a_single_artist_answers_null() {
        crate::test_support::init_test_caches();
        let client = test_client();
        let r = client
            .get("/api/resolve/artist-split?name=No%20Separator%20Here")
            .dispatch();
        assert_eq!(
            r.into_json::<serde_json::Value>().unwrap()["artists"],
            serde_json::json!(null)
        );
    }

    #[test]
    fn custom_separators_are_parsed_from_the_comma_separated_query_value() {
        crate::test_support::init_test_caches();
        let client = test_client();
        let r = client
            .get("/api/resolve/artist-split?name=A%7CB&separators=%7C")
            .dispatch();
        assert_eq!(
            r.into_json::<serde_json::Value>().unwrap()["artists"],
            serde_json::json!(["A", "B"])
        );
    }
}
