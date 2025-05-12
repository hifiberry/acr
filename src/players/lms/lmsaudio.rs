use std::any::Any;
use std::sync::{Arc, RwLock, Weak, atomic::{AtomicBool, Ordering}};
use std::time::{SystemTime, Duration};
use std::thread;
use std::net::IpAddr;
use log::{debug, info, warn};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::data::{LoopMode, PlaybackState, PlayerCapabilitySet, PlayerCommand, Song, Track};
use crate::PlayerStateListener;
use crate::data::library::LibraryInterface;
use crate::players::player_controller::{BasePlayerController, PlayerController};
use crate::players::lms::jsonrps::LmsRpcClient;
use crate::players::lms::lmsserver::{find_local_servers, get_local_mac_addresses, set_connected_server};
use crate::players::lms::lmspplayer::LMSPlayer;
use crate::helpers::macaddress::normalize_mac_address;

/// Configuration for LMSAudioController
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LMSAudioConfig {
    /// Server address (hostname or IP)
    pub server: Option<String>,
    
    /// Server port (usually 9000)
    #[serde(default = "default_lms_port")]
    pub port: u16,
    
    /// Auto-discovery enabled
    #[serde(default = "default_true")]
    pub autodiscovery: bool,
    
    /// Player name to connect to
    pub player_name: Option<String>,
    
    /// Player MAC addresses to connect to (multiple MACs)
    #[serde(default)]
    pub player_macs: Vec<String>,
    
    /// Reconnection interval in seconds (0 = disabled)
    #[serde(default = "default_reconnection_interval")]
    pub reconnection_interval: u64,
}

/// Default LMS server port
fn default_lms_port() -> u16 {
    9000
}

/// Default value for autodiscovery
fn default_true() -> bool {
    true
}

/// Default reconnection interval in seconds (30 seconds)
fn default_reconnection_interval() -> u64 {
    30
}

impl Default for LMSAudioConfig {
    fn default() -> Self {
        Self {
            server: None,
            port: default_lms_port(),
            autodiscovery: true,
            player_name: None,
            player_macs: Vec::new(),
            reconnection_interval: default_reconnection_interval(),
        }
    }
}

/// Helper function to check if a MAC address is valid (not all zeros)
fn is_valid_mac(mac: &mac_address::MacAddress) -> bool {
    // Convert MAC to string and check if it's all zeros
    let mac_str = mac.to_string();
    
    // Check common representations of zero MAC addresses
    !(mac_str == "00:00:00:00:00:00" || 
      mac_str == "00-00-00-00-00-00" ||
      mac_str == "000000000000")
}

/// Controller for Logitech Media Server (LMS) audio players
pub struct LMSAudioController {
    /// Base controller providing common functionality
    base: BasePlayerController,
    
    /// Controller configuration
    config: Arc<RwLock<LMSAudioConfig>>,
    
    /// LMS RPC client for API calls
    #[allow(dead_code)]
    client: Arc<RwLock<Option<LmsRpcClient>>>,
    
    /// Player object for interacting with the LMS server
    player: Arc<RwLock<Option<LMSPlayer>>>,
    
    /// Last known connection state
    is_connected: Arc<AtomicBool>,
    
    /// Flag to control the reconnection thread
    running: Arc<AtomicBool>,
    
    /// Last connected server address
    last_connected_server: Arc<RwLock<Option<String>>>,
}

impl LMSAudioController {
    /// Helper method to process player_mac configuration values
    /// 
    /// # Arguments
    /// * `mac_strings` - Configured MAC addresses to check
    /// * `include_local` - If true, add local MAC addresses too
    ///
    /// # Returns
    /// A vector of MAC addresses to check
    fn prepare_mac_addresses(&self, mac_strings: &[String], include_local: bool) -> Vec<String> {
        let mut result = Vec::new();
        let mut should_include_local = include_local;
        
        // Check if "local" is in the list, which is a special value
        for mac in mac_strings {
            if mac.to_lowercase() == "local" {
                should_include_local = true;
            } else {
                // Add any non-special MAC addresses to the result
                result.push(mac.clone());
            }
        }
        
        // If we need to include local MACs, add them now
        if should_include_local {
            match get_local_mac_addresses() {
                Ok(addresses) => {
                    // Format local MAC addresses as strings
                    let local_macs: Vec<String> = addresses.iter()
                        .map(|mac| crate::helpers::macaddress::mac_to_lowercase_string(mac))
                        .collect();
                    
                    // Add local MACs that aren't already in the list (case insensitive comparison)
                    for local_mac in local_macs {
                        let already_exists = result.iter().any(|existing_mac| 
                            crate::helpers::macaddress::mac_equal_ignore_case(existing_mac, &local_mac));
                        
                        if !already_exists {
                            result.push(local_mac);
                        }
                    }
                },
                Err(e) => {
                    warn!("Failed to get local MAC addresses: {}", e);
                }
            }
        }
        
        result
    }

