use rocket::get;
use rocket::post;
use rocket::serde::json::Json;
use rocket::serde::{Deserialize, Serialize};
use log::{debug, info, warn, error};
use crate::helpers::coverart::{get_coverart_manager, CoverartMethod, CoverartResult, ProviderInfo};
use crate::helpers::url_encoding::decode_url_safe;
use crate::helpers::settingsdb;

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

/// Get cover art for an artist
/// 
/// # Parameters
/// * `artist_b64` - Base64 encoded artist name
#[get("/artist/<artist_b64>")]
pub fn get_artist_coverart(artist_b64: String) -> Json<CoverartResponse> {
    let artist = match decode_url_safe(&artist_b64) {
        Some(decoded) => decoded,
        None => {
            log::warn!("Failed to decode artist parameter: {}", artist_b64);
            return Json(CoverartResponse {
                results: vec![],
            });
        }
    };

    let manager = get_coverart_manager();
    let manager_lock = manager.lock();
    let results = manager_lock.get_artist_coverart(&artist);

    Json(CoverartResponse { results })
}

/// Get cover art for a song
/// 
/// # Parameters
/// * `title_b64` - Base64 encoded song title
/// * `artist_b64` - Base64 encoded artist name
#[get("/song/<title_b64>/<artist_b64>")]
pub fn get_song_coverart(title_b64: String, artist_b64: String) -> Json<CoverartResponse> {
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

    let manager = get_coverart_manager();
    let manager_lock = manager.lock();
    let results = manager_lock.get_song_coverart(&title, &artist);

    Json(CoverartResponse { results })
}

/// Get cover art for an album
/// 
/// # Parameters
/// * `title_b64` - Base64 encoded album title
/// * `artist_b64` - Base64 encoded artist name
/// * `year` - Optional release year
#[get("/album/<title_b64>/<artist_b64>")]
pub fn get_album_coverart(title_b64: String, artist_b64: String) -> Json<CoverartResponse> {
    get_album_coverart_with_year(title_b64, artist_b64, None)
}

/// Get cover art for an album with year
/// 
/// # Parameters
/// * `title_b64` - Base64 encoded album title
/// * `artist_b64` - Base64 encoded artist name
/// * `year` - Release year
#[get("/album/<title_b64>/<artist_b64>/<year>")]
pub fn get_album_coverart_with_year(title_b64: String, artist_b64: String, year: Option<i32>) -> Json<CoverartResponse> {
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

    let manager = get_coverart_manager();
    let manager_lock = manager.lock();
    let results = manager_lock.get_album_coverart(&title, &artist, year);

    Json(CoverartResponse { results })
}

/// Get cover art from a URL
/// 
/// # Parameters
/// * `url_b64` - Base64 encoded URL
#[get("/url/<url_b64>")]
pub fn get_url_coverart(url_b64: String) -> Json<CoverartResponse> {
    let url = match decode_url_safe(&url_b64) {
        Some(decoded) => decoded,
        None => {
            log::warn!("Failed to decode url parameter: {}", url_b64);
            return Json(CoverartResponse {
                results: vec![],
            });
        }
    };

    let manager = get_coverart_manager();
    let manager_lock = manager.lock();
    let results = manager_lock.get_url_coverart(&url);

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
                        remove_artist_image_variants(&cache_path);
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

/// Delete every variant beside an artist image. Called when the original is replaced.
fn remove_artist_image_variants(cache_path: &str) {
    let path = std::path::Path::new(cache_path);
    let (Some(parent), Some(stem)) = (path.parent(), path.file_stem().and_then(|s| s.to_str())) else {
        return;
    };
    let Ok(entries) = std::fs::read_dir(parent) else { return };

    for entry in entries.flatten() {
        let candidate = entry.path();
        let Some(candidate_stem) = candidate.file_stem().and_then(|s| s.to_str()) else { continue };
        if crate::helpers::imageresize::variant_size_of(candidate_stem).is_none() {
            continue;
        }
        if candidate_stem.rsplit_once('@').map(|(base, _)| base) != Some(stem) {
            continue;
        }
        if let Err(e) = std::fs::remove_file(&candidate) {
            log::warn!("Failed to remove artist image variant {}: {}", candidate.display(), e);
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
    use crate::api::imageresponse::{reply, REVALIDATE_DAILY_CACHE};

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

    // Try to get the cached image from the artist store
    match crate::helpers::artist_store::get_or_download_artist_image(&artist_name) {
        Some(cache_path) => {
            // Read the image file
            match std::fs::read(&cache_path) {
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
                        Some(rung) => artist_image_variant(&cache_path, &image_data, rung)
                            .unwrap_or((image_data, mime.to_string())),
                        None => (image_data, mime.to_string()),
                    };

                    debug!("Serving artist image for '{}' from cache: {}", artist_name, cache_path);
                    Ok(reply(image_data, &mime, REVALIDATE_DAILY_CACHE, if_none_match.0))
                },
                Err(e) => {
                    log::warn!("Failed to read cached image for artist '{}' at '{}': {}", artist_name, cache_path, e);
                    Err(Custom(
                        Status::InternalServerError,
                        format!("Failed to read cached image: {}", e),
                    ))
                }
            }
        },
        None => {
            debug!("No cached image found for artist: {}", artist_name);
            Err(Custom(
                Status::NotFound,
                format!("No image found for artist '{}'", artist_name),
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{DynamicImage, RgbaImage};
    use std::io::Cursor;

    /// Encode a test image that has a real alpha channel, matching how the
    /// artist store shapes real files: content decides the format, not the
    /// (always `.jpg`) name it is saved under.
    fn alpha_png(w: u32, h: u32) -> Vec<u8> {
        let img = DynamicImage::ImageRgba8(RgbaImage::from_pixel(w, h, image::Rgba([10, 120, 200, 128])));
        let mut buf = Cursor::new(Vec::new());
        img.write_to(&mut buf, image::ImageFormat::Png).unwrap();
        buf.into_inner()
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
    fn removing_variants_deletes_only_the_matching_base() {
        let dir = tempfile::TempDir::new().expect("temp dir");
        let original = dir.path().join("custom.jpg");
        let stale_variant = dir.path().join("custom@400.jpg");
        let unrelated_variant = dir.path().join("cover@200.jpg");

        std::fs::write(&original, b"original bytes").unwrap();
        std::fs::write(&stale_variant, b"stale thumbnail").unwrap();
        std::fs::write(&unrelated_variant, b"unrelated thumbnail").unwrap();

        remove_artist_image_variants(original.to_str().unwrap());

        assert!(original.exists(), "the original image must survive");
        assert!(!stale_variant.exists(), "the stale variant must be removed");
        assert!(unrelated_variant.exists(), "an unrelated base's variant must survive");
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
}
