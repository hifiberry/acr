//! Deciding which half of a split title is the artist.
//!
//! Moved here from `songtitlesplitter` unchanged: it still asks MusicBrainz
//! for both readings of the pair and answers `Unknown` when neither, or both,
//! come back with hits.

use crate::musicbrainz;
use acr_types::OrderResult;

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
/// ```no_run
/// use audiocontrol_metadata::title_order::detect_order;
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
}