    /// Create a new LMS audio controller
    /// 
    /// # Arguments
    /// * `config` - JSON configuration
    pub fn new(config_json: Value) -> Self {
        // Parse configuration from JSON
        let config = match serde_json::from_value::<LMSAudioConfig>(config_json) {
            Ok(cfg) => {
                info!("LMS controller configured with server: {:?}", cfg.server);
                cfg
            },
            Err(e) => {
                warn!("Failed to parse LMS configuration: {}. Using defaults.", e);
                LMSAudioConfig::default()
            }
        };
        
        // Log the configured MAC addresses
        if !config.player_macs.is_empty() {
            info!("LMS controller configured with player MACs: {:?}", config.player_macs);
        }
        
        let is_connected = Arc::new(AtomicBool::new(false));
        let running = Arc::new(AtomicBool::new(true));
        let last_connected_server = Arc::new(RwLock::new(None));
        
        // Create a new controller
        let controller = Self {
            base: BasePlayerController::with_player_info("lms", "lms"),
            config: Arc::new(RwLock::new(config.clone())),
            client: Arc::new(RwLock::new(None)),
            player: Arc::new(RwLock::new(None)),
            is_connected,
            running,
            last_connected_server,
        };
        
        // Initialize the player with the default configuration
        // We'll connect to the server in the start() method
        if let Some(server) = &config.server {
            // Create a client for the configured server
            let client = LmsRpcClient::new(server, config.port);
            
            // Get the player MAC address or use a default if not provided
            // For initial connection, use the first MAC in player_macs if available
            let player_id = if !config.player_macs.is_empty() {
                config.player_macs[0].clone()
            } else {
                "00:00:00:00:00:00".to_string()
            };
            
            // Create the LMSPlayer instance
            let player = LMSPlayer::new(client.clone(), &player_id);
            
            // Store the client and player
            if let Ok(mut client_lock) = controller.client.write() {
                *client_lock = Some(client);
            }
            
            if let Ok(mut player_lock) = controller.player.write() {
                *player_lock = Some(player);
            }
            
            // Check if this system is currently registered as a player on the LMS server
            // This is an early check just during initialization, the full check happens in start()
            if controller.check_connected() {
                info!("System is registered as a player on LMS server at {}:{}", server, config.port);
                controller.is_connected.store(true, Ordering::SeqCst);
            } else {
                info!("System is NOT registered as a player on LMS server at {}:{}", server, config.port);
                info!("Will keep checking during reconnection attempts");
            }
        } else if config.autodiscovery {
            info!("LMS server autodiscovery enabled - will search for servers during start");
        } else {
            info!("No LMS server configured and autodiscovery disabled - player will remain disconnected");
        }
        
        debug!("Created new LMS audio controller");
        controller
    }
    
