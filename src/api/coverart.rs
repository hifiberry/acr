use rocket::get;
use rocket::post;
use rocket::serde::json::Json;
use rocket::serde::{Deserialize, Serialize};
use log::{debug, info, warn, error};
use crate::helpers::coverart::{
    get_coverart_manager, query_coverart, CoverartMethod, CoverartQuery, CoverartResult,
    ProviderInfo, QueryOptions,
};
use crate::helpers::url_encoding::decode_url_safe;
use crate::helpers::settingsdb;
use std::collections::HashMap;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use crate::helpers::artist_store::{ArtistImageResult, ArtistImageSource};
use crate::constants::API_PREFIX;

#[derive(Serialize, Deserialize)]
pub struct CoverartResponse {
    pub results: Vec<CoverartResult>,
}

#[derive(Serialize, Deserialize)]
pub struct CoverartMethodInfo {
    pub method: String,
    pub providers: Vec<ProviderInfo>,
}

#[derive(Serialize)]
pub struct CoverartMethodsResponse {
    methods: Vec<CoverartMethodInfo>,
}

#[derive(Deserialize)]
pub struct UpdateImageRequest {
    url: String,
}

#[derive(Serialize)]
pub struct UpdateImageResponse {
    success: bool,
    message: String,
}

/// One artist's image being uploaded in a batch request.
#[derive(Deserialize)]
pub struct UploadArtistsImagesRequest {
    /// Map of artist name to base64-encoded image bytes.
    images: HashMap<String, String>,
}

/// The outcome for a single artist within an upload batch.
#[derive(Serialize)]
pub struct UploadImageResultResponse {
    success: bool,
    message: String,
}

/// Per-artist outcomes from an upload batch.
#[derive(Serialize)]
pub struct UploadArtistsImagesResponse {
    results: HashMap<String, UploadImageResultResponse>,
}

/// Options for a cover art request.
///
/// `include_slow` is opt-in: providers that may take tens of seconds are off
/// the default path, so an existing client's request is never made slower by
/// a slow provider being configured on the device.
fn query_options(include_slow: Option<bool>) -> QueryOptions {
    QueryOptions {
        include_slow: include_slow.unwrap_or(false),
        ..QueryOptions::default()
    }
}

/// Get cover art for an artist
///
/// # Parameters
/// * `artist_b64` - Base64 encoded artist name
#[get("/artist/<artist_b64>?<include_slow>")]
pub fn get_artist_coverart(artist_b64: String, include_slow: Option<bool>) -> Json<CoverartResponse> {
    let artist = match decode_url_safe(&artist_b64) {
        Some(decoded) => decoded,
        None => {
            log::warn!("Failed to decode artist parameter: {}", artist_b64);
            return Json(CoverartResponse {
                results: vec![],
            });
        }
    };

    let results = query_coverart(
        &CoverartQuery::Artist(artist),
        &query_options(include_slow),
    );

    Json(CoverartResponse { results })
}

/// Get cover art for a song
/// 
/// # Parameters
/// * `title_b64` - Base64 encoded song title
/// * `artist_b64` - Base64 encoded artist name
#[get("/song/<title_b64>/<artist_b64>?<include_slow>")]
pub fn get_song_coverart(
    title_b64: String,
    artist_b64: String,
    include_slow: Option<bool>,
) -> Json<CoverartResponse> {
    let title = match decode_url_safe(&title_b64) {
        Some(decoded) => decoded,
        None => {
            log::warn!("Failed to decode title parameter: {}", title_b64);
            return Json(CoverartResponse {
                results: vec![],
            });
        }
    };

    let artist = match decode_url_safe(&artist_b64) {
        Some(decoded) => decoded,
        None => {
            log::warn!("Failed to decode artist parameter: {}", artist_b64);
            return Json(CoverartResponse {
                results: vec![],
            });
        }
    };

    let results = query_coverart(
        &CoverartQuery::Song { title, artist },
        &query_options(include_slow),
    );

    Json(CoverartResponse { results })
}

/// Get cover art for an album
/// 
/// # Parameters
/// * `title_b64` - Base64 encoded album title
/// * `artist_b64` - Base64 encoded artist name
/// * `year` - Optional release year
#[get("/album/<title_b64>/<artist_b64>?<include_slow>")]
pub fn get_album_coverart(
    title_b64: String,
    artist_b64: String,
    include_slow: Option<bool>,
) -> Json<CoverartResponse> {
    get_album_coverart_with_year(title_b64, artist_b64, None, include_slow)
}

/// Get cover art for an album with year
///
/// # Parameters
/// * `title_b64` - Base64 encoded album title
/// * `artist_b64` - Base64 encoded artist name
/// * `year` - Release year
#[get("/album/<title_b64>/<artist_b64>/<year>?<include_slow>")]
pub fn get_album_coverart_with_year(
    title_b64: String,
    artist_b64: String,
    year: Option<i32>,
    include_slow: Option<bool>,
) -> Json<CoverartResponse> {
    let title = match decode_url_safe(&title_b64) {
        Some(decoded) => decoded,
        None => {
            log::warn!("Failed to decode title parameter: {}", title_b64);
            return Json(CoverartResponse {
                results: vec![],
            });
        }
    };

    let artist = match decode_url_safe(&artist_b64) {
        Some(decoded) => decoded,
        None => {
            log::warn!("Failed to decode artist parameter: {}", artist_b64);
            return Json(CoverartResponse {
                results: vec![],
            });
        }
    };

    let results = query_coverart(
        &CoverartQuery::Album { title, artist, year },
        &query_options(include_slow),
    );

    Json(CoverartResponse { results })
}

