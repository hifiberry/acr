/*!
 * URL encoding helper for creating URL-safe encoded identifiers
 * 
 * This module provides functionality to encode long URLs/paths into
 * URL-safe base64 strings that can be used as shorter identifiers.
 */

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use log::debug;

/// Encode a string to URL-safe base64 without padding
/// 
/// This function takes any string (typically a long file path or URL)
/// and encodes it to a URL-safe base64 string without padding characters.
/// The resulting string can be safely used in URLs and is shorter than
/// URL-encoded paths.
/// 
/// # Arguments
/// * `input` - The string to encode
/// 
/// # Returns
/// A URL-safe base64 encoded string without padding
/// 
/// # Example
/// ```
/// use audiocontrol::helpers::url_encoding::encode_url_safe;
/// let long_path = "Music/Some Artist/Some Album (2023)/01 - Track Name.mp3";
/// let encoded = encode_url_safe(long_path);
/// // encoded will be something like: "TXVzaWMvU29tZSBBcnRpc3QvU29tZSBBbGJ1bSAoMjAyMyk_LzAxIC0gVHJhY2sgTmFtZS5tcDM"
/// ```
pub fn encode_url_safe(input: &str) -> String {
    let encoded = URL_SAFE_NO_PAD.encode(input.as_bytes());
    debug!("Encoded '{}' to URL-safe base64: '{}'", input, encoded);
    encoded
}

/// Decode a URL-safe base64 string back to the original string
/// 
/// This function decodes a URL-safe base64 string back to the original
/// string. Returns None if the input is not valid base64 or cannot be
/// decoded to a valid UTF-8 string.
/// 
/// # Arguments
/// * `encoded` - The URL-safe base64 encoded string
/// 
/// # Returns
/// The original string if decoding is successful, None otherwise
/// 
/// # Example
/// ```
/// use audiocontrol::helpers::url_encoding::decode_url_safe;
/// let encoded = "TXVzaWMvU29tZSBBcnRpc3Q";
/// if let Some(decoded) = decode_url_safe(encoded) {
///     println!("Decoded: {}", decoded);
/// }
/// ```
pub fn decode_url_safe(encoded: &str) -> Option<String> {
    match URL_SAFE_NO_PAD.decode(encoded.as_bytes()) {
        Ok(decoded_bytes) => {
            match String::from_utf8(decoded_bytes) {
                Ok(decoded_string) => {
                    debug!("Decoded URL-safe base64 '{}' to: '{}'", encoded, decoded_string);
                    Some(decoded_string)
                }
                Err(e) => {
                    debug!("Failed to convert decoded bytes to UTF-8 string: {}", e);
                    None
                }
            }
        }
        Err(e) => {
            debug!("Failed to decode base64 string '{}': {}", encoded, e);
            None
        }
    }
}

/// Check if a string looks like a URL-safe base64 encoded string
/// 
/// This function performs a basic check to see if the input string
/// could be a URL-safe base64 encoded string. It checks the character
/// set and attempts to decode it.
/// 
/// # Arguments
/// * `input` - The string to check
/// 
/// # Returns
/// True if the string appears to be URL-safe base64 encoded, false otherwise
pub fn is_url_safe_base64(input: &str) -> bool {
    // Check if the string contains only valid URL-safe base64 characters
    // URL-safe base64 uses: A-Z, a-z, 0-9, -, _ (no padding since we use NO_PAD)
    if input.is_empty() {
        return false;
    }
    
    let valid_chars = input.chars().all(|c| {
        c.is_ascii_alphanumeric() || c == '-' || c == '_'
    });
    
    if !valid_chars {
        return false;
    }
    
    // Try to decode it to see if it's valid base64
    decode_url_safe(input).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encode_decode_roundtrip() {
        let test_cases = vec![
            "simple",
            "path/with/slashes",
            "Music/Artist Name/Album (2023)/01 - Track.mp3",
            "Special chars: åäö",
            "",
        ];

        for input in test_cases {
            let encoded = encode_url_safe(input);
            let decoded = decode_url_safe(&encoded);
            assert_eq!(Some(input.to_string()), decoded, "Failed roundtrip for: {}", input);
        }
    }

    #[test]
    fn test_url_safe_characters() {
        let input = "path/with/spaces and special chars";
        let encoded = encode_url_safe(input);
        
        // Should not contain +, /, or = (which are not URL-safe)
        assert!(!encoded.contains('+'));
        assert!(!encoded.contains('/'));
        assert!(!encoded.contains('='));
        
        // Should only contain URL-safe characters
        assert!(encoded.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
    }

    #[test]
    fn test_is_url_safe_base64() {
        // Valid base64 strings
        let valid_input = encode_url_safe("test string");
        assert!(is_url_safe_base64(&valid_input));
        
        // Invalid strings
        assert!(!is_url_safe_base64(""));
        assert!(!is_url_safe_base64("not base64!"));
        assert!(!is_url_safe_base64("contains/slash"));
        assert!(!is_url_safe_base64("contains+plus"));
        assert!(!is_url_safe_base64("contains=equal"));
    }

    #[test]
    fn test_decode_invalid_base64() {
        assert_eq!(None, decode_url_safe("invalid!"));
        assert_eq!(Some("".to_string()), decode_url_safe("")); // Empty string is valid base64
        assert_eq!(None, decode_url_safe("contains spaces"));
    }
}
