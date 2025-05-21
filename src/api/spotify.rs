// Spotify API module for managing authentication and tokens

use rocket::serde::{json::Json, Deserialize, Serialize};
use rocket::post;
use rocket::get;
use log::{error, info};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::helpers::spotify::{Spotify, SpotifyTokens};
use crate::helpers::http_client::new_http_client;
use rocket::http::Status;
use serde_json::Value;

#[derive(Debug, Serialize, Deserialize)]
pub struct StoreTokensRequest {
    access_token: String,
    refresh_token: String,
    expires_in: u64, // Seconds until token expires
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ApiResponse {
    status: String,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    expires_at: Option<u64>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TokenStatus {
    authenticated: bool,
    expires_at: Option<u64>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct OAuthConfig {
    oauth_url: String,
    redirect_uri: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateSessionResponse {
    session_id: String,
}

/// Store Spotify tokens in the security store
#[post("/tokens", data = "<request>")]
pub fn store_tokens(
    request: Json<StoreTokensRequest>,
) -> Json<ApiResponse> {
    let spotify = Spotify::new();
    
    // Calculate expiration time
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    
    let expires_at = now + request.expires_in;
    
    // Create token object
    let tokens = SpotifyTokens {
        access_token: request.access_token.clone(),
        refresh_token: request.refresh_token.clone(),
        expires_at,
    };
    
    // Store tokens
    match spotify.store_tokens(&tokens) {
        Ok(_) => {
            info!("Spotify tokens stored successfully");
            Json(ApiResponse {
                status: "success".to_string(),
                message: "Tokens stored successfully".to_string(),
                expires_at: Some(expires_at),
            })
        },
        Err(e) => {
            error!("Failed to store Spotify tokens: {}", e);
            Json(ApiResponse {
                status: "error".to_string(),
                message: format!("Failed to store tokens: {}", e),
                expires_at: None,
            })
        }
    }
}

/// Get status of Spotify authentication
#[get("/status")]
pub fn token_status() -> Json<TokenStatus> {
    let spotify = Spotify::new();
    
    let status = match spotify.get_tokens() {
        Ok(tokens) => {
            TokenStatus {
                authenticated: true,
                expires_at: Some(tokens.expires_at),
            }
        },
        Err(_) => {
            TokenStatus {
                authenticated: false,
                expires_at: None,
            }
        }
    };
    
    Json(status)
}

/// Clear all Spotify tokens and user data
#[post("/logout")]
pub fn logout() -> Json<ApiResponse> {
    let spotify = Spotify::new();
    
    match spotify.clear_tokens() {
        Ok(_) => {
            Json(ApiResponse {
                status: "success".to_string(),
                message: "Logged out successfully".to_string(),
                expires_at: None,
            })
        },
        Err(e) => {
            Json(ApiResponse {
                status: "error".to_string(),
                message: format!("Failed to logout: {}", e),
                expires_at: None,
            })
        }
    }
}

/// Get OAuth configuration for Spotify authentication
#[get("/oauth_config")]
pub fn get_oauth_config() -> Json<OAuthConfig> {
    let spotify = Spotify::new();
    
    // Get the base URL of the request to construct the redirect URI
    // We assume the app will be hosted at /example/spotify.html
    let redirect_uri = format!("/example/spotify.html");
    
    Json(OAuthConfig {
        oauth_url: spotify.get_oauth_url().to_string(),
        redirect_uri,
    })
}

/// Create a new OAuth session
#[get("/create_session")]
pub fn create_session() -> Result<Json<CreateSessionResponse>, Status> {
    let spotify = Spotify::new();
    // Create HTTP client with a reasonable timeout
    let http_client = new_http_client(10);    
    // Proxy the request to the OAuth service
    // Ensure the OAuth URL has a trailing slash before adding the endpoint path
    let base_url = spotify.get_oauth_url();
    let url = if base_url.ends_with('/') {
        format!("{}create_session", base_url)
    } else {
        format!("{}/create_session", base_url)
    };
    
    // IMPORTANT: Use X-Proxy-Secret header instead of Authorization: Bearer
    let proxy_secret = spotify.get_proxy_secret();
    
    // Log more details about the request
    info!("Creating OAuth session with URL: {}", url);
    info!("OAuth URL from config: '{}'", base_url);
    info!("Proxy secret length: {} chars", proxy_secret.len());
    
    // Check for issues with the OAuth URL format
    if !url.starts_with("http") {
        error!("Invalid OAuth URL: does not start with http/https");
    }
    
    if spotify.get_proxy_secret().trim().is_empty() {
        error!("Proxy secret is empty or whitespace only");
    }
    
    let headers = [
        ("X-Proxy-Secret", proxy_secret)
    ];
    
    match http_client.get_json_with_headers(&url, &headers) {
        Ok(response) => {
            info!("Successfully received response from OAuth service");
            // Parse the session ID from the response
            match response.get("session_id").and_then(|id| id.as_str()) {
                Some(session_id) => {
                    info!("Created OAuth session with ID: {}", session_id);
                    Ok(Json(CreateSessionResponse {
                        session_id: session_id.to_string(),
                    }))
                },
                None => {
                    error!("Invalid response from OAuth service: missing session_id");
                    error!("Response content: {}", serde_json::to_string_pretty(&response).unwrap_or_else(|_| format!("{:?}", response)));
                    Err(Status::InternalServerError)
                }
            }
        },
        Err(e) => {
            error!("Failed to create OAuth session: {}", e);
            Err(Status::InternalServerError)
        }
    }
}

/// Proxy OAuth login request
#[get("/login/<session_id>")]
pub fn login(session_id: String) -> Result<Json<ApiResponse>, Status> {
    let spotify = Spotify::new();
    
    // Ensure the OAuth URL has a trailing slash before adding the endpoint path
    let base_url = spotify.get_oauth_url();
    let url = if base_url.ends_with('/') {
        format!("{}login/{}", base_url, session_id)
    } else {
        format!("{}/login/{}", base_url, session_id)
    };
    
    // Get the proxy secret for X-Proxy-Secret header
    let proxy_secret = spotify.get_proxy_secret();
    
    info!("Proxying login request for session: {}", session_id);
    info!("Full login URL: {}", url);
    
    // Create a custom agent that doesn't follow redirects
    let agent = ureq::AgentBuilder::new()
        .redirects(0)  // Disable automatic redirect following
        .build();
        
    // Make the request with our custom agent
    let result = agent.get(&url)
        .timeout(std::time::Duration::from_secs(10))
        .set("X-Proxy-Secret", proxy_secret)
        .call();    match result {
        // Check for redirect status codes (3xx)
        Err(ureq::Error::Status(code, response)) if (300..400).contains(&code) => {
            // Extract the Location header for the redirect
            if let Some(location) = response.header("Location") {
                info!("Successfully got redirect URL from OAuth service: {}", location);
                
                // Decode HTML entities in the redirect URL
                let decoded_location = location
                    .replace("&amp;", "&")
                    .replace("&quot;", "\"")
                    .replace("&lt;", "<")
                    .replace("&gt;", ">");
                
                info!("Decoded redirect URL: {}", decoded_location);
                
                // Return the redirect URL to the client
                return Ok(Json(ApiResponse {
                    status: "redirect".to_string(),
                    message: decoded_location, // Contains the Spotify authorization URL
                    expires_at: None,
                }));
            } else {
                error!("OAuth server returned a redirect without Location header");
                return Err(Status::InternalServerError);
            }
        },// Regular successful response (unlikely with OAuth but handle it anyway)
        Ok(response) => {
            // Capture the status code before consuming the response
            let status_code = response.status();
            
            // In the case of a non-redirect successful response, check if the body contains a Spotify URL
            if let Ok(body) = response.into_string() {
                if body.contains("spotify.com/authorize") || body.contains("accounts.spotify.com") {
                    info!("Response contains Spotify authorization URL");
                    
                    // Extract the URL from the response if possible
                    if let Some(url_start) = body.find("https://accounts.spotify.com") {
                        if let Some(url_end) = body[url_start..].find("\"") {                            let spotify_url = &body[url_start..(url_start + url_end)];
                            info!("Extracted Spotify URL: {}", spotify_url);
                            
                            // Decode HTML entities
                            let decoded_url = spotify_url
                                .replace("&amp;", "&")
                                .replace("&quot;", "\"")
                                .replace("&lt;", "<")
                                .replace("&gt;", ">");
                            
                            info!("Decoded Spotify URL: {}", decoded_url);
                            
                            return Ok(Json(ApiResponse {
                                status: "redirect".to_string(),
                                message: decoded_url,
                                expires_at: None,
                            }));
                        }
                    }
                    
                    // If we found Spotify references but couldn't extract the URL
                    info!("Found Spotify references but couldn't extract the exact URL");
                }
                
                // If we couldn't extract a URL, just log what we got
                info!("Got response body of length {} with status {}", body.len(), status_code);
                
                return Ok(Json(ApiResponse {
                    status: "success".to_string(),
                    message: "Login request processed".to_string(),
                    expires_at: None,
                }));
            } else {
                error!("Could not read response body");
                return Err(Status::InternalServerError);
            }
        },
        // Handle other HTTP status errors
        Err(ureq::Error::Status(code, response)) => {
            let error_body = response.into_string().unwrap_or_else(|_| "<failed to read response body>".to_string());
            error!("OAuth server returned error {}: {}", code, error_body);
            return Err(Status::InternalServerError);
        },
        // Handle network and other errors
        Err(e) => {
            error!("Failed to proxy login request for session {}: {}", session_id, e);
            return Err(Status::InternalServerError);
        }
    }
}

/// Poll for token data
#[get("/poll/<session_id>")]
pub fn poll_session(session_id: String) -> Result<Json<Value>, Status> {
    let spotify = Spotify::new();
    // Create HTTP client with a reasonable timeout
    let http_client = new_http_client(10);
    
    // Ensure the OAuth URL has a trailing slash before adding the endpoint path
    let base_url = spotify.get_oauth_url();
    let url = if base_url.ends_with('/') {
        format!("{}poll/{}", base_url, session_id)
    } else {
        format!("{}/poll/{}", base_url, session_id)
    };
    
    // Get the proxy secret for X-Proxy-Secret header
    let proxy_secret = spotify.get_proxy_secret();
    
    info!("Polling session: {}", session_id);
    info!("Full poll URL: {}", url);
    
    let headers = [
        ("X-Proxy-Secret", proxy_secret)
    ];
    
    match http_client.get_json_with_headers(&url, &headers) {
        Ok(data) => {
            info!("Successfully polled session: {}", session_id);
            // Log the response status but not all data (might contain tokens)
            if let Some(status) = data.get("status").and_then(|s| s.as_str()) {
                info!("Poll status: {}", status);
                
                // Log if completed
                if status == "completed" {
                    info!("OAuth flow completed successfully for session: {}", session_id);
                }
            } else {
                info!("Poll response has no status field");
            }
            
            Ok(Json(data))
        },
        Err(e) => {
            error!("Failed to poll session {}: {}", session_id, e);
            Err(Status::InternalServerError)
        }
    }
}

/// Check if the OAuth server is reachable
#[get("/check_server")]
pub fn check_server() -> Json<ApiResponse> {
    let spotify = Spotify::new();
    
    match spotify.check_oauth_server() {
        Ok(valid) => {
            if valid {
                info!("OAuth server check: server is reachable and appears valid");
                Json(ApiResponse {
                    status: "success".to_string(),
                    message: "OAuth server is reachable and responding correctly".to_string(),
                    expires_at: None,
                })
            } else {
                info!("OAuth server check: server is reachable but response doesn't look like an OAuth server");
                Json(ApiResponse {
                    status: "warning".to_string(),
                    message: "OAuth server is reachable but response doesn't match expected format".to_string(),
                    expires_at: None,
                })
            }
        },
        Err(e) => {
            error!("OAuth server check failed: {}", e);
            Json(ApiResponse {
                status: "error".to_string(),
                message: format!("Failed to connect to OAuth server: {}", e),
                expires_at: None,
            })
        }
    }
}

/// Get the current Spotify playback state for the authenticated user
#[get("/playback")]
pub fn get_playback() -> Result<Json<Value>, Status> {    let spotify = Spotify::new();
    
    // Try to ensure we have valid tokens, refreshing if necessary
    match spotify.ensure_valid_token() {
        Err(e) => {
            error!("Failed to get valid Spotify token: {}", e);
            return Err(Status::Unauthorized);
        },
        Ok(_) => info!("Successfully obtained valid Spotify token")
    }
    
    match spotify.get_playback_state() {
        Ok(Some(playback)) => {
            // Convert to generic JSON value to avoid exposing internal types
            match serde_json::to_value(playback) {
                Ok(json) => Ok(Json(json)),
                Err(e) => {
                    error!("Error serializing playback state: {}", e);
                    Err(Status::InternalServerError)
                }
            }
        },
        Ok(None) => {
            // No active playback, return empty object
            Ok(Json(serde_json::json!({"is_playing": false, "message": "No active playback"})))
        },
        Err(e) => {
            error!("Error getting playback state: {}", e);
            // Check if this is an authentication error
            if e.to_string().contains("token") || e.to_string().contains("auth") {
                Err(Status::Unauthorized)
            } else {
                Err(Status::InternalServerError)
            }
        }
    }
}

/// Handle Spotify commands like play, pause, next, previous, seek, repeat, and shuffle
#[post("/command/<command>", data = "<args>")]
pub fn spotify_command(command: &str, args: Json<Value>) -> Json<ApiResponse> {
    let spotify = Spotify::new();
    match spotify.send_command(command, &args.0) {
        Ok(_) => Json(ApiResponse {
            status: "success".to_string(),
            message: format!("Command '{}' sent successfully", command),
            expires_at: None,
        }),
        Err(e) => {
            error!("Spotify command error: {}", e);
            Json(ApiResponse {
                status: "error".to_string(),
                message: format!("Command failed: {}", e),
                expires_at: None,
            })
        }
    }
}
