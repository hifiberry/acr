use std::collections::HashMap;
use std::net::{IpAddr, UdpSocket};
use std::time::Duration;
use parking_lot::Mutex;
use log::{debug, info, warn};
use std::io;
use get_if_addrs::get_if_addrs;
use mac_address::MacAddress;
use std::time::Instant;
use once_cell::sync::Lazy;

use crate::players::lms::jsonrps::LmsRpcClient;

/// Default timeout for server discovery in seconds
const DEFAULT_DISCOVERY_TIMEOUT: u64 = 2;

/// UDP port for LMS SlimProto protocol (discovery, streaming, control)
const LMS_SLIMPROTO_PORT: u16 = 3483;

/// Default HTTP port for LMS JSON-RPC API
const LMS_HTTP_PORT: u16 = 9000;

/// HELO message buffer size
const BUFFER_SIZE: usize = 1024;

/// Global server registry to track discovered servers
static SERVER_REGISTRY: Lazy<Mutex<LmsServerRegistry>> = Lazy::new(|| {
    Mutex::new(LmsServerRegistry::new())
});

/// Registry to track discovered LMS servers and current connection
pub struct LmsServerRegistry {
    /// Known LMS servers by IP address
    servers: HashMap<IpAddr, LmsServer>,
    
    /// Currently connected server (if any)
    connected_server: Option<IpAddr>,
}

impl Default for LmsServerRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl LmsServerRegistry {
    /// Create a new server registry
    pub fn new() -> Self {
        Self {
            servers: HashMap::new(),
            connected_server: None,
        }
    }
    
    /// Add a server to the registry
    /// 
    /// # Returns
    /// `true` if this is a new server, `false` if the server was already known
    pub fn add_server(&mut self, server: LmsServer) -> bool {
        let ip = server.ip;
        if !self.servers.contains_key(&ip) {
            // New server discovered
            info!("Found new LMS server: {} at {}:{}", server.name, ip, server.port);
            self.servers.insert(ip, server);
            true
        } else {
            // Server already known - update it if name or version changed
            let existing = self.servers.get(&ip).unwrap();
            if existing.name != server.name || existing.version != server.version {
                info!("Updated LMS server: {} at {}:{}", server.name, ip, server.port);
                self.servers.insert(ip, server);
                true
            } else {
                debug!("LMS server already registered: {} at {}:{}", server.name, ip, server.port);
                false
            }
        }
    }
    
    /// Remove a server from the registry
    /// 
    /// # Returns
    /// `true` if a server was removed, `false` otherwise
    pub fn remove_server(&mut self, ip: &IpAddr) -> bool {
        if let Some(server) = self.servers.remove(ip) {
            info!("Removed LMS server: {} at {}:{}", server.name, ip, server.port);
            
            // If this was the connected server, clear the connection
            if self.connected_server == Some(*ip) {
                self.connected_server = None;
                info!("Disconnected from LMS server at {}", ip);
            }
            true
        } else {
            false
        }
    }
    
    /// Get a list of all known servers
    pub fn get_servers(&self) -> Vec<LmsServer> {
        self.servers.values().cloned().collect()
    }
    
    /// Get a specific server by IP
    pub fn get_server(&self, ip: &IpAddr) -> Option<&LmsServer> {
        self.servers.get(ip)
    }
    
    /// Set the current connected server
    /// 
    /// # Returns
    /// `true` if connection status changed, `false` otherwise
    pub fn set_connected(&mut self, ip: Option<&IpAddr>) -> bool {
        let new_connection = match (ip, &self.connected_server) {
            (Some(new_ip), Some(current_ip)) if new_ip == current_ip => false,
            (None, None) => false,
            _ => true,
        };
        
        if new_connection {
            if let Some(new_ip) = ip {
                if let Some(server) = self.servers.get(new_ip) {
                    info!("Connected to LMS server: {} at {}:{}", server.name, new_ip, server.port);
                    self.connected_server = Some(*new_ip);
                } else {
                    // Server not in registry but we're connecting to it
                    info!("Connected to unknown LMS server at {}", new_ip);
                    self.connected_server = Some(*new_ip);
                }
            } else {
                // Disconnecting from server
                if let Some(old_ip) = &self.connected_server {
                    if let Some(server) = self.servers.get(old_ip) {
                        info!("Disconnected from LMS server: {} at {}:{}", server.name, old_ip, server.port);
                    } else {
                        info!("Disconnected from LMS server at {}", old_ip);
                    }
                }
                self.connected_server = None;
            }
            true
        } else {
            false
        }
    }
    
