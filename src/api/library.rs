use crate::AudioController;
use crate::data::{Album, Artist, Identifier};
use crate::data::library::ArtistMatchType;
use rocket::serde::json::Json;
use rocket::{delete, get, post, State};
use std::sync::Arc;
use rocket::response::status::Custom;
use rocket::http::Status;
use serde::Serialize;
use chrono::Datelike;

fn match_type_str(mt: &ArtistMatchType) -> String {
    match mt {
        ArtistMatchType::Exact => "exact".to_string(),
        ArtistMatchType::CaseInsensitive => "case_insensitive".to_string(),
        ArtistMatchType::Fuzzy => "fuzzy".to_string(),
    }
}

/// Response structure for library information
#[derive(serde::Serialize)]
pub struct LibraryResponse {
    player_name: String,
    player_id: String,
    has_library: bool,
    is_loaded: bool,
    albums_count: usize,
    artists_count: usize,
    tracks_count: usize,
    supports_delete: bool,
    /// Increases whenever the library's contents change. Absent for a backend
    /// that does not track changes - the same signal as a missing ETag.
    #[serde(skip_serializing_if = "Option::is_none")]
    library_version: Option<String>,
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
    supports_delete: bool,
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
    /// Only present when a fuzzy search was requested
    #[serde(skip_serializing_if = "Option::is_none")]
    match_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    match_score: Option<f64>,
    /// Actual name in the library (may differ from query when fuzzy/CI match)
    #[serde(skip_serializing_if = "Option::is_none")]
    matched_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    query: Option<String>,
}

impl ArtistResponse {
    /// Build a single-artist response, rewriting the artist's image paths for
    /// the client's prefix on the way in.
    ///
    /// The rewrite lives here rather than at each call site for the same
    /// reason `create_album_dto` takes a prefix: a struct literal preceded by
    /// a separate rewrite is forgettable, and forgetting it is silent - the
    /// un-prefixed path falls through nginx to the SPA, which answers 200 with
    /// index.html. Built this way, a further artist endpoint cannot compile
    /// without supplying a prefix.
    fn new(
        player_name: String,
        artist: Option<Artist>,
        forwarded_prefix: Option<&str>,
    ) -> Self {
        let artist = artist.map(|mut a| {
            crate::api::urlprefix::rewrite_artist_thumb_urls(&mut a, forwarded_prefix);
            a
        });

        Self {
            player_name,
            artist,
            match_type: None,
            match_score: None,
            matched_name: None,
            query: None,
        }
    }

    /// Attach the fuzzy-search fields to a response built by `new`.
    fn with_match(
        mut self,
        match_type: Option<String>,
        match_score: Option<f64>,
        matched_name: Option<String>,
        query: Option<String>,
    ) -> Self {
        self.match_type = match_type;
        self.match_score = match_score;
        self.matched_name = matched_name;
        self.query = query;
        self
    }
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
    /// Only present when a fuzzy search was requested
    #[serde(skip_serializing_if = "Option::is_none")]
    match_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    match_score: Option<f64>,
    /// Actual name in the library (may differ from query when fuzzy/CI match)
    #[serde(skip_serializing_if = "Option::is_none")]
    matched_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    query: Option<String>,
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

impl ArtistCustomResponse {
    /// Build a list entry, rewriting its thumbnails for the client's prefix.
    ///
    /// The prefix is a required argument for the same reason it is on
    /// `create_album_dto`: omitting it is silent, and only wrong for proxied
    /// clients.
    fn new(
        name: String,
        id: String,
        is_multi: bool,
        album_count: usize,
        mut thumb_url: Vec<String>,
        forwarded_prefix: Option<&str>,
    ) -> Self {
        crate::api::urlprefix::rewrite_thumb_urls(&mut thumb_url, forwarded_prefix);
        Self {
            name,
            id,
            is_multi,
            album_count,
            thumb_url,
        }
    }
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
    #[serde(skip_serializing_if = "Vec::is_empty")]
    genres: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    categories: Vec<String>,
}

/// Creates an AlbumDTO from an Album with optional track inclusion.
///
/// `forwarded_prefix` is required rather than optional-by-omission: a handler
/// that forgot it would emit paths a proxied client cannot fetch, and the
/// failure is silent - the un-prefixed path falls through nginx to the SPA,
/// which answers 200 with index.html.
///
/// This is the only way to build an `AlbumDTO`. The conversion used to live in
/// a `From<Album>` impl with the rewrite layered on top here, which left an
/// un-rewritten DTO obtainable without a prefix in hand - exactly the mistake
/// this signature exists to make impossible.
fn create_album_dto(
    album: Album,
    include_tracks: bool,
    forwarded_prefix: Option<&str>,
) -> AlbumDTO {
    // Get the tracks for counting and optional inclusion
    let tracks_lock = album.tracks.lock();

    let tracks_count = tracks_lock.len();
    let tracks = if include_tracks {
        Some(tracks_lock.clone())
    } else {
        None
    };

    // Get artists
    let artists = album.artists.lock().clone();

    // Drop the lock before returning
    drop(tracks_lock);

    // Compute categories: only genres with explicit mappings configured
    let categories = crate::helpers::genre_cleanup::map_to_categories_global(album.genres.clone());

    AlbumDTO {
        id: album.id.to_string(),
        name: album.name,
        artists,
        release_date: album.release_date,
        tracks_count,
        tracks,
        cover_art: album
            .cover_art
            .map(|url| crate::api::urlprefix::rewrite_api_relative_url(&url, forwarded_prefix)),
        uri: album.uri,
        genres: album.genres,
        categories,
    }
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
        let (has_library, is_loaded, supports_delete) = match &library {
            Some(lib) => (true, lib.is_loaded(), lib.supports_delete()),
            None => (false, false, false),
        };

        // Add player info to the list
        players.push(LibraryPlayerInfo {
            player_name,
            player_id,
            has_library,
            is_loaded,
            supports_delete,
        });
    }
    
    Json(LibraryListResponse { players })
}