/// Get cover art from a URL
/// 
/// # Parameters
/// * `url_b64` - Base64 encoded URL
#[get("/url/<url_b64>?<include_slow>")]
pub fn get_url_coverart(url_b64: String, include_slow: Option<bool>) -> Json<CoverartResponse> {
    let url = match decode_url_safe(&url_b64) {
        Some(decoded) => decoded,
        None => {
            log::warn!("Failed to decode url parameter: {}", url_b64);
            return Json(CoverartResponse {
                results: vec![],
            });
        }
    };

    let results = query_coverart(&CoverartQuery::Url(url), &query_options(include_slow));

    Json(CoverartResponse { results })
}

/// Get information about available coverart methods and providers
#[get("/methods")]
pub fn get_coverart_methods() -> Json<CoverartMethodsResponse> {
    let manager = get_coverart_manager();
    let manager_lock = manager.lock();
    let providers = manager_lock.get_providers();
    
    log::debug!("API: Total providers found: {}", providers.len());
    for (i, provider) in providers.iter().enumerate() {
        log::debug!("API: Provider {}: {} ({})", i, provider.name(), provider.display_name());
        log::debug!("API: Provider {} supported methods: {:?}", i, provider.supported_methods());
    }
    
    // Group providers by supported methods
    let mut method_providers = std::collections::HashMap::new();
    
    for provider in providers {
        let supported_methods = provider.supported_methods();
        let provider_info = ProviderInfo {
            name: provider.name().to_string(),
            display_name: provider.display_name().to_string(),
        };
        
        for method in supported_methods {
            method_providers
                .entry(method)
                .or_insert_with(Vec::new)
                .push(provider_info.clone());
        }
    }
    
    // Convert to response format
    let methods: Vec<CoverartMethodInfo> = [
        CoverartMethod::Artist,
        CoverartMethod::Song,
        CoverartMethod::Album,
        CoverartMethod::Url,
    ]
    .iter()
    .map(|method| {
        let method_name = match method {
            CoverartMethod::Artist => "Artist",
            CoverartMethod::Song => "Song", 
            CoverartMethod::Album => "Album",
            CoverartMethod::Url => "Url",
        };
        
        CoverartMethodInfo {
            method: method_name.to_string(),
            providers: method_providers.get(method).cloned().unwrap_or_default(),
        }
    })
    .collect();
    
    Json(CoverartMethodsResponse { methods })
}

/// Upload custom artist images for one or more artists in a single request.
///
/// The request body maps each artist name to base64-encoded image bytes:
///
/// ```json
/// { "images": { "Artist One": "<base64>", "Artist Two": "<base64>" } }
/// ```
///
/// Each image is decoded, validated as a decodable image within this daemon's
/// safe limits, and stored to the artist's user custom-image path, so it takes
/// precedence over anything the cover-art providers auto-downloaded. The
/// response reports per-artist outcomes so a partially-valid batch can still
/// succeed where it can and explain what failed and why.
///
/// Unlike a value set through `POST /artist/<artist_b64>/update`, an uploaded
/// image is stored directly and has no remote URL to re-download, so nothing
/// is written to the settings database.
///
/// The whole batch must fit in one request body: Rocket's JSON limit is
/// pinned to 10 MiB (see `rocket_config`), and base64 adds a third, so that
/// is roughly 7.5 MiB of image bytes in total. A batch past the limit is
/// rejected with a 413 before any entry is stored.
///
/// Entries are keyed as given in the request map. Two names that sanitise to
/// the same file (for example `The Beatles` and `the beatles!`) name the same
/// stored image; one of them is stored and the other is reported as a failure
/// naming the entry that won, rather than both being reported as stored when
/// only one image exists.
#[post("/artists/upload", data = "<request>")]
pub fn upload_artists_images(request: Json<UploadArtistsImagesRequest>) -> Json<UploadArtistsImagesResponse> {
    debug!("Received artist image upload batch with {} artist(s)", request.images.len());

    let mut results: HashMap<String, UploadImageResultResponse> = HashMap::new();
    let collisions = colliding_entries(request.images.keys());

    for (artist_name, b64data) in &request.images {
        if let Some(winner) = collisions.get(artist_name) {
            debug!(
                "Artist '{}' names the same stored file as '{}'; not storing it",
                artist_name, winner
            );
            results.insert(
                artist_name.clone(),
                UploadImageResultResponse {
                    success: false,
                    message: format!(
                        "'{}' is stored under the same file name as '{}'; only '{}' was stored",
                        artist_name, winner, winner
                    ),
                },
            );
            continue;
        }

        let outcome = match validate_upload_entry(artist_name, b64data) {
            Err(message) => UploadImageResultResponse {
                success: false,
                message,
            },
            Ok((name, bytes)) => {
                let store = crate::helpers::artist_store::get_artist_store();
                let mut store_lock = store.lock();
                match store_lock.store_user_uploaded_image(&name, &bytes) {
                    ArtistImageResult::Found { cache_path } => {
                        crate::helpers::imageresize::remove_variants_of(&cache_path);
                        UploadImageResultResponse {
                            success: true,
                            message: format!("Stored image for '{}'", name),
                        }
                    }
                    ArtistImageResult::Error(e) => UploadImageResultResponse {
                        success: false,
                        message: e,
                    },
                    ArtistImageResult::NotFound => UploadImageResultResponse {
                        success: false,
                        message: "Failed to store image".to_string(),
                    },
                }
            }
        };
        results.insert(artist_name.clone(), outcome);
    }

    Json(UploadArtistsImagesResponse { results })
}

