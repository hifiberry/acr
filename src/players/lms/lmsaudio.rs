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
use crate::players::lms::lmsserver::{get_local_mac_addresses};
use crate::players::lms::lmspplayer::LMSPlayer;
use crate::players::lms::cli_listener::{LMSListener, AudioControllerRef};
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
    
    /// Currently connected server address
    connected_server: Arc<RwLock<Option<String>>>,
    
    /// CLI listener for receiving real-time events from the LMS server
    cli_listener: Arc<RwLock<Option<LMSListener>>>,
    
    /// Strong reference to the AudioControllerRef trait object
    /// This ensures the controller stays alive while the listener is active
    controller_ref: Arc<RwLock<Option<Arc<dyn AudioControllerRef>>>>,
    
    /// Last time an event was seen from this player
    last_seen: Arc<RwLock<Option<SystemTime>>>,
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
        let connected_server = Arc::new(RwLock::new(None));
        
        // Create a new controller
        let controller = Self {
            base: BasePlayerController::with_player_info("lms", "lms"),
            config: Arc::new(RwLock::new(config.clone())),
            client: Arc::new(RwLock::new(None)),
            player: Arc::new(RwLock::new(None)),
            is_connected,
            running,
            connected_server,
            cli_listener: Arc::new(RwLock::new(None)),
            controller_ref: Arc::new(RwLock::new(None)),
            last_seen: Arc::new(RwLock::new(None)),
        };
        
        // Initialize the player using find_server_connection
        let (connected, found_server, matched_mac, player_name) = controller.find_server_connection(&config);
        
        if connected {
            if let Some(found_server) = found_server {
                info!("Found a matching LMS server: {}", found_server);
                
                // Create a client for the found server
                let client = LmsRpcClient::new(&found_server, config.port);
                
                if let Some(matched_mac) = matched_mac {
                    info!("Found matching player: {} (MAC: {})", player_name.unwrap_or_default(), matched_mac);
                    
                    // Create the LMSPlayer instance
                    let player = LMSPlayer::new(client.clone(), &matched_mac);
                    
                    // Store the client and player
                    if let Ok(mut client_lock) = controller.client.write() {
                        *client_lock = Some(client);
                    }
                    
                    if let Ok(mut player_lock) = controller.player.write() {
                        *player_lock = Some(player);
                    }
                    
                    // Update connection state
                    controller.is_connected.store(true, Ordering::SeqCst);
                    
                    // Update the config with the discovered server
                    if let Ok(mut cfg_lock) = controller.config.write() {
                        cfg_lock.server = Some(found_server.clone());
                    }
                    
                    // Update connected server
                    if let Ok(mut connected_server) = controller.connected_server.write() {
                        *connected_server = Some(found_server.clone());
                    }
                    
                    // Start the CLI listener
                    controller.start_cli_listener(&found_server, &matched_mac);
                }
            }
        } else {
            info!("No LMS server found with our MAC addresses connected");
        }
        
        debug!("Created new LMS audio controller");
        controller
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
        
        // Create a clone of the controller so we can use the find_server_connection method
        let controller = self.clone();
        
        thread::spawn(move || {
            info!("LMS reconnection thread started (interval: {} seconds)", config.reconnection_interval);
            
            while running.load(Ordering::SeqCst) {
                // Sleep for the configured interval
                thread::sleep(interval);
                
                if !running.load(Ordering::SeqCst) {
                    break;
                }
                
                // Get the current connection state
                let was_connected = is_connected.load(Ordering::SeqCst);
                
                // Read the current configuration
                let current_config = match controller_config.read() {
                    Ok(cfg) => cfg.clone(),
                    Err(_) => {
                        warn!("Failed to acquire read lock on LMS configuration");
                        continue;
                    }
                };
                
                // Check connection status using find_server_connection
                let (now_connected, found_server, matched_mac, _) = controller.find_server_connection(&current_config);
                
                // Update connection state if it changed
                if was_connected != now_connected {
                    is_connected.store(now_connected, Ordering::SeqCst);
                    
                    if now_connected {
                        info!("LMS connection established");
                        base.notify_state_changed(PlaybackState::Stopped);
                        
                        // Start the CLI listener if we have both server and player information
                        if let (Some(server), Some(player_id)) = (found_server, matched_mac) {
                            controller.start_cli_listener(&server, &player_id);
                        }
                    } else {
                        info!("LMS connection lost");
                        base.notify_state_changed(PlaybackState::Disconnected);
                        
                        // Stop the CLI listener when connection is lost
                        controller.stop_cli_listener();
                    }
                }
                
                // If still disconnected, log an attempt with MAC addresses
                if !now_connected {
                    if !current_config.player_macs.is_empty() {
                        // Check if "local" was in the original configuration
                        let has_local = current_config.player_macs.iter().any(|m| m.to_lowercase() == "local");
                        if has_local {
                            info!("LMS player still disconnected (tested configured and local MAC addresses) - will retry in {} seconds", 
                                  config.reconnection_interval);
                        } else {
                            info!("LMS player still disconnected (tested configured MAC addresses: {}) - will retry in {} seconds", 
                                  current_config.player_macs.join(", "), config.reconnection_interval);
                        }
                    } else {
                        debug!("LMS player still disconnected, no MAC addresses available - will retry in {} seconds", 
                               config.reconnection_interval);
                    }
                }
            }
            
            // Stop the CLI listener when stopping the reconnection thread
            controller.stop_cli_listener();
            
            info!("LMS reconnection thread stopped");
        });
    }
    
    /// Find a server that any of the configured MAC addresses is connected to
    /// 
    /// # Arguments
    /// * `config` - Controller configuration
    /// 
    /// # Returns
    /// A tuple containing:
    /// - Boolean indicating if a connection was found
    /// - Optional server address if found
    /// - Optional matched MAC address if found
    /// - Optional player name if found
    fn find_server_connection(&self, config: &LMSAudioConfig) -> (bool, Option<String>, Option<String>, Option<String>) {
        // First check if we are already connected to a server
        if self.is_connected.load(Ordering::SeqCst) {
            // Get the connected server
            if let Ok(connected_server_guard) = self.connected_server.read() {
                if let Some(server) = connected_server_guard.as_ref() {
                    // Get player ID
                    if let Ok(player_guard) = self.player.read() {
                        if let Some(player) = player_guard.as_ref() {
                            let player_id = player.get_player_id();
                            
                            debug!("Already connected to server {}, checking if still connected", server);
                            
                            // Check if still connected to this server
                            if crate::players::lms::player_finder::is_player(server, vec![player_id.to_string()]) {
                                debug!("Still connected to server {}", server);
                                return (true, Some(server.clone()), Some(player_id.to_string()), None);
                            } else {
                                debug!("No longer connected to server {}", server);
                            }
                        }
                    }
                }
            }
        }
        
        // If not already connected or no longer connected, proceed with normal server discovery
        
        // Gather servers to check
        let mut servers_to_check = Vec::new();
        let mac_addresses = config.player_macs.clone();
        
        // Add explicitly configured server if available
        if let Some(server) = &config.server {
            servers_to_check.push(server.clone());
        }
        
        // Use autodiscovery if enabled
        if config.autodiscovery {
            match crate::players::lms::lmsserver::find_local_servers(Some(5)) {
                Ok(discovered_servers) => {
                    for server in discovered_servers {
                        if !servers_to_check.contains(&server.ip.to_string()) {
                            servers_to_check.push(server.ip.to_string());
                        }
                    }
                },
                Err(e) => {
                    warn!("Failed to discover LMS servers: {}", e);
                }
            }
        }
        
        // Process MAC addresses including "local" keyword
        let all_mac_addresses = self.prepare_mac_addresses(&mac_addresses, true);
        
        // Try to find a server with any of our MAC addresses connected
        if all_mac_addresses.is_empty() || servers_to_check.is_empty() {
            debug!("No MAC addresses or servers available to check");
            return (false, None, None, None);
        }
        
        // Use find_my_server to locate a matching server
        if let Some(found_server) = crate::players::lms::player_finder::find_my_server(servers_to_check, all_mac_addresses.clone()) {
            debug!("Found matching server: {}", found_server);
            
            // Create a client for the found server
            let client = LmsRpcClient::new(&found_server, config.port);
            
            // Find the specific matched player
            if let Ok(players) = client.clone().get_players() {
                for player in &players {
                    match normalize_mac_address(&player.playerid) {
                        Ok(player_mac) => {
                            let player_mac_str = crate::helpers::macaddress::mac_to_lowercase_string(&player_mac);
                            
                            // Check if this player matches any of our MAC addresses
                            for mac in &all_mac_addresses {
                                if crate::helpers::macaddress::mac_equal_ignore_case(&player_mac_str, mac) {
                                    return (
                                        true,
                                        Some(found_server),
                                        Some(player.playerid.clone()),
                                        Some(player.name.clone())
                                    );
                                }
                            }
                        },
                        Err(_) => continue
                    }
                }
            }
            
            // Found a server but couldn't determine the specific player
            return (true, Some(found_server), None, None);
        }
        
        // No matching server found
        (false, None, None, None)
    }

    /// Start the CLI listener for this player and server
    fn start_cli_listener(&self, server: &str, player_id: &str) {
        debug!("Starting CLI listener for server {} and player {}", server, player_id);
        
        // First stop any existing listener
        self.stop_cli_listener();
        
        // Create a strong reference to self that will be stored alongside the listener
        let controller_arc: Arc<dyn AudioControllerRef> = Arc::new(self.clone());
        
        // Create a weak reference from the strong reference
        let controller_ref = Arc::downgrade(&controller_arc);
        
        // Create a new CLI listener
        let mut listener = LMSListener::new(server, player_id, controller_ref);
        
        // Start the listener
        listener.start();
        
        // Store the listener and the strong reference to the controller
        if let Ok(mut cli_lock) = self.cli_listener.write() {
            // Store both the listener and the strong reference to keep it alive
            *cli_lock = Some(listener);
            debug!("CLI listener started and stored");
        } else {
            warn!("Failed to acquire write lock for CLI listener");
        }
        
        // Store the strong reference to the controller
        if let Ok(mut controller_ref_lock) = self.controller_ref.write() {
            *controller_ref_lock = Some(controller_arc);
        } else {
            warn!("Failed to acquire write lock for controller_ref");
        }
    }
    
    /// Stop the CLI listener if running
    fn stop_cli_listener(&self) {
        if let Ok(mut cli_lock) = self.cli_listener.write() {
            if let Some(mut listener) = cli_lock.take() {
                debug!("Stopping CLI listener");
                listener.stop();
            }
        }
        
        // Clear the strong reference to the controller
        if let Ok(mut controller_ref_lock) = self.controller_ref.write() {
            *controller_ref_lock = None;
        }
    }

    /// Get the current song and send a SongChanged event to listeners
    /// 
    /// This method fetches the current song from the LMS server and
    /// sends a SongChanged event to all registered listeners.
    /// 
    /// # Returns
    /// The current Song if available, or None if no song is playing
    pub fn update_and_notify_song(&self) -> Option<Song> {
        // Skip if not connected
        if !self.is_connected.load(Ordering::SeqCst) {
            return None;
        }
        
        // Get the current song
        let song = self.get_song();
        
        // Send the SongChanged event
        debug!("Sending SongChanged event: {:?}", song);
        if let Some(ref s) = song {
            self.base.notify_song_changed(Some(s));
        } else {
            self.base.notify_song_changed(None);
        }
        
        // Return the song for potential further use
        song
    }
    
    /// Get the current position and send a PositionChanged event to listeners
    /// 
    /// This method fetches the current playback position from the LMS server and
    /// sends a PositionChanged event to all registered listeners.
    /// 
    /// # Returns
    /// The current position in seconds if available, or None if position cannot be determined
    pub fn update_and_notify_position(&self) -> Option<f64> {
        // Skip if not connected
        if !self.is_connected.load(Ordering::SeqCst) {
            return None;
        }
        
        // Get the current position
        let position = self.get_position();
        
        if let Some(pos) = position {
            // Send the PositionChanged event
            debug!("Sending PositionChanged event: position={}", pos);
            self.base.notify_position_changed(pos);
        }
        
        // Return the position for potential further use
        position
    }
}

