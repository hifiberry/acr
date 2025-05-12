use std::any::Any;
use std::sync::{Arc, RwLock, Weak, atomic::{AtomicBool, Ordering}};
use std::time::{SystemTime, Duration};
use std::thread;
use log::{debug, info, warn};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::data::{LoopMode, PlaybackState, PlayerCapabilitySet, PlayerCommand, Song, Track};
use crate::PlayerStateListener;
use crate::data::library::LibraryInterface;
use crate::players::player_controller::{BasePlayerController, PlayerController};
use crate::players::lms::jsonrps::LmsRpcClient;
use crate::players::lms::lmsserver::{find_local_servers, get_local_mac_addresses};
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
    
    /// Player MAC address to connect to
    pub player_mac: Option<String>,
    
    /// Reconnection interval in seconds (0 = disabled)
    #[serde(default = "default_reconnection_interval")]
    pub reconnection_interval: u64,
    
    /// Polling interval in seconds for now playing information (0 = disabled)
    #[serde(default = "default_polling_interval")]
    pub polling_interval: u64,
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

/// Default polling interval in seconds (30 seconds)
fn default_polling_interval() -> u64 {
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
            polling_interval: default_polling_interval(),
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
    
    /// Flag to control the polling thread
    polling: Arc<AtomicBool>,
    
    /// Currently playing song
    current_song: Arc<RwLock<Option<Song>>>,
    
    /// Current playback position
    current_position: Arc<RwLock<Option<f64>>>,
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
        let polling = Arc::new(AtomicBool::new(true));
        let current_song = Arc::new(RwLock::new(None));
        let current_position = Arc::new(RwLock::new(None));
        
        // Create a new controller
        let controller = Self {
            base: BasePlayerController::with_player_info("lms", "lms"),
            config: Arc::new(RwLock::new(config.clone())),
            client: Arc::new(RwLock::new(None)),
            player: Arc::new(RwLock::new(None)),
            is_connected,
            running,
            polling,
            current_song,
            current_position,
        };
        
        // Initialize the player with the default configuration
        // We'll connect to the server in the start() method
        if let Some(server) = &config.server {
            // Create a client for the configured server
            let client = LmsRpcClient::new(server, config.port);
            
            // Get the player MAC address or use a default if not provided
            let player_id = config.player_mac.as_deref().unwrap_or("00:00:00:00:00:00");
            
            // Create the LMSPlayer instance
            let player = LMSPlayer::new(client.clone(), player_id);
            
            // Store the client and player
            if let Ok(mut client_lock) = controller.client.write() {
                *client_lock = Some(client);
            }
            
            if let Ok(mut player_lock) = controller.player.write() {
                *player_lock = Some(player);
            }
        }
        
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
                        let mut client = server.create_client();
                        
                        // Get the players from the server
                        match client.get_players() {
                            Ok(players) => {
                                debug!("Found {} players on server {}", players.len(), server.name);
                                
                                // Check if any player matches our MAC address
                                for player in players {
                                    match normalize_mac_address(&player.playerid) {
                                        Ok(player_mac) => {
                                            // Check if this player's MAC matches any of our local MACs
                                            if normalized_local_macs.contains(&player_mac) {
                                                // Verify this isn't an all-zeros placeholder MAC address
                                                if !is_valid_mac(&player_mac) {
                                                    debug!("Ignoring invalid (all zeros) MAC address: {:?}", player_mac);
                                                    continue;
                                                }
                                                
                                                info!("Found matching player: {} ({:?}) on server {}", 
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
                    polling: Arc::new(AtomicBool::new(true)),
                    current_song: Arc::new(RwLock::new(None)),
                    current_position: Arc::new(RwLock::new(None)),
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
                
                // If still disconnected, log an attempt
                if !now_connected {
                    debug!("LMS player still disconnected, will retry in {} seconds", config.reconnection_interval);
                }
            }
            
            info!("LMS reconnection thread stopped");
        });
    }
    
    /// Start the polling thread for now playing information
    fn start_polling_thread(&self) {
        let config = match self.config.read() {
            Ok(cfg) => cfg.clone(),
            Err(_) => {
                warn!("Failed to acquire read lock on LMS configuration");
                return;
            }
        };
        
        // Don't start the polling thread if the interval is 0 (disabled)
        if config.polling_interval == 0 {
            info!("LMS now playing polling is disabled (interval = 0)");
            return;
        }
        
        let interval = Duration::from_secs(config.polling_interval);
        let is_connected = self.is_connected.clone();
        let polling = self.polling.clone();
        let player = self.player.clone();
        let current_song = self.current_song.clone();
        let current_position = self.current_position.clone();
        let base = self.base.clone();
        
        thread::spawn(move || {
            info!("LMS now playing polling thread started (interval: {} seconds)", config.polling_interval);
            
            // Keep track of last song to detect changes
            let mut last_song: Option<Song> = None;
            
            while polling.load(Ordering::SeqCst) {
                // Only poll if connected
                if is_connected.load(Ordering::SeqCst) {
                    // Get a reference to the player if available
                    if let Ok(player_guard) = player.read() {
                        if let Some(player_instance) = player_guard.as_ref() {
                            // Poll for now playing info
                            if let Some((song, position)) = player_instance.now_playing() {
                                // Update stored current song and position
                                if let Ok(mut song_guard) = current_song.write() {
                                    *song_guard = Some(song.clone());
                                }
                                
                                if let Ok(mut pos_guard) = current_position.write() {
                                    *pos_guard = Some(position as f64);
                                }
                                
                                // If song changed, notify listeners
                                let song_changed = match &last_song {
                                    Some(prev_song) => prev_song != &song,
                                    None => true, // First song detected
                                };
                                
                                if song_changed {
                                    debug!("Song changed: {:?}", song.title);
                                    last_song = Some(song.clone());
                                    
                                    // Notify listeners about the song change
                                    base.notify_song_changed(Some(&song));
                                }
                            } else {
                                // No song is playing
                                if last_song.is_some() {
                                    debug!("Playback stopped");
                                    last_song = None;
                                    
                                    // Clear stored song and position
                                    if let Ok(mut song_guard) = current_song.write() {
                                        *song_guard = None;
                                    }
                                    
                                    if let Ok(mut pos_guard) = current_position.write() {
                                        *pos_guard = None;
                                    }
                                    
                                    // Notify listeners that no song is playing
                                    base.notify_song_changed(None);
                                }
                            }
                        }
                    }
                }
                
                // Sleep for the configured interval
                thread::sleep(interval);
                
                // Check if we should exit the loop
                if !polling.load(Ordering::SeqCst) {
                    break;
                }
            }
            
            info!("LMS now playing polling thread stopped");
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
        
        // Use the cached song information from the polling thread
        match self.current_song.read() {
            Ok(song_guard) => song_guard.clone(),
            Err(e) => {
                warn!("Failed to acquire read lock on current song: {}", e);
                None
            }
        }
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
        // First check if player is connected
        if !self.is_connected.load(Ordering::SeqCst) {
            return PlaybackState::Disconnected;
        }
        
        // Use the cached state from the polling thread instead of making an async call
        // This makes the function fully synchronous and avoids runtime conflicts
        match self.current_song.read() {
            Ok(song_guard) => {
                if song_guard.is_some() {
                    // If we have a current song, we're playing
                    PlaybackState::Playing
                } else {
                    // No current song, assume stopped
                    PlaybackState::Stopped
                }
            },
            Err(e) => {
                warn!("Failed to acquire read lock on current song: {}", e);
                PlaybackState::Unknown
            }
        }
    }
    
    fn get_position(&self) -> Option<f64> {
        // Check if we're connected first
        if !self.is_connected.load(Ordering::SeqCst) {
            return None;
        }
        
        // Use the cached position information from the polling thread
        match self.current_position.read() {
            Ok(pos_guard) => *pos_guard,
            Err(e) => {
                warn!("Failed to acquire read lock on current position: {}", e);
                None
            }
        }
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
        // Simply do a synchronous connection check instead of using tokio
        let is_connected = self.check_connected();
        self.is_connected.store(is_connected, Ordering::SeqCst);
        
        if is_connected {
            info!("LMS player successfully connected");
        } else {
            info!("LMS player is disconnected");
        }
        
        // Start the reconnection thread
        self.start_reconnection_thread();
        
        // Start the now playing polling thread
        self.start_polling_thread();
        
        // Return true as the player controller started successfully,
        // even if the connection to LMS server failed
        true
    }
    
    fn stop(&self) -> bool {
        // Stop the reconnection thread
        self.running.store(false, Ordering::SeqCst);
        info!("LMS player stopping, reconnection thread will terminate");
        
        // Stop the polling thread
        self.polling.store(false, Ordering::SeqCst);
        info!("LMS player stopping, polling thread will terminate");
        
        // Not yet implemented - would perform any necessary cleanup
        true
    }
    
    fn get_library(&self) -> Option<Box<dyn LibraryInterface>> {
        // Not yet implemented
        None
    }
}