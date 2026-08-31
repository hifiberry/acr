use std::io::Cursor;

use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;
use log::debug;

/// JPEG quality for generated variants. 82 is visually transparent for cover art
/// at grid sizes while staying far below the original's byte count.
const JPEG_QUALITY: u8 = 82;

/// The outcome of a resize request.
#[derive(Debug)]
pub enum Resized {
    /// The source is already at or below the target size. Serve it unchanged —
    /// acr never upscales, because a blurred 800px cover is worse than a sharp
    /// 500px one.
    Original,
    /// Re-encoded image data and its MIME type.
    Image(Vec<u8>, String),
}

#[derive(Debug, thiserror::Error)]
pub enum ResizeError {
    #[error("failed to decode image: {0}")]
    Decode(String),
    #[error("failed to encode image: {0}")]
    Encode(String),
}

/// Scale an image so its longest edge is at most `target_px`, preserving aspect ratio.
///
/// Opaque sources are re-encoded as JPEG; sources with an alpha channel stay PNG so
/// transparency survives. Returns `Resized::Original` when the source is already
/// small enough.
pub fn resize(data: &[u8], target_px: u32) -> Result<Resized, ResizeError> {
    let img = image::load_from_memory(data).map_err(|e| ResizeError::Decode(e.to_string()))?;

    let longest = img.width().max(img.height());
    if longest <= target_px {
        debug!("Image longest edge {} <= target {}, serving original", longest, target_px);
        return Ok(Resized::Original);
    }

    let scaled = img.resize(target_px, target_px, FilterType::Lanczos3);
    let has_alpha = img.color().has_alpha();
    let mut buf = Cursor::new(Vec::new());

    if has_alpha {
        scaled
            .write_to(&mut buf, image::ImageFormat::Png)
            .map_err(|e| ResizeError::Encode(e.to_string()))?;
        Ok(Resized::Image(buf.into_inner(), "image/png".to_string()))
    } else {
        // JpegEncoder cannot take RGBA, and an opaque source loses nothing as RGB.
        let rgb = scaled.to_rgb8();
        JpegEncoder::new_with_quality(&mut buf, JPEG_QUALITY)
            .encode_image(&rgb)
            .map_err(|e| ResizeError::Encode(e.to_string()))?;
        Ok(Resized::Image(buf.into_inner(), "image/jpeg".to_string()))
    }
}

/// The only sizes acr will ever generate.
///
/// A fixed ladder bounds the cache at four variants per image no matter what
/// clients ask for. Requested sizes snap up to the next rung.
pub const SIZE_LADDER: [u32; 4] = [100, 200, 400, 800];

/// The rung an album grid uses: 120 pt at 3x. This is the size the pre-warm job
/// generates.
pub const GRID_SIZE: u32 = 400;

/// Separator between a base name and its variant size in a file name.
const VARIANT_MARKER: char = '@';

/// Round a requested size up to the next ladder rung.
///
/// Returns `None` when the request is larger than the biggest rung, which the
/// caller must treat as "serve the original untouched".
pub fn snap_to_rung(requested: u32) -> Option<u32> {
    SIZE_LADDER.iter().copied().find(|rung| *rung >= requested)
}

/// Build the file stem for a variant: `("cover", 400)` becomes `"cover@400"`.
pub fn variant_stem(base_stem: &str, size: u32) -> String {
    format!("{}{}{}", base_stem, VARIANT_MARKER, size)
}

/// Extract the size from a variant stem, or `None` if this is not a variant.
pub fn variant_size_of(stem: &str) -> Option<u32> {
    let (_, size) = stem.rsplit_once(VARIANT_MARKER)?;
    if size.is_empty() || !size.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    size.parse().ok()
}

/// Whether a file name is a generated variant.
///
/// Classification is by name rather than stored metadata, because the filesystem
/// scan that falls back on it runs exactly when metadata is missing.
pub fn is_variant_file_name(file_name: &str) -> bool {
    let stem = std::path::Path::new(file_name)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    variant_size_of(stem).is_some()
}