    /// Get the currently connected server
    pub fn get_connected(&self) -> Option<&LmsServer> {
        self.connected_server.as_ref().and_then(|ip| self.servers.get(ip))
    }
    
    /// Clear all servers from the registry
    pub fn clear(&mut self) {
        if !self.servers.is_empty() {
            info!("Cleared {} LMS servers from registry", self.servers.len());
            self.servers.clear();
            self.connected_server = None;
        }
    }
    
    /// Get the number of servers in the registry
    pub fn count(&self) -> usize {
        self.servers.len()
    }
}

// Global functions to work with the registry

/// Get all known LMS servers
pub fn get_known_servers() -> Vec<LmsServer> {
    let registry = SERVER_REGISTRY.lock();
    registry.get_servers()
}

/// Get the currently connected LMS server
pub fn get_connected_server() -> Option<LmsServer> {
    let registry = SERVER_REGISTRY.lock();
    registry.get_connected().cloned()
}

/// Set the currently connected LMS server
pub fn set_connected_server(ip: Option<&IpAddr>) -> bool {
    let mut registry = SERVER_REGISTRY.lock();
    registry.set_connected(ip)
}

/// Discovered LMS server information
#[derive(Debug, Clone, PartialEq)]
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
    let timeout_duration = Duration::from_secs(timeout_secs.unwrap_or(DEFAULT_DISCOVERY_TIMEOUT));
    
    debug!("Starting LMS discovery with timeout of {}s", timeout_duration.as_secs());
    
    // Create a UDP socket for broadcast
    let socket = UdpSocket::bind("0.0.0.0:0")?;
    socket.set_broadcast(true)?;
    
    // Set socket timeout for receive operations
    socket.set_read_timeout(Some(Duration::from_millis(500)))?;
    
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
    let broadcast_addr = format!("255.255.255.255:{}", LMS_SLIMPROTO_PORT);
    debug!("Sending discovery broadcast to {}", broadcast_addr);
    socket.send_to(&discovery_msg, &broadcast_addr)?;
    
    // Also try more specific broadcast addresses
    // This covers common subnet broadcast addresses
    for subnet in &["192.168.1.255", "192.168.0.255", "10.0.0.255", "10.0.1.255"] {
        let addr = format!("{}:{}", subnet, LMS_SLIMPROTO_PORT);
        let _ = socket.send_to(&discovery_msg, &addr);
    }
    
    let mut local_servers = HashMap::new();
    let mut buffer = [0u8; BUFFER_SIZE];
    
    // Keep receiving until timeout
    let start_time = Instant::now();
    
    while start_time.elapsed() < timeout_duration {
        match socket.recv_from(&mut buffer) {
            Ok((bytes_read, src_addr)) => {
                debug!("Received {} bytes from {}", bytes_read, src_addr);
                
                // Try to parse the response
                if let Some(server) = parse_server_response(&buffer[..bytes_read], src_addr.ip()) {
                    // Add to our local HashMap for later merging
                    local_servers.insert(server.ip, server);
                }
            },
            Err(e) if e.kind() == io::ErrorKind::WouldBlock || e.kind() == io::ErrorKind::TimedOut => {
                // Just a timeout on this recv attempt, continue
                debug!("No response in recv_from after waiting {}ms", start_time.elapsed().as_millis());
            },
            Err(e) => {
                warn!("Error receiving response: {}", e);
                // Continue trying to receive messages
            }
        }
    }
    
    // Update the global server registry with discovered servers
    update_server_registry(&local_servers);
    
    // Convert HashMap to Vec
    let discovered_servers: Vec<LmsServer> = local_servers.values().cloned().collect();
    
    // Only log once at the end with the total count
    if !discovered_servers.is_empty() {
        info!("Discovered {} LMS servers", discovered_servers.len());
    } else {
        debug!("No LMS servers discovered");
    }
    
    Ok(discovered_servers)
}

