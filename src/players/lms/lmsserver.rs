use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr, UdpSocket};
use std::time::Duration;
use log::{debug, info, warn};
use std::io;

use crate::players::lms::jsonrps::LmsRpcClient;

/// Default timeout for server discovery in seconds
const DEFAULT_DISCOVERY_TIMEOUT: u64 = 2;

/// UDP port for LMS SlimProto protocol (discovery, streaming, control)
const LMS_SLIMPROTO_PORT: u16 = 3483;

/// Default HTTP port for LMS JSON-RPC API
const LMS_HTTP_PORT: u16 = 9000;

/// HELO message buffer size
const BUFFER_SIZE: usize = 1024;

/// Discovered LMS server information
#[derive(Debug, Clone)]
pub struct LmsServer {
    /// IP address of the server
    pub ip: IpAddr,
    
    /// HTTP port for JSON-RPC API (typically 9000)
    pub port: u16,
    
    /// Server name or hostname
    pub name: String,
    
    /// Server version if available
    pub version: Option<String>,
}

impl LmsServer {
    /// Create a new RPC client for this server
    pub fn create_client(&self) -> LmsRpcClient {
        LmsRpcClient::new(&self.ip.to_string(), self.port)
    }
}

/// Find all LMS servers on the local network using UDP broadcast discovery
///
/// # Arguments
/// * `timeout_secs` - Timeout in seconds for the discovery process (default: 10)
///
/// # Returns
/// A vector of discovered LMS servers
pub fn find_local_servers(timeout_secs: Option<u64>) -> io::Result<Vec<LmsServer>> {
    let timeout = Duration::from_secs(timeout_secs.unwrap_or(DEFAULT_DISCOVERY_TIMEOUT));
    
    debug!("Starting LMS discovery with timeout of {}s", timeout.as_secs());
    
    // Create a UDP socket for broadcast
    let socket = UdpSocket::bind("0.0.0.0:0")?;
    socket.set_read_timeout(Some(timeout))?;
    socket.set_broadcast(true)?;
    
    // Prepare the discovery message that matches the working client format
    // Based on the format: eIPAD\0NAME\0JSON\0VERS\0UUID\0JVID\x06\x12\x34\x56\x78\x12\x34
    let player_name = "ACR_Discovery";
    let uuid = "ACR00000000000000"; // 16 byte UUID (simplified)
    let version = "1.0";  // Version string
    
    // Build message following the working client format
    let mut discovery_msg = Vec::new();
    discovery_msg.push(b'e');  // Start with 'e' character
    discovery_msg.extend_from_slice(b"IPAD\0");  // IP
    discovery_msg.extend_from_slice(b"NAME\0");  // Name field
    discovery_msg.extend_from_slice(player_name.as_bytes());
    discovery_msg.push(0);  // Null terminator
    discovery_msg.extend_from_slice(b"JSON\0");  // JSON capability 
    discovery_msg.extend_from_slice(b"VERS\0");  // Version field
    discovery_msg.extend_from_slice(version.as_bytes());
    discovery_msg.push(0);  // Null terminator
    discovery_msg.extend_from_slice(b"UUID\0");  // UUID field
    discovery_msg.extend_from_slice(uuid.as_bytes());
    discovery_msg.push(0);  // Null terminator

    
    // Add some identifying bytes (similar to the example)
    discovery_msg.extend_from_slice(&[0x12, 0x34, 0x56, 0x78, 0x12, 0x34]);
    
    debug!("Sending discovery message: {:?}", discovery_msg);
    
    // Send broadcast to the standard LMS SlimProto port
    debug!("Sending discovery broadcast to port {}", LMS_SLIMPROTO_PORT);
    socket.send_to(&discovery_msg, format!("255.255.255.255:{}", LMS_SLIMPROTO_PORT))?;
    
    // Also try more specific broadcast addresses
    // This covers common subnet broadcast addresses
    for subnet in &["192.168.1.255", "192.168.0.255", "10.0.0.255", "10.0.1.255"] {
        let _ = socket.send_to(&discovery_msg, format!("{}:{}", subnet, LMS_SLIMPROTO_PORT));
    }
    
    let mut servers = HashMap::new();
    let mut buffer = [0u8; BUFFER_SIZE];
    
    // Continue receiving until timeout
    let start_time = std::time::Instant::now();
    
    while start_time.elapsed() < timeout {
        match socket.recv_from(&mut buffer) {
            Ok((bytes_read, src_addr)) => {
                debug!("Received {} bytes from {}", bytes_read, src_addr);
                
                // Try to parse the response
                if let Some(server) = parse_server_response(&buffer[..bytes_read], src_addr) {
                    // Log before inserting into the HashMap
                    info!("Found LMS server: {} at {}:{}", server.name, server.ip, server.port);
                    
                    // Use IP address as key to deduplicate servers
                    servers.insert(server.ip, server);
                }
            },
            Err(err) if err.kind() == io::ErrorKind::WouldBlock || 
                       err.kind() == io::ErrorKind::TimedOut => {
                // No more responses within timeout - break out of the loop
                debug!("No more responses after waiting {}ms", start_time.elapsed().as_millis());
                break;
            },
            Err(err) => {
                warn!("Error receiving response: {}", err);
                // Continue trying to receive messages
            }
        }
    }
    
    // Convert HashMap to Vec
    let discovered_servers: Vec<LmsServer> = servers.values().cloned().collect();
    info!("Discovered {} LMS servers", discovered_servers.len());
    
    Ok(discovered_servers)
}

