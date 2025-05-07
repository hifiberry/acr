use crate::AudioController;
use crate::data::{Album, Artist};
use rocket::serde::json::Json;
use rocket::{get, State};
use std::sync::Arc;
use rocket::response::status::Custom;
use rocket::http::Status;
use serde::{Serialize, Serializer};

/// Response structure for library information
#[derive(serde::Serialize)]
pub struct LibraryResponse {
    player_name: String,
    player_id: String,
    has_library: bool,
    is_loaded: bool,
    albums_count: usize,
    artists_count: usize,
}

/// Response structure for library list - lists all players with library info
#[derive(serde::Serialize)]
pub struct LibraryListResponse {
    players: Vec<LibraryPlayerInfo>,
}

/// Player information with library status
#[derive(serde::Serialize)]
pub struct LibraryPlayerInfo {
    player_name: String,
    player_id: String,
    has_library: bool,
    is_loaded: bool,
}

/// Response structure for albums list
#[derive(serde::Serialize)]
pub struct AlbumsResponse {
    player_name: String,
    count: usize,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    albums: Vec<Album>,
}

/// Enhanced artist information with album count
#[derive(Serialize)]
struct EnhancedArtist<'a> {
    /// Reference to the original artist
    #[serde(flatten)]
    artist: &'a Artist,
    /// Number of albums associated with this artist
    albums_count: usize,
}

/// Response structure for artists list
#[derive(serde::Serialize)]
pub struct ArtistsResponse<'a> {
    player_name: String,
    count: usize,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    artists: Vec<EnhancedArtist<'a>>,
}

/// Response structure for a single artist
#[derive(serde::Serialize)]
pub struct ArtistResponse {
    player_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    artist: Option<Artist>,
}

/// Response structure for a single album with conditional track inclusion
#[derive(serde::Serialize)]
pub struct AlbumResponse {
    player_name: String,
    include_tracks: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    album: Option<Album>,
}

/// Response structure for albums by artist with conditional track inclusion
#[derive(serde::Serialize)]
pub struct ArtistAlbumsResponse {
    player_name: String,
    artist_name: String,
    count: usize,
    include_tracks: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    albums: Vec<Album>,
}

/// Custom response structure for artist data with specific field order
#[derive(serde::Serialize)]
struct ArtistCustomResponse {
    name: String,
    id: String,
    is_multi: bool,
    album_count: usize,
    thumb_url: Vec<String>,
}

/// List all players with library information
#[get("/library")]
pub fn list_libraries(controller: &State<Arc<AudioController>>) -> Json<LibraryListResponse> {
    let controllers = controller.inner().list_controllers();
    let mut players = Vec::new();
    
    // Iterate through all controllers and check their library status
    for ctrl_lock in controllers {
        if let Ok(ctrl) = ctrl_lock.read() {
            let player_name = ctrl.get_player_name();
            let player_id = ctrl.get_player_id();
            let library = ctrl.get_library();
            
            // Determine library status
            let (has_library, is_loaded) = match &library {
                Some(lib) => (true, lib.is_loaded()),
                None => (false, false),
            };
            
            // Add player info to the list
            players.push(LibraryPlayerInfo {
                player_name,
                player_id,
                has_library,
                is_loaded,
            });
        }
    }
    
    Json(LibraryListResponse { players })
}

