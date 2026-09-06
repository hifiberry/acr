//! Sniffing an image's format directly from its bytes.

/// The file extension for these bytes, if they are an image we can serve.
///
/// Taken from the content, never from anything a client said: the serving
/// route derives the `Content-Type` from the extension, so a `.jpg` holding a
/// PNG would be served under the wrong type.
///
/// Limited to the formats the upload path can actually decode: the `image`
/// crate backing `imageresize::validate` is built with only jpeg/png/webp
/// support, so accepting a GIF or BMP extension here would store a file the
/// daemon can never resize.
///
/// A recognised magic number alone is not enough: the gate also requires a
/// header the decoder can actually read far enough to report dimensions.
/// That is deliberate, not incidental strictness — the one caller,
/// `store_uploaded_image`, treats `None` as "reject the upload", so a file
/// that is just a signature followed by junk must fail here rather than be
/// stored and fail every later decode instead.
pub fn image_extension(bytes: &[u8]) -> Option<&'static str> {
    let extension = match image::guess_format(bytes).ok()? {
        image::ImageFormat::Jpeg => "jpg",
        image::ImageFormat::Png => "png",
        image::ImageFormat::WebP => "webp",
        _ => return None,
    };

    image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .ok()?
        .into_dimensions()
        .ok()?;

    Some(extension)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png_bytes(w: u32, h: u32) -> Vec<u8> {
        use image::{DynamicImage, RgbaImage};
        let img = DynamicImage::ImageRgba8(RgbaImage::from_pixel(w, h, image::Rgba([10, 120, 200, 255])));
        let mut buf = std::io::Cursor::new(Vec::new());
        img.write_to(&mut buf, image::ImageFormat::Png).unwrap();
        buf.into_inner()
    }

    fn jpeg_bytes(w: u32, h: u32) -> Vec<u8> {
        use image::{DynamicImage, RgbImage};
        let img = DynamicImage::ImageRgb8(RgbImage::from_pixel(w, h, image::Rgb([10, 120, 200])));
        let mut buf = std::io::Cursor::new(Vec::new());
        img.write_to(&mut buf, image::ImageFormat::Jpeg).unwrap();
        buf.into_inner()
    }

    fn webp_bytes(w: u32, h: u32) -> Vec<u8> {
        use image::{DynamicImage, RgbaImage};
        let img = DynamicImage::ImageRgba8(RgbaImage::from_pixel(w, h, image::Rgba([10, 120, 200, 255])));
        let mut buf = std::io::Cursor::new(Vec::new());
        img.write_to(&mut buf, image::ImageFormat::WebP).unwrap();
        buf.into_inner()
    }

    #[test]
    fn an_extension_is_sniffed_from_the_bytes() {
        assert_eq!(image_extension(&png_bytes(8, 8)), Some("png"));
        assert_eq!(image_extension(b"<html>not an image</html>"), None);
    }

    #[test]
    fn a_real_jpeg_is_sniffed_too() {
        assert_eq!(image_extension(&jpeg_bytes(8, 8)), Some("jpg"));
    }

    #[test]
    fn a_real_webp_is_sniffed_too() {
        assert_eq!(image_extension(&webp_bytes(8, 8)), Some("webp"));
    }

    #[test]
    fn a_bare_signature_with_no_readable_header_is_rejected() {
        // The PNG magic number alone: enough for `guess_format`, not enough
        // for the decoder to report dimensions.
        assert_eq!(image_extension(b"\x89PNG\r\n\x1a\n"), None);
    }

    #[test]
    fn a_signature_followed_by_junk_is_rejected() {
        let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
        bytes.extend_from_slice(b"not a real png header at all");
        assert_eq!(image_extension(&bytes), None);
    }

    #[test]
    fn a_webp_signature_followed_by_junk_is_rejected() {
        // "RIFF" + a bogus size, then "WEBP": enough to be guessed as WebP,
        // not enough for the decoder to read a header out of.
        let mut bytes = b"RIFF\x00\x00\x00\x00WEBP".to_vec();
        bytes.extend_from_slice(b"not a real webp chunk at all");
        assert_eq!(image_extension(&bytes), None);
    }

    #[test]
    fn a_jpeg_signature_followed_by_junk_is_rejected() {
        // The JPEG SOI marker plus an APP0 signature: enough for
        // `guess_format`, not enough for the decoder to read a header out of.
        let mut bytes = b"\xff\xd8\xff\xe0".to_vec();
        bytes.extend_from_slice(b"not a real jpeg header at all");
        assert_eq!(image_extension(&bytes), None);
    }
}