    /// Check if the current system is connected to the configured LMS server
    /// 
    /// This method determines if the current device is registered as a player with
    /// the configured LMS server, or connects to a specified player by MAC address.
    /// 
    /// # Returns
    /// `true` if connected, `false` otherwise
    pub fn check_connected(&self) -> bool {
        // Get configuration
        let config = match self.config.read() {
            Ok(cfg) => cfg.clone(),
            Err(_) => {
                warn!("Failed to acquire read lock on LMS configuration");
                return false;
            }
        };
        
        // First check if we already have an active client connection
        if let Ok(client_guard) = self.client.read() {
            if let Some(client) = client_guard.as_ref() {
                // Attempt a simple operation to verify the connection is still alive
                let mut client_clone = client.clone();
                debug!("Testing existing LMS connection");
                
                // Simple ping test to check if server is reachable
                match client_clone.get_players() {
                    Ok(_) => {
                        debug!("Existing LMS connection is still active");
                        
                        // If we have a configured player and previously succeeded to connect,
                        // we consider it still connected without checking the MAC addresses again
                        if self.is_connected.load(Ordering::SeqCst) {
                            return true;
                        }
                        
                        // If we made it here, the server is reachable but we need to verify 
                        // the player MAC address - fall through to the MAC check logic below
                    },
                    Err(e) => {
                        debug!("Existing LMS connection failed: {}", e);
                        // Connection failed, we'll need to try rediscovery
                    }
                }
            }
        }

        // If we have a server configured, try direct connection
        if let Some(server) = &config.server {
            debug!("Checking connection to configured LMS server: {}", server);
            
            // Create a client for the configured server
            let mut client = LmsRpcClient::new(server, config.port);
            
            // Check if specific player MAC addresses are configured
            let has_configured_macs = !config.player_macs.is_empty();
            
            if has_configured_macs {
                // Collect all MAC addresses to check, handling special values like "local"
                let mac_strings = self.prepare_mac_addresses(&config.player_macs, false);
                
                debug!("Looking for specific players with MACs: {:?}", mac_strings);
                
                // Normalize the configured MAC addresses
                let configured_macs: Vec<mac_address::MacAddress> = mac_strings.iter()
                    .filter_map(|mac_str| {
                        match normalize_mac_address(mac_str) {
                            Ok(mac) => Some(mac),
                            Err(e) => {
                                warn!("Failed to normalize MAC address {}: {}", mac_str, e);
                                None
                            }
                        }
                    })
                    .collect();
                
                if configured_macs.is_empty() {
                    warn!("No valid MAC addresses found in configuration");
                    return false;
                }
                
                // Get the players from the server
                match client.get_players() {
                    Ok(players) => {
                        debug!("Found {} players on LMS server", players.len());
                        
                        // Check if any player matches the configured MACs
                        for player in &players {
                            // Log detailed player info to help diagnose connection issues
                            debug!("Server player: {} (MAC: {})", player.name, player.playerid);
                            
                            // The playerid field contains the MAC address
                            match normalize_mac_address(&player.playerid) {
                                Ok(player_mac) => {
                                    // Convert player MAC to a standardized string format for comparison
                                    let player_mac_str = crate::helpers::macaddress::mac_to_lowercase_string(&player_mac);
                                    debug!("Checking if player MAC {} matches any configured MAC", player_mac_str);
                                    
                                    // Check each configured MAC explicitly for better debugging
                                    for configured_mac in &mac_strings {
                                        let match_result = crate::helpers::macaddress::mac_equal_ignore_case(&player_mac_str, configured_mac);
                                        debug!("  Comparing with configured MAC {}: {}", configured_mac, if match_result { "MATCH" } else { "no match" });
                                        
                                        if match_result {
                                            // Verify this isn't an all-zeros placeholder MAC address
                                            if !is_valid_mac(&player_mac) {
                                                debug!("Ignoring invalid (all zeros) MAC address: {:?}", player_mac);
                                                continue;
                                            }
                                            
                                            info!("Found matching player: {} with MAC {} matches configured MAC {}", 
                                                player.name, player_mac_str, configured_mac);
                                                
                                            // Store the client for future use
                                            if let Ok(mut client_lock) = self.client.write() {
                                                *client_lock = Some(client.clone());
                                            }
                                            
                                            // Create and store the player instance
                                            let player_instance = LMSPlayer::new(client.clone(), &player.playerid);
                                            if let Ok(mut player_lock) = self.player.write() {
                                                *player_lock = Some(player_instance);
                                            }
                                            
                                            // Update the connected server registry
                                            if let Ok(ip_addr) = server.parse::<IpAddr>() {
                                                set_connected_server(Some(&ip_addr));
                                            } else {
                                                debug!("Unable to parse server address: {}", server);
                                            }
                                            
                                            // Update last connected server
                                            if let Ok(mut last_server) = self.last_connected_server.write() {
                                                *last_server = Some(server.clone());
                                            }
                                            
                                            return true;
                                        }
                                    }
                                    
                                    // If we get here, none of the configured MACs matched this player
                                    debug!("Player {} with MAC {} didn't match any configured MAC", 
                                          player.name, player_mac_str);
                                },
                                Err(e) => {
                                    debug!("Failed to normalize player MAC {}: {}", player.playerid, e);
                                }
                            }
                        }
                    },
                    Err(e) => {
                        warn!("Failed to get players from LMS server: {}", e);
                    }
                }
                
                // If we've reached here with configured MACs, we didn't find any matching players
                debug!("Couldn't find players with configured MAC addresses, won't fall back to local MAC detection");
                return false;
            }
            
            // If no MACs were explicitly configured, try to find a player matching local MAC addresses
            let mac_addresses = match get_local_mac_addresses() {
                Ok(addresses) => addresses,
                Err(e) => {
                    warn!("Failed to get local MAC addresses: {}", e);
                    return false;
                }
            };
            
            // Normalize all local MAC addresses for comparison
            let normalized_local_macs: Vec<mac_address::MacAddress> = mac_addresses
                .iter()
                .map(|mac| mac.clone())
                .collect();
            
            debug!("Local MAC addresses: {:?}", normalized_local_macs);
            
            // Get the players from the server
            match client.get_players() {
                Ok(players) => {
                    debug!("Found {} players on LMS server", players.len());
                    
                    // Check if any player matches our MAC address
                    for player in players {
                        // The playerid field contains the MAC address
                        match normalize_mac_address(&player.playerid) {
                            Ok(player_mac) => {
                                debug!("Checking player MAC: {:?}", player_mac);
                                
                                // Check if this player's MAC matches any of our local MACs
                                if normalized_local_macs.contains(&player_mac) {
                                    // Verify this isn't an all-zeros placeholder MAC address
                                    if !is_valid_mac(&player_mac) {
                                        debug!("Ignoring invalid (all zeros) MAC address: {:?}", player_mac);
                                        continue;
                                    }
                                    
                                    info!("Found matching player: {} ({:?})", 
                                         player.name, 
                                         player_mac);
                                    
                                    // Store the client for future use
                                    if let Ok(mut client_lock) = self.client.write() {
                                        *client_lock = Some(client.clone());
                                    }
                                    
                                    // Create and store the player instance
                                    let player_instance = LMSPlayer::new(client.clone(), &player.playerid);
                                    if let Ok(mut player_lock) = self.player.write() {
                                        *player_lock = Some(player_instance);
                                    }
                                    
                                    // Update the connected server registry
                                    if let Ok(ip_addr) = server.parse::<IpAddr>() {
                                        set_connected_server(Some(&ip_addr));
                                    } else {
                                        debug!("Unable to parse server address: {}", server);
                                    }
                                    
                                    // Update last connected server
                                    if let Ok(mut last_server) = self.last_connected_server.write() {
                                        *last_server = Some(server.clone());
                                    }
                                    
                                    return true;
                                }
                            },
                            Err(e) => {
                                debug!("Failed to normalize player MAC: {}", e);
                            }
                        }
                    }
                    
                    debug!("No matching players found on the server");
                    false
                },
                Err(e) => {
                    warn!("Failed to get players from LMS server: {}", e);
                    false
                }
            }
        } 
        // If autodiscovery is enabled, try to find LMS servers on the network
        else if config.autodiscovery {
            debug!("No server configured, attempting autodiscovery");
            
            // Try to discover LMS servers
            match find_local_servers(Some(5)) {
                Ok(servers) => {
                    if servers.is_empty() {
                        debug!("No LMS servers found via autodiscovery");
                        return false;
                    }
                    
                    debug!("Found {} LMS servers via autodiscovery", servers.len());
                    
                    // Check each server for matching players
                    for server in servers {
                        debug!("Checking server: {} at {}", server.name, server.ip);
                        
                        // Create a client for this server
                        let mut client = server.create_client();
                        
                        // Get all MAC addresses to check (both configured and local if needed)
                        let mac_strings = self.prepare_mac_addresses(&config.player_macs, false);
                        debug!("Looking for players with MACs: {:?} via autodiscovery", mac_strings);
                        
                        // Get the players from the server
                        match client.get_players() {
                            Ok(players) => {
                                debug!("Found {} players on server {}", players.len(), server.name);
                                
                                // Check for matches with configured MACs
                                for player in &players {
                                    debug!("Checking autodiscovered player: {} (MAC: {})", player.name, player.playerid);
                                    
                                    match normalize_mac_address(&player.playerid) {
                                        Ok(player_mac) => {
                                            // Skip invalid MACs
                                            if !is_valid_mac(&player_mac) {
                                                debug!("Ignoring invalid (all zeros) MAC address: {:?}", player_mac);
                                                continue;
                                            }
                                            
                                            // Convert to lowercase string for comparison
                                            let player_mac_str = crate::helpers::macaddress::mac_to_lowercase_string(&player_mac);
                                            
                                            // Try to match against configured MACs
                                            for configured_mac in &mac_strings {
                                                // Skip the special "local" value
                                                if configured_mac.to_lowercase() == "local" {
                                                    continue;
                                                }
                                                
                                                let matches = crate::helpers::macaddress::mac_equal_ignore_case(&player_mac_str, configured_mac);
                                                debug!("  Comparing with configured MAC {}: {}", configured_mac, if matches { "MATCH" } else { "no match" });
                                                
                                                if matches {
                                                    info!("Found matching player: {} (MAC: {}) on server {}",
                                                         player.name, player_mac_str, server.name);
                                                         
                                                    // Store the client and update configuration with discovered server
                                                    if let Ok(mut client_lock) = self.client.write() {
                                                        *client_lock = Some(client.clone());
                                                    }
                                                    
                                                    // Update configuration with discovered server
                                                    if let Ok(mut cfg_lock) = self.config.write() {
                                                        cfg_lock.server = Some(server.ip.to_string());
                                                        cfg_lock.port = server.port;
                                                    }
                                                    
                                                    // Create and store the player instance
                                                    let player_instance = LMSPlayer::new(client.clone(), &player.playerid);
                                                    if let Ok(mut player_lock) = self.player.write() {
                                                        *player_lock = Some(player_instance);
                                                    }
                                                    
                                                    // Update the connected server registry
                                                    set_connected_server(Some(&server.ip));
                                                    
                                                    // Update last connected server
                                                    if let Ok(mut last_server) = self.last_connected_server.write() {
                                                        *last_server = Some(server.ip.to_string());
                                                    }
                                                    
                                                    // Return immediately when a match is found
                                                    return true;
                                                }
                                            }
                                        },
                                        Err(e) => {
                                            debug!("Failed to normalize player MAC {}: {}", player.playerid, e);
                                        }
                                    }
                                }
                                
                                // If we haven't found a match and "local" was in the config,
                                // fall back to checking local MAC addresses
                                if config.player_macs.iter().any(|mac| mac.to_lowercase() == "local") {
                                    debug!("Checking local MAC addresses for matches");
                                    
                                    // Get the local MAC addresses
                                    match get_local_mac_addresses() {
                                        Ok(addresses) => {
                                            // Normalize all local MAC addresses for comparison
                                            let normalized_local_macs: Vec<mac_address::MacAddress> = addresses
                                                .iter()
                                                .map(|mac| mac.clone())
                                                .collect();
                                            
                                            // Check if any player matches our local MAC address
                                            for player in &players {
                                                match normalize_mac_address(&player.playerid) {
                                                    Ok(player_mac) => {
                                                        // Skip invalid MACs
                                                        if !is_valid_mac(&player_mac) {
                                                            debug!("Ignoring invalid (all zeros) MAC address: {:?}", player_mac);
                                                            continue;
                                                        }
                                                        
                                                        // Check if this player's MAC matches any of our local MACs
                                                        if normalized_local_macs.contains(&player_mac) {
                                                            info!("Found matching player: {} with local MAC {:?} on server {}", 
                                                                 player.name,
                                                                 player_mac,
                                                                 server.name);
                                                            
                                                            // Store the client and update configuration with discovered server
                                                            if let Ok(mut client_lock) = self.client.write() {
                                                                *client_lock = Some(client.clone());
                                                            }
                                                            
                                                            // Update configuration with discovered server
                                                            if let Ok(mut cfg_lock) = self.config.write() {
                                                                cfg_lock.server = Some(server.ip.to_string());
                                                                cfg_lock.port = server.port;
                                                            }
                                                            
                                                            // Create and store the player instance
                                                            let player_instance = LMSPlayer::new(client.clone(), &player.playerid);
                                                            if let Ok(mut player_lock) = self.player.write() {
                                                                *player_lock = Some(player_instance);
                                                            }
                                                            
                                                            // Update the connected server registry
                                                            set_connected_server(Some(&server.ip));
                                                            
                                                            // Update last connected server
                                                            if let Ok(mut last_server) = self.last_connected_server.write() {
                                                                *last_server = Some(server.ip.to_string());
                                                            }
                                                            
                                                            return true;
                                                        }
                                                    },
                                                    Err(e) => {
                                                        debug!("Failed to normalize player MAC: {}", e);
                                                    }
                                                }
                                            }
                                        },
                                        Err(e) => {
                                            warn!("Failed to get local MAC addresses: {}", e);
                                        }
                                    }
                                }
                            },
                            Err(e) => {
                                warn!("Failed to get players from server {}: {}", server.name, e);
                                // Continue to check other servers
                            }
                        }
                    }
                    
                    debug!("No matching players found on any discovered server");
                    false
                },
                Err(e) => {
                    warn!("Failed to discover LMS servers: {}", e);
                    false
                }
            }
        } else {
            debug!("No server configured and autodiscovery disabled");
            false
        }
    }
    