/// Get library information for a player
#[get("/library/<player_name>")]
pub fn get_library_info(player_name: &str, controller: &State<Arc<AudioController>>) -> Result<Json<LibraryResponse>, Custom<Json<LibraryResponse>>> {
    let controllers = controller.inner().list_controllers();
    
    // Find the controller with the matching name
    for ctrl_lock in controllers {
        if let Ok(ctrl) = ctrl_lock.read() {
            if ctrl.get_player_name() == player_name {
                // Check if the player has a library
                if let Some(library) = ctrl.get_library() {
                    // Get basic library info
                    let is_loaded = library.is_loaded();
                    let albums = library.get_albums();
                    let artists = library.get_artists();
                    
                    return Ok(Json(LibraryResponse {
                        player_name: player_name.to_string(),
                        player_id: ctrl.get_player_id(),
                        has_library: true,
                        is_loaded,
                        albums_count: albums.len(),
                        artists_count: artists.len(),
                    }));
                } else {
                    // Player exists but doesn't have a library
                    return Err(Custom(
                        Status::NotFound,
                        Json(LibraryResponse {
                            player_name: player_name.to_string(),
                            player_id: ctrl.get_player_id(),
                            has_library: false,
                            is_loaded: false,
                            albums_count: 0,
                            artists_count: 0,
                        }),
                    ));
                }
            }
        }
    }
    
    // Player not found
    Err(Custom(
        Status::NotFound,
        Json(LibraryResponse {
            player_name: player_name.to_string(),
            player_id: "unknown".to_string(),
            has_library: false,
            is_loaded: false,
            albums_count: 0,
            artists_count: 0,
        }),
    ))
}

/// Get all albums for a player
/// 
/// Optional query parameter:
/// - include_tracks: When set to "true", includes track data for each album
#[get("/library/<player_name>/albums?<include_tracks>")]
pub fn get_player_albums(
    player_name: &str, 
    include_tracks: Option<bool>,
    controller: &State<Arc<AudioController>>
) -> Result<Json<AlbumsResponse>, Custom<String>> {
    let controllers = controller.inner().list_controllers();
    
    // Find the controller with the matching name
    for ctrl_lock in controllers {
        if let Ok(ctrl) = ctrl_lock.read() {
            if ctrl.get_player_name() == player_name {
                // Check if the player has a library
                if let Some(library) = ctrl.get_library() {
                    // Get all albums
                    let mut albums = library.get_albums();
                    
                    // If include_tracks is not set to true, remove tracks from albums
                    if include_tracks != Some(true) {
                        for album in &mut albums {
                            // Clear the tracks to reduce response size
                            if let Ok(mut tracks) = album.tracks.lock() {
                                tracks.clear();
                            }
                        }
                    }
                    
                    return Ok(Json(AlbumsResponse {
                        player_name: player_name.to_string(),
                        count: albums.len(),
                        albums,
                    }));
                } else {
                    // Player exists but doesn't have a library
                    return Err(Custom(
                        Status::NotFound,
                        format!("Player '{}' does not have a library", player_name),
                    ));
                }
            }
        }
    }
    
    // Player not found
    Err(Custom(
        Status::NotFound,
        format!("Player '{}' not found", player_name),
    ))
}

/// Get all artists for a player
#[get("/library/<player_name>/artists")]
pub fn get_player_artists(
    player_name: &str,
    controller: &State<Arc<AudioController>>
) -> Result<Json<serde_json::Value>, Custom<String>> {
    let controllers = controller.inner().list_controllers();
    
    // Find the controller with the matching name
    for ctrl_lock in controllers {
        if let Ok(ctrl) = ctrl_lock.read() {
            if ctrl.get_player_name() == player_name {
                // Check if the player has a library
                if let Some(library) = ctrl.get_library() {
                    // Get all artists
                    let mut artists = library.get_artists();
                    
                    // Sort artists by name
                    artists.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
                    
                    // Create a custom JSON response with only the required fields
                    let mut artists_json = Vec::with_capacity(artists.len());
                    
                    for artist in &artists {
                        // Get albums for this artist by name to determine the count
                        let albums = library.get_albums_by_artist(&artist.name);
                        let album_count = albums.len();
                        
                        // Extract all thumbnail URLs from metadata if available
                        let thumb_urls = artist.metadata.as_ref()
                            .map(|meta| meta.thumb_url.clone())
                            .unwrap_or_default();
                        
                        // Create a struct with fields in the specific order
                        let artist_data = ArtistCustomResponse {
                            name: artist.name.clone(),
                            id: artist.id.to_string(),
                            is_multi: artist.is_multi,
                            album_count,
                            thumb_url: thumb_urls,
                        };
                        
                        // Convert to serde_json::Value to include in the response
                        if let Ok(json_value) = serde_json::to_value(artist_data) {
                            artists_json.push(json_value);
                        }
                    }
                    
                    // Build the final response
                    let response = serde_json::json!({
                        "player_name": player_name,
                        "count": artists.len(),
                        "artists": artists_json
                    });
                    
                    return Ok(Json(response));
                } else {
                    // Player exists but doesn't have a library
                    return Err(Custom(
                        Status::NotFound,
                        format!("Player '{}' does not have a library", player_name),
                    ));
                }
            }
        }
    }
    
    // Player not found
    Err(Custom(
        Status::NotFound,
        format!("Player '{}' not found", player_name),
    ))
}