/// Parse a server response to extract LMS server information
fn parse_server_response(buffer: &[u8], src_addr: SocketAddr) -> Option<LmsServer> {
    // Check if this is a valid response from an LMS server
    if buffer.len() < 4 {
        debug!("Response too short: {} bytes", buffer.len());
        return None;
    }
    
    let response_type = &buffer[0..4];
    
    if response_type == b"SERV" {
        debug!("Received SERV response from {}", src_addr);
        
        // Extract server info from the response
        // Parse more detailed server information if available
        let mut name = "Logitech Media Server".to_string();
        let mut version = None;
        
        // Try to extract server name, version from the response
        if let Ok(response_str) = std::str::from_utf8(&buffer[4..]) {
            // Extract name if available
            if let Some(name_start) = response_str.find("name=") {
                if let Some(end) = response_str[name_start + 5..].find('&') {
                    name = response_str[name_start + 5..name_start + 5 + end].to_string();
                }
            }
            
            // Extract version if available
            if let Some(ver_start) = response_str.find("vers=") {
                if let Some(end) = response_str[ver_start + 5..].find('&') {
                    version = Some(response_str[ver_start + 5..ver_start + 5 + end].to_string());
                }
            }
        } else {
            // Try binary parsing for older LMS versions
            name = format!("LMS at {}", src_addr.ip());
        }
        
        Some(LmsServer {
            ip: src_addr.ip(),
            port: LMS_HTTP_PORT,  // Default HTTP port for LMS JSON-RPC API
            name,
            version,
        })
    } else if response_type == b"ENAM" && buffer.len() > 5 {
        // Handle ENAME format responses (seen in logs)
        debug!("Received ENAME response from {}", src_addr);
        
        if buffer.len() > 6 { // Ensure we have enough bytes for the separator and some text
            // Check if the response has the 0x1C separator byte
            let server_name_start = if buffer[5] == 0x1C {
                // Skip the ENAME + separator byte
                6
            } else {
                // No separator, start after ENAME
                5
            };
            
            if let Ok(response_str) = std::str::from_utf8(&buffer[server_name_start..]) {
                let name = response_str.trim().to_string();
                debug!("Extracted server name: {}", name);
                
                Some(LmsServer {
                    ip: src_addr.ip(),
                    port: LMS_HTTP_PORT,
                    name,
                    version: None,
                })
            } else {
                // Fallback if UTF-8 parsing fails
                Some(LmsServer {
                    ip: src_addr.ip(),
                    port: LMS_HTTP_PORT,
                    name: format!("LMS at {}", src_addr.ip()),
                    version: None,
                })
            }
        } else {
            Some(LmsServer {
                ip: src_addr.ip(),
                port: LMS_HTTP_PORT,
                name: format!("LMS at {}", src_addr.ip()),
                version: None,
            })
        }
    } else if let Ok(response_str) = std::str::from_utf8(buffer) {
        // Try to handle other text-based responses
        debug!("Received text response: {}", response_str);
        
        // Check if this looks like an LMS announcement
        if response_str.contains("ENAME") || 
           response_str.contains("SqueezeCenter") || 
           response_str.contains("Logitech Media Server") ||
           response_str.contains("Squeezebox Server") ||
           response_str.contains("Music Server") {  // More permissive check
            
            // Extract the server name if possible
            let name = extract_server_name(response_str)
                .unwrap_or_else(|| {
                    // If standard extraction fails, try direct extraction for ENAME format
                    if let Some(idx) = response_str.find("ENAME") {
                        // Account for possible 0x1C separator after ENAME
                        let start_idx = idx + 5;
                        if start_idx < response_str.len() && 
                          (response_str.as_bytes()[start_idx] == 0x1C) {
                            response_str[start_idx + 1..].trim().to_string()
                        } else {
                            response_str[start_idx..].trim().to_string()
                        }
                    } else {
                        format!("LMS at {}", src_addr.ip())
                    }
                });
            
            // Extract version if available
            let version = extract_server_version(response_str);
            
            Some(LmsServer {
                ip: src_addr.ip(),
                port: LMS_HTTP_PORT,
                name,
                version,
            })
        } else {
            debug!("Text response doesn't appear to be from an LMS server");
            None
        }
    } else {
        debug!("Unrecognized response format");
        None
    }
}