    /// Start the reconnection thread
    fn start_reconnection_thread(&self) {
        let config = match self.config.read() {
            Ok(cfg) => cfg.clone(),
            Err(_) => {
                warn!("Failed to acquire read lock on LMS configuration");
                return;
            }
        };
        
        // Don't start the reconnection thread if the interval is 0 (disabled)
        if config.reconnection_interval == 0 {
            info!("LMS reconnection is disabled (interval = 0)");
            return;
        }
        
        let interval = Duration::from_secs(config.reconnection_interval);
        let is_connected = self.is_connected.clone();
        let running = self.running.clone();
        let controller_config = self.config.clone();
        let base = self.base.clone();
        
        thread::spawn(move || {
            info!("LMS reconnection thread started (interval: {} seconds)", config.reconnection_interval);
            
            while running.load(Ordering::SeqCst) {
                // Sleep for the configured interval
                thread::sleep(interval);
                
                if !running.load(Ordering::SeqCst) {
                    break;
                }
                
                // If we're already connected, just check the connection
                let was_connected = is_connected.load(Ordering::SeqCst);
                
                // Create a temporary LMSAudioController for connection check
                let temp_controller = LMSAudioController {
                    base: base.clone(),
                    config: controller_config.clone(),
                    client: Arc::new(RwLock::new(None)),
                    player: Arc::new(RwLock::new(None)),
                    is_connected: Arc::new(AtomicBool::new(was_connected)),
                    running: Arc::new(AtomicBool::new(true)),
                    last_connected_server: Arc::new(RwLock::new(None)),
                };
                
                // Check if connection state has changed
                let now_connected = temp_controller.check_connected();
                
                // Update connection state if it changed
                if was_connected != now_connected {
                    is_connected.store(now_connected, Ordering::SeqCst);
                    
                    if now_connected {
                        info!("LMS connection established");
                        base.notify_state_changed(PlaybackState::Stopped);
                    } else {
                        info!("LMS connection lost");
                        base.notify_state_changed(PlaybackState::Disconnected);
                    }
                }
                
                // If still disconnected, log an attempt with MAC addresses
                if !now_connected {
                    // Get the configured MAC addresses from the current config
                    if let Ok(cfg) = controller_config.read() {
                        // Get MAC addresses to test, including local ones if "local" is configured
                        let macs_to_test = temp_controller.prepare_mac_addresses(&cfg.player_macs, false);
                        
                        if !macs_to_test.is_empty() {
                            // Check if "local" was in the original configuration
                            let has_local = cfg.player_macs.iter().any(|m| m.to_lowercase() == "local");
                            if has_local {
                                info!("LMS player still disconnected (tested configured and local MAC addresses) - will retry in {} seconds", 
                                      config.reconnection_interval);
                            } else {
                                info!("LMS player still disconnected (tested configured MAC addresses: {}) - will retry in {} seconds", 
                                      macs_to_test.join(", "), config.reconnection_interval);
                            }
                            continue;
                        }
                    }
                    
                    // Fallback to just showing local MAC addresses
                    match get_local_mac_addresses() {
                        Ok(addresses) if !addresses.is_empty() => {
                            // Format MAC addresses as strings
                            let mac_addrs = addresses.iter()
                                .map(|mac| mac.to_string())
                                .collect::<Vec<String>>()
                                .join(", ");
                            
                            info!("LMS player still disconnected (tested local MACs: {}) - will retry in {} seconds", 
                                  mac_addrs, config.reconnection_interval);
                        },
                        Ok(_) => {
                            debug!("LMS player still disconnected, no MAC addresses found - will retry in {} seconds", 
                                   config.reconnection_interval);
                        },
                        Err(e) => {
                            debug!("LMS player still disconnected, error getting MAC addresses: {} - will retry in {} seconds", 
                                   e, config.reconnection_interval);
                        }
                    }
                }
            }
            
            info!("LMS reconnection thread stopped");
        });
    }
}