/// Update the global server registry with newly discovered servers
fn update_server_registry(new_servers: &HashMap<IpAddr, LmsServer>) {
    {
        let mut registry = SERVER_REGISTRY.lock();
        // Track servers to remove (not found in the current discovery)
        let mut servers_to_remove = Vec::new();
        
        // Check for servers that have disappeared
        for ip in registry.servers.keys() {
            if !new_servers.contains_key(ip) {
                servers_to_remove.push(*ip);
            }
        }
        
        // Remove servers that weren't found in this discovery
        for ip in servers_to_remove {
            registry.remove_server(&ip);
        }
        
        // Add or update new servers
        for server in new_servers.values() {
            registry.add_server(server.clone());
        }
    }
}

/// Parse a server response to extract LMS server information
fn parse_server_response(buffer: &[u8], src_addr: IpAddr) -> Option<LmsServer> {
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
            name = format!("LMS at {}", src_addr);
        }
        
        Some(LmsServer {
            ip: src_addr,
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
                    ip: src_addr,
                    port: LMS_HTTP_PORT,
                    name,
                    version: None,
                })
            } else {
                // Fallback if UTF-8 parsing fails
                Some(LmsServer {
                    ip: src_addr,
                    port: LMS_HTTP_PORT,
                    name: format!("LMS at {}", src_addr),
                    version: None,
                })
            }
        } else {
            Some(LmsServer {
                ip: src_addr,
                port: LMS_HTTP_PORT,
                name: format!("LMS at {}", src_addr),
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
                        format!("LMS at {}", src_addr)
                    }
                });
            
            // Extract version if available
            let version = extract_server_version(response_str);
            
            Some(LmsServer {
                ip: src_addr,
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

/// Get all MAC addresses from local network interfaces
///
/// # Returns
/// A vector of MAC addresses for all network interfaces
pub fn get_local_mac_addresses() -> io::Result<Vec<MacAddress>> {
    let mut addresses = Vec::new();
    
    match get_if_addrs() {
        Ok(if_addrs) => {
            for interface in if_addrs {
                // Try to get MAC address from interface name using the mac_address crate
                if let Ok(Some(mac)) = mac_address::mac_address_by_name(&interface.name) {
                    // Check if the MAC is non-zero
                    let mac_str = mac.to_string();
                    if mac_str != "00:00:00:00:00:00" && 
                       mac_str != "00-00-00-00-00-00" && 
                       mac_str != "000000000000" {
                        debug!("Found MAC address {} for interface {}", 
                               mac, interface.name);
                        addresses.push(mac);
                    } else {
                        debug!("Skipping zero MAC address for interface {}", interface.name);
                    }
                } else {
                    debug!("No MAC address found for interface {}", interface.name);
                }
            }
            
            if addresses.is_empty() {
                // Fallback to getting all MAC addresses if the above method didn't work
                if let Ok(all_macs) = mac_address::get_mac_address() {
                    if let Some(mac) = all_macs {
                        // Check if the MAC is non-zero
                        let mac_str = mac.to_string();
                        if mac_str != "00:00:00:00:00:00" && 
                           mac_str != "00-00-00-00-00-00" && 
                           mac_str != "000000000000" {
                            debug!("Found MAC address using fallback method: {}", mac);
                            addresses.push(mac);
                        } else {
                            debug!("Skipping zero MAC address from fallback method");
                        }
                    }
                }
            }
            
            if addresses.is_empty() {
                warn!("No valid MAC addresses found for local interfaces");
            } else {
                debug!("Found {} valid local MAC addresses", addresses.len());
            }
            
            Ok(addresses)
        },
        Err(e) => {
            Err(io::Error::other(format!("Failed to get network interfaces: {}", e)))
        }
    }
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