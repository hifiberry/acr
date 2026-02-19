// Spotify API module for managing authentication and tokens

use rocket::serde::{json::Json, Deserialize, Serialize};
use rocket::post;
use rocket::get;
use log::{error, info};
use std::time::{SystemTime, UNIX_EPOCH};
use serde_json::json;

use crate::helpers::spotify::{Spotify, SpotifyTokens};
use crate::helpers::http_client::new_http_client;
use rocket::http::{Status};
use rocket::response::content;
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

#[derive(Debug, Serialize, Deserialize)]
pub struct SearchRequest {
    pub query: String,
    pub types: Vec<String>,
    pub filters: Option<serde_json::Value>,
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
    let redirect_uri = "/example/spotify.html".to_string();
    
    Json(OAuthConfig {
        oauth_url: spotify.get_oauth_url().to_string(),
        redirect_uri,
    })
}

/// Create a new OAuth session
#[get("/create_session")]
pub fn create_session() -> Result<Json<CreateSessionResponse>, Status> {
    let spotify = Spotify::new();
    let http_client = new_http_client(10);
    // Use the helper to build the correct URL with scopes
    let url = spotify.build_create_session_url();
    let headers = spotify.build_oauth_headers();
    let headers_ref: Vec<(&str, &str)> = headers.iter().map(|(k, v)| (*k, v.as_str())).collect();
    match http_client.get_json_with_headers(&url, &headers_ref) {
        Ok(response) => {
            match response.get("session_id").and_then(|id| id.as_str()) {
                Some(session_id) => Ok(Json(CreateSessionResponse { session_id: session_id.to_string() })),
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
    let base_url = spotify.get_oauth_url();
    let url = if base_url.ends_with('/') {
        format!("{}login/{}", base_url, session_id)
    } else {
        format!("{}/login/{}", base_url, session_id)
    };
    let agent = ureq::AgentBuilder::new().redirects(0).build();
    let mut request = agent.get(&url).timeout(std::time::Duration::from_secs(10));
    for (name, value) in spotify.build_oauth_headers() {
        request = request.set(name, &value);
    }
    let result = request.call();
    match result {
        Err(ureq::Error::Status(code, response)) if (300..400).contains(&code) => {
            if let Some(location) = response.header("Location") {
                let decoded_location = location
                    .replace("&amp;", "&")
                    .replace("&quot;", "\"")
                    .replace("&lt;", "<")
                    .replace("&gt;", ">\\");
                Ok(Json(ApiResponse {
                    status: "redirect".to_string(),
                    message: decoded_location,
                    expires_at: None,
                }))
            } else {
                error!("OAuth server returned a redirect without Location header");
                Err(Status::InternalServerError)
            }
        },
        Ok(response) => {
            let status_code = response.status();
            if let Ok(body) = response.into_string() {
                if body.contains("spotify.com/authorize") || body.contains("accounts.spotify.com") {
                    if let Some(url_start) = body.find("https://accounts.spotify.com") {
                        if let Some(url_end) = body[url_start..].find('"') {
                            let spotify_url = &body[url_start..(url_start + url_end)];
                            let decoded_url = spotify_url
                                .replace("&amp;", "&")
                                .replace("&quot;", "\"")
                                .replace("&lt;", "<")
                                .replace("&gt;", ">\\");
                            return Ok(Json(ApiResponse {
                                status: "redirect".to_string(),
                                message: decoded_url,
                                expires_at: None,
                            }));
                        }
                    }
                    info!("Found Spotify references but couldn't extract the exact URL");
                }
                info!("Got response body of length {} with status {}", body.len(), status_code);
                Ok(Json(ApiResponse {
                    status: "success".to_string(),
                    message: "Login request processed".to_string(),
                    expires_at: None,
                }))
            } else {
                error!("Could not read response body");
                Err(Status::InternalServerError)
            }
        },
        Err(ureq::Error::Status(code, response)) => {
            let error_body = response.into_string().unwrap_or_else(|_| "<failed to read response body>".to_string());
            error!("OAuth server returned error {}: {}", code, error_body);
            Err(Status::InternalServerError)
        },
        Err(e) => {
            error!("Failed to proxy login request for session {}: {}", session_id, e);
            Err(Status::InternalServerError)
        }
    }
}

/// Poll for token data
#[get("/poll/<session_id>")]
pub fn poll_session(session_id: String) -> Result<Json<Value>, Status> {
    let spotify = Spotify::new();
    let http_client = new_http_client(10);
    let base_url = spotify.get_oauth_url();
    let url = if base_url.ends_with('/') {
        format!("{}poll/{}", base_url, session_id)
    } else {
        format!("{}/poll/{}", base_url, session_id)
    };
    let headers = spotify.build_oauth_headers();
    let headers_ref: Vec<(&str, &str)> = headers.iter().map(|(k, v)| (*k, v.as_str())).collect();
    match http_client.get_json_with_headers(&url, &headers_ref) {
        Ok(data) => Ok(Json(data)),
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

/// Get currently playing track information
#[get("/currently_playing")]
pub fn spotify_currently_playing() -> Json<Value> {
    let spotify = Spotify::new();
    match spotify.get_currently_playing() {
        Ok(Some(json)) => Json(json),
        Ok(None) => Json(json!({"status": "no_track"})),
        Err(e) => Json(json!({"status": "error", "message": format!("{}", e)})),
    }
}

/// Search for Spotify content (tracks, albums, artists, playlists)
#[post("/search", data = "<request>")]
pub fn spotify_search(request: Json<SearchRequest>) -> Json<Value> {
    let spotify = Spotify::new();
    let types: Vec<&str> = request.types.iter().map(|s| s.as_str()).collect();
    match spotify.search(&request.query, &types, request.filters.as_ref()) {
        Ok(json) => Json(json),
        Err(e) => Json(json!({"status": "error", "message": format!("{}", e)})),
    }
}

/// Get the current Spotify access token as plain text
#[get("/access_token")]
pub fn get_access_token() -> Result<content::RawText<String>, Status> {
    let spotify = Spotify::new();
    
    match spotify.ensure_valid_token() {
        Ok(token) => {
            info!("Successfully retrieved Spotify access token");
            Ok(content::RawText(token))
        },
        Err(e) => {
            let error_msg = e.to_string();
            if error_msg.contains("Token not found") || error_msg.contains("not found") {
                error!("Spotify access token not found: {}", e);
                Err(Status::NotFound)
            } else {
                error!("Failed to get Spotify access token: {}", e);
                Err(Status::InternalServerError)
            }
        }
    }
}