/// Get a specific album by name
/// 
/// This endpoint always includes track data for the album
#[get("/library/<player_name>/album/by-name/<album_name>")]
pub fn get_album_by_name(
    player_name: &str, 
    album_name: &str,
    controller: &State<Arc<AudioController>>
) -> Result<Json<AlbumResponse>, Custom<String>> {
    get_album_internal(player_name, album_name, controller, false)
}

/// Get a specific album by ID
/// 
/// This endpoint always includes track data for the album
#[get("/library/<player_name>/album/by-id/<album_id>")]
pub fn get_album_by_id(
    player_name: &str, 
    album_id: &str,
    controller: &State<Arc<AudioController>>
) -> Result<Json<AlbumResponse>, Custom<String>> {
    get_album_internal(player_name, album_id, controller, true)
}

/// Internal function to handle album lookup by either name or ID
/// 
/// This function abstracts the common logic for both endpoints
fn get_album_internal(
    player_name: &str,
    identifier: &str,
    controller: &State<Arc<AudioController>>,
    is_id_lookup: bool
) -> Result<Json<AlbumResponse>, Custom<String>> {
    let controllers = controller.inner().list_controllers();
    
    // Find the controller with the matching name
    for ctrl_lock in controllers {
        if let Ok(ctrl) = ctrl_lock.read() {
            if ctrl.get_player_name() == player_name {
                // Check if the player has a library
                if let Some(library) = ctrl.get_library() {
                    // Get the album by name or ID depending on the lookup type
                    let album = if is_id_lookup {
                        // Try to parse the ID as u64
                        match identifier.parse::<u64>() {
                            Ok(id) => library.get_album_by_id(id),
                            Err(_) => {
                                return Err(Custom(
                                    Status::BadRequest,
                                    format!("Invalid album ID format: {}", identifier),
                                ));
                            }
                        }
                    } else {
                        // Get album by name
                        library.get_album(identifier)
                    };
                    
                    return Ok(Json(AlbumResponse {
                        player_name: player_name.to_string(),
                        album,
                        include_tracks: true, // Always include tracks
                    }));
                } else {
                    // Player exists but doesn't have a library
                    return Err(Custom(
                        Status::NotFound,
                        format!("Player '{}' does not have a library", player_name),
                    ));
                }
            }
        }
    }
    
    // Player not found
    Err(Custom(
        Status::NotFound,
        format!("Player '{}' not found", player_name),
    ))
}

/// Get all albums by a specific artist
/// 
/// Optional query parameter:
/// - include_tracks: When set to "true", includes track data for each album
#[get("/library/<player_name>/albums/by-artist/<artist_name>?<include_tracks>")]
pub fn get_albums_by_artist(
    player_name: &str, 
    artist_name: &str,
    include_tracks: Option<bool>,
    controller: &State<Arc<AudioController>>
) -> Result<Json<ArtistAlbumsResponse>, Custom<String>> {
    let controllers = controller.inner().list_controllers();
    
    // Find the controller with the matching name
    for ctrl_lock in controllers {
        if let Ok(ctrl) = ctrl_lock.read() {
            if ctrl.get_player_name() == player_name {
                // Check if the player has a library
                if let Some(library) = ctrl.get_library() {
                    // Get albums by artist
                    let mut albums = library.get_albums_by_artist(artist_name);
                    
                    // If include_tracks is not set to true, remove tracks from albums
                    let include_tracks_flag = include_tracks == Some(true);
                    
                    if !include_tracks_flag {
                        for album in &mut albums {
                            // Clear the tracks to reduce response size
                            if let Ok(mut tracks) = album.tracks.lock() {
                                tracks.clear();
                            }
                        }
                    }
                    
                    return Ok(Json(ArtistAlbumsResponse {
                        player_name: player_name.to_string(),
                        artist_name: artist_name.to_string(),
                        count: albums.len(),
                        albums,
                        include_tracks: include_tracks_flag,
                    }));
                } else {
                    // Player exists but doesn't have a library
                    return Err(Custom(
                        Status::NotFound,
                        format!("Player '{}' does not have a library", player_name),
                    ));
                }
            }
        }
    }
    
    // Player not found
    Err(Custom(
        Status::NotFound,
        format!("Player '{}' not found", player_name),
    ))
}

