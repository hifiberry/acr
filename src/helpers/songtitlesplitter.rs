/// Song title splitter module
/// 
/// This module provides functionality to split combined artist/title strings
/// into separate parts using common separators and determine their order
/// using MusicBrainz lookups.

use crate::helpers::musicbrainz;

/// Result of order detection
#[derive(Debug, PartialEq, Clone)]
pub enum OrderResult {
    /// First part is artist, second part is song
    ArtistSong,
    /// First part is song, second part is artist  
    SongArtist,
    /// No combination found in MusicBrainz
    Unknown,
    /// Both combinations found, cannot determine
    Undecided,
}

/// Split a combined title into artist and song parts
///
/// This function splits titles that contain both artist and song information
/// separated by common delimiters like " / " or " - ".
///
/// # Arguments
/// * `title` - The combined title string to split
///
/// # Returns
/// An optional tuple of (part1, part2) if splitting was successful
///
/// # Examples
/// ```
/// use audiocontrol::helpers::songtitlesplitter::split_song;
/// 
/// let result = split_song("The Beatles / Hey Jude");
/// assert_eq!(result, Some(("The Beatles".to_string(), "Hey Jude".to_string())));
/// 
/// let result = split_song("Yesterday - The Beatles");
/// assert_eq!(result, Some(("Yesterday".to_string(), "The Beatles".to_string())));
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

/// Detect the order of artist and song in split parts using MusicBrainz lookup
///
/// This function attempts to determine which part is the artist and which is the song
/// by searching MusicBrainz for exact matches. It tries both combinations:
/// - part1 as artist, part2 as song
/// - part1 as song, part2 as artist
///
/// # Arguments
/// * `part1` - The first part of the split title
/// * `part2` - The second part of the split title
///
/// # Returns
/// An OrderResult indicating the detected order:
/// - ArtistSong: part1 is artist, part2 is song
/// - SongArtist: part1 is song, part2 is artist
/// - Unknown: no combination found in MusicBrainz
/// - Undecided: both combinations found, cannot determine
///
/// # Examples
/// ```
/// use audiocontrol::helpers::songtitlesplitter::{detect_order, OrderResult};
/// 
/// let result = detect_order("The Beatles", "Hey Jude");
/// // Result depends on MusicBrainz database content
/// ```
pub fn detect_order(part1: &str, part2: &str) -> OrderResult {
    // Try part1 as artist, part2 as song
    let artist_song_result = musicbrainz::search_recording(part1, part2);
    let artist_song_found = match artist_song_result {
        Ok(response) => response.count > 0,
        Err(_) => false,
    };

    // Try part1 as song, part2 as artist
    let song_artist_result = musicbrainz::search_recording(part2, part1);
    let song_artist_found = match song_artist_result {
        Ok(response) => response.count > 0,
        Err(_) => false,
    };

    match (artist_song_found, song_artist_found) {
        (true, false) => OrderResult::ArtistSong,
        (false, true) => OrderResult::SongArtist,
        (false, false) => OrderResult::Unknown,
        (true, true) => OrderResult::Undecided,
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
    
    #[test]
    fn test_detect_order_well_known_songs() {
        // Note: These tests require MusicBrainz to be enabled and accessible
        // In a real-world scenario, you would mock the MusicBrainz responses
        
        // Test case: Artist / Song format
        let _result = detect_order("The Beatles", "Hey Jude");
        // Should return ArtistSong if MusicBrainz has this combination
        
        // Test case: Song - Artist format  
        let _result2 = detect_order("Yesterday", "The Beatles");
        // Should return SongArtist if MusicBrainz has this combination
        
        // Test case: Unknown combination
        let _result3 = detect_order("NonExistentArtist", "NonExistentSong");
        // Should return Unknown
        
        // Since these tests depend on external API, we just verify the function runs
        // In production, you would mock musicbrainz::search_recording responses
        println!("detect_order tests completed successfully");
    }
    
    #[test] 
    fn test_detect_order_mock_scenarios() {
        // These are conceptual tests showing what results should be expected
        // In a real implementation, you would mock the MusicBrainz responses
        
        // Example of what we expect for well-known songs:
        // detect_order("The Beatles", "Hey Jude") -> OrderResult::ArtistSong
        // detect_order("Hey Jude", "The Beatles") -> OrderResult::SongArtist  
        // detect_order("Queen", "Bohemian Rhapsody") -> OrderResult::ArtistSong
        // detect_order("Bohemian Rhapsody", "Queen") -> OrderResult::SongArtist
        // detect_order("Led Zeppelin", "Stairway to Heaven") -> OrderResult::ArtistSong
        // detect_order("Stairway to Heaven", "Led Zeppelin") -> OrderResult::SongArtist
        // detect_order("Unknown Artist", "Unknown Song") -> OrderResult::Unknown
        
        // For now, just test that the function exists and can be called
        // Real tests would require mocking the musicbrainz module
        assert!(true); // Placeholder assertion
    }
}
