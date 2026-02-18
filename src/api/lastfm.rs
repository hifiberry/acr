use crate::helpers::lastfm::{LASTFM_CLIENT, LastfmError, LovedTrack}; // Added LovedTrack
use log::{debug, error, info}; // Removed warn
use rocket::serde::json::Json;
use rocket::{get, post};
use serde::{Deserialize, Serialize};

// Unified AuthStatus struct (previously LastFmStatus and AuthStatus)
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct AuthStatus {
    pub authenticated: bool,
    pub username: Option<String>,
    pub error: Option<String>,
    pub error_description: Option<String>,
}

#[derive(Serialize)]
pub struct AuthUrlResponse {
    url: String,
    request_token: String,
    error: Option<String>,
}

#[derive(Deserialize)]
pub struct PrepareAuthRequest {
    token: String,
}

#[derive(Serialize)]
pub struct PrepareAuthResponse {
    success: bool,
    error: Option<String>,
}

/// Get Last.fm authentication status
///
/// Returns the current authentication state, including whether a user is authenticated,
/// their username if available, and any potential error information.
#[get("/status")] // Changed path to be consistent
pub fn get_status() -> Json<AuthStatus> {
    let client_guard = LASTFM_CLIENT.lock();
    match client_guard.as_ref() {
        Some(client) => {
            Json(AuthStatus {
                authenticated: client.is_authenticated(),
                username: client.get_username(),
                error: None,
                error_description: None,
            })
        }
        None => {
            error!("[get_status] Last.fm client not initialized");
            Json(AuthStatus {
                authenticated: false,
                username: None,
                error: Some("ClientNotInitialized".to_string()),
                error_description: Some("Last.fm client has not been initialized.".to_string()),
            })
        }
    }
}

/// Get Last.fm authentication URL
///
/// Initiates the authentication flow. The backend requests a temporary request token
/// from Last.fm and constructs a Last.fm authorization URL.
/// This URL is for the user to visit to authorize the application.
/// The temporary request token is also returned and should be stored by the frontend
/// to be sent back later in the `/prepare_complete_auth` step.
#[get("/auth")] // Changed path to be consistent
pub fn get_auth_url_handler() -> Json<AuthUrlResponse> { // Made synchronous
    let mut client_guard = LASTFM_CLIENT.lock(); // Lock and get guard
    match client_guard.as_mut() { // Get mutable reference to Option<LastfmClient>
        Some(client_ref) => { // client_ref is &mut LastfmClient
            match client_ref.get_auth_url() { // Call synchronous method
                Ok((url, token)) => {
                    debug!("[get_auth_url_handler] Generated Last.fm auth URL and request token: {}", token);
                    Json(AuthUrlResponse { url, request_token: token, error: None })
                }
                Err(e) => {
                    error!("[get_auth_url_handler] Failed to get auth URL: {}", e);
                    Json(AuthUrlResponse {
                        url: String::new(),
                        request_token: String::new(),
                        error: Some(format!("Failed to get auth URL: {}", e)),
                    })
                }
            }
        }
        None => {
            error!("[get_auth_url_handler] Last.fm client not initialized");
            Json(AuthUrlResponse {
                url: String::new(),
                request_token: String::new(),
                error: Some("ClientNotInitialized: Last.fm client has not been initialized.".to_string()),
            })
        }
    }
}

/// New endpoint to allow the frontend to set the request token on the backend
///
/// Prepare to complete Last.fm authentication by setting the request token.
///
/// After the user authorizes the application on Last.fm, the frontend
/// should call this endpoint, providing the temporary request token that was
/// initially obtained from the `/auth` endpoint.
/// This step stores the request token on the backend, making it ready to be
/// exchanged for a permanent session key in the `/complete_auth` step.
///
/// # Arguments
/// * `request_data`: JSON payload containing the `token` (the temporary request token).
#[post("/prepare_complete_auth", data = "<request_data>")] // Changed path
pub fn prepare_complete_auth(request_data: Json<PrepareAuthRequest>) -> Json<PrepareAuthResponse> {
    info!("[prepare_complete_auth] Received token from frontend: {}", request_data.token);
    let mut client_guard = LASTFM_CLIENT.lock();
    match client_guard.as_mut() {
        Some(client_ref) => {
            match client_ref.set_auth_token(request_data.token.clone()) {
                Ok(_) => {
                    debug!("[prepare_complete_auth] Successfully set auth token from frontend.");
                    Json(PrepareAuthResponse {
                        success: true,
                        error: None,
                    })
                }
                Err(e) => {
                    error!("[prepare_complete_auth] Failed to set auth token from frontend: {}", e);
                    Json(PrepareAuthResponse {
                        success: false,
                        error: Some(format!("Failed to set token: {}", e)),
                    })
                }
            }
        }
        None => {
            error!("[prepare_complete_auth] Last.fm client not initialized");
            Json(PrepareAuthResponse {
                success: false,
                error: Some("ClientNotInitialized: Last.fm client has not been initialized.".to_string()),
            })
        }
    }
}