impl PlayerController for LMSAudioController {
    fn get_capabilities(&self) -> PlayerCapabilitySet {
        self.base.get_capabilities()
    }
    
    fn get_song(&self) -> Option<Song> {
        // Check if we're connected first
        if !self.is_connected.load(Ordering::SeqCst) {
            return None;
        }
        
        // Get direct access to the player instance
        if let Ok(player_guard) = self.player.read() {
            if let Some(player_instance) = player_guard.as_ref() {
                // Get real-time song information directly from the server
                debug!("Fetching real-time song information from LMS server");
                return player_instance.get_current_song();
            }
        }
        
        None
    }

    fn get_queue(&self) -> Vec<Track> {
        // Not yet implemented
        Vec::new()
    }
    
    fn get_loop_mode(&self) -> LoopMode {
        // Not yet implemented
        LoopMode::None
    }
    
    fn get_playback_state(&self) -> PlaybackState {
        // First check if player is connected - this is just an atomic read, so it's safe
        if !self.is_connected.load(Ordering::SeqCst) {
            return PlaybackState::Disconnected;
        }
        
        // Get player and server configuration
        let config = match self.config.try_read() {
            Ok(cfg) => cfg.clone(),
            Err(_) => {
                warn!("Could not acquire non-blocking read lock on config");
                return PlaybackState::Unknown;
            }
        };
        
        // Get server address from config
        let server_address = match &config.server {
            Some(address) => address.clone(),
            None => {
                warn!("No server address configured");
                return PlaybackState::Unknown;
            }
        };
        
        // Get player ID without locks that could block
        let player_id = match self.player.try_read() {
            Ok(guard) => {
                match guard.as_ref() {
                    Some(player) => player.get_player_id().to_string(), // Clone the string
                    None => {
                        warn!("Player object is missing");
                        return PlaybackState::Unknown;
                    }
                }
            },
            Err(_) => {
                warn!("Could not acquire non-blocking read lock on player");
                return PlaybackState::Unknown;
            }
        };

        // Create a fresh LmsRpcClient for this specific request
        // Uses our centralized HTTP client implementation that's fully synchronous
        let mut temp_client = LmsRpcClient::new(&server_address, config.port)
            .with_timeout(2); // short 2-second timeout
        
        // Make a direct synchronous request 
        match temp_client.get_player_status(&player_id) {
            Ok(status) => {
                // Check if power is on first
                if status.power == 0 {
                    return PlaybackState::Disconnected;  // Use Disconnected for powered-off state
                }
                
                // Check mode to determine playback state
                match status.mode.as_str() {
                    "play" => PlaybackState::Playing,
                    "pause" => PlaybackState::Paused,
                    "stop" => PlaybackState::Stopped,
                    "" => PlaybackState::Stopped,
                    _ => {
                        debug!("Unknown LMS playback mode: {}", status.mode);
                        PlaybackState::Unknown
                    }
                }
            },
            Err(e) => {
                debug!("Failed to get LMS player status: {}", e);
                PlaybackState::Unknown
            }
        }
    }
    
