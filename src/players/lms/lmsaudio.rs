use std::any::Any;
use std::sync::{Arc, RwLock, Weak, atomic::{AtomicBool, Ordering}};
use std::time::{SystemTime, Duration};
use std::thread;
use log::{debug, info, warn};
use serde::{Deserialize, Serialize};
use serde_json::Value;
// Remove the direct tokio runtime import since we'll use the global one
// use tokio::runtime::Runtime;

use crate::data::{LoopMode, PlaybackState, PlayerCapabilitySet, PlayerCommand, Song, Track};
use crate::PlayerStateListener;
use crate::data::library::LibraryInterface;
use crate::players::player_controller::{BasePlayerController, PlayerController};
use crate::players::lms::jsonrps::LmsRpcClient;
use crate::players::lms::lmsserver::{find_local_servers, get_local_mac_addresses};
use crate::helpers::macaddress::normalize_mac_address;
// Import the global runtime accessor function
use crate::get_tokio_runtime;

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
    
    /// Player MAC address to connect to
    pub player_mac: Option<String>,
    
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
            player_mac: None,
            reconnection_interval: default_reconnection_interval(),
        }
    }
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
    
    /// Last known connection state
    is_connected: Arc<AtomicBool>,
    
    /// Flag to control the reconnection thread
    running: Arc<AtomicBool>,
}

impl LMSAudioController {
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
        
        let is_connected = Arc::new(AtomicBool::new(false));
        let running = Arc::new(AtomicBool::new(true));
        
        // Create a new controller
        let controller = Self {
            base: BasePlayerController::with_player_info("lms", "lms"),
            config: Arc::new(RwLock::new(config)),
            client: Arc::new(RwLock::new(None)),
            is_connected,
            running,
        };
        
        debug!("Created new LMS audio controller");
        controller
    }
    
    /// Check if the current system is connected to the configured LMS server
    /// 
    /// This method determines if the current device is registered as a player with
    /// the configured LMS server.
    /// 
    /// # Returns
    /// `true` if connected, `false` otherwise
    pub async fn check_connected(&self) -> bool {
        // Get configuration
        let config = match self.config.read() {
            Ok(cfg) => cfg.clone(),
            Err(_) => {
                warn!("Failed to acquire read lock on LMS configuration");
                return false;
            }
        };
        
        // If we have a server configured, try direct connection
        if let Some(server) = &config.server {
            debug!("Checking connection to configured LMS server: {}", server);
            
            // Create a client for the configured server
            let mut client = LmsRpcClient::new(server, config.port);
            
            // Get the local MAC addresses
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
            match client.get_players().await {
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
                                    info!("Found matching player: {} ({:?})", 
                                         player.name, 
                                         player_mac);
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
            match find_local_servers(Some(5)).await {
                Ok(servers) => {
                    if servers.is_empty() {
                        debug!("No LMS servers found via autodiscovery");
                        return false;
                    }
                    
                    debug!("Found {} LMS servers via autodiscovery", servers.len());
                    
                    // Get the local MAC addresses
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
                    
                    // Check each server for matching players
                    for server in servers {
                        debug!("Checking server: {} at {}", server.name, server.ip);
                        
                        // Create a client for this server
                        let mut client = server.create_client().await;
                        
                        // Get the players from the server
                        match client.get_players().await {
                            Ok(players) => {
                                debug!("Found {} players on server {}", players.len(), server.name);
                                
                                // Check if any player matches our MAC address
                                for player in players {
                                    match normalize_mac_address(&player.playerid) {
                                        Ok(player_mac) => {
                                            // Check if this player's MAC matches any of our local MACs
                                            if normalized_local_macs.contains(&player_mac) {
                                                info!("Found matching player: {} ({:?}) on server {}", 
                                                     player.name,
                                                     player_mac,
                                                     server.name);
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
            
            // Use the global Tokio runtime for this thread
            let rt = get_tokio_runtime();
            
            while running.load(Ordering::SeqCst) {
                // Sleep for the configured interval
                thread::sleep(interval);
                
                if !running.load(Ordering::SeqCst) {
                    break;
                }
                
                // If we're already connected, just check the connection
                let was_connected = is_connected.load(Ordering::SeqCst);
                
                // Check if connection state has changed
                let now_connected = match rt.block_on(async {
                    // Create a temporary LMSAudioController for connection check
                    let temp_controller = LMSAudioController {
                        base: base.clone(),
                        config: controller_config.clone(),
                        client: Arc::new(RwLock::new(None)),
                        is_connected: Arc::new(AtomicBool::new(was_connected)),
                        running: Arc::new(AtomicBool::new(true)),
                    };
                    
                    temp_controller.check_connected().await
                }) {
                    true => true,
                    false => false,
                };
                
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
                
                // If still disconnected, log an attempt
                if !now_connected {
                    debug!("LMS player still disconnected, will retry in {} seconds", config.reconnection_interval);
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
        // Not yet implemented
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
        // Use the cached connection state instead of checking each time
        if !self.is_connected.load(Ordering::SeqCst) {
            return PlaybackState::Disconnected;
        }
        
        // If connected but state is unknown, return Stopped as default
        // This can be enhanced later to fetch the actual state from the LMS server
        PlaybackState::Stopped
    }
    
    fn get_position(&self) -> Option<f64> {
        // Not yet implemented
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
        // Use the global Tokio runtime for initial connection check
        let rt = get_tokio_runtime();
        
        // When starting, attempt to connect and update initial connection state
        let is_connected = rt.block_on(self.check_connected());
        self.is_connected.store(is_connected, Ordering::SeqCst);
        
        if is_connected {
            info!("LMS player successfully connected");
        } else {
            info!("LMS player is disconnected");
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