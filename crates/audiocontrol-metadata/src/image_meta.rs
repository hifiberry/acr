use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::fs::File;
use log::{debug, warn};
use serde::{Serialize, Deserialize};
use acr_store::attributecache::get_attribute_cache;
use acr_http::http_client::new_http_client;

/// Cache key prefix for image metadata
pub const IMAGE_META_CACHE_PREFIX: &str = "image_meta::";

/// Image metadata containing resolution and size information
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ImageMetadata {
    /// Width in pixels
    pub width: u32,
    /// Height in pixels  
    pub height: u32,
    /// Size in bytes
    pub size_bytes: u64,
    /// Image format (e.g., "JPEG", "PNG", "GIF", "WebP")
    pub format: String,
}

/// Get image metadata (resolution and size) for a given URL
///
/// This function supports remote URLs, local files, and the daemon's own
/// `/api/imagecache/` route. Results are cached using the attribute cache
/// for performance, keyed by the URL and with no expiry, so a URL whose
/// content can change has to invalidate its entry -- see [`forget`].
///
/// # Arguments
/// * `url` - URL or file path to the image
///
/// # Returns
/// * `Result<ImageMetadata, String>` - Image metadata or error message
pub fn image_size(url: &str) -> Result<ImageMetadata, String> {
    // Check cache first
    let cache_key = format!("{}{}", IMAGE_META_CACHE_PREFIX, url);
    
    {
        let mut cache = get_attribute_cache();
        if let Ok(Some(metadata)) = cache.get::<ImageMetadata>(&cache_key) {
            debug!("Retrieved image metadata from cache for: {}", url);
            return Ok(metadata);
        }
    }
    
    // Not in cache, analyze the image
    let metadata = if url.starts_with("http://") || url.starts_with("https://") {
        analyze_remote_image(url)?
    } else if let Some(relative) = acr_store::imagecache::relative_path_from_url(url) {
        let full = acr_store::imagecache::get_full_path(relative);
        analyze_local_image(&full.to_string_lossy())?
    } else {
        analyze_local_image(url)?
    };
    
    // Cache the result
    {
        let mut cache = get_attribute_cache();
        if let Err(e) = cache.set(&cache_key, &metadata) {
            warn!("Failed to cache image metadata for {}: {}", url, e);
        } else {
            debug!("Cached image metadata for: {}", url);
        }
    }
    
    Ok(metadata)
}

/// Forget the cached metadata for one URL.
///
/// [`image_size`] caches by URL with no expiry, which is right for a URL
/// whose content never changes -- the two other producers of
/// `/api/imagecache/` URLs name their files after a hash of the bytes, so new
/// content means a new URL. Localised external cover art deliberately does
/// not: its path is derived from the query so that a re-lookup overwrites its
/// own file instead of accumulating a copy per lookup. Without this, the
/// dimensions measured the first time would be reported for every later
/// image at that path, and the grader would rank the cover on a stale size
/// for as long as the entry lived.
pub(crate) fn forget(url: &str) {
    let cache_key = format!("{}{}", IMAGE_META_CACHE_PREFIX, url);
    match get_attribute_cache().remove(&cache_key) {
        Ok(_) => debug!("Forgot cached image metadata for: {}", url),
        Err(e) => warn!("Failed to forget image metadata for {}: {}", url, e),
    }
}

/// Analyze a local image file
fn analyze_local_image(file_path: &str) -> Result<ImageMetadata, String> {
    let file = File::open(file_path)
        .map_err(|e| format!("Failed to open file {}: {}", file_path, e))?;
    
    let metadata = file.metadata()
        .map_err(|e| format!("Failed to get file metadata: {}", e))?;
    
    let size_bytes = metadata.len();
    let mut reader = BufReader::new(file);
    
    let (width, height, format) = detect_image_dimensions(&mut reader)?;
    
    Ok(ImageMetadata {
        width,
        height,
        size_bytes,
        format,
    })
}

/// Analyze a remote image URL
fn analyze_remote_image(url: &str) -> Result<ImageMetadata, String> {
    debug!("Analyzing remote image: {}", url);
    
    let client = new_http_client(10); // 10 second timeout
    
    // Download the image data  
    let image_data = client.get_binary(url)
        .map_err(|e| format!("Failed to download image from {}: {}", url, e))?;
    
    let (data_bytes, _mime_type) = image_data;
    let size_bytes = data_bytes.len() as u64;
    
    let mut cursor = std::io::Cursor::new(&data_bytes);
    let (width, height, format) = detect_image_dimensions(&mut cursor)?;
    
    Ok(ImageMetadata {
        width,
        height,
        size_bytes,
        format,
    })
}

