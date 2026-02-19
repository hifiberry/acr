use rocket::get;
use rocket::http::ContentType;
use rocket::response::status::Custom;
use rocket::http::Status;
use std::path::{Path, PathBuf};
use crate::helpers::imagecache;

/// Retrieve an image from the image cache based on a filepath
///
/// This endpoint provides direct access to images stored in the image cache.
/// The filepath parameter maps to the internal structure of the image cache.
#[get("/<filepath..>")]
pub fn get_image_from_cache(filepath: PathBuf) -> Result<(ContentType, Vec<u8>), Custom<String>> {
    // Log the request
    log::debug!("Request for image cache file: {:?}", filepath);
    
    // Check if image exists in the cache
    if !imagecache::image_exists(&filepath) {
        return Err(Custom(
            Status::NotFound,
            format!("Image '{}' not found in cache", filepath.display()),
        ));
    }

    // Get the image data
    match imagecache::get_image_data(&filepath) {
        Ok(data) => {
            // Detect the content type based on the file extension
            let content_type = detect_content_type(&filepath);
            Ok((content_type, data))
        },
        Err(e) => {
            Err(Custom(
                Status::InternalServerError,
                format!("Failed to retrieve image from cache: {}", e),
            ))
        }
    }
}

/// Detect the content type based on the file extension
fn detect_content_type(path: &Path) -> ContentType {
    match path.extension() {
        Some(ext) if ext == "jpg" || ext == "jpeg" => ContentType::JPEG,
        Some(ext) if ext == "png" => ContentType::PNG,
        Some(ext) if ext == "gif" => ContentType::GIF,
        Some(ext) if ext == "webp" => ContentType::new("image", "webp"),
        Some(ext) if ext == "bmp" => ContentType::new("image", "bmp"),
        Some(ext) if ext == "svg" => ContentType::new("image", "svg+xml"),
        _ => ContentType::Binary, // Default to binary for unknown types
    }
}