/// Attempt to complete Last.fm authentication.
///
/// This endpoint finalizes the authentication process. It should be called after
/// `/prepare_complete_auth`.
/// The backend uses the previously stored temporary request token to request a
/// permanent session key from Last.fm. If successful, the session key and
/// username are stored securely, and the user is considered authenticated.
#[get("/complete_auth")] // Changed path
pub async fn complete_auth() -> Json<AuthStatus> { // Remains async for Rocket, but internal calls are sync
    let mut client_guard = LASTFM_CLIENT.lock(); // Lock and get guard
    match client_guard.as_mut() { // Get mutable reference
        Some(client_ref) => { // client_ref is &mut LastfmClient
            match client_ref.get_session() { // get_session is synchronous
                Ok((_session_key, username)) => {
                    debug!("[complete_auth] Successfully authenticated with Last.fm as {}", username);
                    Json(AuthStatus {
                        authenticated: true,
                        username: Some(username),
                        error: None,
                        error_description: None,
                    })
                }
                Err(e) => {
                    error!("[complete_auth] Error completing Last.fm auth: {:?}", e);
                    let (error_type, error_desc) = match &e {
                        LastfmError::ApiError(msg, 14) => { // Token not authorized
                            (Some("TokenNotAuthorized".to_string()), Some(msg.clone()))
                        }
                        LastfmError::ApiError(msg, _) => { // Other API error
                            (Some("ApiError".to_string()), Some(msg.clone()))
                        }
                        _ => (Some("AuthFailed".to_string()), Some(e.to_string())),
                    };

                    Json(AuthStatus {
                        authenticated: false,
                        username: None,
                        error: error_type,
                        error_description: error_desc,
                    })
                }
            }
        }
        None => {
            error!("[complete_auth] Last.fm client not initialized");
            Json(AuthStatus {
                authenticated: false,
                username: None,
                error: Some("ClientNotInitialized".to_string()),
                error_description: Some("Last.fm client has not been initialized.".to_string()),
            })
        }
    }
}

/// Disconnect the current user from Last.fm.
///
/// Clears the stored Last.fm session key and username from both memory and
/// the persistent security store. This effectively logs the user out of Last.fm
/// within the ACR application.
#[post("/disconnect")]
pub fn disconnect_handler() -> Json<AuthStatus> { // Made synchronous
    let mut client_guard = LASTFM_CLIENT.lock(); // Lock and get guard
    match client_guard.as_mut() { // Get mutable reference
        Some(client_ref) => { // client_ref is &mut LastfmClient
            match client_ref.disconnect() { // Call synchronous method
                Ok(_) => {
                    debug!("Successfully disconnected from Last.fm and cleared credentials.");
                    Json(AuthStatus {
                        authenticated: false,
                        username: None,
                        error: None,
                        error_description: None,
                    })
                }
                Err(e) => {
                    error!("Error during Last.fm disconnect: {}", e);
                    // Reflect the state of the client after the disconnect attempt.
                    // Since disconnect modifies client_ref directly, its state is current.
                    Json(AuthStatus {
                        authenticated: client_ref.is_authenticated(), 
                        username: client_ref.get_username(),    
                        error: Some("DisconnectError".to_string()),
                        error_description: Some(format!("Failed to disconnect: {}", e)),
                    })
                }
            }
        }
        None => {
            error!("Attempted to disconnect Last.fm, but client was not initialized.");
            Json(AuthStatus {
                authenticated: false,
                username: None,
                error: Some("ClientNotInitialized".to_string()),
                error_description: Some("Last.fm client not initialized. Cannot disconnect.".to_string()),
            })
        }
    }
}

#[derive(Serialize)]
pub struct LovedTracksResponse {
    tracks: Option<Vec<LovedTrack>>,
    error: Option<String>,
    error_description: Option<String>,
}