/// A stable description of the ladder, stored beside the cache so a future change
/// to the rungs can purge variants that no longer correspond to anything.
pub fn ladder_fingerprint() -> String {
    SIZE_LADDER
        .iter()
        .map(|s| s.to_string())
        .collect::<Vec<_>>()
        .join("-")
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{DynamicImage, RgbImage, RgbaImage};
    use std::io::Cursor;

    /// Encode an opaque test image of the given dimensions as PNG.
    fn opaque_png(w: u32, h: u32) -> Vec<u8> {
        let img = DynamicImage::ImageRgb8(RgbImage::from_pixel(w, h, image::Rgb([10, 120, 200])));
        let mut buf = Cursor::new(Vec::new());
        img.write_to(&mut buf, image::ImageFormat::Png).unwrap();
        buf.into_inner()
    }

    /// Encode a test image that has a real alpha channel.
    fn alpha_png(w: u32, h: u32) -> Vec<u8> {
        let img = DynamicImage::ImageRgba8(RgbaImage::from_pixel(w, h, image::Rgba([10, 120, 200, 128])));
        let mut buf = Cursor::new(Vec::new());
        img.write_to(&mut buf, image::ImageFormat::Png).unwrap();
        buf.into_inner()
    }

    fn dimensions(data: &[u8]) -> (u32, u32) {
        let img = image::load_from_memory(data).unwrap();
        (img.width(), img.height())
    }

    #[test]
    fn downscales_longest_edge_to_target() {
        let src = opaque_png(1280, 1280);
        match resize(&src, 400).unwrap() {
            Resized::Image(data, mime) => {
                assert_eq!(dimensions(&data), (400, 400));
                assert_eq!(mime, "image/jpeg");
            }
            Resized::Original => panic!("expected a resized image"),
        }
    }

    #[test]
    fn preserves_aspect_ratio() {
        let src = opaque_png(1000, 500);
        match resize(&src, 400).unwrap() {
            Resized::Image(data, _) => assert_eq!(dimensions(&data), (400, 200)),
            Resized::Original => panic!("expected a resized image"),
        }
    }

    #[test]
    fn never_upscales() {
        let src = opaque_png(300, 300);
        assert!(matches!(resize(&src, 800).unwrap(), Resized::Original));
    }

    #[test]
    fn equal_to_target_is_left_alone() {
        let src = opaque_png(400, 400);
        assert!(matches!(resize(&src, 400).unwrap(), Resized::Original));
    }

    #[test]
    fn alpha_sources_stay_png() {
        let src = alpha_png(1280, 1280);
        match resize(&src, 200).unwrap() {
            Resized::Image(data, mime) => {
                assert_eq!(mime, "image/png");
                let img = image::load_from_memory(&data).unwrap();
                assert!(img.color().has_alpha(), "alpha channel must survive");
            }
            Resized::Original => panic!("expected a resized image"),
        }
    }

    #[test]
    fn corrupt_input_is_an_error_not_a_panic() {
        let err = resize(b"this is not an image", 400).unwrap_err();
        assert!(matches!(err, ResizeError::Decode(_)));
    }

    #[test]
    fn snaps_requested_size_up_to_the_next_rung() {
        assert_eq!(snap_to_rung(360), Some(400));
        assert_eq!(snap_to_rung(1), Some(100));
        assert_eq!(snap_to_rung(100), Some(100));
        assert_eq!(snap_to_rung(800), Some(800));
    }

    #[test]
    fn sizes_above_the_ladder_have_no_rung() {
        assert_eq!(snap_to_rung(801), None);
        assert_eq!(snap_to_rung(4000), None);
    }

    #[test]
    fn variant_stems_round_trip() {
        assert_eq!(variant_stem("cover", 400), "cover@400");
        assert_eq!(variant_size_of("cover@400"), Some(400));
        assert_eq!(variant_size_of("cover"), None);
    }

    #[test]
    fn variant_detection_works_on_file_names() {
        assert!(is_variant_file_name("cover@400.jpg"));
        assert!(!is_variant_file_name("cover.jpg"));
        // An '@' that is not followed only by digits is not a variant marker.
        assert!(!is_variant_file_name("live@wembley.jpg"));
        assert!(!is_variant_file_name("cover@.jpg"));
    }

    #[test]
    fn fingerprint_describes_the_ladder() {
        assert_eq!(ladder_fingerprint(), "100-200-400-800");
    }
}
