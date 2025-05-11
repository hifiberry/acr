use mac_address::MacAddress;

/// Normalize MAC address string to MacAddress type
/// 
/// # Arguments
/// * `mac_str` - MAC address string in various formats: 00:04:20:ab:cd:ef, 00-04-20-AB-CD-EF, etc.
///
/// # Returns
/// A MacAddress instance
pub fn normalize_mac_address(mac_str: &str) -> Result<MacAddress, String> {
    // Remove any separators and spaces
    let clean_mac = mac_str
        .replace(':', "")
        .replace('-', "")
        .replace('.', "")
        .replace(' ', "");
    
    if clean_mac.len() != 12 {
        return Err(format!("Invalid MAC address length: {}", mac_str));
    }
    
    // Parse as hex bytes
    let bytes = match hex::decode(clean_mac) {
        Ok(bytes) => bytes,
        Err(e) => return Err(format!("Invalid hex in MAC address {}: {}", mac_str, e))
    };
    
    if bytes.len() != 6 {
        return Err(format!("MAC address didn't convert to 6 bytes: {}", mac_str));
    }
    
    // Create MacAddress using a fixed-size array of 6 bytes
    let mut mac_bytes = [0u8; 6];
    mac_bytes.copy_from_slice(&bytes[0..6]);
    
    // MacAddress::new doesn't return a Result, it just returns MacAddress
    Ok(MacAddress::new(mac_bytes))
}