    fn get_position(&self) -> Option<f64> {
        // Check if we're connected first
        if !self.is_connected.load(Ordering::SeqCst) {
            return None;
        }
        
        // Get direct access to the player instance
        if let Ok(player_guard) = self.player.read() {
            if let Some(player_instance) = player_guard.as_ref() {
                // Get real-time position information directly from the server
                debug!("Fetching real-time position information from LMS server");
                return player_instance.get_current_position().ok().map(|pos| pos as f64);
            }
        }
        
        None
    }
    
    fn get_shuffle(&self) -> bool {
        // Not yet implemented
        false
    }
    
    fn get_player_name(&self) -> String {
        self.base.get_player_name()
    }
    
    fn get_player_id(&self) -> String {
        self.base.get_player_id()
    }
    
    fn get_last_seen(&self) -> Option<SystemTime> {
        self.base.get_last_seen()
    }
    
    fn send_command(&self, _command: PlayerCommand) -> bool {
        // Use cached connection state
        if !self.is_connected.load(Ordering::SeqCst) {
            debug!("Cannot send command - LMS player is disconnected");
            return false;
        }
        
        // Not yet implemented - would send the command to the LMS server
        false
    }
    
    fn register_state_listener(&mut self, listener: Weak<dyn PlayerStateListener>) -> bool {
        self.base.register_state_listener(listener)
    }
    