/// Detect an image's format and dimensions from its first bytes.
///
/// `pub(crate)` rather than private because two callers sniff bytes rather
/// than trust a name. Localised external cover art sniffs what it is about to
/// store: the extension it writes has to come from the image itself, not from
/// a field the endpoint filled in, and bytes that are not a recognised image
/// must be refused rather than served to clients as a broken picture. The
/// artist image listing sniffs what it is about to report, so a file whose
/// header will not parse is dropped from the set instead of being described
/// with invented dimensions.
pub(crate) fn detect_image_dimensions<R: BufRead + Seek>(reader: &mut R) -> Result<(u32, u32, String), String> {
    // Read the first few bytes to determine format
    let mut header = [0u8; 32];
    reader.read_exact(&mut header)
        .map_err(|e| format!("Failed to read image header: {}", e))?;
    
    // Reset to beginning
    reader.seek(SeekFrom::Start(0))
        .map_err(|e| format!("Failed to seek to start: {}", e))?;
    
    // Detect format and parse dimensions
    if is_jpeg(&header) {
        parse_jpeg_dimensions(reader)
    } else if is_png(&header) {
        parse_png_dimensions(reader)
    } else if is_gif(&header) {
        parse_gif_dimensions(reader)
    } else if is_webp(&header) {
        parse_webp_dimensions(reader)
    } else if is_bmp(&header) {
        parse_bmp_dimensions(reader)
    } else {
        Err("Unsupported image format".to_string())
    }
}

/// Check if the header indicates a JPEG image
fn is_jpeg(header: &[u8]) -> bool {
    header.len() >= 2 && header[0] == 0xFF && header[1] == 0xD8
}

/// Check if the header indicates a PNG image
fn is_png(header: &[u8]) -> bool {
    header.len() >= 8 && &header[0..8] == b"\x89PNG\r\n\x1a\n"
}

/// Check if the header indicates a GIF image
fn is_gif(header: &[u8]) -> bool {
    header.len() >= 6 && (&header[0..6] == b"GIF87a" || &header[0..6] == b"GIF89a")
}

/// Check if the header indicates a WebP image
fn is_webp(header: &[u8]) -> bool {
    header.len() >= 12 && &header[0..4] == b"RIFF" && &header[8..12] == b"WEBP"
}

/// The DIB header sizes that exist: BITMAPCOREHEADER, BITMAPINFOHEADER, the
/// two undocumented V2/V3 info headers, BITMAPV4HEADER and BITMAPV5HEADER.
const BMP_DIB_HEADER_SIZES: [u32; 6] = [12, 40, 52, 56, 108, 124];

/// Check if the header indicates a BMP image
///
/// The two-byte magic alone is not enough: any text starting with "BM" has
/// it, and the bytes that follow then read as a plausible header size and as
/// nonsense dimensions. The DIB header size has to be one that exists.
fn is_bmp(header: &[u8]) -> bool {
    header.len() >= 18
        && &header[0..2] == b"BM"
        && BMP_DIB_HEADER_SIZES
            .contains(&u32::from_le_bytes([header[14], header[15], header[16], header[17]]))
}

/// Parse JPEG dimensions
fn parse_jpeg_dimensions<R: BufRead + Seek>(reader: &mut R) -> Result<(u32, u32, String), String> {
    let mut buffer = [0u8; 4];
    
    // Skip past JPEG SOI marker (0xFF 0xD8)
    reader.seek(SeekFrom::Start(2))
        .map_err(|e| format!("Failed to seek in JPEG: {}", e))?;
    
    loop {
        // Read marker
        reader.read_exact(&mut buffer[0..2])
            .map_err(|e| format!("Failed to read JPEG marker: {}", e))?;
        
        if buffer[0] != 0xFF {
            return Err("Invalid JPEG marker".to_string());
        }
        
        let marker = buffer[1];
        
        // SOF markers contain frame information
        if (0xC0..=0xC3).contains(&marker) || (0xC5..=0xC7).contains(&marker) || 
           (0xC9..=0xCB).contains(&marker) || (0xCD..=0xCF).contains(&marker) {
            
            // Read segment length
            reader.read_exact(&mut buffer[0..2])
                .map_err(|e| format!("Failed to read JPEG segment length: {}", e))?;
            
            // Skip precision byte
            reader.read_exact(&mut buffer[0..1])
                .map_err(|e| format!("Failed to read JPEG precision: {}", e))?;
            
            // Read height and width
            reader.read_exact(&mut buffer)
                .map_err(|e| format!("Failed to read JPEG dimensions: {}", e))?;
            
            let height = u32::from_be_bytes([0, 0, buffer[0], buffer[1]]);
            let width = u32::from_be_bytes([0, 0, buffer[2], buffer[3]]);
            
            return Ok((width, height, "JPEG".to_string()));
        }
        
        // Read segment length and skip
        reader.read_exact(&mut buffer[0..2])
            .map_err(|e| format!("Failed to read JPEG segment length: {}", e))?;
        
        let length = u16::from_be_bytes([buffer[0], buffer[1]]) as i64;
        if length < 2 {
            return Err("Invalid JPEG segment length".to_string());
        }
        
        reader.seek(SeekFrom::Current(length - 2))
            .map_err(|e| format!("Failed to skip JPEG segment: {}", e))?;
    }
}

