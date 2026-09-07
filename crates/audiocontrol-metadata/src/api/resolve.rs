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
/// Copied from `src/api/splitters.rs`'s `order_name` rather than shared
/// *across crates*: the player crate owns its own `OrderResult` wire mapping
/// for the splitter settings API and must not depend on this crate, so the
/// two mappings are kept in sync by eye. `pub(crate)` because `core_client`,
/// in this same crate, needs the identical mapping to report a title-order
/// observation to that same player-crate route — a second copy in this crate
/// would be exactly the duplication this comment already exists to explain,
/// for no reason. The spelling — `artist_song` / `song_artist` / `unknown` /
/// `undecided` — is the one wire format both sides agree on.
pub(crate) fn order_name(order: OrderResult) -> &'static str {
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
/// Each separator is its own `separator=` parameter, repeated; no parameter at
/// all means the module's own defaults.
///
/// Deliberately not one comma-separated parameter, which is what this route
/// first shipped with and which could not survive its own default list: `,` is
/// itself the first entry of `DEFAULT_ARTIST_SEPARATORS`, so joining on a comma
/// and splitting on one turned the defaults into `["", "&", " feat ", ...]` and
/// stopped comma-separated artists splitting at all. Worse, a configured
/// `[", "]` arrived as `[" "]` and split every two-word artist name in the
/// library into two artists. A separator is arbitrary text; it cannot share a
/// delimiter with the list that carries it.
#[get("/resolve/artist-split?<name>&<separator>")]
pub fn artist_split(name: &str, separator: Vec<String>) -> Json<serde_json::Value> {
    // Absent and empty are the same request: the caller named no separators,
    // so the defaults apply. `split_album_artist` never sends an empty list,
    // and a caller that did cannot mean "split on nothing" -- that would make
    // every name a single artist, which is what `None` already answers.
    let seps: Option<Vec<String>> = if separator.is_empty() { None } else { Some(separator) };
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

    /// The case a comma-joined separator list cannot express, and the one the
    /// route first shipped broken: a comma IS a separator, and it is the first
    /// of the defaults. Each name here is split by a different entry of
    /// `DEFAULT_ARTIST_SEPARATORS`, sent the way `MetadataClient` sends them.
    #[test]
    fn every_default_separator_survives_the_query_string() {
        crate::test_support::init_test_caches();
        let client = test_client();

        let defaults: Vec<String> = acr_types::artist_split::DEFAULT_ARTIST_SEPARATORS
            .iter()
            .map(|s| s.to_string())
            .collect();
        let query: String = defaults
            .iter()
            .map(|s| format!("&separator={}", urlencoding::encode(s)))
            .collect();

        for (name, expected) in [
            ("Sepcomma Alpha, Sepcomma Beta", vec!["Sepcomma Alpha", "Sepcomma Beta"]),
            ("Sepamp Alpha & Sepamp Beta", vec!["Sepamp Alpha", "Sepamp Beta"]),
            ("Sepfeat Alpha feat Sepfeat Beta", vec!["Sepfeat Alpha", "Sepfeat Beta"]),
        ] {
            let r = client
                .get(format!(
                    "/api/resolve/artist-split?name={}{}",
                    urlencoding::encode(name),
                    query
                ))
                .dispatch();
            assert_eq!(
                r.into_json::<serde_json::Value>().unwrap()["artists"],
                serde_json::json!(expected),
                "{name} should split on its own default separator"
            );
        }
    }

    /// A configured separator that contains a comma. Under the joined encoding
    /// this arrived as a bare space and split every two-word artist name.
    #[test]
    fn a_separator_containing_a_comma_is_not_torn_apart() {
        crate::test_support::init_test_caches();
        let client = test_client();

        let r = client
            .get("/api/resolve/artist-split?name=Comma%20Sep%20Floyd&separator=%2C%20")
            .dispatch();
        assert_eq!(
            r.into_json::<serde_json::Value>().unwrap()["artists"],
            serde_json::json!(null),
            "\", \" must not degrade into \" \", which would split every two-word name"
        );

        let r = client
            .get("/api/resolve/artist-split?name=Commapair%20One%2C%20Commapair%20Two&separator=%2C%20")
            .dispatch();
        assert_eq!(
            r.into_json::<serde_json::Value>().unwrap()["artists"],
            serde_json::json!(["Commapair One", "Commapair Two"]),
            "and it must still split where it genuinely occurs"
        );
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
    fn a_custom_separator_replaces_the_defaults_rather_than_adding_to_them() {
        crate::test_support::init_test_caches();
        let client = test_client();

        let r = client
            .get("/api/resolve/artist-split?name=Pipe%20Alpha%7CPipe%20Beta&separator=%7C")
            .dispatch();
        assert_eq!(
            r.into_json::<serde_json::Value>().unwrap()["artists"],
            serde_json::json!(["Pipe Alpha", "Pipe Beta"])
        );

        // And the defaults are gone while it is named: `&` is a default, so a
        // caller that asked only for `|` must not get a split on `&` as well.
        //
        // A name no other test uses, deliberately. `split_artist_names_with_
        // mbid_lookup` caches on the NAME ALONE -- the separators are not part
        // of the key -- so a name another test has already split would be
        // answered from that cache and this assertion would pass or fail on
        // test ordering rather than on the code it names.
        let r = client
            .get("/api/resolve/artist-split?name=Onlypipe%20Alpha%20%26%20Onlypipe%20Beta&separator=%7C")
            .dispatch();
        assert_eq!(
            r.into_json::<serde_json::Value>().unwrap()["artists"],
            serde_json::json!(null)
        );
    }
}
