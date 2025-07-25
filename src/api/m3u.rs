use crate::helpers::m3u::{M3UParser, M3UPlaylist, M3UError};
use rocket::serde::json::Json;
use rocket::{post};
use rocket::response::status::Custom;
use serde::{Deserialize, Serialize};
use log::{debug, warn, error, info};

/// Request structure for M3U playlist parsing
#[derive(Deserialize, Serialize)]
pub struct M3UParseRequest {
    /// URL of the M3U playlist to download and parse
    pub url: String,
    
    /// Optional timeout in seconds (default: 30)
    pub timeout_seconds: Option<u64>,
}

/// Response structure for M3U playlist parsing
#[derive(Serialize, Deserialize)]
pub struct M3UParseResponse {
    /// Whether the parsing was successful
    pub success: bool,
    
    /// The parsed playlist data
    #[serde(skip_serializing_if = "Option::is_none")]
    pub playlist: Option<M3UPlaylist>,
    
    /// Error message if parsing failed
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    
    /// The original URL that was requested
    pub url: String,
    
    /// Timestamp when the parsing was performed
    pub timestamp: String,
}

/// Parse an M3U playlist from a URL
/// 
/// POST /api/m3u/parse
#[post("/parse", data = "<request>")]
pub fn parse_m3u_playlist(
    request: Json<M3UParseRequest>,
) -> Result<Json<M3UParseResponse>, Custom<String>> {
    let request = request.into_inner();
    let url = request.url.trim();
    
    info!("Received M3U parse request for URL: {}", url);
    
    // Validate URL
    if url.is_empty() {
        warn!("Empty URL provided for M3U parsing");
        return Ok(Json(M3UParseResponse {
            success: false,
            playlist: None,
            error: Some("URL cannot be empty".to_string()),
            url: url.to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
        }));
    }
    
    // Create parser with optional custom timeout
    let parser = if let Some(timeout) = request.timeout_seconds {
        debug!("Using custom timeout: {} seconds", timeout);
        M3UParser::with_timeout(timeout)
    } else {
        M3UParser::new()
    };
    
    // Attempt to parse the playlist
    match parser.parse_from_url(url) {
        Ok(playlist) => {
            info!("Successfully parsed M3U playlist from {} with {} entries", 
                  url, playlist.count);
            
            Ok(Json(M3UParseResponse {
                success: true,
                playlist: Some(playlist),
                error: None,
                url: url.to_string(),
                timestamp: chrono::Utc::now().to_rfc3339(),
            }))
        }
        Err(e) => {
            error!("Failed to parse M3U playlist from {}: {}", url, e);
            
            let error_message = match e {
                M3UError::DownloadError(ref reqwest_err) => {
                    format!("Failed to download playlist: {}", reqwest_err)
                }
                M3UError::InvalidUrl(ref msg) => {
                    format!("Invalid URL: {}", msg)
                }
                M3UError::EmptyPlaylist => {
                    "The playlist is empty or contains no valid entries".to_string()
                }
                M3UError::InvalidFormat(ref msg) => {
                    format!("Invalid M3U format: {}", msg)
                }
                M3UError::IoError(ref io_err) => {
                    format!("IO error: {}", io_err)
                }
            };
            
            Ok(Json(M3UParseResponse {
                success: false,
                playlist: None,
                error: Some(error_message),
                url: url.to_string(),
                timestamp: chrono::Utc::now().to_rfc3339(),
            }))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json;

    #[test]
    fn test_m3u_parse_request_serialization() {
        let request = M3UParseRequest {
            url: "http://example.com/playlist.m3u".to_string(),
            timeout_seconds: Some(60),
        };
        
        let json = serde_json::to_string(&request).unwrap();
        let deserialized: M3UParseRequest = serde_json::from_str(&json).unwrap();
        
        assert_eq!(request.url, deserialized.url);
        assert_eq!(request.timeout_seconds, deserialized.timeout_seconds);
    }

    #[test]
    fn test_m3u_parse_response_serialization() {
        let response = M3UParseResponse {
            success: true,
            playlist: None,
            error: None,
            url: "http://example.com/test.m3u".to_string(),
            timestamp: "2025-07-25T12:00:00Z".to_string(),
        };
        
        let json = serde_json::to_string(&response).unwrap();
        let deserialized: M3UParseResponse = serde_json::from_str(&json).unwrap();
        
        assert_eq!(response.success, deserialized.success);
        assert_eq!(response.url, deserialized.url);
        assert_eq!(response.timestamp, deserialized.timestamp);
    }

    #[test]
    fn test_empty_url_validation() {
        let request = Json(M3UParseRequest {
            url: "".to_string(),
            timeout_seconds: None,
        });
        
        let result = parse_m3u_playlist(request);
        assert!(result.is_ok());
        
        let response = result.unwrap().into_inner();
        assert!(!response.success);
        assert!(response.error.is_some());
        assert!(response.error.unwrap().contains("URL cannot be empty"));
    }

    #[test]
    fn test_whitespace_url_validation() {
        let request = Json(M3UParseRequest {
            url: "   ".to_string(),
            timeout_seconds: None,
        });
        
        let result = parse_m3u_playlist(request);
        assert!(result.is_ok());
        
        let response = result.unwrap().into_inner();
        assert!(!response.success);
        assert!(response.error.is_some());
    }
}