/// Parse PNG dimensions
fn parse_png_dimensions<R: BufRead + Seek>(reader: &mut R) -> Result<(u32, u32, String), String> {
    // Skip PNG signature (8 bytes)
    reader.seek(SeekFrom::Start(8))
        .map_err(|e| format!("Failed to seek in PNG: {}", e))?;
    
    // Read IHDR chunk length (should be 13)
    let mut buffer = [0u8; 8];
    reader.read_exact(&mut buffer)
        .map_err(|e| format!("Failed to read PNG IHDR: {}", e))?;
    
    // Verify this is IHDR chunk
    if &buffer[4..8] != b"IHDR" {
        return Err("Expected PNG IHDR chunk".to_string());
    }
    
    // Read width and height
    reader.read_exact(&mut buffer)
        .map_err(|e| format!("Failed to read PNG dimensions: {}", e))?;
    
    let width = u32::from_be_bytes([buffer[0], buffer[1], buffer[2], buffer[3]]);
    let height = u32::from_be_bytes([buffer[4], buffer[5], buffer[6], buffer[7]]);
    
    Ok((width, height, "PNG".to_string()))
}

/// Parse GIF dimensions
fn parse_gif_dimensions<R: BufRead + Seek>(reader: &mut R) -> Result<(u32, u32, String), String> {
    // Skip GIF signature (6 bytes)
    reader.seek(SeekFrom::Start(6))
        .map_err(|e| format!("Failed to seek in GIF: {}", e))?;
    
    // Read logical screen descriptor
    let mut buffer = [0u8; 4];
    reader.read_exact(&mut buffer)
        .map_err(|e| format!("Failed to read GIF dimensions: {}", e))?;
    
    let width = u32::from_le_bytes([buffer[0], buffer[1], 0, 0]);
    let height = u32::from_le_bytes([buffer[2], buffer[3], 0, 0]);
    
    Ok((width, height, "GIF".to_string()))
}

/// Parse WebP dimensions
fn parse_webp_dimensions<R: BufRead + Seek>(reader: &mut R) -> Result<(u32, u32, String), String> {
    // Skip RIFF header (12 bytes: "RIFF" + size + "WEBP")
    reader.seek(SeekFrom::Start(12))
        .map_err(|e| format!("Failed to seek in WebP: {}", e))?;
    
    // Read chunk header
    let mut buffer = [0u8; 8];
    reader.read_exact(&mut buffer)
        .map_err(|e| format!("Failed to read WebP chunk: {}", e))?;
    
    let chunk_type = &buffer[0..4];
    
    match chunk_type {
        b"VP8 " => {
            // Simple VP8 format
            reader.seek(SeekFrom::Current(6))
                .map_err(|e| format!("Failed to seek in VP8: {}", e))?;
            
            reader.read_exact(&mut buffer[0..4])
                .map_err(|e| format!("Failed to read VP8 dimensions: {}", e))?;
            
            let width = (u16::from_le_bytes([buffer[0], buffer[1]]) & 0x3FFF) as u32;
            let height = (u16::from_le_bytes([buffer[2], buffer[3]]) & 0x3FFF) as u32;
            
            Ok((width, height, "WebP".to_string()))
        }
        b"VP8L" => {
            // Lossless VP8L format
            reader.seek(SeekFrom::Current(1))
                .map_err(|e| format!("Failed to seek in VP8L: {}", e))?;
            
            reader.read_exact(&mut buffer[0..4])
                .map_err(|e| format!("Failed to read VP8L dimensions: {}", e))?;
            
            let bits = u32::from_le_bytes([buffer[0], buffer[1], buffer[2], buffer[3]]);
            let width = (bits & 0x3FFF) + 1;
            let height = ((bits >> 14) & 0x3FFF) + 1;
            
            Ok((width, height, "WebP".to_string()))
        }
        b"VP8X" => {
            // Extended VP8X format
            reader.seek(SeekFrom::Current(4))
                .map_err(|e| format!("Failed to seek in VP8X: {}", e))?;
            
            reader.read_exact(&mut buffer[0..6])
                .map_err(|e| format!("Failed to read VP8X dimensions: {}", e))?;
            
            let width = (u32::from_le_bytes([buffer[0], buffer[1], buffer[2], 0]) & 0xFFFFFF) + 1;
            let height = (u32::from_le_bytes([buffer[3], buffer[4], buffer[5], 0]) & 0xFFFFFF) + 1;
            
            Ok((width, height, "WebP".to_string()))
        }
        _ => Err("Unsupported WebP format".to_string()),
    }
}

