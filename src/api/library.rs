use crate::AudioController;
use crate::data::{Album, Artist};
use rocket::serde::json::Json;
use rocket::{get, post, State};
use std::sync::Arc;
use rocket::response::status::Custom;
use rocket::http::Status;
use serde::Serialize;

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

/// Response structure for library metadata
#[derive(serde::Serialize)]
pub struct MetadataResponse {
    player_name: String,
    metadata: std::collections::HashMap<String, serde_json::Value>,
}

/// Response structure for a single metadata key-value pair
#[derive(serde::Serialize)]
pub struct MetadataKeyResponse {
    player_name: String,
    key: String,
    value: Option<serde_json::Value>,
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

/// Response structure for albums list using the DTO model
#[derive(serde::Serialize)]
pub struct AlbumsDTOResponse {
    player_name: String,
    count: usize,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    albums: Vec<AlbumDTO>,
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

/// Response structure for a single album (always includes tracks)
#[derive(serde::Serialize)]
pub struct AlbumResponse {
    player_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    album: Option<Album>,
}

/// Response structure for a single album using the DTO model
#[derive(serde::Serialize)]
pub struct AlbumDTOResponse {
    player_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    album: Option<AlbumDTO>,
}

/// Response structure for albums by artist (without tracks)
#[derive(serde::Serialize)]
pub struct ArtistAlbumsResponse {
    player_name: String,
    artist_name: String,
    count: usize,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    albums: Vec<Album>,
}

/// Response structure for albums by artist using the DTO model
#[derive(serde::Serialize)]
pub struct ArtistAlbumsDTOResponse {
    player_name: String,
    artist_name: String,
    count: usize,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    albums: Vec<AlbumDTO>,
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

/// Data Transfer Object for Album to include tracks_count without modifying Album struct
#[derive(serde::Serialize)]
struct AlbumDTO {
    id: String,
    name: String,
    artists: Vec<String>,
    release_date: Option<chrono::NaiveDate>,
    tracks_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    tracks: Option<Vec<crate::data::track::Track>>,
    cover_art: Option<String>,
    uri: Option<String>,
}

impl From<Album> for AlbumDTO {
    fn from(album: Album) -> Self {
        // Get the tracks for counting and optional inclusion
        let tracks_lock = album.tracks.lock();

        let tracks_count = tracks_lock.len();
        let tracks_clone = Some(tracks_lock.clone());

        // Get artists
        let artists = album.artists.lock().clone();

        // Drop the lock before returning
        drop(tracks_lock);
        
        AlbumDTO {
            id: album.id.to_string(),
            name: album.name,
            artists,
            release_date: album.release_date,
            tracks_count,
            tracks: tracks_clone,
            cover_art: album.cover_art,
            uri: album.uri,
        }
    }
}

/// Creates an AlbumDTO from an Album with optional track inclusion
fn create_album_dto(album: Album, include_tracks: bool) -> AlbumDTO {
    let mut dto = AlbumDTO::from(album);
    
    // If we don't want to include tracks, set to None
    if !include_tracks {
        dto.tracks = None;
    }
    
    dto
}

/// List all players with library information
#[get("/library")]
pub fn list_libraries(controller: &State<Arc<AudioController>>) -> Json<LibraryListResponse> {
    let controllers = controller.inner().list_controllers();
    let mut players = Vec::new();
    
    // Iterate through all controllers and check their library status
    for ctrl_lock in controllers {
        let ctrl = ctrl_lock.read();
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
    
    Json(LibraryListResponse { players })
}

/// Get library information for a player
#[get("/library/<player_name>")]
pub fn get_library_info(player_name: &str, controller: &State<Arc<AudioController>>) -> Result<Json<LibraryResponse>, Custom<Json<LibraryResponse>>> {
    let controllers = controller.inner().list_controllers();
    
    // Find the controller with the matching name
    for ctrl_lock in controllers {
        let ctrl = ctrl_lock.read();
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
/// This endpoint returns albums without track data but includes track count
#[get("/library/<player_name>/albums")]
pub fn get_player_albums(
    player_name: &str,
    controller: &State<Arc<AudioController>>
) -> Result<Json<AlbumsDTOResponse>, Custom<String>> {
    let controllers = controller.inner().list_controllers();
    
    // Find the controller with the matching name
    for ctrl_lock in controllers {
        let ctrl = ctrl_lock.read();
        if ctrl.get_player_name() == player_name {
            // Check if the player has a library
            if let Some(library) = ctrl.get_library() {
                // Get all albums
                let albums = library.get_albums();

                // Convert albums to DTOs without including tracks
                let album_dtos = albums.into_iter()
                    .map(|album| create_album_dto(album, false))
                    .collect::<Vec<AlbumDTO>>();

                return Ok(Json(AlbumsDTOResponse {
                    player_name: player_name.to_string(),
                    count: album_dtos.len(),
                    albums: album_dtos,
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
        let ctrl = ctrl_lock.read();
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
                    let albums = library.get_albums_by_artist_id(&artist.id);
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

    // Player not found
    Err(Custom(
        Status::NotFound,
        format!("Player '{}' not found", player_name),
    ))
}

/// Get a specific album by ID
/// 
/// This endpoint always includes track data for the album
#[get("/library/<player_name>/album/by-id/<album_id>")]
pub fn get_album_by_id(
    player_name: &str, 
    album_id: &str,
    controller: &State<Arc<AudioController>>
) -> Result<Json<AlbumDTOResponse>, Custom<String>> {
    let controllers = controller.inner().list_controllers();
    
    // Find the controller with the matching name
    for ctrl_lock in controllers {
        let ctrl = ctrl_lock.read();
        if ctrl.get_player_name() == player_name {
            // Check if the player has a library
            if let Some(library) = ctrl.get_library() {
                // Create identifier based on album_id format
                let identifier = if let Ok(id) = album_id.parse::<u64>() {
                    crate::data::Identifier::Numeric(id)
                } else {
                    crate::data::Identifier::String(album_id.to_string())
                };
                
                // Get the album by ID
                let album_option = library.get_album_by_id(&identifier);
                
                // Convert album to DTO with tracks included
                let album_dto = album_option.map(|album| create_album_dto(album, true));
                
                return Ok(Json(AlbumDTOResponse {
                    player_name: player_name.to_string(),
                    album: album_dto,
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

    // Player not found
    Err(Custom(
        Status::NotFound,
        format!("Player '{}' not found", player_name),
    ))
}

/// Get all albums by a specific artist
/// 
/// This endpoint returns albums without track data but includes track count
#[get("/library/<player_name>/albums/by-artist/<artist_name>")]
pub fn get_albums_by_artist(
    player_name: &str, 
    artist_name: &str,
    controller: &State<Arc<AudioController>>
) -> Result<Json<ArtistAlbumsDTOResponse>, Custom<String>> {
    let controllers = controller.inner().list_controllers();
    
    // Find the controller with the matching name
    for ctrl_lock in controllers {
        let ctrl = ctrl_lock.read();
        if ctrl.get_player_name() == player_name {
            // Check if the player has a library
            if let Some(library) = ctrl.get_library() {
                // First get the artist to get their ID
                if let Some(artist) = library.get_artist_by_name(artist_name) {
                    // Get albums by artist ID
                    let albums = library.get_albums_by_artist_id(&artist.id);
                    
                    // Convert albums to DTOs without including tracks
                    let album_dtos = albums.into_iter()
                        .map(|album| create_album_dto(album, false))
                        .collect::<Vec<AlbumDTO>>();
                    
                    return Ok(Json(ArtistAlbumsDTOResponse {
                        player_name: player_name.to_string(),
                        artist_name: artist_name.to_string(),
                        count: album_dtos.len(),
                        albums: album_dtos,
                    }));
                } else {
                    // Artist not found
                    return Err(Custom(
                        Status::NotFound,
                        format!("Artist '{}' not found", artist_name),
                    ));
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

    // Player not found
    Err(Custom(
        Status::NotFound,
        format!("Player '{}' not found", player_name),
    ))
}

/// Get all albums by a specific artist ID
/// 
/// This endpoint returns albums without track data but includes track count
#[get("/library/<player_name>/albums/by-artist-id/<artist_id>")]
pub fn get_albums_by_artist_id(
    player_name: &str, 
    artist_id: &str,
    controller: &State<Arc<AudioController>>
) -> Result<Json<ArtistAlbumsDTOResponse>, Custom<String>> {
    let controllers = controller.inner().list_controllers();
    
    // Find the controller with the matching name
    for ctrl_lock in controllers {
        let ctrl = ctrl_lock.read();
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
                
                // Create Identifier and get albums by artist ID
                let artist_id_identifier = crate::data::Identifier::Numeric(artist_id_parsed);
                let albums = library.get_albums_by_artist_id(&artist_id_identifier);
                
                // Convert albums to DTOs without including tracks
                let album_dtos = albums.into_iter()
                    .map(|album| create_album_dto(album, false))
                    .collect::<Vec<AlbumDTO>>();
                
                // Try to find the artist name for better response
                let artist_name = library.get_artists().into_iter()
                    .find(|artist| artist.id == crate::data::Identifier::Numeric(artist_id_parsed))
                    .map_or_else(
                        || format!("Artist ID: {}", artist_id),
                        |artist| artist.name
                    );
                
                return Ok(Json(ArtistAlbumsDTOResponse {
                    player_name: player_name.to_string(),
                    artist_name,
                    count: album_dtos.len(),
                    albums: album_dtos,
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
        let ctrl = ctrl_lock.read();
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

    // Player not found
    Err(Custom(
        Status::NotFound,
        format!("Player '{}' not found", player_name),
    ))
}

/// Force an update of the underlying library in the player system
/// 
/// This endpoint tells the player to scan for new or changed files, which
/// may trigger a media database update in the backend system.
#[post("/library/<player_name>/update")]
pub fn update_player_library(
    player_name: &str, 
    controller: &State<Arc<AudioController>>
) -> Result<Json<serde_json::Value>, Custom<String>> {
    let controllers = controller.inner().list_controllers();
    
    // Find the controller with the matching name
    for ctrl_lock in controllers {
        let ctrl = ctrl_lock.read();
        if ctrl.get_player_name() == player_name {
            // Check if the player has a library
            if let Some(library) = ctrl.get_library() {
                // Force an update of the library
                let success = library.force_update();
                
                // Return the result
                return Ok(Json(serde_json::json!({
                    "player_name": player_name,
                    "update_started": success
                })));
            } else {
                // Player exists but doesn't have a library
                return Err(Custom(
                    Status::NotFound,
                    format!("Player '{}' does not have a library", player_name),
                ));
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
        let ctrl = ctrl_lock.read();
        if ctrl.get_player_name() == player_name {
            // Check if the player has a library
            if let Some(library) = ctrl.get_library() {
                // Get the artist based on the lookup type
                let artist = match lookup_type {
                    ArtistLookupType::ByName => {
                        // Get artist by name
                        library.get_artist_by_name(identifier)
                    },
                    ArtistLookupType::ById => {
                        // Try to parse the ID as u64
                        match identifier.parse::<u64>() {
                            Ok(id) => {
                                // Find artist with matching ID
                                let all_artists = library.get_artists();
                                all_artists.into_iter().find(|a| a.id == crate::data::Identifier::Numeric(id))
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

    // Player not found
    Err(Custom(
        Status::NotFound,
        format!("Player '{}' not found", player_name),
     ))
}

/// Retrieve an image from the library based on an identifier
/// 
/// This endpoint maps directly to the library's get_image function, allowing
/// access to image data like album covers and artist images through the REST API.
/// The identifier format depends on the library implementation, but typically
/// supports formats like "album:123" for album covers and "artist:Artist Name" for artist images.
#[get("/library/<player_name>/image/<identifier>")]
pub fn get_image(
    player_name: &str,
    identifier: &str,
    controller: &State<Arc<AudioController>>
) -> Result<(rocket::http::ContentType, Vec<u8>), Custom<String>> {
    let controllers = controller.inner().list_controllers();
    
    // Find the controller with the matching name
    for ctrl_lock in controllers {
        let ctrl = ctrl_lock.read();
        if ctrl.get_player_name() == player_name {
            // Check if the player has a library
            if let Some(library) = ctrl.get_library() {
                // Call the library's get_image function
                if let Some((data, mime_type)) = library.get_image(identifier.to_string()) {
                    // Extract MIME type components
                    let media_type = mime_type.split('/').next().unwrap_or("application").to_string();
                    let media_subtype = mime_type.split('/').nth(1).unwrap_or("octet-stream").to_string();
                    
                    // Create a ContentType object
                    let content_type = rocket::http::ContentType::new(media_type, media_subtype);
                    
                    // Return the content type paired with data, which implements Responder
                    return Ok((content_type, data));
                } else {
                    // Image not found
                    return Err(Custom(
                        Status::NotFound,
                        format!("Image with identifier '{}' not found", identifier),
                    ));
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

    // Player not found
    Err(Custom(
        Status::NotFound,
        format!("Player '{}' not found", player_name),
      ))
}

/// Get all metadata for a player's library
#[get("/library/<player_name>/meta")]
pub fn get_library_metadata(
    player_name: &str,
    controller: &State<Arc<AudioController>>
) -> Result<Json<MetadataResponse>, Custom<String>> {
    let controllers = controller.inner().list_controllers();
    
    // Find the controller with the matching name
    for ctrl_lock in controllers {
        let ctrl = ctrl_lock.read();
        if ctrl.get_player_name() == player_name {
            // Check if the player has a library
            if let Some(library) = ctrl.get_library() {
                // Get all metadata as a HashMap
                let metadata = library.get_metadata()
                    .unwrap_or_default();
                
                return Ok(Json(MetadataResponse {
                    player_name: player_name.to_string(),
                    metadata,
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

    // Player not found
    Err(Custom(
        Status::NotFound,
        format!("Player '{}' not found", player_name),
    ))
}

/// Get a specific metadata key for a player's library
#[get("/library/<player_name>/meta/<key>")]
pub fn get_library_metadata_key(
    player_name: &str,
    key: &str,
    controller: &State<Arc<AudioController>>
) -> Result<Json<MetadataKeyResponse>, Custom<String>> {
    let controllers = controller.inner().list_controllers();
    
    // Find the controller with the matching name
    for ctrl_lock in controllers {
        let ctrl = ctrl_lock.read();
        if ctrl.get_player_name() == player_name {
            // Check if the player has a library
            if let Some(library) = ctrl.get_library() {
                // Get all metadata
                let metadata = library.get_metadata()
                    .unwrap_or_default();
                
                // Get the specific key
                let value = metadata.get(key).cloned();
                
                return Ok(Json(MetadataKeyResponse {
                    player_name: player_name.to_string(),
                    key: key.to_string(),
                    value,
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

    // Player not found
    Err(Custom(
        Status::NotFound,
        format!("Player '{}' not found", player_name),
    ))
}