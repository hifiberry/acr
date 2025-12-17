use std::io::{BufRead, BufReader};
use std::time::Duration;
use reqwest;
use serde::{Deserialize, Serialize};
use log::{debug, info};
use thiserror::Error;

/// Errors that can occur during M3U playlist parsing
#[derive(Error, Debug)]
pub enum M3UError {
    #[error("Failed to download playlist: {0}")]
    DownloadError(#[from] reqwest::Error),
    
    #[error("Invalid URL: {0}")]
    InvalidUrl(String),
    
    #[error("Empty playlist")]
    EmptyPlaylist,
    
    #[error("Invalid M3U format: {0}")]
    InvalidFormat(String),
    
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
}

/// Represents a single entry in an M3U playlist
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct M3UEntry {
    /// The URL or file path of the media
    pub url: String,
    
    /// Optional title from #EXTINF directive
    pub title: Option<String>,
    
    /// Optional duration in seconds from #EXTINF directive
    pub duration: Option<f64>,
    
    /// Optional additional info from #EXTINF directive
    pub info: Option<String>,
}

/// Represents a parsed M3U playlist
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct M3UPlaylist {
    /// List of media entries in the playlist
    pub entries: Vec<M3UEntry>,
    
    /// Total number of entries
    pub count: usize,
    
    /// Whether this is an extended M3U playlist (with #EXTM3U header)
    pub is_extended: bool,
}

/// M3U Parser with HTTP download capability
pub struct M3UParser {
    /// HTTP client for downloading playlists
    client: reqwest::blocking::Client,
}

impl Default for M3UParser {
    fn default() -> Self {
        Self::new()
    }
}

impl M3UParser {
    /// Create a new M3U parser with default HTTP client settings
    pub fn new() -> Self {
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(30))
            .user_agent("HiFiBerry-AudioControl/0.6.7")
            .build()
            .expect("Failed to create HTTP client");
            
        Self { client }
    }
    
    /// Create a new M3U parser with custom timeout
    pub fn with_timeout(timeout_secs: u64) -> Self {
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(timeout_secs))
            .user_agent("HiFiBerry-AudioControl/0.6.7")
            .build()
            .expect("Failed to create HTTP client");
            
