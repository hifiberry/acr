/// Example of how to use the lyrics provider with MPD
/// 
/// This example demonstrates:
/// 1. Creating an MPD lyrics provider
/// 2. Getting lyrics by file path (URL)
/// 3. Parsing LRC format lyrics
/// 4. Working with timed lyrics

use audiocontrol::helpers::lyrics::{
    LyricsProvider, MPDLyricsProvider, LyricsLookup, LyricsContent, parse_lrc_content
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create an MPD lyrics provider with music directory
    let lyrics_provider = MPDLyricsProvider::new("/var/lib/mpd/music".to_string());
    
    // Example 1: Get lyrics by file path
    // This looks for /var/lib/mpd/music/artist/album/song.lrc
    let file_path = "artist/album/song.mp3";
    match lyrics_provider.get_lyrics_by_url(file_path) {
        Ok(content) => {
            println!("Found lyrics for {}", file_path);
            match content {
                LyricsContent::Timed(timed_lyrics) => {
                    println!("Timed lyrics with {} lines:", timed_lyrics.len());
                    for lyric in timed_lyrics.iter().take(5) {
                        println!("  {} - {}", lyric.format_timestamp(), lyric.text);
                    }
                }
                LyricsContent::PlainText(text) => {
                    println!("Plain text lyrics:\n{}", text);
                }
            }
        }
        Err(e) => {
            println!("No lyrics found for {}: {}", file_path, e);
        }
    }
    
    // Example 2: Parse LRC content manually
    let lrc_content = r#"[00:10.73] Alright
[00:12.56] 
[00:36.24] I left my heart
[00:40.76] In the darkside of the city
[00:45.48] I left my soul"#;
    
    match parse_lrc_content(lrc_content) {
        Ok(timed_lyrics) => {
            println!("\nParsed {} timed lyrics:", timed_lyrics.len());
            for lyric in &timed_lyrics {
                if lyric.text.is_empty() {
                    println!("  {} (pause)", lyric.format_timestamp());
                } else {
                    println!("  {} - {}", lyric.format_timestamp(), lyric.text);
                }
            }
        }
        Err(e) => {
            println!("Failed to parse LRC: {}", e);
        }
    }
    
    // Example 3: Working with metadata lookup (not implemented for MPD yet)
    let lookup = LyricsLookup::new("Artist Name".to_string(), "Song Title".to_string())
        .with_duration(180.0)
        .with_album("Album Name".to_string());
    
    match lyrics_provider.get_lyrics_by_metadata(&lookup) {
        Ok(content) => {
            println!("Found lyrics by metadata: {}", content.as_plain_text());
        }
        Err(_) => {
            println!("Metadata-based lookup not implemented for MPD yet");
        }
    }
    
    Ok(())
}
