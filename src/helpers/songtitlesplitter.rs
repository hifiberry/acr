/// Song title splitter module
/// 
/// This module provides functionality to split combined artist/title strings
/// into separate parts using common separators.

/// Split a song string into two parts using common separators
/// 
/// Attempts to split the input string on the first occurrence of either "/" or "-".
/// Both parts are trimmed of whitespace.
/// 
/// # Arguments
/// * `input` - The string to split (e.g., "Artist - Song Title")
/// 
/// # Returns
/// * `Some((part1, part2))` - If a separator is found, returns both trimmed parts
/// * `None` - If no separator is found
/// 
/// # Examples
/// ```
/// use audiocontrol::helpers::songtitlesplitter::split_song;
/// 
/// assert_eq!(split_song("Jay's Soul Connection - Frankes Party Life"), 
///            Some(("Jay's Soul Connection".to_string(), "Frankes Party Life".to_string())));
/// assert_eq!(split_song("Artist / Song Title"), 
///            Some(("Artist".to_string(), "Song Title".to_string())));
/// assert_eq!(split_song("No separator here"), None);
/// ```
pub fn split_song(input: &str) -> Option<(String, String)> {
    // Find the first occurrence of either "/" or "-"
    let dash_pos = input.find('-');
    let slash_pos = input.find('/');
    
    // Determine which separator comes first (or if any exists)
    let split_pos = match (dash_pos, slash_pos) {
        (Some(dash), Some(slash)) => Some(dash.min(slash)),
        (Some(dash), None) => Some(dash),
        (None, Some(slash)) => Some(slash),
        (None, None) => None,
    };
    
    // If we found a separator, split the string
    if let Some(pos) = split_pos {
        let part1 = input[..pos].trim().to_string();
        let part2 = input[pos + 1..].trim().to_string();
        
        // Only return if both parts are non-empty after trimming
        if !part1.is_empty() && !part2.is_empty() {
            Some((part1, part2))
        } else {
            None
        }
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_split_with_dash() {
        let result = split_song("Jay's Soul Connection - Frankes Party Life");
        assert_eq!(result, Some(("Jay's Soul Connection".to_string(), "Frankes Party Life".to_string())));
    }

    #[test]
    fn test_split_with_slash() {
        let result = split_song("Artist / Song Title");
        assert_eq!(result, Some(("Artist".to_string(), "Song Title".to_string())));
    }

    #[test]
    fn test_split_with_extra_whitespace() {
        let result = split_song("  Artist  -  Song Title  ");
        assert_eq!(result, Some(("Artist".to_string(), "Song Title".to_string())));
    }

    #[test]
    fn test_split_first_separator_dash() {
        // When both separators exist, should split on the first one (dash in this case)
        let result = split_song("Artist - Song / Other Part");
        assert_eq!(result, Some(("Artist".to_string(), "Song / Other Part".to_string())));
    }

    #[test]
    fn test_split_first_separator_slash() {
        // When both separators exist, should split on the first one (slash in this case)
        let result = split_song("Artist / Song - Other Part");
        assert_eq!(result, Some(("Artist".to_string(), "Song - Other Part".to_string())));
    }

    #[test]
    fn test_no_separator() {
        let result = split_song("No separator here");
        assert_eq!(result, None);
    }

    #[test]
    fn test_empty_string() {
        let result = split_song("");
        assert_eq!(result, None);
    }

    #[test]
    fn test_only_separator() {
        let result = split_song("-");
        assert_eq!(result, None);
    }

    #[test]
    fn test_separator_at_start() {
        let result = split_song("- Song Title");
        assert_eq!(result, None); // Empty first part
    }

    #[test]
    fn test_separator_at_end() {
        let result = split_song("Artist -");
        assert_eq!(result, None); // Empty second part
    }

    #[test]
    fn test_multiple_dashes() {
        let result = split_song("Artist - Song - More Info");
        assert_eq!(result, Some(("Artist".to_string(), "Song - More Info".to_string())));
    }

    #[test]
    fn test_multiple_slashes() {
        let result = split_song("Artist / Song / More Info");
        assert_eq!(result, Some(("Artist".to_string(), "Song / More Info".to_string())));
    }

    #[test]
    fn test_single_character_parts() {
        let result = split_song("A - B");
        assert_eq!(result, Some(("A".to_string(), "B".to_string())));
    }

    #[test]
    fn test_unicode_characters() {
        let result = split_song("Артист - Песня");
        assert_eq!(result, Some(("Артист".to_string(), "Песня".to_string())));
    }

    #[test]
    fn test_special_characters_in_title() {
        let result = split_song("Band (feat. Someone) - Song Title & More");
        assert_eq!(result, Some(("Band (feat. Someone)".to_string(), "Song Title & More".to_string())));
    }
}