/// Get library information for a player
#[get("/library/<player_name>")]
pub fn get_library_info(
    player_name: &str,
    forwarded_prefix: crate::api::urlprefix::ForwardedPrefix,
    controller: &State<Arc<AudioController>>,
) -> Result<Json<LibraryResponse>, Custom<Json<LibraryResponse>>> {
    let controllers = controller.inner().list_controllers();
    
    // Find the controller with the matching name
    for ctrl_lock in controllers {
        let ctrl = ctrl_lock.read();
        if ctrl.get_player_name() == player_name {
            // Check if the player has a library
            if let Some(library) = ctrl.get_library() {
                // Read the version before the data - see the comment in
                // get_player_albums below for why the order matters. It is
                // benign for this handler today, since it emits no ETag,
                // but this is the endpoint a client is meant to poll instead
                // of revalidating each list, which makes it the most likely
                // place to grow one next - so keep the ordering right now.
                //
                // The token folds in the client's prefix, exactly as the list
                // ETags do. If it did not, a client whose route changed would
                // poll this, see no change, and never re-fetch lists whose
                // paths are now wrong for it.
                let library_version = crate::api::urlprefix::prefixed_library_version(
                    library.library_version(),
                    forwarded_prefix.as_deref(),
                );

                // Get basic library info
                let is_loaded = library.is_loaded();
                let supports_delete = library.supports_delete();
                let albums = library.get_albums();
                let artists = library.get_artists();
                let tracks_count: usize = albums.iter().map(|a| a.tracks.lock().len()).sum();

                return Ok(Json(LibraryResponse {
                    player_name: player_name.to_string(),
                    player_id: ctrl.get_player_id(),
                    has_library: true,
                    is_loaded,
                    albums_count: albums.len(),
                    artists_count: artists.len(),
                    tracks_count,
                    supports_delete,
                    library_version,
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
                        tracks_count: 0,
                        supports_delete: false,
                        library_version: None,
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
            tracks_count: 0,
            supports_delete: false,
            library_version: None,
        }),
    ))
}

/// Get all albums for a player
/// 
/// This endpoint returns albums without track data but includes track count
#[get("/library/<player_name>/albums")]
pub fn get_player_albums(
    player_name: &str,
    if_none_match: crate::api::imageresponse::IfNoneMatch<'_>,
    forwarded_prefix: crate::api::urlprefix::ForwardedPrefix,
    controller: &State<Arc<AudioController>>
) -> Result<crate::api::validated::Validated<AlbumsDTOResponse>, Custom<String>> {
    let controllers = controller.inner().list_controllers();

    // Find the controller with the matching name
    for ctrl_lock in controllers {
        let ctrl = ctrl_lock.read();
        if ctrl.get_player_name() == player_name {
            // Check if the player has a library
            if let Some(library) = ctrl.get_library() {
                // Read the version before the data. The background sweep
                // writes to the library and only then bumps the version, on
                // its own thread, under a lock this read does not share. If
                // we read the data first, a sweep landing in between would
                // let a client walk away with pre-update data labelled with
                // the post-update token - a false 304 on every request until
                // the next bump, serving stale data the whole time. Reading
                // the version first can only make the token stale relative
                // to the data, which costs one extra revalidation - never a
                // false hit.
                //
                // The client's prefix is part of the token: the paths in this
                // body are built for it, so two clients on different routes
                // hold different representations of this one URL, and a
                // validator naming only the library's contents would let one
                // of them get a 304 for the other's body.
                let version = crate::api::urlprefix::prefixed_library_version(
                    library.library_version(),
                    forwarded_prefix.as_deref(),
                );

                // Fast path: if the client's token already matches this
                // version, return the 304 right here, before touching any
                // data. Skipping straight past `get_albums()` and the DTO
                // build is the whole point - on the test library that build
                // costs ~0.37s on a Pi to send a few hundred bytes on a hit.
                // This is safe, not just faster: `version` was just read
                // above, before any data access, per the ordering rationale
                // there, and a match here means the client's token equals a
                // version read moments ago with no data access in between -
                // there is nothing that could have changed in that gap for
                // this to be wrong about.
                if let Some(not_modified) = crate::api::validated::not_modified(
                    "albums",
                    &version,
                    if_none_match.0,
                ) {
                    return Ok(not_modified);
                }

                // Get all albums
                let albums = library.get_albums();

                // Convert albums to DTOs without including tracks
                let album_dtos = albums.into_iter()
                    .map(|album| create_album_dto(album, false, forwarded_prefix.as_deref()))
                    .collect::<Vec<AlbumDTO>>();

                let response = AlbumsDTOResponse {
                    player_name: player_name.to_string(),
                    count: album_dtos.len(),
                    albums: album_dtos,
                };

                return Ok(crate::api::validated::validated(
                    response,
                    "albums",
                    version,
                    if_none_match.0,
                ));
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
    if_none_match: crate::api::imageresponse::IfNoneMatch<'_>,
    forwarded_prefix: crate::api::urlprefix::ForwardedPrefix,
    controller: &State<Arc<AudioController>>
) -> Result<crate::api::validated::Validated<serde_json::Value>, Custom<String>> {
    let controllers = controller.inner().list_controllers();
    
    // Find the controller with the matching name
    for ctrl_lock in controllers {
        let ctrl = ctrl_lock.read();
        if ctrl.get_player_name() == player_name {
            // Check if the player has a library
            if let Some(library) = ctrl.get_library() {
                // Read the version before the data - see the comment in
                // get_player_albums above for why the order matters: reading
                // it after the data risks labelling a pre-update list with a
                // post-update token, which is a false 304 (a stale list that
                // never revalidates until the next bump). Reading it first
                // only risks the opposite - one wasted revalidation.
                //
                // The client's prefix is part of the token - see the same
                // call in get_player_albums for why a validator built from
                // library state alone would serve one client's body to
                // another on a different route.
                let version = crate::api::urlprefix::prefixed_library_version(
                    library.library_version(),
                    forwarded_prefix.as_deref(),
                );

                // Fast path: if the client's token already matches this
                // version, return the 304 right here, before touching any
                // data - skipping `get_artists()`, the sort, and the
                // per-artist album count below. See the comment on the same
                // fast path in `get_player_albums` for why this cannot turn
                // real content into a false 304: `version` was just read
                // above, before any data access, and a match here means the
                // client's token equals that just-read version, with nothing
                // in between that could have changed it.
                if let Some(not_modified) = crate::api::validated::not_modified(
                    "artists",
                    &version,
                    if_none_match.0,
                ) {
                    return Ok(not_modified);
                }

                // Get all artists
                let mut artists = library.get_artists();

                // Sort artists by name
                artists.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));

                // Create a custom JSON response with only the required fields
                let mut artists_json = Vec::with_capacity(artists.len());

                for artist in &artists {
                    // Count from the album-artist mapping. Listing the albums
                    // here walked the whole library once per artist.
                    let album_count = library.album_count_for_artist(&artist.id);

                    // Extract all thumbnail URLs from metadata if available
                    let thumb_urls = artist.metadata.as_ref()
                        .map(|meta| meta.thumb_url.clone())
                        .unwrap_or_default();

                    // Create a struct with fields in the specific order
                    let artist_data = ArtistCustomResponse::new(
                        artist.name.clone(),
                        artist.id.to_string(),
                        artist.is_multi,
                        album_count,
                        thumb_urls,
                        forwarded_prefix.as_deref(),
                    );

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

                return Ok(crate::api::validated::validated(
                    response,
                    "artists",
                    version,
                    if_none_match.0,
                ));
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
    forwarded_prefix: crate::api::urlprefix::ForwardedPrefix,
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
                let album_dto = album_option.map(|album| create_album_dto(album, true, forwarded_prefix.as_deref()));
                
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
/// Pass `?fuzzy=true` to enable fuzzy/flexible artist name matching.
/// The response will then include `match_type`, `match_score`, `matched_name`
/// and `query` fields to indicate how the artist was found.
/// This endpoint returns albums without track data but includes track count.
#[get("/library/<player_name>/albums/by-artist/<artist_name>?<fuzzy>")]
pub fn get_albums_by_artist(
    player_name: &str,
    artist_name: &str,
    fuzzy: Option<bool>,
    forwarded_prefix: crate::api::urlprefix::ForwardedPrefix,
    controller: &State<Arc<AudioController>>
) -> Result<Json<ArtistAlbumsDTOResponse>, Custom<String>> {
    let controllers = controller.inner().list_controllers();

    for ctrl_lock in controllers {
        let ctrl = ctrl_lock.read();
        if ctrl.get_player_name() == player_name {
            if let Some(library) = ctrl.get_library() {
                // Resolve artist – either via fuzzy or exact lookup
                let (artist, mt, ms, mn) = if fuzzy.unwrap_or(false) {
                    match library.find_artist_fuzzy(artist_name) {
                        Some(m) => {
                            let mt = match_type_str(&m.match_type);
                            let mn = m.artist.name.clone();
                            (Some(m.artist), Some(mt), Some(m.score), Some(mn))
                        }
                        None => (None, None, None, None),
                    }
                } else {
                    (library.get_artist_by_name(artist_name), None, None, None)
                };

                return match artist {
                    Some(a) => {
                        let albums = library.get_albums_by_artist_id(&a.id);
                        let album_dtos: Vec<AlbumDTO> = albums.into_iter()
                            .map(|album| create_album_dto(album, false, forwarded_prefix.as_deref()))
                            .collect();
                        Ok(Json(ArtistAlbumsDTOResponse {
                            player_name: player_name.to_string(),
                            artist_name: mn.clone().unwrap_or_else(|| artist_name.to_string()),
                            count: album_dtos.len(),
                            albums: album_dtos,
                            match_type: mt,
                            match_score: ms,
                            matched_name: mn,
                            query: fuzzy.unwrap_or(false).then(|| artist_name.to_string()),
                        }))
                    }
                    None => Err(Custom(
                        Status::NotFound,
                        format!("Artist '{}' not found", artist_name),
                    )),
                };
            } else {
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
    forwarded_prefix: crate::api::urlprefix::ForwardedPrefix,
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
                    .map(|album| create_album_dto(album, false, forwarded_prefix.as_deref()))
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
                    match_type: None,
                    match_score: None,
                    matched_name: None,
                    query: None,
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

/// Response structure for genres list
#[derive(serde::Serialize)]
pub struct GenresResponse {
    player_name: String,
    count: usize,
    genres: Vec<String>,
}

/// Response structure for categories list
#[derive(serde::Serialize)]
pub struct CategoriesResponse {
    player_name: String,
    count: usize,
    categories: Vec<String>,
}

/// Get all genres available in the library (union of album tags and artist metadata)
///
/// Pass `?raw=true` to skip genre cleanup and return the raw tags from files/metadata.
#[get("/library/<player_name>/genres?<raw>")]
pub fn get_library_genres(
    player_name: &str,
    raw: Option<bool>,
    controller: &State<Arc<AudioController>>
) -> Result<Json<GenresResponse>, Custom<String>> {
    let controllers = controller.inner().list_controllers();
    for ctrl_lock in controllers {
        let ctrl = ctrl_lock.read();
        if ctrl.get_player_name() == player_name {
            if let Some(library) = ctrl.get_library() {
                let genres = if raw.unwrap_or(false) {
                    library.get_raw_genres()
                } else {
                    library.get_genres()
                };
                let count = genres.len();
                return Ok(Json(GenresResponse {
                    player_name: player_name.to_string(),
                    count,
                    genres,
                }));
            } else {
                return Err(Custom(
                    Status::NotFound,
                    format!("Player '{}' does not have a library", player_name),
                ));
            }
        }
    }
    Err(Custom(Status::NotFound, format!("Player '{}' not found", player_name)))
}

/// Get all albums filtered by genre (case-insensitive)
#[get("/library/<player_name>/albums/by-genre/<genre>")]
pub fn get_albums_by_genre(
    player_name: &str,
    genre: &str,
    forwarded_prefix: crate::api::urlprefix::ForwardedPrefix,
    controller: &State<Arc<AudioController>>
) -> Result<Json<AlbumsDTOResponse>, Custom<String>> {
    let controllers = controller.inner().list_controllers();
    for ctrl_lock in controllers {
        let ctrl = ctrl_lock.read();
        if ctrl.get_player_name() == player_name {
            if let Some(library) = ctrl.get_library() {
                let albums = library.get_albums_by_genre(genre);
                let album_dtos: Vec<AlbumDTO> = albums.into_iter()
                    .map(|album| create_album_dto(album, false, forwarded_prefix.as_deref()))
                    .collect();
                return Ok(Json(AlbumsDTOResponse {
                    player_name: player_name.to_string(),
                    count: album_dtos.len(),
                    albums: album_dtos,
                }));
            } else {
                return Err(Custom(
                    Status::NotFound,
                    format!("Player '{}' does not have a library", player_name),
                ));
            }
        }
    }
    Err(Custom(Status::NotFound, format!("Player '{}' not found", player_name)))
}

/// Get all categories (mapped/cleaned genre labels) available in the library
#[get("/library/<player_name>/categories")]
pub fn get_library_categories(
    player_name: &str,
    controller: &State<Arc<AudioController>>
) -> Result<Json<CategoriesResponse>, Custom<String>> {
    let controllers = controller.inner().list_controllers();
    for ctrl_lock in controllers {
        let ctrl = ctrl_lock.read();
        if ctrl.get_player_name() == player_name {
            if let Some(library) = ctrl.get_library() {
                let categories = library.get_categories();
                let count = categories.len();
                return Ok(Json(CategoriesResponse {
                    player_name: player_name.to_string(),
                    count,
                    categories,
                }));
            } else {
                return Err(Custom(
                    Status::NotFound,
                    format!("Player '{}' does not have a library", player_name),
                ));
            }
        }
    }
    Err(Custom(Status::NotFound, format!("Player '{}' not found", player_name)))
}

/// Get all albums filtered by category (case-insensitive, cleanup applied)
#[get("/library/<player_name>/albums/by-category/<category>")]
pub fn get_albums_by_category(
    player_name: &str,
    category: &str,
    forwarded_prefix: crate::api::urlprefix::ForwardedPrefix,
    controller: &State<Arc<AudioController>>
) -> Result<Json<AlbumsDTOResponse>, Custom<String>> {
    let controllers = controller.inner().list_controllers();
    for ctrl_lock in controllers {
        let ctrl = ctrl_lock.read();
        if ctrl.get_player_name() == player_name {
            if let Some(library) = ctrl.get_library() {
                let albums = library.get_albums_by_category(category);
                let album_dtos: Vec<AlbumDTO> = albums.into_iter()
                    .map(|album| create_album_dto(album, false, forwarded_prefix.as_deref()))
                    .collect();
                return Ok(Json(AlbumsDTOResponse {
                    player_name: player_name.to_string(),
                    count: album_dtos.len(),
                    albums: album_dtos,
                }));
            } else {
                return Err(Custom(
                    Status::NotFound,
                    format!("Player '{}' does not have a library", player_name),
                ));
            }
        }
    }
    Err(Custom(Status::NotFound, format!("Player '{}' not found", player_name)))
}

/// Get all artists filtered by category via artist metadata (case-insensitive, cleanup applied)
#[get("/library/<player_name>/artists/by-category/<category>")]
pub fn get_artists_by_category(
    player_name: &str,
    category: &str,
    controller: &State<Arc<AudioController>>
) -> Result<Json<serde_json::Value>, Custom<String>> {
    let controllers = controller.inner().list_controllers();
    for ctrl_lock in controllers {
        let ctrl = ctrl_lock.read();
        if ctrl.get_player_name() == player_name {
            if let Some(library) = ctrl.get_library() {
                let artists = library.get_artists_by_category(category);
                let all_albums = library.get_albums();
                let enhanced: Vec<serde_json::Value> = artists.iter().map(|artist| {
                    let albums_count = all_albums.iter().filter(|album| {
                        album.artists.lock().iter().any(|a| a == &artist.name)
                    }).count();
                    serde_json::json!({
                        "id": artist.id.to_string(),
                        "name": artist.name,
                        "is_multi": artist.is_multi,
                        "albums_count": albums_count,
                    })
                }).collect();
                return Ok(Json(serde_json::json!({
                    "player_name": player_name,
                    "category": category,
                    "count": enhanced.len(),
                    "artists": enhanced,
                })));
            } else {
                return Err(Custom(
                    Status::NotFound,
                    format!("Player '{}' does not have a library", player_name),
                ));
            }
        }
    }
    Err(Custom(Status::NotFound, format!("Player '{}' not found", player_name)))
}

/// Get all artists filtered by genre via artist metadata (case-insensitive)
#[get("/library/<player_name>/artists/by-genre/<genre>")]
pub fn get_artists_by_genre(
    player_name: &str,
    genre: &str,
    controller: &State<Arc<AudioController>>
) -> Result<Json<serde_json::Value>, Custom<String>> {
    let controllers = controller.inner().list_controllers();
    for ctrl_lock in controllers {
        let ctrl = ctrl_lock.read();
        if ctrl.get_player_name() == player_name {
            if let Some(library) = ctrl.get_library() {
                let artists = library.get_artists_by_genre(genre);
                let all_albums = library.get_albums();
                let enhanced: Vec<serde_json::Value> = artists.iter().map(|artist| {
                    let albums_count = all_albums.iter().filter(|album| {
                        album.artists.lock().iter().any(|a| a == &artist.name)
                    }).count();
                    serde_json::json!({
                        "id": artist.id.to_string(),
                        "name": artist.name,
                        "is_multi": artist.is_multi,
                        "albums_count": albums_count,
                    })
                }).collect();
                return Ok(Json(serde_json::json!({
                    "player_name": player_name,
                    "genre": genre,
                    "count": enhanced.len(),
                    "artists": enhanced,
                })));
            } else {
                return Err(Custom(
                    Status::NotFound,
                    format!("Player '{}' does not have a library", player_name),
                ));
            }
        }
    }
    Err(Custom(Status::NotFound, format!("Player '{}' not found", player_name)))
}

/// Refresh the library for a player
#[get("/library/<player_name>/refresh")]
pub fn refresh_player_library(
    player_name: &str,
    forwarded_prefix: crate::api::urlprefix::ForwardedPrefix,
    controller: &State<Arc<AudioController>>,
) -> Result<Json<LibraryResponse>, Custom<String>> {
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
                        // Read the version before the data - see the comment
                        // in get_player_albums for why the order matters. The
                        // token folds in the client's prefix, as every other
                        // emission of it does; a client comparing this against
                        // one from a different route must see them differ.
                        let library_version = crate::api::urlprefix::prefixed_library_version(
                            library.library_version(),
                            forwarded_prefix.as_deref(),
                        );

                        // Get updated library info
                        let is_loaded = library.is_loaded();
                        let albums = library.get_albums();
                        let artists = library.get_artists();
                        let tracks_count: usize = albums.iter().map(|a| a.tracks.lock().len()).sum();

                        return Ok(Json(LibraryResponse {
                            player_name: player_name.to_string(),
                            player_id: ctrl.get_player_id(),
                            has_library: true,
                            is_loaded,
                            albums_count: albums.len(),
                            artists_count: artists.len(),
                            tracks_count,
                            supports_delete: library.supports_delete(),
                            library_version,
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

/// Get a specific artist by name.
///
/// Pass `?fuzzy=true` to enable fuzzy/flexible matching.
/// When a fuzzy match is found, the response includes `match_type`,
/// `match_score`, `matched_name` (actual library name), and `query`.
#[get("/library/<player_name>/artist/by-name/<artist_name>?<fuzzy>")]
pub fn get_artist_by_name(
    player_name: &str,
    artist_name: &str,
    fuzzy: Option<bool>,
    forwarded_prefix: crate::api::urlprefix::ForwardedPrefix,
    controller: &State<Arc<AudioController>>
) -> Result<Json<ArtistResponse>, Custom<String>> {
    if !fuzzy.unwrap_or(false) {
        return get_artist_internal(player_name, artist_name, controller, ArtistLookupType::ByName, forwarded_prefix.as_deref());
    }

    // Flexible path
    let controllers = controller.inner().list_controllers();
    for ctrl_lock in controllers {
        let ctrl = ctrl_lock.read();
        if ctrl.get_player_name() == player_name {
            if let Some(library) = ctrl.get_library() {
                let (artist, mt, ms, mn) = match library.find_artist_fuzzy(artist_name) {
                    Some(m) => {
                        let mt = match_type_str(&m.match_type);
                        let mn = m.artist.name.clone();
                        (Some(m.artist), Some(mt), Some(m.score), Some(mn))
                    }
                    None => (None, None, None, None),
                };
                return Ok(Json(
                    ArtistResponse::new(
                        player_name.to_string(),
                        artist,
                        forwarded_prefix.as_deref(),
                    )
                    .with_match(mt, ms, mn, Some(artist_name.to_string())),
                ));
            } else {
                return Err(Custom(
                    Status::NotFound,
                    format!("Player '{}' does not have a library", player_name),
                ));
            }
        }
    }
    Err(Custom(
        Status::NotFound,
        format!("Player '{}' not found", player_name),
    ))
}

/// Get a specific artist by ID
#[get("/library/<player_name>/artist/by-id/<artist_id>")]
pub fn get_artist_by_id(
    player_name: &str,
    artist_id: &str,
    forwarded_prefix: crate::api::urlprefix::ForwardedPrefix,
    controller: &State<Arc<AudioController>>
) -> Result<Json<ArtistResponse>, Custom<String>> {
    get_artist_internal(player_name, artist_id, controller, ArtistLookupType::ById, forwarded_prefix.as_deref())
}

/// Get a specific artist by MusicBrainz ID (MBID)
#[get("/library/<player_name>/artist/by-mbid/<mbid>")]
pub fn get_artist_by_mbid(
    player_name: &str,
    mbid: &str,
    forwarded_prefix: crate::api::urlprefix::ForwardedPrefix,
    controller: &State<Arc<AudioController>>
) -> Result<Json<ArtistResponse>, Custom<String>> {
    get_artist_internal(player_name, mbid, controller, ArtistLookupType::ByMbid, forwarded_prefix.as_deref())
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
    lookup_type: ArtistLookupType,
    forwarded_prefix: Option<&str>,
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
                
                return Ok(Json(ArtistResponse::new(
                    player_name.to_string(),
                    artist,
                    forwarded_prefix,
                )));
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

/// Interpret the `size` query parameter.
///
/// `Ok(None)` means "serve the original": either no size was asked for, or the
/// request is larger than the top rung and acr does not upscale. `Err` is a client
/// mistake and must become a 400 — a client sending nonsense should find out rather
/// than silently receive a 243 KB original.
pub fn parse_size(raw: Option<&str>) -> Result<Option<u32>, String> {
    let Some(raw) = raw else { return Ok(None) };

    let requested: u32 = raw
        .parse()
        .map_err(|_| format!("Invalid size '{}': expected a positive integer", raw))?;
    if requested == 0 {
        return Err(format!("Invalid size '{}': expected a positive integer", raw));
    }

    Ok(crate::helpers::imageresize::snap_to_rung(requested))
}

/// Build the body of a 400 for a bad `size`, including the sizes that would work.
///
/// Being told only that a value was wrong leaves a client guessing; the list costs
/// a few bytes on a path that is already an error.
pub fn size_error_body(message: &str) -> String {
    serde_json::json!({
        "error": message,
        "image_sizes": crate::helpers::imageresize::sizes(),
    })
    .to_string()
}

/// Produce a downscaled version of a library image through the image cache.
///
/// Returns `None` when the identifier does not correspond to a cached album cover,
/// in which case the caller serves the original. Resizing is a best-effort
/// improvement, never a reason to fail a request that would otherwise succeed.
fn resize_via_cache(
    library: &dyn crate::data::library::LibraryInterface,
    identifier: &str,
    rung: u32,
) -> Option<(Vec<u8>, String)> {
    let album_id_str = identifier.strip_prefix("album:")?;
    let album_id = crate::data::Identifier::Numeric(album_id_str.parse().ok()?);
    let album = library.get_album_by_id(&album_id)?;

    let artist = {
        let artists = album.artists.lock();
        artists.first().cloned().unwrap_or_else(|| "Unknown Artist".to_string())
    };
    let year = album.release_date.map(|d| d.year());
    let base = crate::helpers::local_coverart::album_cache_key(&artist, &album.name, year);

    match crate::helpers::imagecache::get_or_create_variant(format!("{}/cover", base), rung) {
        Ok(result) => Some(result),
        Err(e) => {
            log::debug!("No variant for {} at {}px: {}", identifier, rung, e);
            None
        }
    }
}

/// Retrieve an image from the library based on an identifier
///
/// This endpoint maps directly to the library's get_image function, allowing
/// access to image data like album covers and artist images through the REST API.
/// The identifier format depends on the library implementation, but typically
/// supports formats like "album:123" for album covers and "artist:Artist Name" for artist images.
///
/// An optional `size` query parameter requests a downscaled variant, rounded up
/// to the next rung of 100/200/400/800 pixels on the longest edge. Omitting it,
/// or requesting a size above the top rung or the original's own size, serves the
/// original bytes unchanged.
#[get("/library/<player_name>/image/<identifier>?<size>")]
pub fn get_image(
    player_name: &str,
    identifier: &str,
    size: Option<&str>,
    if_none_match: crate::api::imageresponse::IfNoneMatch<'_>,
    controller: &State<Arc<AudioController>>
) -> Result<crate::api::imageresponse::ImageReply, Custom<String>> {
    use crate::api::imageresponse::{reply, IMMUTABLE_CACHE, REVALIDATE_DAILY_CACHE};

    let target = parse_size(size)
        .map_err(|e| Custom(Status::BadRequest, size_error_body(&e)))?;

    // Only `album:` identifiers are immutable under their id. `artist:` art can be
    // replaced by the user (see `src/api/coverart.rs`), and bare track URLs are not
    // addressed by a stable id either, so both must revalidate rather than being
    // cached for a year with no server-side remedy.
    let cache_control = if identifier.starts_with("album:") {
        IMMUTABLE_CACHE
    } else {
        REVALIDATE_DAILY_CACHE
    };

    let controllers = controller.inner().list_controllers();

    // Find the controller with the matching name
    for ctrl_lock in controllers {
        let ctrl = ctrl_lock.read();
        if ctrl.get_player_name() == player_name {
            // Check if the player has a library
            if let Some(library) = ctrl.get_library() {
                // Try the variant first when one was asked for. The original is only
                // fetched if that finds nothing, because fetching it unconditionally
                // costs a ~243KB read per thumbnail on MPD and a full HTTP round trip
                // to the server on LMS -- paid on every request, and thrown away
                // whenever a variant is served.
                //
                // This cannot turn a 404 into a 200: `resize_via_cache` only answers
                // for an `album:` identifier whose album resolves and whose cover is
                // in the image cache, and every library that populates that cache
                // consults it first in `get_image` too. With no `size`, `target` is
                // `None`, nothing here runs, and the original bytes are returned
                // exactly as before -- the compatibility guarantee this whole feature
                // rests on.
                let resized = target
                    .and_then(|rung| resize_via_cache(library.as_ref(), identifier, rung));

                let image = match resized {
                    Some(variant) => Some(variant),
                    // Call the library's get_image function
                    None => library.get_image(identifier.to_string()),
                };

                if let Some((data, mime_type)) = image {
                    return Ok(reply(data, &mime_type, cache_control, if_none_match.0));
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

/// Response structure for delete operations
#[derive(serde::Serialize)]
pub struct DeleteResponse {
    success: bool,
    message: String,
}

/// Delete an album and all its tracks from the library filesystem
#[delete("/library/<player_name>/album/<album_id>")]
pub fn delete_library_album(
    player_name: &str,
    album_id: &str,
    controller: &State<Arc<AudioController>>,
) -> Custom<Json<DeleteResponse>> {
    let controllers = controller.inner().list_controllers();

    for ctrl_lock in controllers {
        let ctrl = ctrl_lock.read();
        if ctrl.get_player_name() == player_name {
            if let Some(library) = ctrl.get_library() {
                if !library.supports_delete() {
                    return Custom(
                        Status::MethodNotAllowed,
                        Json(DeleteResponse {
                            success: false,
                            message: format!("Player '{}' does not support deletion", player_name),
                        }),
                    );
                }
                let id = if let Ok(num) = album_id.parse::<u64>() {
                    Identifier::Numeric(num)
                } else {
                    Identifier::String(album_id.to_string())
                };
                match library.delete_album(&id) {
                    Ok(()) => return Custom(
                        Status::Ok,
                        Json(DeleteResponse {
                            success: true,
                            message: format!("Album '{}' deleted", album_id),
                        }),
                    ),
                    Err(e) => return Custom(
                        Status::InternalServerError,
                        Json(DeleteResponse {
                            success: false,
                            message: format!("Failed to delete album: {}", e),
                        }),
                    ),
                }
            } else {
                return Custom(
                    Status::NotFound,
                    Json(DeleteResponse {
                        success: false,
                        message: format!("Player '{}' does not have a library", player_name),
                    }),
                );
            }
        }
    }

    Custom(
        Status::NotFound,
        Json(DeleteResponse {
            success: false,
            message: format!("Player '{}' not found", player_name),
        }),
    )
}

/// Delete a single track from the library filesystem by its URI
///
/// The track_uri path segment is percent-encoded (standard URL encoding).
#[delete("/library/<player_name>/track/<track_uri>")]
pub fn delete_library_track(
    player_name: &str,
    track_uri: &str,
    controller: &State<Arc<AudioController>>,
) -> Custom<Json<DeleteResponse>> {
    let controllers = controller.inner().list_controllers();

    let decoded_uri = match urlencoding::decode(track_uri) {
        Ok(s) => s.into_owned(),
        Err(_) => track_uri.to_string(),
    };

    for ctrl_lock in controllers {
        let ctrl = ctrl_lock.read();
        if ctrl.get_player_name() == player_name {
            if let Some(library) = ctrl.get_library() {
                if !library.supports_delete() {
                    return Custom(
                        Status::MethodNotAllowed,
                        Json(DeleteResponse {
                            success: false,
                            message: format!("Player '{}' does not support deletion", player_name),
                        }),
                    );
                }
                match library.delete_track(&decoded_uri) {
                    Ok(()) => return Custom(
                        Status::Ok,
                        Json(DeleteResponse {
                            success: true,
                            message: format!("Track '{}' deleted", decoded_uri),
                        }),
                    ),
                    Err(e) => return Custom(
                        Status::InternalServerError,
                        Json(DeleteResponse {
                            success: false,
                            message: format!("Failed to delete track: {}", e),
                        }),
                    ),
                }
            } else {
                return Custom(
                    Status::NotFound,
                    Json(DeleteResponse {
                        success: false,
                        message: format!("Player '{}' does not have a library", player_name),
                    }),
                );
            }
        }
    }

    Custom(
        Status::NotFound,
        Json(DeleteResponse {
            success: false,
            message: format!("Player '{}' not found", player_name),
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    // `Album`, `Artist`, `Identifier` and `Arc` all arrive via `super::*`
    // from the file's existing imports; only `Mutex` is new here.
    use parking_lot::Mutex;

    #[test]
    fn absent_size_means_the_original() {
        assert_eq!(parse_size(None).unwrap(), None);
    }

    #[test]
    fn a_valid_size_snaps_up_to_a_rung() {
        assert_eq!(parse_size(Some("360")).unwrap(), Some(400));
        assert_eq!(parse_size(Some("100")).unwrap(), Some(100));
    }

    #[test]
    fn a_size_above_the_ladder_means_the_original() {
        assert_eq!(parse_size(Some("2000")).unwrap(), None);
    }

    #[test]
    fn nonsense_sizes_are_rejected_rather_than_ignored() {
        assert!(parse_size(Some("wide")).is_err());
        assert!(parse_size(Some("0")).is_err());
        assert!(parse_size(Some("-40")).is_err());
        assert!(parse_size(Some("")).is_err());
    }

    #[test]
    fn size_errors_carry_the_valid_sizes() {
        let body: serde_json::Value =
            serde_json::from_str(&size_error_body("Invalid size 'wide'")).unwrap();
        assert_eq!(body["error"], "Invalid size 'wide'");
        assert_eq!(body["image_sizes"], serde_json::json!([100, 140, 200, 280, 400, 800]));
    }

    // Both `resize_via_cache` tests below only need to reach the identifier
    // and album-id parsing at the top of the function, so they never actually
    // touch the cache — but they still point the global image cache at a
    // `TempDir` (the seam `ImageCache::initialize` provides) rather than the
    // real `/var/lib/audiocontrol/cache/images`, and are `#[serial]` because
    // that cache is process-global, same as imagecache.rs's own tests.
    #[test]
    #[serial]
    fn resize_via_cache_ignores_non_album_identifiers() {
        use crate::data::library::LibraryInterface;
        use crate::helpers::imagecache::ImageCache;
        use crate::players::mpd::library::MPDLibrary;

        let temp_dir = tempfile::TempDir::new().unwrap();
        ImageCache::initialize(temp_dir.path()).unwrap();

        let library = MPDLibrary::new();
        assert_eq!(resize_via_cache(&library, "artist:Foo", 400), None);
    }

    #[test]
    #[serial]
    fn resize_via_cache_ignores_unparseable_album_ids() {
        use crate::data::library::LibraryInterface;
        use crate::helpers::imagecache::ImageCache;
        use crate::players::mpd::library::MPDLibrary;

        let temp_dir = tempfile::TempDir::new().unwrap();
        ImageCache::initialize(temp_dir.path()).unwrap();

        let library = MPDLibrary::new();
        assert_eq!(resize_via_cache(&library, "album:not-a-number", 400), None);
    }

    fn album_with_cover(cover_art: Option<&str>) -> Album {
        Album {
            id: Identifier::Numeric(7),
            name: "Test Album".to_string(),
            artists: Arc::new(Mutex::new(vec!["Test Artist".to_string()])),
            artists_flat: None,
            release_date: None,
            tracks: Arc::new(Mutex::new(Vec::new())),
            cover_art: cover_art.map(ToOwned::to_owned),
            uri: None,
            genres: Vec::new(),
        }
    }

    #[test]
    fn an_album_dto_gains_the_prefix_on_its_cover_art() {
        let dto = create_album_dto(
            album_with_cover(Some("/api/library/mpd/image/album:7")),
            false,
            Some("/api/audiocontrol"),
        );
        assert_eq!(
            dto.cover_art.as_deref(),
            Some("/api/audiocontrol/library/mpd/image/album:7")
        );
    }

    #[test]
    fn an_album_dto_without_a_prefix_is_unchanged() {
        let dto = create_album_dto(
            album_with_cover(Some("/api/library/mpd/image/album:7")),
            false,
            None,
        );
        assert_eq!(dto.cover_art.as_deref(), Some("/api/library/mpd/image/album:7"));
    }

    #[test]
    fn an_album_without_cover_art_is_handled() {
        let dto = create_album_dto(album_with_cover(None), false, Some("/api/audiocontrol"));
        assert!(dto.cover_art.is_none());
    }

    #[test]
    fn an_already_prefixed_cover_is_not_doubled() {
        let dto = create_album_dto(
            album_with_cover(Some("/api/audiocontrol/library/mpd/image/album:7")),
            false,
            Some("/api/audiocontrol"),
        );
        assert_eq!(
            dto.cover_art.as_deref(),
            Some("/api/audiocontrol/library/mpd/image/album:7")
        );
    }

    use crate::data::metadata::ArtistMeta;

    fn artist_with_thumbs(thumbs: Vec<&str>) -> Artist {
        let mut meta = ArtistMeta::new();
        meta.thumb_url = thumbs.into_iter().map(ToOwned::to_owned).collect();
        Artist {
            id: Identifier::Numeric(3),
            name: "Test Artist".to_string(),
            is_multi: false,
            metadata: Some(meta),
        }
    }

    #[test]
    fn an_artist_list_entry_gains_the_prefix() {
        let entry = ArtistCustomResponse::new(
            "Test Artist".to_string(),
            "3".to_string(),
            false,
            2,
            vec!["/api/coverart/artist/YWJj/image".to_string()],
            Some("/api/audiocontrol"),
        );
        assert_eq!(entry.thumb_url[0], "/api/audiocontrol/coverart/artist/YWJj/image");
    }

    // ---------------------------------------------------------------------
    // Route-level tests.
    //
    // The unit tests above prove the builders rewrite what they are given.
    // They say nothing about whether a handler passes the request's prefix to
    // them at all, and a handler that dropped it would keep compiling and keep
    // passing every one of them. These dispatch a real request through Rocket
    // and read the prefix out of the JSON the client would actually receive.
    // ---------------------------------------------------------------------

    use crate::data::library::{LibraryError, LibraryInterface};
    use crate::data::{LoopMode, PlaybackState, PlayerCapabilitySet, PlayerCommand, Song, Track};
    use crate::players::PlayerController;
    use rocket::http::Header;
    use rocket::local::blocking::Client;

    /// A library holding exactly one album and one artist, both with internal
    /// image paths, and a fixed version token.
    struct StubLibrary;

    impl StubLibrary {
        fn album() -> Album {
            Album {
                id: Identifier::Numeric(7),
                name: "Stub Album".to_string(),
                artists: Arc::new(Mutex::new(vec!["Stub Artist".to_string()])),
                artists_flat: None,
                release_date: None,
                tracks: Arc::new(Mutex::new(Vec::new())),
                cover_art: Some("/api/library/stub/image/album:7".to_string()),
                uri: None,
                genres: Vec::new(),
            }
        }

        fn artist() -> Artist {
            artist_with_thumbs(vec![
                "/api/coverart/artist/YWJj/image",
                "https://example.com/artist.png",
            ])
        }
    }

    impl LibraryInterface for StubLibrary {
        fn new() -> Self {
            StubLibrary
        }
        fn is_loaded(&self) -> bool {
            true
        }
        fn refresh_library(&self) -> Result<(), LibraryError> {
            Ok(())
        }
        fn get_albums(&self) -> Vec<Album> {
            vec![Self::album()]
        }
        fn get_artists(&self) -> Vec<Artist> {
            vec![Self::artist()]
        }
        fn get_album_by_artist_and_name(&self, _artist: &str, _album: &str) -> Option<Album> {
            None
        }
        fn get_album_by_id(&self, _id: &Identifier) -> Option<Album> {
            None
        }
        fn get_artist_by_name(&self, _name: &str) -> Option<Artist> {
            Some(Self::artist())
        }
        fn get_albums_by_artist_id(&self, _artist_id: &Identifier) -> Vec<Album> {
            vec![Self::album()]
        }
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
        fn get_image(&self, _identifier: String) -> Option<(Vec<u8>, String)> {
            None
        }
        fn update_artist_metadata(&self) {}
        fn library_version(&self) -> Option<String> {
            Some("42".to_string())
        }
    }

    /// The smallest player that owns a library, so the handlers' controller
    /// lookup finds something to ask.
    struct StubPlayer;

    impl PlayerController for StubPlayer {
        fn get_capabilities(&self) -> PlayerCapabilitySet {
            PlayerCapabilitySet::empty()
        }
        fn get_song(&self) -> Option<Song> {
            None
        }
        fn get_queue(&self) -> Vec<Track> {
            Vec::new()
        }
        fn get_loop_mode(&self) -> LoopMode {
            LoopMode::None
        }
        fn get_playback_state(&self) -> PlaybackState {
            PlaybackState::Stopped
        }
        fn get_position(&self) -> Option<f64> {
            None
        }
        fn get_shuffle(&self) -> bool {
            false
        }
        fn get_player_name(&self) -> String {
            "stub".to_string()
        }
        fn get_player_id(&self) -> String {
            "stub".to_string()
        }
        fn get_last_seen(&self) -> Option<std::time::SystemTime> {
            None
        }
        fn send_command(&self, _command: PlayerCommand) -> bool {
            true
        }
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
        fn start(&self) -> bool {
            true
        }
        fn stop(&self) -> bool {
            true
        }
        fn get_library(&self) -> Option<Box<dyn LibraryInterface>> {
            Some(Box::new(StubLibrary))
        }
    }

    fn stub_client() -> Client {
        let mut controller = AudioController::new();
        controller.add_controller(Box::new(StubPlayer));

        let rocket = rocket::build()
            .manage(Arc::new(controller))
            .mount(
                "/api",
                rocket::routes![
                    get_library_info,
                    get_player_albums,
                    get_player_artists,
                    get_artist_by_name,
                ],
            );
        Client::tracked(rocket).unwrap()
    }

    /// GET `path`, optionally announcing a forwarded prefix, and parse the
    /// JSON body.
    fn get_json(path: &str, prefix: Option<&str>) -> serde_json::Value {
        let client = stub_client();
        let mut request = client.get(path.to_string());
        if let Some(prefix) = prefix {
            request = request.header(Header::new("X-Forwarded-Prefix", prefix.to_string()));
        }
        let response = request.dispatch();
        assert_eq!(response.status(), Status::Ok, "unexpected status for {}", path);
        serde_json::from_str(&response.into_string().unwrap()).unwrap()
    }

    #[test]
    fn the_albums_route_emits_the_forwarded_prefix() {
        let body = get_json("/api/library/stub/albums", Some("/api/audiocontrol"));
        assert_eq!(
            body["albums"][0]["cover_art"],
            "/api/audiocontrol/library/stub/image/album:7"
        );
    }

    #[test]
    fn the_albums_route_without_a_prefix_emits_the_internal_path() {
        let body = get_json("/api/library/stub/albums", None);
        assert_eq!(body["albums"][0]["cover_art"], "/api/library/stub/image/album:7");
    }

    #[test]
    fn the_artists_route_emits_the_forwarded_prefix() {
        let body = get_json("/api/library/stub/artists", Some("/api/audiocontrol"));
        let thumbs = &body["artists"][0]["thumb_url"];
        assert_eq!(thumbs[0], "/api/audiocontrol/coverart/artist/YWJj/image");
        // An external provider URL is not the daemon's to rewrite.
        assert_eq!(thumbs[1], "https://example.com/artist.png");
    }

    #[test]
    fn the_artists_route_without_a_prefix_emits_the_internal_path() {
        let body = get_json("/api/library/stub/artists", None);
        assert_eq!(body["artists"][0]["thumb_url"][0], "/api/coverart/artist/YWJj/image");
    }

    #[test]
    fn a_single_artist_route_emits_the_forwarded_prefix() {
        let body = get_json(
            "/api/library/stub/artist/by-name/Stub%20Artist",
            Some("/api/audiocontrol"),
        );
        let thumbs = &body["artist"]["metadata"]["thumb_url"];
        assert_eq!(thumbs[0], "/api/audiocontrol/coverart/artist/YWJj/image");
        assert_eq!(thumbs[1], "https://example.com/artist.png");
    }

    #[test]
    fn a_single_artist_route_without_a_prefix_emits_the_internal_path() {
        let body = get_json("/api/library/stub/artist/by-name/Stub%20Artist", None);
        assert_eq!(
            body["artist"]["metadata"]["thumb_url"][0],
            "/api/coverart/artist/YWJj/image"
        );
    }

    // The validator has to name the prefix as well as the library's contents.
    // Sharing one token between the two routes lets the origin answer 304 for
    // a body the client does not hold - and every path in that body then
    // resolves to the web interface's index.html rather than to an image.

    fn etag(path: &str, prefix: Option<&str>) -> String {
        let client = stub_client();
        let mut request = client.get(path.to_string());
        if let Some(prefix) = prefix {
            request = request.header(Header::new("X-Forwarded-Prefix", prefix.to_string()));
        }
        let response = request.dispatch();
        response
            .headers()
            .get_one("ETag")
            .expect("a versioned library must emit an ETag")
            .to_string()
    }

    #[test]
    fn two_prefixes_do_not_share_one_validator() {
        let direct = etag("/api/library/stub/albums", None);
        let proxied = etag("/api/library/stub/albums", Some("/api/audiocontrol"));
        assert_ne!(direct, proxied);
        assert_ne!(
            etag("/api/library/stub/artists", None),
            etag("/api/library/stub/artists", Some("/api/audiocontrol"))
        );
    }

    #[test]
    fn one_prefix_gets_one_stable_validator() {
        assert_eq!(
            etag("/api/library/stub/albums", None),
            etag("/api/library/stub/albums", None)
        );
        assert_eq!(
            etag("/api/library/stub/albums", Some("/api/audiocontrol")),
            etag("/api/library/stub/albums", Some("/api/audiocontrol"))
        );
    }

    #[test]
    fn a_validator_from_another_prefix_does_not_win_a_304() {
        let direct = etag("/api/library/stub/albums", None);
        let client = stub_client();
        let response = client
            .get("/api/library/stub/albums")
            .header(Header::new("X-Forwarded-Prefix", "/api/audiocontrol"))
            .header(Header::new("If-None-Match", direct))
            .dispatch();
        assert_eq!(
            response.status(),
            Status::Ok,
            "a token from the direct route must not satisfy a proxied request"
        );
        let body: serde_json::Value =
            serde_json::from_str(&response.into_string().unwrap()).unwrap();
        assert_eq!(
            body["albums"][0]["cover_art"],
            "/api/audiocontrol/library/stub/image/album:7"
        );
    }

    #[test]
    fn the_matching_validator_still_wins_a_304() {
        // The prefix component must not defeat revalidation for a client that
        // stayed on one route.
        let proxied = etag("/api/library/stub/albums", Some("/api/audiocontrol"));
        let client = stub_client();
        let response = client
            .get("/api/library/stub/albums")
            .header(Header::new("X-Forwarded-Prefix", "/api/audiocontrol"))
            .header(Header::new("If-None-Match", proxied))
            .dispatch();
        assert_eq!(response.status(), Status::NotModified);
    }

    #[test]
    fn the_polled_library_version_tracks_the_same_prefix_as_the_etags() {
        // doc/api.md tells clients to poll this instead of revalidating each
        // list. If it omitted the prefix, a client whose route changed would
        // poll, see no change, and never re-fetch.
        let direct = get_json("/api/library/stub", None);
        let proxied = get_json("/api/library/stub", Some("/api/audiocontrol"));
        assert_ne!(direct["library_version"], proxied["library_version"]);

        // And it must be the component the ETag carries, not some other one.
        let tag = etag("/api/library/stub/albums", Some("/api/audiocontrol"));
        let version = proxied["library_version"].as_str().unwrap();
        assert!(
            tag.contains(version),
            "the ETag {} should be built from the polled token {}",
            tag,
            version
        );
    }
}