    fn unregister_state_listener(&mut self, listener: &Arc<dyn PlayerStateListener>) -> bool {
        self.base.unregister_state_listener(listener)
    }
    
    fn as_any(&self) -> &dyn Any {
        self
    }
    
    fn start(&self) -> bool {
        // Read the configuration to get access to configured MACs
        let config = match self.config.read() {
            Ok(cfg) => cfg.clone(),
            Err(_) => {
                warn!("Failed to acquire read lock on LMS configuration");
                LMSAudioConfig::default()
            }
        };
        
        // Get all MAC addresses to test, including special values like "local"
        let all_test_macs = self.prepare_mac_addresses(&config.player_macs, false);
        
        // Do a synchronous connection check
        let is_connected = self.check_connected();
        self.is_connected.store(is_connected, Ordering::SeqCst);
        
        if is_connected {
            info!("LMS player successfully connected");
        } else {
            // Log all the MAC addresses that were tested
            if all_test_macs.is_empty() {
                info!("LMS player is disconnected - no MAC addresses available for testing");
            } else {
                // Check if "local" was in the original configuration
                let has_local = config.player_macs.iter().any(|mac| mac.to_lowercase() == "local");
                
                // Get local MACs for logging if requested
                let mut display_macs = all_test_macs.clone();
                if has_local {
                    // Add local MACs to the display list
                    match get_local_mac_addresses() {
                        Ok(addresses) => {
                            // Format local MAC addresses as strings
                            let local_macs: Vec<String> = addresses.iter()
                                .map(|mac| mac.to_string())
                                .collect();
                                
                            for mac in local_macs {
                                if !display_macs.contains(&mac) {
                                    display_macs.push(mac);
                                }
                            }
                            info!("LMS player is disconnected - tested configured and local MAC addresses: {}", 
                                display_macs.join(", "));
                        },
                        Err(_) => {
                            info!("LMS player is disconnected - tested configured MAC addresses (local MACs unavailable): {}", 
                                display_macs.join(", "));
                        }
                    }
                } else {
                    info!("LMS player is disconnected - tested configured MAC addresses: {}", 
                        display_macs.join(", "));
                }
            }
        }
        
        // Start the reconnection thread
        self.start_reconnection_thread();
        
        // Return true as the player controller started successfully,
        // even if the connection to LMS server failed
        true
    }
    
    fn stop(&self) -> bool {
        // Stop the reconnection thread
        self.running.store(false, Ordering::SeqCst);
        info!("LMS player stopping, reconnection thread will terminate");
        
        // Not yet implemented - would perform any necessary cleanup
        true
    }
    
    fn get_library(&self) -> Option<Box<dyn LibraryInterface>> {
        // Not yet implemented
        None
    }
}