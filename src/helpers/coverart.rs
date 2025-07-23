use std::path::Path;
use std::fs::File;
use std::io::{Read, Write};
use log::debug;

/// Extracts cover art from music files in a directory
pub fn extract_cover_from_music_files(dir_path: &str) -> Option<(Vec<u8>, String)> {
    use walkdir::WalkDir;
    use lofty::{Probe, TaggedFileExt};
    
    debug!("Searching for music files with embedded cover art in: {}", dir_path);
    
    // Check if the directory exists
    if !Path::new(dir_path).exists() {
        debug!("Directory does not exist: {}", dir_path);
        return None;
    }

    debug!("Scanning directory for music files with embedded cover art");
    // Walk through the directory looking for music files
    let walker = WalkDir::new(dir_path).max_depth(1).into_iter();

    let mut audio_file_count = 0;

    for entry in walker.filter_map(|e| e.ok()) {
        let path = entry.path();
        
        // Skip directories and non-music files
        if path.is_dir() || !is_audio_file(path) {
            continue;
        }

        audio_file_count += 1;
        
        // Try to read tags from the file
        let tagged_file = match Probe::open(path).and_then(|probe| {
            probe.read()
        }) {
            Ok(file) => {
                file
            },
            Err(e) => {
                debug!("Failed to read tags from file {}: {}", path.display(), e);
                continue;
            }
        };

        // Try to get picture from the primary tag
        let tag = tagged_file.primary_tag().or_else(|| tagged_file.first_tag());
        
        if let Some(tag) = tag {
            // Look for pictures in the tag
            if let Some(picture) = tag.pictures().first() {
                debug!("Found embedded cover art in file: {}", path.display());
                
                // Determine MIME type
                let mime_type = if let Some(mime) = picture.mime_type() {
                    match mime {
                        lofty::MimeType::Jpeg => "image/jpeg",
                        lofty::MimeType::Png => "image/png",
                        lofty::MimeType::Gif => "image/gif",
                        lofty::MimeType::Tiff => "image/tiff",
                        lofty::MimeType::Bmp => "image/bmp",
                        lofty::MimeType::Unknown(_) => "application/octet-stream",
                        _ => "application/octet-stream",
                    }
                } else {
                    "application/octet-stream"
                }.to_string();

                // Get the image data
                let data = picture.data().to_vec();
                
                debug!("Returning embedded cover art data, {} bytes", data.len());
                return Some((data, mime_type));
            }
        }
    }

    debug!("No embedded cover art found in {} audio files, checking for standard cover files", audio_file_count);
    // Also check for standard cover files in the directory
    let standard_covers = ["cover.jpg", "cover.png", "folder.jpg", "folder.png", "album.jpg", "album.png", "front.jpg", "front.png"];
    
    for cover_name in standard_covers.iter() {
        let cover_path = format!("{}/{}", dir_path, cover_name);
        let path = Path::new(&cover_path);
        
        if path.exists() && path.is_file() {
            debug!("Found standard cover file: {}", cover_path);
            
            // Read the file
            match File::open(path) {
                Ok(mut file) => {
                    let mut data = Vec::new();
                    if file.read_to_end(&mut data).is_ok() {
                        debug!("Successfully read {} bytes from {}", data.len(), cover_path);
                        // Determine MIME type based on file extension
                        let mime_type = if cover_name.ends_with(".jpg") || cover_name.ends_with(".jpeg") {
                            "image/jpeg"
                        } else if cover_name.ends_with(".png") {
                            "image/png"
                        } else {
                            "application/octet-stream"
                        }.to_string();
                        
                        return Some((data, mime_type));
                    } else {
                        debug!("Failed to read data from {}", cover_path);
                    }
                }
                Err(e) => debug!("Failed to open cover file {}: {}", cover_path, e),
            }
        }
    }
    
    debug!("No cover art found, returning None");
    None
}

/// Save cover art to a directory as cover.jpg
pub fn save_cover_to_dir(dir_path: &str, data: &[u8]) -> bool {
    let cover_path = format!("{}/cover.jpg", dir_path);
    debug!("Attempting to save cover art to: {}", cover_path);
    
    // Try to create the file and write the data
    match File::create(&cover_path) {
        Ok(mut file) => {
            match file.write_all(data) {
                Ok(_) => {
                    debug!("Successfully saved cover.jpg to directory");
                    true
                }
                Err(e) => {
                    debug!("Failed to write data to cover.jpg: {}", e);
                    false
                }
            }
        }
        Err(e) => {
            debug!("Failed to create cover.jpg in directory: {}", e);
            false
        }
    }
}