/// Extract server name from text response
fn extract_server_name(message: &str) -> Option<String> {
    // Look for server name in different formats
    if let Some(idx) = message.find("SERVER_NAME=") {
        let start = idx + "SERVER_NAME=".len();
        if let Some(end) = message[start..].find(&['\r', '\n', '&'][..]) {
            return Some(message[start..start + end].trim().to_string());
        }
    }
    
    // Try alternative formats
    if let Some(idx) = message.find("Name: ") {
        let start = idx + "Name: ".len();
        if let Some(end) = message[start..].find(&['\r', '\n', '&'][..]) {
            return Some(message[start..start + end].trim().to_string());
        }
    }
    
    if let Some(idx) = message.find("name=") {
        let start = idx + "name=".len();
        if let Some(end) = message[start..].find(&['\r', '\n', '&'][..]) {
            return Some(message[start..start + end].trim().to_string());
        }
    }
    
    None
}

/// Extract server version from text response
fn extract_server_version(message: &str) -> Option<String> {
    // Look for version in different formats
    if let Some(idx) = message.find("VERSION=") {
        let start = idx + "VERSION=".len();
        if let Some(end) = message[start..].find(&['\r', '\n', '&'][..]) {
            return Some(message[start..start + end].trim().to_string());
        }
    }
    
    // Try alternative formats
    if let Some(idx) = message.find("Version: ") {
        let start = idx + "Version: ".len();
        if let Some(end) = message[start..].find(&['\r', '\n', '&'][..]) {
            return Some(message[start..start + end].trim().to_string());
        }
    }
    
    if let Some(idx) = message.find("vers=") {
        let start = idx + "vers=".len();
        if let Some(end) = message[start..].find(&['\r', '\n', '&'][..]) {
            return Some(message[start..start + end].trim().to_string());
        }
    }
    
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_discover_lms_servers() {
        // This test will actively try to discover LMS servers
        match find_local_servers(Some(5)) {
            Ok(servers) => {
                println!("Discovered {} LMS servers:", servers.len());
                for (i, server) in servers.iter().enumerate() {
                    println!("  {}. {} at {}:{} (version: {:?})", 
                             i+1, server.name, server.ip, server.port, server.version);
                }
            },
            Err(e) => {
                println!("Error discovering LMS servers: {}", e);
            }
        }
    }
}