use std::io::Cursor;

use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;
use log::debug;

/// JPEG quality for generated variants. 82 is visually transparent for cover art
/// at grid sizes while staying far below the original's byte count.
const JPEG_QUALITY: u8 = 82;

/// Largest edge, in pixels, this module will decode.
///
/// The bytes reaching `resize` are not trusted: they are provider downloads
/// (fanart.tv and friends) and art embedded in arbitrary files in the user's
/// library, and the pre-warm job decodes every cover in that library unattended
/// after each library load. Until this feature, acr never decoded an image at all —
/// `helpers/image_meta` reads dimensions out of headers precisely to avoid it — so
/// one decompression bomb, or merely one enormous scan, would be a new way to OOM
/// the daemon on a 1GB Pi, repeatedly and with no user action.
///
/// 4096 is generous for cover art: the largest originals measured in a real library
/// are 1280x1280.
const MAX_DECODE_EDGE: u32 = 4096;

/// Ceiling on what a single decode may allocate.
///
/// A 4096x4096 RGBA buffer is 64MiB; this leaves headroom for the decoder's own
/// scratch space while staying far below what a Pi can spare. `image` refuses the
/// decode rather than allocating past it.
const MAX_DECODE_ALLOC: u64 = 128 * 1024 * 1024;

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
///
/// The decode is bounded by [`MAX_DECODE_EDGE`] and [`MAX_DECODE_ALLOC`]. An image
/// beyond either is a `ResizeError::Decode`, which every caller already handles by
/// serving the original — so an oversized source degrades to a full-size response
/// rather than taking the daemon down with it.
pub fn resize(data: &[u8], target_px: u32) -> Result<Resized, ResizeError> {
    let img = decode_within_limits(data)?;

    let longest = img.width().max(img.height());
    if longest <= target_px {
        debug!("Image longest edge {} <= target {}, serving original", longest, target_px);
        return Ok(Resized::Original);
    }

    // CatmullRom, not Lanczos3: Lanczos3 is the most expensive filter the crate
    // offers, and this runs on a Raspberry Pi. For a thumbnail drawn in a grid
    // cell at roughly its own size the two are visually indistinguishable, while
    // CatmullRom is several times faster — and the pre-warm job pays this cost
    // once per rung per album across a whole library, so the difference is
    // measured in hours.
    let scaled = img.resize(target_px, target_px, FilterType::CatmullRom);
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

/// Decode image bytes with explicit resource limits.
///
/// `image::load_from_memory` applies the crate's defaults, which do not bound the
/// image's dimensions at all. Everything a limit violation produces is folded into
/// `ResizeError::Decode`, because to every caller "this image is unusable" and "this
/// image is too big to touch" have the same remedy: serve the original.
fn decode_within_limits(data: &[u8]) -> Result<image::DynamicImage, ResizeError> {
    let mut limits = image::Limits::no_limits();
    limits.max_image_width = Some(MAX_DECODE_EDGE);
    limits.max_image_height = Some(MAX_DECODE_EDGE);
    limits.max_alloc = Some(MAX_DECODE_ALLOC);

    let mut reader = image::ImageReader::new(Cursor::new(data))
        .with_guessed_format()
        .map_err(|e| ResizeError::Decode(e.to_string()))?;
    reader.limits(limits);
    reader.decode().map_err(|e| ResizeError::Decode(e.to_string()))
}

/// The only sizes acr will ever generate.
///
/// A fixed ladder bounds the cache at six variants per image no matter what
/// clients ask for. Requested sizes snap up to the next rung.
///
/// The rungs between 100 and 400 are spaced for grid cells at the display
/// scales clients actually run at: a 100 pt cell is 100, 140, 200 or 280 px at
/// 1x, 1.4x, 2x and 2.8x. Without them a 2x grid snapped all the way up to 400,
/// which is four times the pixels it can show.
pub const SIZE_LADDER: [u32; 6] = [100, 140, 200, 280, 400, 800];

/// The rungs the pre-warm job generates: the grid sizes a client asks for on
/// first scroll, at 1x, 2x and the fractional scales in between.
///
/// The larger rungs are left to be generated on demand — they are asked for one
/// image at a time, when a single cover is opened, not a screenful at once.
pub const PREWARM_SIZES: [u32; 3] = [140, 200, 280];

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
    fn an_image_beyond_the_decode_limit_is_refused_not_decoded() {
        // One edge past MAX_DECODE_EDGE, the other tiny: the file is a few hundred
        // bytes, so this asserts the limit without making the test allocate the
        // thing the limit exists to prevent.
        let src = opaque_png(MAX_DECODE_EDGE + 1, 4);
        let err = resize(&src, 400).unwrap_err();
        assert!(
            matches!(err, ResizeError::Decode(_)),
            "an oversized source must be a Decode error the caller can fall back from, got {:?}",
            err
        );
    }

    #[test]
    fn an_image_at_the_decode_limit_still_resizes() {
        // The boundary is inclusive: 4096 is generous for cover art, and a source
        // that just reaches it must not be refused.
        let src = opaque_png(MAX_DECODE_EDGE, 4);
        assert!(matches!(resize(&src, 400).unwrap(), Resized::Image(_, _)));
    }

    #[test]
    fn snaps_requested_size_up_to_the_next_rung() {
        assert_eq!(snap_to_rung(360), Some(400));
        assert_eq!(snap_to_rung(1), Some(100));
        assert_eq!(snap_to_rung(100), Some(100));
        assert_eq!(snap_to_rung(800), Some(800));
        // The rungs added for fractional display scales: a request must land on
        // the nearest one at or above it, not skip past to 400.
        assert_eq!(snap_to_rung(101), Some(140));
        assert_eq!(snap_to_rung(140), Some(140));
        assert_eq!(snap_to_rung(200), Some(200));
        assert_eq!(snap_to_rung(201), Some(280));
        assert_eq!(snap_to_rung(280), Some(280));
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
        assert_eq!(ladder_fingerprint(), "100-140-200-280-400-800");
    }
}