/// Decode `b64` and confirm the bytes are an image this daemon can serve.
///
/// Splitting base64 decoding from the store keeps the pure decision —
/// "is this a usable image" — testable without touching the artist store or
/// the filesystem.
fn decode_image(b64: &str) -> Result<Vec<u8>, String> {
    let bytes = STANDARD
        .decode(b64)
        .map_err(|e| format!("Invalid base64 data: {}", e))?;
    crate::helpers::imageresize::validate(&bytes)
        .map_err(|e| format!("Invalid image data: {}", e))?;
    Ok(bytes)
}

/// Validate one entry of an upload batch before anything is written.
///
/// Returns the trimmed name and the decoded, validated image bytes. The
/// emptiness check runs on the *sanitised* name, because that is the value
/// that decides the storage path: a name like `!!!` is not empty, but it
/// sanitises to nothing and would land on `{user_dir}/artists/custom.jpg`, a
/// path shared by every such name.
fn validate_upload_entry(artist_name: &str, b64data: &str) -> Result<(String, Vec<u8>), String> {
    let name = artist_name.trim();
    if crate::helpers::sanitize::filename_from_string(name).is_empty() {
        return Err("Empty artist name".to_string());
    }
    let bytes = decode_image(b64data)?;
    Ok((name.to_string(), bytes))
}

/// The entries of a batch that lose a name collision, each mapped to the entry
/// that won it.
///
/// Two request keys that sanitise to the same filename name the same stored
/// file. Storing both leaves one image on disk and reports success for two,
/// and which of them survives depends on the iteration order of the request
/// map — so two identical requests could store different images. The batch is
/// collapsed before anything is written instead, and the winner is picked by
/// sorting the keys rather than by that order, which makes the outcome the
/// same every time.
///
/// A name that sanitises to nothing is skipped rather than collided: those
/// entries are refused by `validate_upload_entry` with a message about the
/// name, which is more use to a client than a collision report.
fn colliding_entries<'a, I>(names: I) -> HashMap<String, String>
where
    I: IntoIterator<Item = &'a String>,
{
    use std::collections::hash_map::Entry;

    let mut sorted: Vec<&String> = names.into_iter().collect();
    sorted.sort();

    let mut winner_by_file: HashMap<String, &String> = HashMap::new();
    let mut losers: HashMap<String, String> = HashMap::new();

    for name in sorted {
        let file = crate::helpers::sanitize::filename_from_string(name.trim());
        if file.is_empty() {
            continue;
        }
        match winner_by_file.entry(file) {
            Entry::Vacant(slot) => {
                slot.insert(name);
            }
            Entry::Occupied(winner) => {
                losers.insert(name.clone(), (*winner.get()).clone());
            }
        }
    }

    losers
}