impl Clone for LMSAudioController {
    fn clone(&self) -> Self {
        Self {
            base: self.base.clone(),
            config: self.config.clone(),
            client: self.client.clone(),
            player: self.player.clone(),
            is_connected: self.is_connected.clone(),
            running: self.running.clone(),
            connected_server: self.connected_server.clone(),
            cli_listener: self.cli_listener.clone(),
            controller_ref: self.controller_ref.clone(),
            last_seen: self.last_seen.clone(),
        }
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
        
        // Check connection status using find_server_connection
        let (is_connected, _, _, _) = self.find_server_connection(&config);
        
        // Update connection status
        self.is_connected.store(is_connected, Ordering::SeqCst);
        
        if is_connected {
            info!("LMS player successfully connected");
        } else {
            // Log all the MAC addresses that were tested
            let all_test_macs = self.prepare_mac_addresses(&config.player_macs, true);
            if all_test_macs.is_empty() {
                info!("LMS player is disconnected - no MAC addresses available for testing");
            } else {
                // Check if "local" was in the original configuration
                let has_local = config.player_macs.iter().any(|mac| mac.to_lowercase() == "local");
                
                if has_local {
                    info!("LMS player is disconnected - tested configured and local MAC addresses: {}", 
                        all_test_macs.join(", "));
                } else {
                    info!("LMS player is disconnected - tested configured MAC addresses: {}", 
                        all_test_macs.join(", "));
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

/// Implementation of the AudioControllerRef trait for LMSAudioController
impl AudioControllerRef for LMSAudioController {
    /// Update the last_seen timestamp to the current time
    fn seen(&self) {
        if let Ok(mut last_seen) = self.last_seen.write() {
            *last_seen = Some(SystemTime::now());
            debug!("Updated last_seen timestamp for LMS player");
        }
    }
    
    /// Handle state change notifications from CLI listener
    fn state_changed(&self, state: PlaybackState) {
        // First update the last seen timestamp
        self.seen();
        
        // Notify all registered listeners about the state change
        debug!("LMS state changed to: {:?}", state);
        self.base.notify_state_changed(state);
    }
    
    /// Update song information and notify listeners
    fn update_song(&self) {
        debug!("CLI listener requested song update");
        self.update_and_notify_song();
    }
    
    /// Update position information and notify listeners
    fn update_position(&self) {
        debug!("CLI listener requested position update");
        self.update_and_notify_position();
    }
}