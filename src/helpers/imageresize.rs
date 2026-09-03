use std::io::Cursor;
use std::sync::OnceLock;

use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;
use log::{debug, warn};

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

/// Validate that `data` decodes as an image within this module's limits.
///
/// Used by the upload path to accept a user-supplied image only when it is
/// something the daemon could actually serve. Returns `Ok` for anything the
/// bounded decoder accepts, and a `ResizeError` otherwise.
pub fn validate(data: &[u8]) -> Result<(), ResizeError> {
    decode_within_limits(data).map(|_| ())
}

/// The ladder used when configuration does not specify one.
///
/// A fixed ladder bounds the cache at six variants per image no matter what
/// clients ask for. Requested sizes snap up to the next rung.
///
/// The rungs between 100 and 400 are spaced for grid cells at the display
/// scales clients actually run at: a 100 pt cell is 100, 140, 200 or 280 px at
/// 1x, 1.4x, 2x and 2.8x. Without them a 2x grid snapped all the way up to 400,
/// which is four times the pixels it can show.
pub const DEFAULT_SIZES: [u32; 6] = [100, 140, 200, 280, 400, 800];

/// The pre-warm rungs used when configuration does not specify them.
///
/// The larger rungs are left to be generated on demand — they are asked for one
/// image at a time, when a single cover is opened, not a screenful at once.
pub const DEFAULT_PREWARM_SIZES: [u32; 3] = [140, 200, 280];

static SIZES: OnceLock<Vec<u32>> = OnceLock::new();
static PREWARM: OnceLock<Vec<u32>> = OnceLock::new();

/// The sizes this daemon offers, in ascending order.
///
/// Falls back to the built-in default when configuration was never applied, so
/// the module answers correctly in tests and in any start-up path that has not
/// reached `configure` yet.
pub fn sizes() -> &'static [u32] {
    SIZES.get().map(|v| v.as_slice()).unwrap_or(&DEFAULT_SIZES)
}

/// The sizes the pre-warm job generates. May legitimately be empty.
pub fn prewarm_sizes() -> &'static [u32] {
    PREWARM.get().map(|v| v.as_slice()).unwrap_or(&DEFAULT_PREWARM_SIZES)
}

/// Apply configuration. Call once, during start-up, before anything reads the
/// accessors. A second call is ignored rather than panicking.
pub fn configure(raw_sizes: Option<Vec<u32>>, raw_prewarm: Option<Vec<u32>>) {
    let ladder = match raw_sizes {
        Some(v) => sanitise_sizes(v),
        None => DEFAULT_SIZES.to_vec(),
    };
    let prewarm = match raw_prewarm {
        Some(v) => sanitise_prewarm(v, &ladder),
        None => DEFAULT_PREWARM_SIZES
            .iter()
            .copied()
            .filter(|s| ladder.contains(s))
            .collect(),
    };
    let _ = SIZES.set(ladder);
    let _ = PREWARM.set(prewarm);
}

/// Read a list of sizes out of a JSON config value, warning loudly about anything
/// it must reject rather than dropping it silently.
///
/// `key` names the field being read (e.g. `"sizes"` or `"prewarm_sizes"`), used only
/// to make the warnings actionable. `value` is the `services.images` section, or
/// `None` when that section is absent from configuration altogether. When it is
/// present but not a JSON object (e.g. `"images": "yes"`), that is warned about too
/// rather than silently reading as "key absent".
///
/// Returns `None` when the key is absent, so "absent" (use the default) stays
/// distinguishable from "present but every entry was rejected" (also use the
/// default, but only after warning why). Returns `Some(vec)` — which may be empty —
/// of the entries that survived; `sanitise_sizes` / `sanitise_prewarm` still do
/// dedup, sort, ladder-membership and the empty-list decision on that result exactly
/// as they did before this function existed.
///
/// A value outside `u32`'s range is rejected outright rather than cast: `as u32`
/// wraps, so `4294967396` (2^32 + 100) would silently become `100` — a plausible,
/// wrong rung with no sign anything went wrong.
pub fn sizes_from_json(key: &str, value: Option<&serde_json::Value>) -> Option<Vec<u32>> {
    let images = value?;
    if !images.is_object() {
        warn!(
            "Ignoring 'images' configuration section: expected an object, got {}. Using the default.",
            images
        );
        return None;
    }
    let field = match images.get(key) {
        Some(f) => f,
        None => return None,
    };
    let array = match field.as_array() {
        Some(a) => a,
        None => {
            warn!(
                "Ignoring configuration key '{}': expected an array, got {}. Using the default.",
                key, field
            );
            return None;
        }
    };

    let mut accepted = Vec::new();
    for entry in array {
        match entry.as_u64() {
            Some(n) if n <= u32::MAX as u64 => accepted.push(n as u32),
            Some(n) => warn!(
                "Ignoring image size {} in configuration key '{}': larger than u32::MAX ({})",
                n,
                key,
                u32::MAX
            ),
            None => warn!(
                "Ignoring image size {} in configuration key '{}': not a non-negative integer",
                entry, key
            ),
        }
    }
    Some(accepted)
}

