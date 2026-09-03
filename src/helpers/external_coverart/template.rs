//! Substitution of query values into a configured URL template.

use crate::helpers::coverart::CoverartQuery;

/// Expand `{artist}`, `{album}`, `{title}`, `{year}` and `{url}` in a
/// configured template.
///
/// Every value is percent-encoded: song metadata is arbitrary text from a
/// player or a radio stream, and an unescaped `&` or `#` in it would rewrite
/// the query string. A placeholder the query carries no value for expands to
/// the empty string rather than being left in the URL, where the service
/// would be handed the literal text `{album}`.
///
/// For an album lookup the album's name lives in the query's `title` field,
/// which makes both `{album}` and `{title}` plausible readings. The rule is
/// that `{album}` is the album and `{title}` is a song title, so `{title}` is
/// empty for an album lookup.
pub fn expand(template: &str, query: &CoverartQuery) -> String {
    let (artist, album, title, year, url) = match query {
        CoverartQuery::Artist(artist) => (artist.as_str(), "", "", String::new(), ""),
        CoverartQuery::Song { title, artist } => {
            (artist.as_str(), "", title.as_str(), String::new(), "")
        }
        CoverartQuery::Album { title, artist, year } => (
            artist.as_str(),
            title.as_str(),
            "",
            year.map(|y| y.to_string()).unwrap_or_default(),
            "",
        ),
        CoverartQuery::Url(url) => ("", "", "", String::new(), url.as_str()),
    };

    let encode = |value: &str| urlencoding::encode(value).into_owned();

    template
        .replace("{artist}", &encode(artist))
        .replace("{album}", &encode(album))
        .replace("{title}", &encode(title))
        .replace("{year}", &encode(&year))
        .replace("{url}", &encode(url))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::helpers::coverart::CoverartQuery;

    fn song() -> CoverartQuery {
        CoverartQuery::Song {
            title: "Uni Acronym".to_string(),
            artist: "Alva Noto".to_string(),
        }
    }

    #[test]
    fn a_song_query_fills_artist_and_title() {
        assert_eq!(
            expand("https://x.example/c?artist={artist}&title={title}", &song()),
            "https://x.example/c?artist=Alva%20Noto&title=Uni%20Acronym"
        );
    }

    /// Anything that reaches a URL is percent-encoded. Song metadata is
    /// arbitrary text from a radio stream: ampersands and hashes in it would
    /// otherwise rewrite the query string.
    #[test]
    fn values_are_percent_encoded() {
        let query = CoverartQuery::Song {
            title: "A&B #1".to_string(),
            artist: "Fed/Up".to_string(),
        };
        assert_eq!(
            expand("https://x.example/c?a={artist}&t={title}", &query),
            "https://x.example/c?a=Fed%2FUp&t=A%26B%20%231"
        );
    }

    /// A placeholder the query has no value for becomes empty rather than
    /// being left in the URL, where the service would receive the literal
    /// text "{album}".
    #[test]
    fn a_placeholder_with_no_value_becomes_empty() {
        assert_eq!(
            expand("https://x.example/c?album={album}&year={year}", &song()),
            "https://x.example/c?album=&year="
        );
    }

    /// For an album lookup the album's name is in `title`, so both
    /// placeholders would plausibly mean it. `{album}` is the album; `{title}`
    /// is a song title and is empty here.
    #[test]
    fn an_album_query_puts_the_album_in_album_not_title() {
        let query = CoverartQuery::Album {
            title: "Xerrox Vol. 2".to_string(),
            artist: "Alva Noto".to_string(),
            year: Some(2009),
        };
        assert_eq!(
            expand(
                "https://x.example/c?album={album}&title={title}&year={year}&artist={artist}",
                &query
            ),
            "https://x.example/c?album=Xerrox%20Vol.%202&title=&year=2009&artist=Alva%20Noto"
        );
    }

    #[test]
    fn an_artist_query_fills_only_the_artist() {
        assert_eq!(
            expand(
                "https://x.example/c?artist={artist}&album={album}&title={title}",
                &CoverartQuery::Artist("Alva Noto".to_string())
            ),
            "https://x.example/c?artist=Alva%20Noto&album=&title="
        );
    }

    #[test]
    fn a_url_query_fills_the_url_placeholder() {
        assert_eq!(
            expand(
                "https://x.example/c?src={url}",
                &CoverartQuery::Url("https://radio.example/logo.png".to_string())
            ),
            "https://x.example/c?src=https%3A%2F%2Fradio.example%2Flogo.png"
        );
    }

    /// A template that names no placeholders is a valid fixed endpoint.
    #[test]
    fn a_template_without_placeholders_is_returned_unchanged() {
        assert_eq!(
            expand("https://x.example/coverart", &song()),
            "https://x.example/coverart"
        );
    }
}