/// Update artist image with custom URL
/// 
/// # Parameters
/// * `artist_b64` - Base64 encoded artist name
/// * `request` - JSON request body containing the image URL
#[post("/artist/<artist_b64>/update", data = "<request>")]
pub fn update_artist_image(artist_b64: String, request: Json<UpdateImageRequest>) -> Json<UpdateImageResponse> {
    debug!("Received artist image update request: artist_b64={}, url={}", artist_b64, request.url);
    
    let artist_name = match decode_url_safe(&artist_b64) {
        Some(name) => name,
        None => {
            warn!("Invalid artist name encoding: {}", artist_b64);
            return Json(UpdateImageResponse {
                success: false,
                message: "Invalid artist name encoding".to_string(),
            });
        }
    };

    debug!("Decoded artist name: {}", artist_name);

    // Store the custom URL in settings database
    let settings_key = format!("artist.image.{}", artist_name);
    debug!("Storing custom image URL in settings: key={}, url={}", settings_key, request.url);
    
    match settingsdb::set_string(&settings_key, &request.url) {
        Ok(_) => {
            info!("Successfully stored custom image URL for artist '{}': {}", artist_name, request.url);
            
            // Clear any cached image to force refresh
            let cache_path = format!("artists/{}/cover.jpg", crate::helpers::url_encoding::encode_url_safe(&artist_name));
            debug!("Attempting to clear cached image at: {}", cache_path);
            
            match std::fs::remove_file(&cache_path) {
                Ok(_) => {
                    debug!("Successfully cleared cached image for artist: {}", artist_name);
                }
                Err(e) => {
                    debug!("No cached image to clear for artist '{}' ({}): {}", artist_name, cache_path, e);
                }
            }
            
            // If URL is not empty, try to trigger immediate download to user directory
            if !request.url.is_empty() {
                debug!("Attempting to trigger immediate download of custom image to user directory for artist: {}", artist_name);
                
                // Use the global artist store to download the image to user directory
                let artist_store = crate::helpers::artist_store::get_artist_store();
                let mut store_lock = artist_store.lock();
                
                match store_lock.download_and_store_user_image(&artist_name, &request.url, "custom") {
                    crate::helpers::artist_store::ArtistImageResult::Found { cache_path } => {
                        info!("Successfully downloaded and stored custom image in user directory for artist '{}': {}", artist_name, cache_path);
                        // `cache_path` here is the real path the new image was just written to
                        // (unlike the bogus one computed above for the dead `remove_file` call).
                        // A re-upload overwrites this same file but leaves any variants generated
                        // from the *previous* image beside it — drop those now so the grid does
                        // not keep showing the old face at thumbnail size.
                        crate::helpers::imageresize::remove_variants_of(&cache_path);
                    }
                    crate::helpers::artist_store::ArtistImageResult::NotFound => {
                        warn!("Failed to download custom image for artist '{}' from URL: {}", artist_name, request.url);
                    }
                    crate::helpers::artist_store::ArtistImageResult::Error(error) => {
                        warn!("Error downloading custom image for artist '{}' from URL {}: {}", artist_name, request.url, error);
                    }
                }
            } else {
                info!("Empty URL provided - custom image cleared for artist: {}", artist_name);
            }
            
            Json(UpdateImageResponse {
                success: true,
                message: format!("Artist image URL updated successfully for '{}'", artist_name),
            })
        }
        Err(e) => {
            error!("Failed to store custom image URL for artist '{}': {}", artist_name, e);
            Json(UpdateImageResponse {
                success: false,
                message: format!("Failed to update artist image: {}", e),
            })
        }
    }
}

/// Build the path of a variant beside an artist-store image.
///
/// The artist store is a separate store from the image cache, but shares its shape:
/// a directory per entity, a file per image. Only the naming convention is shared.
fn variant_path_for(cache_path: &str, size: u32) -> String {
    let path = std::path::Path::new(cache_path);
    let extension = path.extension().and_then(|e| e.to_str()).unwrap_or("jpg");
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("image");
    let parent = path.parent().map(|p| p.to_string_lossy().to_string()).unwrap_or_default();
    format!(
        "{}/{}.{}",
        parent,
        crate::helpers::imageresize::variant_stem(stem, size),
        extension
    )
}

/// Build the base path (directory + variant stem) of a variant, with no extension.
///
/// The stored extension depends on what `resize()` actually encodes (PNG for alpha
/// sources, JPEG otherwise), not on the original's extension — the artist store
/// names every original `.jpg` regardless of content, so a lookup that assumed the
/// original's extension would only ever check `…@<size>.jpg` and would never find a
/// PNG variant that was written for an alpha source. Callers probe both extensions.
fn variant_base_for(cache_path: &str, size: u32) -> String {
    let with_extension = variant_path_for(cache_path, size);
    match with_extension.rsplit_once('.') {
        Some((base, _extension)) => base.to_string(),
        None => with_extension,
    }
}

/// Read a variant of an artist image, generating it beside the original on first use.
///
/// `original` is the bytes of `cache_path`, which the caller has already read; this
/// function must not read the file a second time. A sized request would otherwise
/// pay two full reads of the same image, one of them thrown away.
///
/// Returns `None` whenever anything goes wrong, so the caller falls back to the
/// original rather than failing a request that would otherwise succeed.
fn artist_image_variant(cache_path: &str, original: &[u8], size: u32) -> Option<(Vec<u8>, String)> {
    let base = variant_base_for(cache_path, size);
    let png_path = format!("{}.png", base);
    let jpg_path = format!("{}.jpg", base);

    // The extension on disk depends on what was actually encoded, not on the
    // original's extension, so both must be probed.
    if let Ok(data) = std::fs::read(&png_path) {
        return Some((data, "image/png".to_string()));
    }
    if let Ok(data) = std::fs::read(&jpg_path) {
        return Some((data, "image/jpeg".to_string()));
    }

    match crate::helpers::imageresize::resize(original, size) {
        Ok(crate::helpers::imageresize::Resized::Original) => None,
        Ok(crate::helpers::imageresize::Resized::Image(data, mime)) => {
            // The stored extension must match what was actually encoded, and must be
            // the same path the lookup above would find on a subsequent call.
            let stored_path = if mime == "image/png" { &png_path } else { &jpg_path };
            // Written atomically: two requests for the same artist at the same size
            // can arrive concurrently, and a torn variant served once would be frozen
            // into client caches by its strong ETag.
            if let Err(e) = crate::helpers::imagecache::write_file_atomically(
                std::path::Path::new(stored_path),
                &data,
            ) {
                log::warn!("Failed to store artist image variant {}: {}", stored_path, e);
            }
            Some((data, mime))
        }
        Err(e) => {
            log::debug!("Failed to resize artist image {}: {}", cache_path, e);
            None
        }
    }
}