/// Drop invalid and duplicate rungs, sort, and fall back if nothing survives.
///
/// Sorting is not cosmetic: `snap_to_rung` returns the first rung greater than
/// or equal to the request and depends on ascending order.
pub fn sanitise_sizes(raw: Vec<u32>) -> Vec<u32> {
    let mut cleaned: Vec<u32> = Vec::new();
    for size in raw {
        if size == 0 {
            warn!("Ignoring image size 0 in configuration: sizes must be positive");
            continue;
        }
        if cleaned.contains(&size) {
            warn!("Ignoring duplicate image size {} in configuration", size);
            continue;
        }
        cleaned.push(size);
    }
    if cleaned.is_empty() {
        warn!(
            "No usable image sizes in configuration, using the default ladder {:?}",
            DEFAULT_SIZES
        );
        return DEFAULT_SIZES.to_vec();
    }
    cleaned.sort_unstable();
    cleaned
}

/// Keep only pre-warm rungs the ladder actually offers.
///
/// An empty result is legitimate — it means pre-warming is off — so this never
/// falls back to the default.
pub fn sanitise_prewarm(raw: Vec<u32>, ladder: &[u32]) -> Vec<u32> {
    let mut cleaned: Vec<u32> = Vec::new();
    for size in raw {
        if !ladder.contains(&size) {
            warn!(
                "Ignoring pre-warm size {}: it is not in the configured ladder {:?}",
                size, ladder
            );
            continue;
        }
        if cleaned.contains(&size) {
            continue;
        }
        cleaned.push(size);
    }
    cleaned.sort_unstable();
    cleaned
}

/// Separator between a base name and its variant size in a file name.
const VARIANT_MARKER: char = '@';