/// The largest width or height a BMP is believed to have. Far above any
/// real cover, and low enough that a header full of arbitrary bytes fails.
const MAX_BMP_DIMENSION: u32 = 65_535;

/// Parse BMP dimensions
fn parse_bmp_dimensions<R: BufRead + Seek>(reader: &mut R) -> Result<(u32, u32, String), String> {
    // Skip BMP file header (14 bytes)
    reader.seek(SeekFrom::Start(14))
        .map_err(|e| format!("Failed to seek in BMP: {}", e))?;
    
    // Read DIB header size
    let mut buffer = [0u8; 4];
    reader.read_exact(&mut buffer)
        .map_err(|e| format!("Failed to read BMP DIB header size: {}", e))?;
    
    let header_size = u32::from_le_bytes(buffer);

    let (width, height) = if header_size == 12 {
        // BITMAPCOREHEADER: two unsigned 16-bit fields.
        reader.read_exact(&mut buffer)
            .map_err(|e| format!("Failed to read BMP dimensions: {}", e))?;
        (
            u16::from_le_bytes([buffer[0], buffer[1]]) as u32,
            u16::from_le_bytes([buffer[2], buffer[3]]) as u32,
        )
    } else if BMP_DIB_HEADER_SIZES.contains(&header_size) {
        // Read width and height. Both are signed: a negative height marks a
        // top-down bitmap, and read as unsigned it came out near four billion.
        reader.read_exact(&mut buffer)
            .map_err(|e| format!("Failed to read BMP width: {}", e))?;
        let width = i32::from_le_bytes(buffer);

        reader.read_exact(&mut buffer)
            .map_err(|e| format!("Failed to read BMP height: {}", e))?;
        let height = i32::from_le_bytes(buffer);

        if width <= 0 {
            return Err("Invalid BMP dimensions".to_string());
        }
        (width as u32, height.unsigned_abs())
    } else {
        return Err("Invalid BMP header size".to_string());
    };

    if width == 0 || height == 0 || width > MAX_BMP_DIMENSION || height > MAX_BMP_DIMENSION {
        return Err("Invalid BMP dimensions".to_string());
    }

    Ok((width, height, "BMP".to_string()))
}

/// Clear cached image metadata for a specific URL
pub fn clear_image_cache(url: &str) -> Result<(), String> {
    let cache_key = format!("{}{}", IMAGE_META_CACHE_PREFIX, url);
    
    {
        let mut cache = get_attribute_cache();
        cache.remove(&cache_key)
            .map_err(|e| format!("Failed to clear image cache for {}: {}", url, e))?;
        debug!("Cleared image metadata cache for: {}", url);
    }
    
    Ok(())
}