/// One member of an artist's image set, as reported to a client.
#[derive(Serialize)]
pub struct ArtistImageInfo {
    id: String,
    url: String,
    source: String,
    selected: bool,
    width: u32,
    height: u32,
    size_bytes: u64,
}

/// An artist's whole image set.
#[derive(Serialize)]
pub struct ArtistImagesResponse {
    images: Vec<ArtistImageInfo>,
}

/// List every image stored for an artist, and say which one is selected.
///
/// The set is read from the filesystem on each call rather than cached: it
/// changes only when a client changes it, and an artist holds at most a dozen
/// files.
///
/// Absence is an answer, not an error: an artist nobody has uploaded anything
/// for, or whose name is simply unknown, lists `{"images": []}` with status
/// 200 rather than a 404 — a client asking "what is there" must be able to
/// tell "none" from "the request failed".
///
/// # Parameters
/// * `artist_b64` - Base64 encoded artist name
#[get("/artist/<artist_b64>/images")]
pub fn get_artist_images(artist_b64: String) -> Result<Json<ArtistImagesResponse>, rocket::response::status::Custom<String>> {
    use rocket::http::Status;
    use rocket::response::status::Custom;

    let artist_name = decode_url_safe(&artist_b64)
        .ok_or_else(|| Custom(Status::BadRequest, "Invalid artist name encoding".to_string()))?;

    let store = crate::helpers::artist_store::get_artist_store();
    let store_lock = store.lock();
    let selected = store_lock.selected_image_id(&artist_name);

    // A member whose file cannot be read or measured is dropped rather than
    // failing the whole listing: one bad file must not cost the artist its
    // entire gallery.
    let images = store_lock
        .artist_images(&artist_name)
        .into_iter()
        .filter_map(|image| {
            let metadata = std::fs::metadata(&image.path).ok()?;
            let mut reader = std::io::BufReader::new(std::fs::File::open(&image.path).ok()?);
            let (width, height, _) = crate::helpers::image_meta::detect_image_dimensions(&mut reader).ok()?;
            Some(ArtistImageInfo {
                url: format!("{}/coverart/artist/{}/image/{}", API_PREFIX, artist_b64, image.id),
                selected: selected.as_deref() == Some(image.id.as_str()),
                source: match image.source {
                    ArtistImageSource::Upload => "upload".to_string(),
                    ArtistImageSource::Download => "download".to_string(),
                },
                id: image.id,
                width,
                height,
                size_bytes: metadata.len(),
            })
        })
        .collect();

    Ok(Json(ArtistImagesResponse { images }))
}

/// Read `cache_path`, pick a mime type from its extension, apply the size rung
/// if one was requested, and build the cached HTTP reply.
///
/// Shared by both artist-image routes: they differ only in how `cache_path` is
/// resolved (the selected/downloaded image vs. one named member of the set),
/// not in how that file, once found, is turned into a response.
fn serve_artist_image_file(
    artist_name: &str,
    cache_path: &str,
    target: Option<u32>,
    if_none_match: Option<&str>,
) -> Result<crate::api::imageresponse::ImageReply, rocket::response::status::Custom<String>> {
    use rocket::http::Status;
    use rocket::response::status::Custom;
    use crate::api::imageresponse::{reply, REVALIDATE_DAILY_CACHE};

    match std::fs::read(cache_path) {
        Ok(image_data) => {
            // Determine content type based on file extension
            let mime = if cache_path.ends_with(".png") {
                "image/png"
            } else if cache_path.ends_with(".gif") {
                "image/gif"
            } else if cache_path.ends_with(".webp") {
                "image/webp"
            } else {
                "image/jpeg" // Default to JPEG
            };

            let (image_data, mime) = match target {
                // The original is handed over rather than re-read from disk.
                Some(rung) => artist_image_variant(cache_path, &image_data, rung)
                    .unwrap_or((image_data, mime.to_string())),
                None => (image_data, mime.to_string()),
            };

            debug!("Serving artist image for '{}' from cache: {}", artist_name, cache_path);
            Ok(reply(image_data, &mime, REVALIDATE_DAILY_CACHE, if_none_match))
        },
        Err(e) => {
            log::warn!("Failed to read cached image for artist '{}' at '{}': {}", artist_name, cache_path, e);
            Err(Custom(
                Status::InternalServerError,
                format!("Failed to read cached image: {}", e),
            ))
        }
    }
}

