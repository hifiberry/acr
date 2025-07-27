use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::fs::File;
use log::{debug, warn};
use serde::{Serialize, Deserialize};
use crate::helpers::attributecache::get_attribute_cache;
use crate::helpers::http_client::new_http_client;

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
/// This function supports both local files and remote URLs.
/// Results are cached using the attribute cache for performance.
/// 
/// # Arguments
/// * `url` - URL or file path to the image
/// 
/// # Returns
/// * `Result<ImageMetadata, String>` - Image metadata or error message
pub fn image_size(url: &str) -> Result<ImageMetadata, String> {
    // Check cache first
    let cache_key = format!("image_meta:{}", url);
    
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

/// Detect image dimensions from a reader
/// 
/// This function reads just enough data to determine the image format and dimensions
/// without loading the entire image into memory.
fn detect_image_dimensions<R: BufRead + Seek>(reader: &mut R) -> Result<(u32, u32, String), String> {
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

/// Check if the header indicates a BMP image
fn is_bmp(header: &[u8]) -> bool {
    header.len() >= 2 && &header[0..2] == b"BM"
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
    
    if header_size >= 16 {
        // Read width and height
        reader.read_exact(&mut buffer)
            .map_err(|e| format!("Failed to read BMP width: {}", e))?;
        let width = u32::from_le_bytes(buffer);
        
        reader.read_exact(&mut buffer)
            .map_err(|e| format!("Failed to read BMP height: {}", e))?;
        let height = u32::from_le_bytes(buffer);
        
        Ok((width, height, "BMP".to_string()))
    } else {
        Err("Invalid BMP header size".to_string())
    }
}

/// Clear cached image metadata for a specific URL
pub fn clear_image_cache(url: &str) -> Result<(), String> {
    let cache_key = format!("image_meta:{}", url);
    
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
    let cache_key = format!("image_meta:{}", url);
    
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

    #[test]
    fn test_bmp_detection() {
        let bmp_header = b"BM";
        assert!(is_bmp(bmp_header));
        
        let not_bmp = [0xFF, 0xD8, 0xFF, 0xE0];
        assert!(!is_bmp(&not_bmp));
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
        use crate::helpers::attributecache::AttributeCache;
        
        let url = "https://www.google.com/images/branding/googlelogo/1x/googlelogo_color_272x92dp.png";
        
        // Create temporary cache
        let temp_dir = std::env::temp_dir().join(format!("audiocontrol_test_{}", std::process::id()));
        std::fs::create_dir_all(&temp_dir).unwrap();
        let db_file = temp_dir.join("test_attributes.db");
        let mut cache = AttributeCache::with_database_file(&db_file);
        
        println!("Test cache enabled: {}", cache.is_enabled());
        assert!(cache.is_enabled(), "Test cache should be enabled");
        
        // Test direct cache operations
        let cache_key = format!("image_meta:{}", url);
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
}