/// Check if a file is an audio file based on its extension
pub fn is_audio_file(path: &Path) -> bool {
    if let Some(ext) = path.extension() {
        let ext = ext.to_string_lossy().to_lowercase();
        return ["mp3", "flac", "ogg", "m4a", "wav", "aac", "opus", "wma"].contains(&ext.as_str());
    }
    false
}

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
fn sanitize_for_path(input: &str) -> String {
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
    use std::fs;
    use std::path::PathBuf;

    fn get_test_data_path() -> PathBuf {
        let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        path.push("testdata");
        path
    }

    #[test]
    fn test_is_audio_file() {
        assert!(is_audio_file(Path::new("test.mp3")));
        assert!(is_audio_file(Path::new("test.flac")));
        assert!(is_audio_file(Path::new("test.ogg")));
        assert!(is_audio_file(Path::new("test.m4a")));
        assert!(is_audio_file(Path::new("test.wav")));
        assert!(is_audio_file(Path::new("test.aac")));
        assert!(is_audio_file(Path::new("test.opus")));
        assert!(is_audio_file(Path::new("test.wma")));
        
        assert!(!is_audio_file(Path::new("test.txt")));
        assert!(!is_audio_file(Path::new("test.jpg")));
        assert!(!is_audio_file(Path::new("test.png")));
        assert!(!is_audio_file(Path::new("test")));
    }

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

    #[test]
    fn test_extract_cover_from_standard_files() {
        let test_path = get_test_data_path();
        let album_path = test_path.join("test_album");
        
        if album_path.exists() {
            let result = extract_cover_from_music_files(&album_path.to_string_lossy());
            
            // Should find the cover.jpg file we created
            assert!(result.is_some());
            
            if let Some((data, mime_type)) = result {
                assert!(!data.is_empty());
                assert_eq!(mime_type, "image/jpeg");
            }
        } else {
            // Skip test if test data is not available
            println!("Warning: Test album directory not found at {:?}", album_path);
        }
    }

    #[test]
    fn test_extract_cover_from_embedded_art() {
        let test_path = get_test_data_path();
        let album_path = test_path.join("test_album_embedded");
        
        if album_path.exists() {
            let result = extract_cover_from_music_files(&album_path.to_string_lossy());
            
            // Should find embedded cover art in the MP3 file
            assert!(result.is_some());
            
            if let Some((data, mime_type)) = result {
                assert!(!data.is_empty());
                assert_eq!(mime_type, "image/jpeg");
                // The embedded image should be at least a few KB
                assert!(data.len() > 1000);
            }
        } else {
            // Skip test if test data is not available
            println!("Warning: Test album embedded directory not found at {:?}", album_path);
        }
    }

    #[test]
    fn test_extract_cover_from_sine_wave_album() {
        let test_path = get_test_data_path();
        let album_path = test_path.join("test_album_sine_waves");
        
        if album_path.exists() {
            let result = extract_cover_from_music_files(&album_path.to_string_lossy());
            
            // Should find embedded cover art from one of the sine wave tracks
            assert!(result.is_some());
            
            if let Some((data, mime_type)) = result {
                assert!(!data.is_empty());
                assert_eq!(mime_type, "image/jpeg");
                // The embedded image should be at least a few KB
                assert!(data.len() > 1000);
                println!("Successfully extracted cover art from sine wave album: {} bytes", data.len());
            }
        } else {
            // Skip test if test data is not available
            println!("Warning: Test sine wave album directory not found at {:?}", album_path);
        }
    }

    #[test]
    fn test_extract_cover_from_nonexistent_directory() {
        let result = extract_cover_from_music_files("/nonexistent/directory");
        assert!(result.is_none());
    }

    #[test]
    fn test_save_cover_to_dir() {
        let test_data = b"fake image data";
        let temp_dir = std::env::temp_dir().join("acr_test_cover");
        
        // Create temp directory
        fs::create_dir_all(&temp_dir).unwrap();
        
        let success = save_cover_to_dir(&temp_dir.to_string_lossy(), test_data);
        assert!(success);
        
        // Check that the file was created
        let cover_path = temp_dir.join("cover.jpg");
        assert!(cover_path.exists());
        
        // Check file contents
        let saved_data = fs::read(&cover_path).unwrap();
        assert_eq!(saved_data, test_data);
        
        // Clean up
        fs::remove_dir_all(&temp_dir).unwrap();
    }

    #[test]
    fn test_save_cover_to_invalid_dir() {
        let test_data = b"fake image data";
        let success = save_cover_to_dir("/invalid/nonexistent/directory", test_data);
        assert!(!success);
    }
}
