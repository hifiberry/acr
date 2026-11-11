use log::debug;

/// Default separators used to split artist names containing multiple artists
pub static DEFAULT_ARTIST_SEPARATORS: &[&str] = &[",", "&", " feat ", " feat.", " featuring ", " with "];

/// Split an artist name that might contain multiple artists using custom separators
///
/// # Arguments
/// * `artist_name` - The artist name to split
/// * `separators` - Custom separators to use for splitting
///
/// # Returns
/// * `Vec<String>` - Vector containing individual artist names
///
/// # Examples
/// ```
/// use acr_types::artist_split::split_artist_with_separators;
///
/// let custom_separators = vec![" x ".to_string(), " vs ".to_string()];
/// let artists = split_artist_with_separators("Artist A x Artist B vs Artist C", &custom_separators);
/// assert_eq!(artists, vec!["Artist A", "Artist B", "Artist C"]);
/// ```
pub fn split_artist_with_separators(artist_name: &str, separators: &[String]) -> Vec<String> {
    debug!("Splitting artist name: '{}' with custom separators: {:?}", artist_name, separators);

    // Initial result will contain the full string
    let mut result = vec![artist_name.to_string()];

    // Iteratively split by each separator
    for separator in separators {
        // Every string contains the empty string, and splitting by it yields
        // one piece per character. A stray "" in a configured list would turn
        // every artist into a list of letters.
        if separator.is_empty() {
            continue;
        }

        let mut new_result = Vec::new();

        for part in result {
            // Skip empty parts
            if part.trim().is_empty() {
                continue;
            }

            // For each existing part, split it by the current separator
            if part.contains(separator) {
                for sub_part in part.split(separator) {
                    let trimmed = sub_part.trim();
                    if !trimmed.is_empty() {
                        new_result.push(trimmed.to_string());
                    }
                }
            } else {
                // If no separator in this part, keep it as is
                new_result.push(part);
            }
        }

        // Update result for the next separator
        result = new_result;
    }

    // Filter out any "feat." prefixes and empty strings
    result = result
        .into_iter()
        .map(|a| a.trim().to_string())
        .filter(|a| !a.is_empty() && !a.to_lowercase().starts_with("feat."))
        .collect();

    debug!("Split artist '{}' into: {:?}", artist_name, result);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_split_artist_with_custom_separators() {
        let custom_separators = vec![" x ".to_string(), " vs ".to_string()];
        let result = split_artist_with_separators("Artist A x Artist B vs Artist C", &custom_separators);
        assert_eq!(result, vec!["Artist A", "Artist B", "Artist C"]);
    }

    #[test]
    fn test_an_empty_separator_is_ignored() {
        let separators = vec!["".to_string(), ",".to_string()];
        let result = split_artist_with_separators("Artist A, Artist B", &separators);
        assert_eq!(result, vec!["Artist A", "Artist B"]);

        let only_empty = vec![String::new()];
        assert_eq!(split_artist_with_separators("Artist A", &only_empty), vec!["Artist A"]);
    }

    fn split_with_defaults(name: &str) -> Vec<String> {
        let separators: Vec<String> = DEFAULT_ARTIST_SEPARATORS.iter().map(|s| s.to_string()).collect();
        split_artist_with_separators(name, &separators)
    }

    #[test]
    fn test_default_separators() {
        assert_eq!(split_with_defaults("Artist A"), vec!["Artist A"]);
        assert_eq!(split_with_defaults("Artist A, Artist B"), vec!["Artist A", "Artist B"]);
        assert_eq!(split_with_defaults("Artist A & Artist B"), vec!["Artist A", "Artist B"]);
        assert_eq!(split_with_defaults("Artist A feat Artist B"), vec!["Artist A", "Artist B"]);
        assert_eq!(split_with_defaults("Artist A feat. Artist B"), vec!["Artist A", "Artist B"]);
        assert_eq!(split_with_defaults("Artist A featuring Artist B"), vec!["Artist A", "Artist B"]);
        assert_eq!(split_with_defaults("Artist A with Artist B"), vec!["Artist A", "Artist B"]);
        assert_eq!(
            split_with_defaults("Artist A, Artist B & Artist C feat. Artist D"),
            vec!["Artist A", "Artist B", "Artist C", "Artist D"]
        );
    }

    /// The word separators are matched with their surrounding spaces, so a
    /// name that merely contains one of the words is left whole.
    #[test]
    fn test_default_separators_leave_words_inside_names_alone() {
        assert_eq!(split_with_defaults("Witherspoon"), vec!["Witherspoon"]);
        assert_eq!(split_with_defaults("Defeater"), vec!["Defeater"]);
    }

    #[test]
    fn test_empty_and_blank_names_split_to_nothing() {
        assert!(split_with_defaults("").is_empty());
        assert!(split_with_defaults("   ").is_empty());
        assert!(split_with_defaults(" , & ").is_empty());
    }
}
