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
pub fn image_extension(bytes: &[u8]) -> Option<&'static str> {
    match image::guess_format(bytes).ok()? {
        image::ImageFormat::Jpeg => Some("jpg"),
        image::ImageFormat::Png => Some("png"),
        image::ImageFormat::WebP => Some("webp"),
        _ => None,
    }
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

    #[test]
    fn an_extension_is_sniffed_from_the_bytes() {
        assert_eq!(image_extension(&png_bytes(8, 8)), Some("png"));
        assert_eq!(image_extension(b"<html>not an image</html>"), None);
    }
}
