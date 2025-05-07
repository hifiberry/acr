use crate::AudioController;
use crate::data::{Album, Artist};
use rocket::serde::json::Json;
use rocket::{get, State};
use std::sync::Arc;
use rocket::response::status::Custom;
use rocket::http::Status;

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

/// Response structure for albums list
#[derive(serde::Serialize)]
pub struct AlbumsResponse {
    player_name: String,
    count: usize,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    albums: Vec<Album>,
}

/// Response structure for artists list
#[derive(serde::Serialize)]
pub struct ArtistsResponse {
    player_name: String,
    count: usize,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    artists: Vec<Artist>,
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
) -> Result<Json<ArtistsResponse>, Custom<String>> {
    let controllers = controller.inner().list_controllers();
    
    // Find the controller with the matching name
    for ctrl_lock in controllers {
        if let Ok(ctrl) = ctrl_lock.read() {
            if ctrl.get_player_name() == player_name {
                // Check if the player has a library
                if let Some(library) = ctrl.get_library() {
                    // Get all artists
                    let artists = library.get_artists();
                    
                    return Ok(Json(ArtistsResponse {
                        player_name: player_name.to_string(),
                        count: artists.len(),
                        artists,
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

/// Get a specific album by name
/// 
/// Optional query parameter:
/// - include_tracks: When set to "true", includes track data for the album
#[get("/library/<player_name>/album/<album_name>?<include_tracks>")]
pub fn get_album_by_name(
    player_name: &str, 
    album_name: &str,
    include_tracks: Option<bool>,
    controller: &State<Arc<AudioController>>
) -> Result<Json<AlbumResponse>, Custom<String>> {
    let controllers = controller.inner().list_controllers();
    
    // Find the controller with the matching name
    for ctrl_lock in controllers {
        if let Ok(ctrl) = ctrl_lock.read() {
            if ctrl.get_player_name() == player_name {
                // Check if the player has a library
                if let Some(library) = ctrl.get_library() {
                    // Get the album by name
                    let mut album = library.get_album(album_name);
                    
                    // If include_tracks is not set to true and we have an album, remove tracks
                    let include_tracks_flag = include_tracks == Some(true);
                    
                    if !include_tracks_flag {
                        if let Some(ref mut alb) = album {
                            // Clear the tracks to reduce response size
                            if let Ok(mut tracks) = alb.tracks.lock() {
                                tracks.clear();
                            }
                        }
                    }
                    
                    return Ok(Json(AlbumResponse {
                        player_name: player_name.to_string(),
                        album,
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

/// Get all albums by a specific artist
/// 
/// Optional query parameter:
/// - include_tracks: When set to "true", includes track data for each album
#[get("/library/<player_name>/artist/<artist_name>/albums?<include_tracks>")]
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

/// Get a specific artist by name or MusicBrainz ID
/// 
/// If the artist_name parameter is formatted like a MusicBrainz ID (UUID format),
/// it will search for an artist with that MBID instead of by name.
#[get("/library/<player_name>/artist/<artist_name>")]
pub fn get_artist_by_name(
    player_name: &str, 
    artist_name: &str,
    controller: &State<Arc<AudioController>>
) -> Result<Json<ArtistResponse>, Custom<String>> {
    let controllers = controller.inner().list_controllers();
    
    // Find the controller with the matching name
    for ctrl_lock in controllers {
        if let Ok(ctrl) = ctrl_lock.read() {
            if ctrl.get_player_name() == player_name {
                // Check if the player has a library
                if let Some(library) = ctrl.get_library() {
                    // Check if artist_name looks like a MusicBrainz ID using our helper function
                    let is_mbid = crate::helpers::musicbrainz::is_mbid(artist_name);
                    
                    let artist = if is_mbid {
                        // If it's an MBID, find artists with this MBID
                        let all_artists = library.get_artists();
                        
                        // Find the first artist with the matching MBID
                        all_artists.into_iter().find(|a| {
                            if let Some(meta) = &a.metadata {
                                meta.mbid.iter().any(|id| id == artist_name)
                            } else {
                                false
                            }
                        })
                    } else {
                        // Get the artist by name (normal lookup)
                        library.get_artist(artist_name)
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