/// Round a requested size up to the next ladder rung.
///
/// Returns `None` when the request is larger than the biggest rung, which the
/// caller must treat as "serve the original untouched".
pub fn snap_to_rung(requested: u32) -> Option<u32> {
    sizes().iter().copied().find(|rung| *rung >= requested)
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
    sizes()
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
    fn validate_accepts_a_decodable_image() {
        let src = opaque_png(400, 400);
        assert!(validate(&src).is_ok());
        let alpha = alpha_png(400, 400);
        assert!(validate(&alpha).is_ok());
    }

    #[test]
    fn validate_rejects_non_image_bytes() {
        assert!(validate(b"this is definitely not an image").is_err());
    }

    #[test]
    fn validate_rejects_empty_bytes() {
        assert!(validate(b"").is_err());
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

    #[test]
    fn sanitising_drops_zero_negative_and_duplicates_and_sorts() {
        // u32 cannot be negative; zero is the representable invalid value.
        assert_eq!(sanitise_sizes(vec![400, 100, 0, 200, 100]), vec![100, 200, 400]);
    }

    #[test]
    fn an_entirely_invalid_ladder_falls_back_to_the_default() {
        assert_eq!(sanitise_sizes(vec![0, 0]), DEFAULT_SIZES.to_vec());
        assert_eq!(sanitise_sizes(Vec::new()), DEFAULT_SIZES.to_vec());
    }

    #[test]
    fn prewarm_entries_outside_the_ladder_are_dropped() {
        let ladder = vec![100, 200, 400];
        assert_eq!(sanitise_prewarm(vec![200, 999, 100], &ladder), vec![100, 200]);
    }

    #[test]
    fn an_empty_prewarm_list_is_honoured_not_defaulted() {
        // This is how an operator turns pre-warming off. Replacing it with the
        // default would silently override that choice.
        let ladder = vec![100, 200, 400];
        assert!(sanitise_prewarm(Vec::new(), &ladder).is_empty());
    }

    #[test]
    fn without_configuration_the_accessors_return_the_defaults() {
        // The module must answer correctly when config was never initialised,
        // which is what keeps these unit tests meaningful without a fixture.
        assert_eq!(sizes(), DEFAULT_SIZES);
        assert_eq!(prewarm_sizes(), DEFAULT_PREWARM_SIZES);
    }

    #[test]
    fn snapping_uses_the_active_ladder() {
        // 140 is a rung in the default ladder; 120 must snap up to it.
        assert_eq!(snap_to_rung(120), Some(140));
        assert_eq!(snap_to_rung(801), None);
    }

    mod sizes_from_json_tests {
        use super::*;
        use serde_json::json;

        #[test]
        fn absent_images_section_is_none() {
            assert_eq!(sizes_from_json("sizes", None), None);
        }

        #[test]
        fn absent_key_is_none() {
            let images = json!({ "other": 1 });
            assert_eq!(sizes_from_json("sizes", Some(&images)), None);
        }

        #[test]
        fn negative_entries_are_rejected_not_dropped_silently() {
            let images = json!({ "sizes": [100, -50, 400] });
            assert_eq!(sizes_from_json("sizes", Some(&images)), Some(vec![100, 400]));
        }

        #[test]
        fn non_integer_entries_are_rejected() {
            let images = json!({ "sizes": [100, 200.5] });
            assert_eq!(sizes_from_json("sizes", Some(&images)), Some(vec![100]));
        }

        #[test]
        fn a_non_array_value_is_none_not_the_default_indistinguishably() {
            // Distinguishable from "absent" only via the log line the caller cannot
            // see in a unit test, but the point tested here is that it does not
            // silently succeed with a bogus single-element interpretation either.
            let images = json!({ "sizes": 400 });
            assert_eq!(sizes_from_json("sizes", Some(&images)), None);
        }

        #[test]
        fn a_non_object_images_section_is_none() {
            // "images": "yes" -- the section itself is malformed, not just the key.
            // This must fall back to the default (via None) the same as an absent
            // section, but it is asserted here that this path is reached without
            // panicking; the accompanying warning is what makes it distinguishable
            // from a genuinely absent section in the daemon's log.
            let images = json!("yes");
            assert_eq!(sizes_from_json("sizes", Some(&images)), None);
        }

        #[test]
        fn an_out_of_range_value_is_rejected_rather_than_wrapped() {
            // 2^32 + 100. `as u32` would wrap this to 100, a plausible wrong value.
            // It must be rejected outright instead.
            let images = json!({ "sizes": [4294967396_u64] });
            assert_eq!(sizes_from_json("sizes", Some(&images)), Some(Vec::new()));
        }

        #[test]
        fn valid_entries_all_pass_through() {
            let images = json!({ "sizes": [100, 200, 400] });
            assert_eq!(sizes_from_json("sizes", Some(&images)), Some(vec![100, 200, 400]));
        }

        #[test]
        fn empty_array_is_some_empty_not_none() {
            // Distinguishes "present but empty" from "absent" -- sanitise_sizes is
            // the one that decides whether an empty list falls back to the default.
            let images = json!({ "sizes": [] });
            assert_eq!(sizes_from_json("sizes", Some(&images)), Some(Vec::new()));
        }

        #[test]
        fn reads_the_prewarm_key_independently() {
            let images = json!({ "sizes": [100], "prewarm_sizes": [100, -1] });
            assert_eq!(sizes_from_json("prewarm_sizes", Some(&images)), Some(vec![100]));
        }
    }
}