/// Get cached image metadata without analyzing the image
pub fn get_cached_image_size(url: &str) -> Option<ImageMetadata> {
    let cache_key = format!("{}{}", IMAGE_META_CACHE_PREFIX, url);
    
    {
        let mut cache = get_attribute_cache();
        if let Ok(Some(metadata)) = cache.get::<ImageMetadata>(&cache_key) {
            debug!("Retrieved cached image metadata for: {}", url);
            return Some(metadata);
        }
    }
    
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn test_jpeg_detection() {
        let jpeg_header = [0xFF, 0xD8, 0xFF, 0xE0];
        assert!(is_jpeg(&jpeg_header));
        
        let not_jpeg = [0x89, 0x50, 0x4E, 0x47];
        assert!(!is_jpeg(&not_jpeg));
    }

    #[test]
    fn test_png_detection() {
        let png_header = b"\x89PNG\r\n\x1a\n";
        assert!(is_png(png_header));
        
        let not_png = [0xFF, 0xD8, 0xFF, 0xE0];
        assert!(!is_png(&not_png));
    }

    #[test]
    fn test_gif_detection() {
        let gif87_header = b"GIF87a";
        let gif89_header = b"GIF89a";
        assert!(is_gif(gif87_header));
        assert!(is_gif(gif89_header));
        
        let not_gif = [0xFF, 0xD8, 0xFF, 0xE0];
        assert!(!is_gif(&not_gif));
    }

    #[test]
    fn test_webp_detection() {
        let webp_header = b"RIFF\x00\x00\x00\x00WEBP";
        assert!(is_webp(webp_header));
        
        let not_webp = [0xFF, 0xD8, 0xFF, 0xE0];
        assert!(!is_webp(&not_webp));
    }

    /// A BMP file header followed by a BITMAPINFOHEADER's first three
    /// fields: enough for detection and for reading the dimensions.
    fn bmp_with(header_size: u32, width: i32, height: i32) -> Vec<u8> {
        let mut data = vec![0u8; 14];
        data[0..2].copy_from_slice(b"BM");
        data.extend_from_slice(&header_size.to_le_bytes());
        data.extend_from_slice(&width.to_le_bytes());
        data.extend_from_slice(&height.to_le_bytes());
        data.resize(54, 0);
        data
    }

    #[test]
    fn test_bmp_detection() {
        assert!(is_bmp(&bmp_with(40, 2, 2)));
        assert!(is_bmp(&bmp_with(124, 2, 2)));

        let not_bmp = [0xFF, 0xD8, 0xFF, 0xE0];
        assert!(!is_bmp(&not_bmp));
    }

    /// The magic is two ASCII letters, so plain text carries it too. The
    /// bytes after it then have to pass for a DIB header size, and text does
    /// not.
    #[test]
    fn text_that_starts_with_bm_is_not_a_bmp() {
        let text = b"BMW 2002 turbo: an owner's handbook, first edition";
        assert!(!is_bmp(&text[..32]));
        assert!(detect_image_dimensions(&mut Cursor::new(&text[..])).is_err());

        // The magic on its own, with nothing after it to check.
        assert!(!is_bmp(b"BM"));
    }

    #[test]
    fn bmp_dimensions_are_read_bottom_up() {
        let mut cursor = Cursor::new(bmp_with(40, 640, 480));
        assert_eq!(
            detect_image_dimensions(&mut cursor),
            Ok((640, 480, "BMP".to_string()))
        );
    }

    /// A top-down BMP stores its height negative. Read as unsigned that is
    /// 4294966816 rows, not 480.
    #[test]
    fn a_top_down_bmp_reports_its_real_height() {
        let mut cursor = Cursor::new(bmp_with(40, 640, -480));
        assert_eq!(
            parse_bmp_dimensions(&mut cursor),
            Ok((640, 480, "BMP".to_string()))
        );
    }

    #[test]
    fn a_core_header_bmp_reads_its_sixteen_bit_dimensions() {
        let mut data = vec![0u8; 14];
        data[0..2].copy_from_slice(b"BM");
        data.extend_from_slice(&12u32.to_le_bytes());
        data.extend_from_slice(&320u16.to_le_bytes());
        data.extend_from_slice(&200u16.to_le_bytes());
        data.resize(32, 0);

        assert_eq!(
            detect_image_dimensions(&mut Cursor::new(data)),
            Ok((320, 200, "BMP".to_string()))
        );
    }

    #[test]
    fn a_bmp_with_impossible_dimensions_is_refused() {
        for (width, height) in [(0, 480), (640, 0), (-640, 480), (1_000_000, 480), (640, i32::MIN)] {
            let mut cursor = Cursor::new(bmp_with(40, width, height));
            assert!(
                parse_bmp_dimensions(&mut cursor).is_err(),
                "{}x{} must be refused",
                width,
                height
            );
        }
    }

    #[test]
    fn a_bmp_with_an_unknown_header_size_is_refused() {
        let mut cursor = Cursor::new(bmp_with(16, 640, 480));
        assert!(parse_bmp_dimensions(&mut cursor).is_err());
    }

    /// SOI, an APP0 segment to skip, then a baseline SOF0 frame header.
    fn jpeg_with(sof_marker: u8, width: u16, height: u16) -> Vec<u8> {
        let mut data = vec![0xFF, 0xD8];
        // APP0, length 16 (the two length bytes plus 14 of payload).
        data.extend_from_slice(&[0xFF, 0xE0, 0x00, 0x10]);
        data.extend_from_slice(&[0u8; 14]);
        // SOFn: length 17, precision 8, height, width, then components.
        data.extend_from_slice(&[0xFF, sof_marker, 0x00, 0x11, 0x08]);
        data.extend_from_slice(&height.to_be_bytes());
        data.extend_from_slice(&width.to_be_bytes());
        data.extend_from_slice(&[0u8; 10]);
        data
    }

    #[test]
    fn jpeg_dimensions_are_read_from_the_frame_header() {
        let mut cursor = Cursor::new(jpeg_with(0xC0, 1200, 800));
        assert_eq!(
            detect_image_dimensions(&mut cursor),
            Ok((1200, 800, "JPEG".to_string()))
        );
    }

    #[test]
    fn progressive_jpeg_dimensions_are_read_too() {
        let mut cursor = Cursor::new(jpeg_with(0xC2, 500, 1000));
        assert_eq!(
            parse_jpeg_dimensions(&mut cursor),
            Ok((500, 1000, "JPEG".to_string()))
        );
    }

    /// A DHT segment (0xC4) sits inside the SOF marker range without being
    /// one, and has to be skipped like any other segment.
    #[test]
    fn a_huffman_table_before_the_frame_header_is_skipped() {
        let mut data = vec![0xFF, 0xD8];
        data.extend_from_slice(&[0xFF, 0xC4, 0x00, 0x06, 1, 2, 3, 4]);
        data.extend_from_slice(&jpeg_with(0xC0, 300, 200)[2..]);

        assert_eq!(
            parse_jpeg_dimensions(&mut Cursor::new(data)),
            Ok((300, 200, "JPEG".to_string()))
        );
    }

    #[test]
    fn a_jpeg_that_ends_before_its_frame_header_is_an_error() {
        let mut data = jpeg_with(0xC0, 1200, 800);
        data.truncate(20);
        assert!(parse_jpeg_dimensions(&mut Cursor::new(data)).is_err());
    }

    #[test]
    fn a_jpeg_with_a_broken_marker_is_an_error() {
        let data = [0xFF, 0xD8, 0x00, 0x00, 0x00, 0x00];
        assert!(parse_jpeg_dimensions(&mut Cursor::new(&data[..])).is_err());
    }

    /// A RIFF/WEBP container around one chunk with the given payload.
    fn webp_with(chunk: &[u8; 4], payload: &[u8]) -> Vec<u8> {
        let mut data = Vec::new();
        data.extend_from_slice(b"RIFF");
        data.extend_from_slice(&((4 + 8 + payload.len()) as u32).to_le_bytes());
        data.extend_from_slice(b"WEBP");
        data.extend_from_slice(chunk);
        data.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        data.extend_from_slice(payload);
        // Padded so the 32-byte format sniff has enough to read.
        if data.len() < 32 {
            data.resize(32, 0);
        }
        data
    }

    #[test]
    fn lossy_webp_dimensions_are_read_from_the_vp8_frame_header() {
        // Frame tag (3 bytes), start code (3 bytes), then 14-bit width and
        // height, each with a 2-bit scale above it that must be masked off.
        let mut payload = vec![0x00, 0x00, 0x00, 0x9D, 0x01, 0x2A];
        payload.extend_from_slice(&(640u16 | 0x4000).to_le_bytes());
        payload.extend_from_slice(&(480u16 | 0x8000).to_le_bytes());

        let mut cursor = Cursor::new(webp_with(b"VP8 ", &payload));
        assert_eq!(
            detect_image_dimensions(&mut cursor),
            Ok((640, 480, "WebP".to_string()))
        );
    }

    #[test]
    fn lossless_webp_dimensions_are_read_from_the_vp8l_header() {
        // Signature byte, then width-1 and height-1 packed 14 bits each.
        let bits: u32 = (800 - 1) | ((600 - 1) << 14);
        let mut payload = vec![0x2F];
        payload.extend_from_slice(&bits.to_le_bytes());

        let mut cursor = Cursor::new(webp_with(b"VP8L", &payload));
        assert_eq!(
            parse_webp_dimensions(&mut cursor),
            Ok((800, 600, "WebP".to_string()))
        );
    }

    #[test]
    fn extended_webp_dimensions_are_read_from_the_vp8x_header() {
        // Flags and three reserved bytes, then width-1 and height-1 as
        // 24-bit little-endian values.
        let mut payload = vec![0x10, 0x00, 0x00, 0x00];
        payload.extend_from_slice(&(3000u32 - 1).to_le_bytes()[..3]);
        payload.extend_from_slice(&(2000u32 - 1).to_le_bytes()[..3]);

        let mut cursor = Cursor::new(webp_with(b"VP8X", &payload));
        assert_eq!(
            parse_webp_dimensions(&mut cursor),
            Ok((3000, 2000, "WebP".to_string()))
        );
    }

    #[test]
    fn a_webp_with_an_unknown_first_chunk_is_an_error() {
        let mut cursor = Cursor::new(webp_with(b"ALPH", &[0u8; 10]));
        assert!(parse_webp_dimensions(&mut cursor).is_err());
    }

    #[test]
    fn test_png_dimensions_parsing() {
        // Minimal PNG with 100x50 dimensions
        let png_data = [
            0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, // PNG signature
            0x00, 0x00, 0x00, 0x0D, // IHDR length (13)
            0x49, 0x48, 0x44, 0x52, // "IHDR"
            0x00, 0x00, 0x00, 0x64, // Width: 100
            0x00, 0x00, 0x00, 0x32, // Height: 50
        ];
        
        let mut cursor = Cursor::new(&png_data);
        let result = parse_png_dimensions(&mut cursor);
        assert!(result.is_ok());
        
        let (width, height, format) = result.unwrap();
        assert_eq!(width, 100);
        assert_eq!(height, 50);
        assert_eq!(format, "PNG");
    }

    #[test]
    fn test_gif_dimensions_parsing() {
        // Minimal GIF with 200x150 dimensions
        let gif_data = [
            0x47, 0x49, 0x46, 0x38, 0x39, 0x61, // "GIF89a"
            0xC8, 0x00, // Width: 200 (little-endian)
            0x96, 0x00, // Height: 150 (little-endian)
        ];
        
        let mut cursor = Cursor::new(&gif_data);
        let result = parse_gif_dimensions(&mut cursor);
        assert!(result.is_ok());
        
        let (width, height, format) = result.unwrap();
        assert_eq!(width, 200);
        assert_eq!(height, 150);
        assert_eq!(format, "GIF");
    }

    #[test]
    fn test_image_metadata_serialization() {
        let metadata = ImageMetadata {
            width: 1920,
            height: 1080,
            size_bytes: 524288,
            format: "JPEG".to_string(),
        };
        
        // Test JSON serialization
        let json = serde_json::to_string(&metadata).unwrap();
        let deserialized: ImageMetadata = serde_json::from_str(&json).unwrap();
        
        assert_eq!(metadata, deserialized);
    }

    #[test]
    fn test_google_logo_url_without_cache() {
        // Test the image analysis functionality without relying on cache
        let url = "https://www.google.com/images/branding/googlelogo/1x/googlelogo_color_272x92dp.png";
        
        // First call should analyze the image
        println!("Making first call to image_size (without cache dependency)...");
        let result1 = image_size(url);
        assert!(result1.is_ok(), "Failed to get image metadata: {:?}", result1.err());
        
        let metadata1 = result1.unwrap();
        println!("Got metadata1: {:?}", metadata1);
        assert_eq!(metadata1.width, 272);
        assert_eq!(metadata1.height, 92);
        assert_eq!(metadata1.format, "PNG");
        assert!(metadata1.size_bytes > 0);
        
        // Second call should also work (even if cache is disabled)
        println!("Making second call to image_size...");
        let result2 = image_size(url);
        assert!(result2.is_ok(), "Failed to get image metadata on second call: {:?}", result2.err());
        
        let metadata2 = result2.unwrap();
        println!("Got metadata2: {:?}", metadata2);
        assert_eq!(metadata1.width, metadata2.width);
        assert_eq!(metadata1.height, metadata2.height);
        assert_eq!(metadata1.format, metadata2.format);
        // Size should be similar (might vary slightly due to network conditions)
        assert!(metadata2.size_bytes > 0);
        
        println!("Image analysis test completed successfully");
    }

    #[test]
    fn test_google_logo_url_with_temp_cache() {
        // Create a test using a temporary directory for the cache
        use acr_store::attributecache::AttributeCache;
        
        let url = "https://www.google.com/images/branding/googlelogo/1x/googlelogo_color_272x92dp.png";
        
        // Create temporary cache
        let temp_dir = std::env::temp_dir().join(format!("audiocontrol_test_{}", std::process::id()));
        std::fs::create_dir_all(&temp_dir).unwrap();
        let db_file = temp_dir.join("test_attributes.db");
        let mut cache = AttributeCache::with_database_file(&db_file);
        
        println!("Test cache enabled: {}", cache.is_enabled());
        assert!(cache.is_enabled(), "Test cache should be enabled");
        
        // Test direct cache operations
        let cache_key = format!("{}{}", IMAGE_META_CACHE_PREFIX, url);
        let test_metadata = ImageMetadata {
            width: 272,
            height: 92,
            size_bytes: 5969,
            format: "PNG".to_string(),
        };
        
        // Test cache set
        let set_result = cache.set(&cache_key, &test_metadata);
        println!("Cache set result: {:?}", set_result);
        assert!(set_result.is_ok(), "Failed to set cache: {:?}", set_result.err());
        
        // Test cache get
        let get_result = cache.get::<ImageMetadata>(&cache_key);
        println!("Cache get result: {:?}", get_result);
        assert!(get_result.is_ok(), "Failed to get from cache: {:?}", get_result.err());
        
        let retrieved = get_result.unwrap();
        assert!(retrieved.is_some(), "Should have retrieved cached metadata");
        assert_eq!(test_metadata, retrieved.unwrap(), "Retrieved metadata should match");
        
        // Test cache remove
        let remove_result = cache.remove(&cache_key);
        println!("Cache remove result: {:?}", remove_result);
        assert!(remove_result.is_ok(), "Failed to remove from cache: {:?}", remove_result.err());
        
        // Verify it's gone
        let get_after_remove = cache.get::<ImageMetadata>(&cache_key);
        assert!(get_after_remove.is_ok());
        assert!(get_after_remove.unwrap().is_none(), "Cache should be empty after removal");
        
        // Clean up
        std::fs::remove_dir_all(&temp_dir).ok();

        println!("Temporary cache test completed successfully");
    }

    /// A 1x1 PNG, as bytes, so the test needs no fixture file.
    fn tiny_png() -> Vec<u8> {
        vec![
            0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48,
            0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00,
            0x00, 0x1F, 0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41, 0x54, 0x78,
            0x9C, 0x63, 0x00, 0x01, 0x00, 0x00, 0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00,
            0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
        ]
    }

    /// `/api/imagecache/...` is neither an http URL nor a filesystem path.
    /// It went to `analyze_local_image`, which tried to `File::open` it and
    /// failed every time -- so a locally served image graded with no
    /// dimensions at all. Shairport has been producing these URLs since it
    /// was written.
    #[test]
    fn an_imagecache_url_is_measured_from_the_cache() {
        // The file is written straight to the path the cache would put it at,
        // rather than through `store_image`. What is under test is resolving
        // the URL to the right file and measuring it; `store_image`
        // additionally records metadata in the process-wide attribute cache,
        // and other tests in this binary leave that singleton pointing
        // somewhere unwritable, which failed this test for a reason that had
        // nothing to do with what it asserts.
        //
        // The name carries a timestamp because `std::process::id()` is the
        // same for every test in a binary, and because a stale metadata entry
        // for this URL would otherwise be answered from cache instead of
        // measured.
        crate::test_support::init_test_caches();
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("a clock after the epoch")
            .as_nanos();
        let relative = format!("test_image_meta/{}.png", unique);
        let full = acr_store::imagecache::get_full_path(&relative);
        std::fs::create_dir_all(full.parent().expect("a parent directory"))
            .expect("the cache directory is creatable");
        std::fs::write(&full, tiny_png()).expect("the image file is written");

        let url = format!("{}/imagecache/{}", acr_types::API_PREFIX, relative);
        let metadata = image_size(&url).expect("a cached image is measurable");

        assert_eq!((metadata.width, metadata.height), (1, 1));

        let _ = std::fs::remove_file(&full);
    }

    /// A PNG of the given size, so a test can change an image's dimensions
    /// without a fixture file.
    fn png_of(width: u32, height: u32) -> Vec<u8> {
        use std::io::Cursor;
        let img = image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
            width,
            height,
            image::Rgb([10, 20, 30]),
        ));
        let mut buf = Cursor::new(Vec::new());
        img.write_to(&mut buf, image::ImageFormat::Png)
            .expect("a PNG is encodable");
        buf.into_inner()
    }

    /// Metadata is cached by URL with no expiry, which is only safe while a
    /// URL's content cannot change. Localised external cover art reuses one
    /// path per query so a re-lookup overwrites its own file, so it can and
    /// does change -- and the grader would otherwise rank a replaced cover on
    /// the size of whatever was measured first.
    #[test]
    fn forgetting_a_url_lets_changed_content_be_measured_again() {
        crate::test_support::init_test_caches();
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("a clock after the epoch")
            .as_nanos();
        let relative = format!("test_image_meta/forget-{}.png", unique);
        let full = acr_store::imagecache::get_full_path(&relative);
        std::fs::create_dir_all(full.parent().expect("a parent directory"))
            .expect("the cache directory is creatable");
        let url = format!("{}/imagecache/{}", acr_types::API_PREFIX, relative);

        std::fs::write(&full, png_of(8, 8)).expect("the first image is written");
        assert_eq!(image_size(&url).expect("measurable").width, 8);

        // Same URL, different content. The cached answer is stale now.
        std::fs::write(&full, png_of(64, 64)).expect("the second image is written");
        assert_eq!(
            image_size(&url).expect("measurable").width,
            8,
            "this is the staleness `forget` exists to clear; if this ever \
             reports 64, the metadata cache gained an expiry and \
             `localize::store_locally` need not call `forget` any more"
        );

        forget(&url);
        assert_eq!(
            image_size(&url).expect("measurable").width,
            64,
            "after forgetting, the replaced image must be measured afresh"
        );

        let _ = std::fs::remove_file(&full);
    }

    #[test]
    fn a_plain_relative_path_is_still_treated_as_a_file() {
        // Not under the imagecache route, so the old behaviour stands: this
        // is a filesystem path, and a missing one is an error rather than a
        // silent zero-sized answer.
        assert!(image_size("definitely/not/a/real/file.png").is_err());
    }

    #[test]
    fn the_sniffer_reads_dimensions_from_bytes_in_memory() {
        let mut cursor = std::io::Cursor::new(tiny_png());
        let (width, height, format) =
            detect_image_dimensions(&mut cursor).expect("a PNG is recognised");

        assert_eq!((width, height), (1, 1));
        assert!(!format.is_empty());
    }
}