/// Get all albums by a specific artist ID
/// 
/// Optional query parameter:
/// - include_tracks: When set to "true", includes track data for each album
#[get("/library/<player_name>/albums/by-artist-id/<artist_id>?<include_tracks>")]
pub fn get_albums_by_artist_id(
    player_name: &str, 
    artist_id: &str,
    include_tracks: Option<bool>,
    controller: &State<Arc<AudioController>>
) -> Result<Json<ArtistAlbumsResponse>, Custom<String>> {
    let controllers = controller.inner().list_controllers();
    
    // Find the controller with the matching name
    for ctrl_lock in controllers {
        if let Ok(ctrl) = ctrl_lock.read() {
            if ctrl.get_player_name() == player_name {
                // Check if the player has a library
                if let Some(library) = ctrl.get_library() {
                    // Parse the artist ID
                    let artist_id_parsed = match artist_id.parse::<u64>() {
                        Ok(id) => id,
                        Err(_) => {
                            return Err(Custom(
                                Status::BadRequest,
                                format!("Invalid artist ID: {}", artist_id),
                            ));
                        }
                    };
                    
                    // Get albums by artist ID
                    let mut albums = library.get_albums_by_artist_id(artist_id_parsed);
                    
                    // If include_tracks is not set to true, remove tracks from albums
                    let include_tracks_flag = include_tracks == Some(true);
                    
                    if !include_tracks_flag {
                        for album in &mut albums {
                            // Clear the tracks to reduce response size
                            if let Ok(mut tracks) = album.tracks.lock() {
                                tracks.clear();
                            }
                        }
                    }
                    
                    // Try to find the artist name for better response
                    let artist_name = library.get_artists().into_iter()
                        .find(|artist| artist.id == artist_id_parsed)
                        .map_or_else(
                            || format!("Artist ID: {}", artist_id),
                            |artist| artist.name
                        );
                    
                    return Ok(Json(ArtistAlbumsResponse {
                        player_name: player_name.to_string(),
                        artist_name,
                        count: albums.len(),
                        albums,
                        include_tracks: include_tracks_flag,
                    }));
                } else {
                    // Player exists but doesn't have a library
                    return Err(Custom(
                        Status::NotFound,
                        format!("Player '{}' does not have a library", player_name),
                    ));
                }
            }
        }
    }
    
    // Player not found
    Err(Custom(
        Status::NotFound,
        format!("Player '{}' not found", player_name),
    ))
}

/// Refresh the library for a player
#[get("/library/<player_name>/refresh")]
pub fn refresh_player_library(player_name: &str, controller: &State<Arc<AudioController>>) -> Result<Json<LibraryResponse>, Custom<String>> {
    let controllers = controller.inner().list_controllers();
    
    // Find the controller with the matching name
    for ctrl_lock in controllers {
        if let Ok(ctrl) = ctrl_lock.read() {
            if ctrl.get_player_name() == player_name {
                // Check if the player has a library
                if let Some(library) = ctrl.get_library() {
                    // Trigger library refresh
                    match library.refresh_library() {
                        Ok(_) => {
                            // Get updated library info
                            let is_loaded = library.is_loaded();
                            let albums = library.get_albums();
                            let artists = library.get_artists();
                            
                            return Ok(Json(LibraryResponse {
                                player_name: player_name.to_string(),
                                player_id: ctrl.get_player_id(),
                                has_library: true,
                                is_loaded,
                                albums_count: albums.len(),
                                artists_count: artists.len(),
                            }));
                        },
                        Err(e) => {
                            return Err(Custom(
                                Status::InternalServerError,
                                format!("Failed to refresh library: {}", e),
                            ));
                        }
                    }
                } else {
                    // Player exists but doesn't have a library
                    return Err(Custom(
                        Status::NotFound,
                        format!("Player '{}' does not have a library", player_name),
                    ));
                }
            }
        }
    }
    
    // Player not found
    Err(Custom(
        Status::NotFound,
        format!("Player '{}' not found", player_name),
    ))
}