/// Get artist image directly
///
/// This endpoint serves the actual artist image file if available in cache.
/// Returns a 404 if no image is found.
///
/// An optional `size` query parameter requests a downscaled variant, rounded up
/// to the next rung of 100/200/400/800 pixels on the longest edge. Omitting it,
/// or requesting a size above the top rung or the original's own size, serves the
/// original bytes unchanged.
///
/// # Parameters
/// * `artist_b64` - Base64 encoded artist name
/// * `size` - Optional requested size in pixels on the longest edge
#[get("/artist/<artist_b64>/image?<size>")]
pub fn get_artist_image(
    artist_b64: String,
    size: Option<&str>,
    if_none_match: crate::api::imageresponse::IfNoneMatch<'_>,
) -> Result<crate::api::imageresponse::ImageReply, rocket::response::status::Custom<String>> {
    use rocket::http::Status;
    use rocket::response::status::Custom;

    let target = crate::api::library::parse_size(size)
        .map_err(|e| Custom(Status::BadRequest, crate::api::library::size_error_body(&e)))?;

    let artist_name = match decode_url_safe(&artist_b64) {
        Some(decoded) => decoded,
        None => {
            log::warn!("Failed to decode artist parameter: {}", artist_b64);
            return Err(Custom(
                Status::BadRequest,
                "Invalid artist name encoding".to_string(),
            ));
        }
    };

    // Try to get the cached image from the artist store, downloading it first
    // if nothing is cached yet.
    match crate::helpers::artist_store::get_or_download_artist_image(&artist_name) {
        Some(cache_path) => serve_artist_image_file(&artist_name, &cache_path, target, if_none_match.0),
        None => {
            debug!("No cached image found for artist: {}", artist_name);
            Err(Custom(
                Status::NotFound,
                format!("No image found for artist '{}'", artist_name),
            ))
        }
    }
}

