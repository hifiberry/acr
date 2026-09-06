/// Generate a cache key for an album based on artist, album name, and year
pub fn album_cache_key(artist: &str, album_name: &str, year: Option<i32>) -> String {
    let sanitized_artist = sanitize_for_path(artist);
    let sanitized_album = sanitize_for_path(album_name);

    if let Some(y) = year {
        format!("albums/{}/{}-{}", sanitized_artist, y, sanitized_album)
    } else {
        format!("albums/{}/{}", sanitized_artist, sanitized_album)
    }
}

/// Sanitize a string for use in a path
pub fn sanitize_for_path(input: &str) -> String {
    let sanitized = input
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == ' ' || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect::<String>();

    sanitized.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_for_path() {
        assert_eq!(sanitize_for_path("Test Artist"), "Test Artist");
        assert_eq!(sanitize_for_path("Test/Artist"), "Test_Artist");
        assert_eq!(sanitize_for_path("Test\\Artist"), "Test_Artist");
        assert_eq!(sanitize_for_path("Test:Artist"), "Test_Artist");
        assert_eq!(sanitize_for_path("Test*Artist"), "Test_Artist");
        assert_eq!(sanitize_for_path("Test?Artist"), "Test_Artist");
        assert_eq!(sanitize_for_path("Test<Artist>"), "Test_Artist_");
        assert_eq!(sanitize_for_path("Test|Artist"), "Test_Artist");
        assert_eq!(sanitize_for_path("Test\"Artist"), "Test_Artist");
        assert_eq!(sanitize_for_path("  Test Artist  "), "Test Artist");
    }

    #[test]
    fn test_album_cache_key() {
        assert_eq!(
            album_cache_key("Test Artist", "Test Album", Some(2023)),
            "albums/Test Artist/2023-Test Album"
        );

        assert_eq!(
            album_cache_key("Test Artist", "Test Album", None),
            "albums/Test Artist/Test Album"
        );

        assert_eq!(
            album_cache_key("Test/Artist", "Test:Album", Some(2023)),
            "albums/Test_Artist/2023-Test_Album"
        );
    }
}