/// Get a specific artist by name
#[get("/library/<player_name>/artist/by-name/<artist_name>")]
pub fn get_artist_by_name(
    player_name: &str, 
    artist_name: &str,
    controller: &State<Arc<AudioController>>
) -> Result<Json<ArtistResponse>, Custom<String>> {
    get_artist_internal(player_name, artist_name, controller, ArtistLookupType::ByName)
}

/// Get a specific artist by ID
#[get("/library/<player_name>/artist/by-id/<artist_id>")]
pub fn get_artist_by_id(
    player_name: &str, 
    artist_id: &str,
    controller: &State<Arc<AudioController>>
) -> Result<Json<ArtistResponse>, Custom<String>> {
    get_artist_internal(player_name, artist_id, controller, ArtistLookupType::ById)
}

/// Get a specific artist by MusicBrainz ID (MBID)
#[get("/library/<player_name>/artist/by-mbid/<mbid>")]
pub fn get_artist_by_mbid(
    player_name: &str, 
    mbid: &str,
    controller: &State<Arc<AudioController>>
) -> Result<Json<ArtistResponse>, Custom<String>> {
    get_artist_internal(player_name, mbid, controller, ArtistLookupType::ByMbid)
}

/// Enum representing the different ways to look up an artist
enum ArtistLookupType {
    ByName,
    ById,
    ByMbid,
}

/// Internal function to handle artist lookup by name, ID, or MBID
/// 
/// This function abstracts the common logic for all artist endpoints
fn get_artist_internal(
    player_name: &str,
    identifier: &str,
    controller: &State<Arc<AudioController>>,
    lookup_type: ArtistLookupType
) -> Result<Json<ArtistResponse>, Custom<String>> {
    let controllers = controller.inner().list_controllers();
    
    // Find the controller with the matching name
    for ctrl_lock in controllers {
        if let Ok(ctrl) = ctrl_lock.read() {
            if ctrl.get_player_name() == player_name {
                // Check if the player has a library
                if let Some(library) = ctrl.get_library() {
                    // Get the artist based on the lookup type
                    let artist = match lookup_type {
                        ArtistLookupType::ByName => {
                            // Get artist by name
                            library.get_artist(identifier)
                        },
                        ArtistLookupType::ById => {
                            // Try to parse the ID as u64
                            match identifier.parse::<u64>() {
                                Ok(id) => {
                                    // Find artist with matching ID
                                    let all_artists = library.get_artists();
                                    all_artists.into_iter().find(|a| a.id == id)
                                },
                                Err(_) => {
                                    return Err(Custom(
                                        Status::BadRequest,
                                        format!("Invalid artist ID format: {}", identifier),
                                    ));
                                }
                            }
                        },
                        ArtistLookupType::ByMbid => {
                            // Find artist with matching MBID
                            let all_artists = library.get_artists();
                            all_artists.into_iter().find(|a| {
                                if let Some(meta) = &a.metadata {
                                    meta.mbid.iter().any(|id| id == identifier)
                                } else {
                                    false
                                }
                            })
                        }
                    };
                    
                    return Ok(Json(ArtistResponse {
                        player_name: player_name.to_string(),
                        artist,
                    }));
                } else {
                    // Player exists but doesn't have a library
                    return Err(Custom(
                        Status::NotFound,
                        format!("Player '{}' does not have a library", player_name),
                    ));
                }
            }
        }
    }
    
    // Player not found
    Err(Custom(
        Status::NotFound,
        format!("Player '{}' not found", player_name),
    ))
}