        Self { client }
    }
    
    /// Parse an M3U playlist from a URL
    /// 
    /// Downloads the playlist from the given URL and parses it
    /// 
    /// # Arguments
    /// * `url` - The URL of the M3U playlist to download and parse
    /// 
    /// # Returns
    /// * `Result<M3UPlaylist, M3UError>` - The parsed playlist or an error
    pub fn parse_from_url(&self, url: &str) -> Result<M3UPlaylist, M3UError> {
        info!("Downloading M3U playlist from URL: {}", url);
        
        // Validate URL format
        if !self.is_valid_url(url) {
            return Err(M3UError::InvalidUrl(format!("Invalid URL format: {}", url)));
        }
        
        // Download the playlist content
        let response = self.client.get(url).send()?.error_for_status()?;
        
        let content = response.text()?;
        debug!("Downloaded {} bytes of playlist content", content.len());
        
        // Parse the content
        self.parse_content(&content, Some(url))
    }
    
    /// Parse M3U content from a string
    /// 
    /// # Arguments
    /// * `content` - The M3U playlist content as a string
    /// * `base_url` - Optional base URL for resolving relative paths
    /// 
    /// # Returns
    /// * `Result<M3UPlaylist, M3UError>` - The parsed playlist or an error
    pub fn parse_content(&self, content: &str, base_url: Option<&str>) -> Result<M3UPlaylist, M3UError> {
        debug!("Parsing M3U content ({} bytes)", content.len());
        
        let reader = BufReader::new(content.as_bytes());
        let lines: Vec<String> = reader.lines().collect::<Result<Vec<_>, _>>()?;
        
        if lines.is_empty() {
            return Err(M3UError::EmptyPlaylist);
        }
        
        let mut entries = Vec::new();
        let mut is_extended = false;
        let mut current_extinf: Option<(Option<f64>, Option<String>)> = None;
        
        for (line_num, line) in lines.iter().enumerate() {
            let trimmed = line.trim();
            
            // Skip empty lines
            if trimmed.is_empty() {
                continue;
            }
            
            // Skip comments (but process M3U directives)
            if trimmed.starts_with('#') {
                if trimmed.starts_with("#EXTM3U") {
                    is_extended = true;
                    debug!("Detected extended M3U format");
                } else if trimmed.starts_with("#EXTINF:") {
                    // Parse #EXTINF directive: #EXTINF:duration,title
                    current_extinf = self.parse_extinf(trimmed);
                    if current_extinf.is_some() {
                        debug!("Parsed EXTINF directive on line {}", line_num + 1);
                    }
                }
                // Skip other comments and directives
                continue;
            }
            
            // This should be a media URL/path
            let url = self.resolve_url(trimmed, base_url);
            
            // Create entry with optional EXTINF info
            let entry = if let Some((duration, title)) = current_extinf.take() {
                M3UEntry {
                    url,
                    title,
                    duration,
                    info: None,
                }
            } else {
                M3UEntry {
                    url,
                    title: None,
                    duration: None,
                    info: None,
                }
            };
            
            entries.push(entry);
            debug!("Added entry {}: {}", entries.len(), entries.last().unwrap().url);
        }
        
        if entries.is_empty() {
            return Err(M3UError::EmptyPlaylist);
        }
        
        let playlist = M3UPlaylist {
            count: entries.len(),
            entries,
            is_extended,
        };
        
        info!("Successfully parsed M3U playlist with {} entries (extended: {})", 
              playlist.count, playlist.is_extended);
        
        Ok(playlist)
    }
    
    /// Parse an #EXTINF directive
    /// 
    /// Format: #EXTINF:duration,title
    /// 
    /// # Arguments
    /// * `line` - The #EXTINF line to parse
    /// 
    /// # Returns
    /// * `Option<(Option<f64>, Option<String>)>` - Duration and title if successfully parsed
    fn parse_extinf(&self, line: &str) -> Option<(Option<f64>, Option<String>)> {
        // Remove #EXTINF: prefix
        let content = line.strip_prefix("#EXTINF:")?;
        
        // Find the comma that separates duration from title
        if let Some(comma_pos) = content.find(',') {
            let duration_str = &content[..comma_pos];
            let title_str = &content[comma_pos + 1..];
            
            // Parse duration (can be integer or float)
            let duration = if duration_str.trim() == "-1" || duration_str.trim().is_empty() {
                None
            } else {
                duration_str.trim().parse::<f64>().ok()
            };
            
            // Parse title (trim and handle empty)
            let title = if title_str.trim().is_empty() {
                None
            } else {
                Some(title_str.trim().to_string())
            };
            
            Some((duration, title))
        } else {
            // No comma found, might be just duration
            let duration = if content.trim() == "-1" || content.trim().is_empty() {
                None
            } else {
                content.trim().parse::<f64>().ok()
            };
            
            Some((duration, None))
        }
    }
    
    /// Resolve a URL against a base URL
    /// 
    /// # Arguments
    /// * `url` - The URL to resolve (may be relative)
    /// * `base_url` - Optional base URL for resolving relative paths
    /// 
    /// # Returns
    /// * `String` - The resolved URL
    fn resolve_url(&self, url: &str, base_url: Option<&str>) -> String {
        // If URL is already absolute, return as-is
        if self.is_valid_url(url) {
            return url.to_string();
        }
        
        // If we have a base URL and the URL is relative, try to resolve it
        if let Some(base) = base_url {
            if url.starts_with('/') {
                // Absolute path - combine with base domain
                if let Ok(base_url_parsed) = reqwest::Url::parse(base) {
                    if let Some(domain) = base_url_parsed.domain() {
                        let scheme = base_url_parsed.scheme();
                        // Only include port if it's not the default port
                        let port = match base_url_parsed.port() {
                            Some(p) => format!(":{}", p),
                            None => String::new(),
                        };
                        return format!("{}://{}{}{}", scheme, domain, port, url);
                    }
                }
            } else {
                // Relative path - resolve against base URL
                if let Ok(base_url_parsed) = reqwest::Url::parse(base) {
                    if let Ok(resolved) = base_url_parsed.join(url) {
                        return resolved.to_string();
                    }
                }
            }
        }
        
        // Return as-is if we can't resolve
        url.to_string()
    }
    
    /// Check if a URL is valid
    fn is_valid_url(&self, url: &str) -> bool {
        url.starts_with("http://") || url.starts_with("https://") || url.starts_with("ftp://")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_extinf_with_duration_and_title() {
        let parser = M3UParser::new();
        let result = parser.parse_extinf("#EXTINF:180,Artist - Song Title");
        
        assert_eq!(result, Some((Some(180.0), Some("Artist - Song Title".to_string()))));
    }

    #[test]
    fn test_parse_extinf_with_float_duration() {
        let parser = M3UParser::new();
        let result = parser.parse_extinf("#EXTINF:123.456,Test Song");
        
        assert_eq!(result, Some((Some(123.456), Some("Test Song".to_string()))));
    }

    #[test]
    fn test_parse_extinf_unknown_duration() {
        let parser = M3UParser::new();
        let result = parser.parse_extinf("#EXTINF:-1,Unknown Duration Song");
        
        assert_eq!(result, Some((None, Some("Unknown Duration Song".to_string()))));
    }

    #[test]
    fn test_parse_extinf_no_title() {
        let parser = M3UParser::new();
        let result = parser.parse_extinf("#EXTINF:240,");
        
        assert_eq!(result, Some((Some(240.0), None)));
    }

    #[test]
    fn test_parse_extinf_only_duration() {
        let parser = M3UParser::new();
        let result = parser.parse_extinf("#EXTINF:300");
        
        assert_eq!(result, Some((Some(300.0), None)));
    }

    #[test]
    fn test_parse_extinf_invalid() {
        let parser = M3UParser::new();
        let result = parser.parse_extinf("#EXTINF:");
        
        assert_eq!(result, Some((None, None)));
    }

    #[test]
    fn test_parse_simple_m3u_content() {
        let parser = M3UParser::new();
        let content = r#"http://example.com/song1.mp3
http://example.com/song2.mp3
http://example.com/song3.mp3"#;
        
        let result = parser.parse_content(content, None).unwrap();
        
        assert_eq!(result.count, 3);
        assert!(!result.is_extended);
        assert_eq!(result.entries[0].url, "http://example.com/song1.mp3");
        assert_eq!(result.entries[0].title, None);
        assert_eq!(result.entries[0].duration, None);
    }

    #[test]
    fn test_parse_extended_m3u_content() {
        let parser = M3UParser::new();
        let content = r#"#EXTM3U
#EXTINF:180,Artist1 - Song1
http://example.com/song1.mp3
#EXTINF:240,Artist2 - Song2
http://example.com/song2.mp3
#EXTINF:-1,Live Stream
http://example.com/stream.m3u8"#;
        
        let result = parser.parse_content(content, None).unwrap();
        
        assert_eq!(result.count, 3);
        assert!(result.is_extended);
        
        assert_eq!(result.entries[0].url, "http://example.com/song1.mp3");
        assert_eq!(result.entries[0].title, Some("Artist1 - Song1".to_string()));
        assert_eq!(result.entries[0].duration, Some(180.0));
        
        assert_eq!(result.entries[1].url, "http://example.com/song2.mp3");
        assert_eq!(result.entries[1].title, Some("Artist2 - Song2".to_string()));
        assert_eq!(result.entries[1].duration, Some(240.0));
        
        assert_eq!(result.entries[2].url, "http://example.com/stream.m3u8");
        assert_eq!(result.entries[2].title, Some("Live Stream".to_string()));
        assert_eq!(result.entries[2].duration, None);
    }

    #[test]
    fn test_parse_m3u_with_comments() {
        let parser = M3UParser::new();
        let content = r#"#EXTM3U
# This is a comment
#EXTINF:180,Song with comment
http://example.com/song1.mp3
# Another comment
http://example.com/song2.mp3"#;
        
        let result = parser.parse_content(content, None).unwrap();
        
        assert_eq!(result.count, 2);
        assert!(result.is_extended);
        assert_eq!(result.entries[0].title, Some("Song with comment".to_string()));
        assert_eq!(result.entries[1].title, None);
    }

    #[test]
    fn test_parse_empty_playlist() {
        let parser = M3UParser::new();
        let content = "#EXTM3U\n# Only comments here\n";
        
        let result = parser.parse_content(content, None);
        assert!(matches!(result, Err(M3UError::EmptyPlaylist)));
    }

    #[test]
    fn test_resolve_absolute_url() {
        let parser = M3UParser::new();
        let result = parser.resolve_url("http://example.com/song.mp3", Some("http://base.com/"));
        assert_eq!(result, "http://example.com/song.mp3");
    }

    #[test]
    fn test_resolve_relative_url() {
        let parser = M3UParser::new();
        let result = parser.resolve_url("songs/song.mp3", Some("http://example.com/playlists/"));
        assert_eq!(result, "http://example.com/playlists/songs/song.mp3");
    }

    #[test]
    fn test_resolve_absolute_path() {
        let parser = M3UParser::new();
        let result = parser.resolve_url("/music/song.mp3", Some("http://example.com/playlists/"));
        assert_eq!(result, "http://example.com/music/song.mp3");
    }

    #[test]
    fn test_is_valid_url() {
        let parser = M3UParser::new();
        assert!(parser.is_valid_url("http://example.com"));
        assert!(parser.is_valid_url("https://example.com"));
        assert!(parser.is_valid_url("ftp://example.com"));
        assert!(!parser.is_valid_url("example.com"));
        assert!(!parser.is_valid_url("/path/to/file"));
        assert!(!parser.is_valid_url("relative/path"));
    }

    #[test]
    fn test_m3u_entry_serialization() {
        let entry = M3UEntry {
            url: "http://example.com/song.mp3".to_string(),
            title: Some("Test Song".to_string()),
            duration: Some(180.0),
            info: None,
        };
        
        let json = serde_json::to_string(&entry).unwrap();
        let deserialized: M3UEntry = serde_json::from_str(&json).unwrap();
        
        assert_eq!(entry, deserialized);
    }

    #[test]
    fn test_m3u_playlist_serialization() {
        let playlist = M3UPlaylist {
            entries: vec![
                M3UEntry {
                    url: "http://example.com/song1.mp3".to_string(),
                    title: Some("Song 1".to_string()),
                    duration: Some(180.0),
                    info: None,
                }
            ],
            count: 1,
            is_extended: true,
        };
        
        let json = serde_json::to_string(&playlist).unwrap();
        let deserialized: M3UPlaylist = serde_json::from_str(&json).unwrap();
        
        assert_eq!(playlist.count, deserialized.count);
        assert_eq!(playlist.is_extended, deserialized.is_extended);
        assert_eq!(playlist.entries.len(), deserialized.entries.len());
    }
}
