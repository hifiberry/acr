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
        .replace([':', '-', '.', ' '], "");
    
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

/// Compare two MAC addresses in a case-insensitive manner
/// 
/// # Arguments
/// * `mac1_str` - First MAC address as string
/// * `mac2_str` - Second MAC address as string
///
/// # Returns
/// `true` if the MAC addresses are equal (ignoring case), `false` otherwise
pub fn mac_equal_ignore_case(mac1_str: &str, mac2_str: &str) -> bool {
    // Clean both MAC addresses to remove separators and make them lowercase
    let clean_mac1 = mac1_str
        .replace([':', '-', '.', ' '], "")
        .to_lowercase();
        
    let clean_mac2 = mac2_str
        .replace([':', '-', '.', ' '], "")
        .to_lowercase();
    
    // Debug log the comparison
    if clean_mac1 == clean_mac2 {
        log::debug!("MAC address match: '{}' equals '{}' (normalized to {} == {})", 
            mac1_str, mac2_str, clean_mac1, clean_mac2);
        true
    } else {
        log::trace!("MAC address mismatch: '{}' vs '{}' (normalized to {} vs {})", 
            mac1_str, mac2_str, clean_mac1, clean_mac2);
        false
    }
}

/// Convert a MacAddress to a lowercase string representation
/// 
/// # Arguments
/// * `mac` - The MacAddress to convert
///
/// # Returns
/// A lowercase string representation of the MAC address with colon separators
pub fn mac_to_lowercase_string(mac: &MacAddress) -> String {
    mac.to_string().to_lowercase()
}

/// Enum representing different types of virtual machine providers
#[derive(Debug, PartialEq, Clone, Copy)]
pub enum VirtualizationProvider {
    MicrosoftHyperV,
    VMware,
    VirtualBox,
    Parallels,
    QEMU,
    XenProject,
    DockerVirtualBridge,
    Unknown
}

/// Check if a MAC address belongs to a known virtualization provider
/// 
/// # Arguments
/// * `mac` - The MacAddress to check
///
/// # Returns
/// Some(VirtualizationProvider) if the MAC is from a known provider, None if it appears to be physical
pub fn is_virtual_mac(mac: &MacAddress) -> Option<VirtualizationProvider> {
    // Get the first three bytes (OUI) of the MAC address
    let bytes = mac.bytes();
    let oui = [bytes[0], bytes[1], bytes[2]];
    
    // Check against known virtualization OUIs
    match oui {
        // Microsoft Hyper-V, Azure, etc.
        [0x00, 0x15, 0x5D] => Some(VirtualizationProvider::MicrosoftHyperV),
        
        // VMware
        [0x00, 0x50, 0x56] | [0x00, 0x0C, 0x29] | [0x00, 0x05, 0x69] => Some(VirtualizationProvider::VMware),
        
        // VirtualBox
        [0x08, 0x00, 0x27] | [0x0A, 0x00, 0x27] => Some(VirtualizationProvider::VirtualBox),
        
        // Parallels
        [0x00, 0x1C, 0x42] => Some(VirtualizationProvider::Parallels),
        
        // QEMU/KVM
        [0x52, 0x54, 0x00] => Some(VirtualizationProvider::QEMU),
        
        // Xen Project
        [0x00, 0x16, 0x3E] => Some(VirtualizationProvider::XenProject),
        
        // Docker default bridge
        [0x02, 0x42, 0xAC] => Some(VirtualizationProvider::DockerVirtualBridge),
        
        _ => {
            // Check if this is a locally administered MAC address
            // Bit 1 of the first octet indicates locally administered (1) vs globally unique (0)
            if bytes[0] & 0x02 != 0 {
                Some(VirtualizationProvider::Unknown)
            } else {
                None // Likely a physical MAC address
            }
        }
    }
}

/// Returns a human-readable string describing a MAC address
/// 
/// # Arguments
/// * `mac` - The MacAddress to describe
///
/// # Returns
/// A string describing if the MAC is virtual or physical, and if virtual, which provider
pub fn describe_mac(mac: &MacAddress) -> String {
    match is_virtual_mac(mac) {
        Some(provider) => match provider {
            VirtualizationProvider::MicrosoftHyperV => format!("Virtual MAC (Microsoft Hyper-V): {}", mac),
            VirtualizationProvider::VMware => format!("Virtual MAC (VMware): {}", mac),
            VirtualizationProvider::VirtualBox => format!("Virtual MAC (VirtualBox): {}", mac),
            VirtualizationProvider::Parallels => format!("Virtual MAC (Parallels): {}", mac),
            VirtualizationProvider::QEMU => format!("Virtual MAC (QEMU/KVM): {}", mac),
            VirtualizationProvider::XenProject => format!("Virtual MAC (Xen): {}", mac),
            VirtualizationProvider::DockerVirtualBridge => format!("Virtual MAC (Docker): {}", mac),
            VirtualizationProvider::Unknown => format!("Virtual MAC (Unknown provider): {}", mac),
        },
        None => format!("Physical MAC: {}", mac),
    }
}