/// Serve one specific member of an artist's image set by id.
///
/// Unlike `get_artist_image`, this never downloads anything: an id that is not
/// a member of the set is a 404, full stop, because there is nothing sensible
/// to fall back to — the client asked for a specific picture, not "an" image.
///
/// # Parameters
/// * `artist_b64` - Base64 encoded artist name
/// * `id` - The member's id, as reported by `GET /artist/<artist_b64>/images`
/// * `size` - Optional requested size in pixels on the longest edge
#[get("/artist/<artist_b64>/image/<id>?<size>")]
pub fn get_artist_image_by_id(
    artist_b64: String,
    id: String,
    size: Option<&str>,
    if_none_match: crate::api::imageresponse::IfNoneMatch<'_>,
) -> Result<crate::api::imageresponse::ImageReply, rocket::response::status::Custom<String>> {
    use rocket::http::Status;
    use rocket::response::status::Custom;

    let target = crate::api::library::parse_size(size)
        .map_err(|e| Custom(Status::BadRequest, crate::api::library::size_error_body(&e)))?;

    let artist_name = match decode_url_safe(&artist_b64) {
        Some(decoded) => decoded,
        None => {
            log::warn!("Failed to decode artist parameter: {}", artist_b64);
            return Err(Custom(
                Status::BadRequest,
                "Invalid artist name encoding".to_string(),
            ));
        }
    };

    let cache_path = {
        let store = crate::helpers::artist_store::get_artist_store();
        let store_lock = store.lock();
        store_lock.artist_image_path(&artist_name, &id)
    };

    match cache_path {
        Some(cache_path) => serve_artist_image_file(&artist_name, &cache_path, target, if_none_match.0),
        None => {
            debug!("No image '{}' for artist: {}", id, artist_name);
            Err(Custom(
                Status::NotFound,
                format!("No image '{}' for artist '{}'", id, artist_name),
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::helpers::coverart::DEFAULT_FAST_DEADLINE;
    use image::{DynamicImage, RgbaImage};
    use std::io::Cursor;
    use serial_test::serial;

    /// Encode a test image that has a real alpha channel, matching how the
    /// artist store shapes real files: content decides the format, not the
    /// (always `.jpg`) name it is saved under.
    fn alpha_png(w: u32, h: u32) -> Vec<u8> {
        let img = DynamicImage::ImageRgba8(RgbaImage::from_pixel(w, h, image::Rgba([10, 120, 200, 128])));
        let mut buf = Cursor::new(Vec::new());
        img.write_to(&mut buf, image::ImageFormat::Png).unwrap();
        buf.into_inner()
    }

    /// Repoint the global settings database and artist store at temporary
    /// directories, once for the whole test binary.
    ///
    /// `get_artist_images` and `get_artist_image_by_id` reach the store
    /// through the same process-wide singleton the live daemon uses, rather
    /// than one built for the test, so a fixture must be written through that
    /// singleton too. The singleton is a `Lazy`, so its configuration is fixed
    /// on the first call anywhere in the process; this diverts the settings
    /// keys it reads its directories from before that first call, the same
    /// way `artist_store`'s own test module diverts the settings database
    /// itself.
    fn init_test_artist_store() {
        use std::sync::Once;
        static INIT: Once = Once::new();

        INIT.call_once(|| {
            let settings_dir = tempfile::TempDir::new().expect("settings db temp dir");
            crate::helpers::settingsdb::SettingsDb::initialize_global(settings_dir.path())
                .expect("settings db should initialize");

            let store_dir = tempfile::TempDir::new().expect("artist store temp dir");
            settingsdb::set_string(
                "datastore.artist_store.cache_dir",
                &store_dir.path().join("cache").to_string_lossy(),
            )
            .expect("cache dir setting should store");
            settingsdb::set_string(
                "datastore.user_image_path",
                &store_dir.path().join("user").to_string_lossy(),
            )
            .expect("user dir setting should store");

            // Leaked deliberately: the singleton this sets up outlives any one
            // test, so the directories it points at must too.
            std::mem::forget(settings_dir);
            std::mem::forget(store_dir);
        });
    }

    /// Call the listing route directly, the way a request handler would.
    fn artist_images_response(artist: &str) -> ArtistImagesResponse {
        init_test_artist_store();
        let b64 = crate::helpers::url_encoding::encode_url_safe(artist);
        get_artist_images(b64).expect("the route should not error").into_inner()
    }

    /// Build an artist with two uploaded images, one of them selected.
    ///
    /// Returns the artist's name and the id of the selected member.
    fn artist_with_two_images() -> (String, String) {
        init_test_artist_store();
        let artist = "Gallery Test Artist";

        let store = crate::helpers::artist_store::get_artist_store();
        let mut store_lock = store.lock();
        store_lock
            .store_uploaded_image(artist, &alpha_png(8, 8))
            .expect("the first upload should store");
        let selected_id = store_lock
            .store_uploaded_image(artist, &alpha_png(16, 16))
            .expect("the second upload should store");
        store_lock
            .select_artist_image(artist, &selected_id)
            .expect("the second upload should be selectable");
        drop(store_lock);

        (artist.to_string(), selected_id)
    }

    #[test]
    #[serial]
    fn an_artist_with_no_images_lists_an_empty_set() {
        // Absence of images is an answer, not an error: a client asking "what
        // is there" must be able to tell "none" from "the request failed".
        let response = artist_images_response("Nobody At All");
        assert!(response.images.is_empty());
    }

    #[test]
    #[serial]
    fn the_listing_marks_the_selected_member() {
        let (artist, id) = artist_with_two_images();
        let response = artist_images_response(&artist);

        assert_eq!(response.images.len(), 2);
        let selected: Vec<&ArtistImageInfo> = response.images.iter().filter(|i| i.selected).collect();
        assert_eq!(selected.len(), 1, "exactly one member is selected");
        assert_eq!(selected[0].id, id);
        assert!(selected[0].url.ends_with(&format!("/image/{}", id)));
        assert_eq!(selected[0].source, "upload");
        assert!(selected[0].width > 0 && selected[0].height > 0);
    }

    /// The by-id route must serve a member that is actually in the set.
    #[test]
    #[serial]
    fn by_id_serves_a_known_member() {
        let (artist, id) = artist_with_two_images();
        let b64 = crate::helpers::url_encoding::encode_url_safe(&artist);

        let result = get_artist_image_by_id(b64, id, None, crate::api::imageresponse::IfNoneMatch(None));
        assert!(result.is_ok(), "a known member should be served");
    }

    /// An id that is not a member of the set is a 404. Nothing about this path
    /// touches `get_or_download_artist_image`, so there is no download to
    /// (accidentally) fall back to.
    #[test]
    #[serial]
    fn by_id_404s_for_an_unknown_member() {
        let (artist, _id) = artist_with_two_images();
        let b64 = crate::helpers::url_encoding::encode_url_safe(&artist);

        let result = get_artist_image_by_id(
            b64,
            "not-a-member".to_string(),
            None,
            crate::api::imageresponse::IfNoneMatch(None),
        );
        match result {
            Err(err) => assert_eq!(err.0, rocket::http::Status::NotFound),
            Ok(_) => panic!("an id outside the set must 404"),
        }
    }

    #[test]
    fn variant_paths_sit_beside_the_original() {
        assert_eq!(
            variant_path_for("/var/lib/audiocontrol/cache/artists/Portishead/custom.jpg", 400),
            "/var/lib/audiocontrol/cache/artists/Portishead/custom@400.jpg"
        );
    }

    #[test]
    fn variant_paths_keep_the_extension_they_are_given() {
        assert_eq!(variant_path_for("/cache/artists/A/thumb.png", 100), "/cache/artists/A/thumb@100.png");
    }

    #[test]
    fn alpha_variant_of_a_jpg_named_original_is_cached_and_reused() {
        let dir = tempfile::TempDir::new().expect("temp dir");
        // The artist store names every original `.jpg` regardless of content, so an
        // alpha source living under a `.jpg` filename is exactly the real-world shape.
        let original = dir.path().join("cover.jpg");
        let original_bytes = alpha_png(800, 800);
        std::fs::write(&original, &original_bytes).unwrap();
        let cache_path = original.to_str().unwrap();

        let (first_data, first_mime) = artist_image_variant(cache_path, &original_bytes, 200)
            .expect("an 800px alpha source resized to 200px should produce a variant");
        assert_eq!(first_mime, "image/png");

        // The variant must have been written to the PNG-extensioned path the next
        // lookup will check, not to a `.jpg`-extensioned path assumed from the
        // original's name.
        let variant_path = dir.path().join("cover@200.png");
        assert!(variant_path.exists(), "variant must be stored under the encoded format's extension");

        // Overwrite the stored variant with a sentinel: if the second call reuses
        // the cached file, it must return the sentinel rather than a freshly
        // re-encoded image. This is the assertion that pins the cache-key bug —
        // a test that only checked the returned bytes on both calls would also
        // pass if every call silently re-resized instead of reading the cache.
        std::fs::write(&variant_path, b"SENTINEL").unwrap();

        let (second_data, second_mime) = artist_image_variant(cache_path, &original_bytes, 200)
            .expect("the cached variant should be served on the second call");
        assert_eq!(second_data, b"SENTINEL", "second call must reuse the on-disk variant, not regenerate it");
        assert_eq!(second_mime, "image/png");

        // Sanity: the first call did produce real (non-sentinel) resized data.
        assert_ne!(first_data, b"SENTINEL");
    }

    /// The default is unchanged behaviour: fast providers only. A client that
    /// has always called this endpoint must not start waiting 40 seconds
    /// because a slow provider was configured on the device.
    #[test]
    fn slow_providers_are_excluded_unless_asked_for() {
        assert!(!query_options(None).include_slow);
        assert!(!query_options(Some(false)).include_slow);
        assert!(query_options(Some(true)).include_slow);
    }

    /// The fast deadline is the same either way; opting in changes which
    /// providers run, and the fan-out sizes the deadline from their own
    /// timeouts.
    #[test]
    fn the_fast_deadline_is_unchanged_by_opting_in() {
        assert_eq!(query_options(Some(true)).fast_deadline, DEFAULT_FAST_DEADLINE);
    }

    fn b64(data: &[u8]) -> String {
        use base64::Engine as _;
        STANDARD.encode(data)
    }

    #[test]
    fn decode_image_accepts_a_valid_image() {
        let bytes = alpha_png(400, 400);
        let decoded = decode_image(&b64(&bytes)).expect("a valid base64 image should decode");
        assert_eq!(decoded, bytes);
    }

    #[test]
    fn decode_image_rejects_non_base64() {
        let err = decode_image("!!!not base64!!!").unwrap_err();
        assert!(err.contains("base64"), "expected a base64 error, got: {}", err);
    }

    #[test]
    fn decode_image_rejects_non_image_bytes() {
        let err = decode_image(&b64(b"this is not an image")).unwrap_err();
        assert!(err.contains("image"), "expected an image error, got: {}", err);
    }

    #[test]
    fn decode_image_rejects_empty_bytes() {
        let err = decode_image(&b64(b"")).unwrap_err();
        assert!(err.contains("image"), "expected an image error, got: {}", err);
    }

    #[test]
    fn an_entry_whose_name_sanitises_to_empty_is_rejected() {
        let payload = b64(&alpha_png(400, 400));
        for name in ["!!!", "???", "-", "   "] {
            let err = validate_upload_entry(name, &payload)
                .expect_err("a name that sanitises to empty must be rejected");
            assert!(
                err.contains("Empty"),
                "expected an empty-name error for {:?}, got: {}",
                name,
                err
            );
        }
    }

    #[test]
    fn an_entry_with_a_usable_name_is_accepted() {
        let payload = b64(&alpha_png(400, 400));
        let (name, bytes) = validate_upload_entry("The Beatles", &payload)
            .expect("a normal name with a valid image should be accepted");
        assert_eq!(name, "The Beatles");
        assert!(!bytes.is_empty());
    }

    #[test]
    fn names_that_share_a_file_leave_one_winner() {
        let names: Vec<String> = ["The Beatles", "the beatles!", "  The   Beatles  "]
            .iter()
            .map(|s| s.to_string())
            .collect();

        let losers = colliding_entries(names.iter());

        assert_eq!(losers.len(), 2, "exactly one of the three should be stored: {:?}", losers);
        let winner = "  The   Beatles  "; // first in sort order of the three
        assert!(!losers.contains_key(winner), "the winner must not be reported as a loser");
        for loser in ["The Beatles", "the beatles!"] {
            assert_eq!(losers.get(loser).map(String::as_str), Some(winner));
        }
    }

    /// The winner is chosen by sorting the keys, not by the order the request
    /// map happens to iterate in, so the same batch always stores the same
    /// image.
    #[test]
    fn the_winner_of_a_collision_does_not_depend_on_iteration_order() {
        let forwards: Vec<String> = vec!["a beatles!".to_string(), "A Beatles".to_string()];
        let backwards: Vec<String> = forwards.iter().rev().cloned().collect();

        let losers = colliding_entries(forwards.iter());
        assert_eq!(losers.get("a beatles!").map(String::as_str), Some("A Beatles"));
        assert_eq!(losers, colliding_entries(backwards.iter()));
    }

    #[test]
    fn names_that_do_not_share_a_file_are_left_alone() {
        let names: Vec<String> = ["The Beatles", "The Rolling Stones", "Björk"]
            .iter()
            .map(|s| s.to_string())
            .collect();

        assert!(colliding_entries(names.iter()).is_empty());
    }

    /// A name that sanitises to nothing is refused by `validate_upload_entry`
    /// for being empty; grouping those together would replace that message
    /// with a collision report and hide the real reason.
    #[test]
    fn names_that_sanitise_to_nothing_are_not_treated_as_a_collision() {
        let names: Vec<String> = ["!!!", "???", "-"].iter().map(|s| s.to_string()).collect();

        assert!(colliding_entries(names.iter()).is_empty());
    }
}
