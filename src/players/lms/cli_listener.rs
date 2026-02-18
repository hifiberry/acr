use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::sync::{Arc, atomic::{AtomicBool, Ordering}, Weak};
use parking_lot::RwLock;
use std::thread;
use std::time::{Duration, SystemTime};
use log::{warn, debug, error, trace, info};
use urlencoding::decode;

use crate::data::PlaybackState;

// Forward declaration to avoid circular dependency
type WeakAudioController = Weak<dyn AudioControllerRef>;

/// Interface for interacting with the Audio Controller
pub trait AudioControllerRef: Send + Sync {
    /// Notify that an event was seen from this player
    fn seen(&self);
    
    /// Notify that player state has changed
    fn state_changed(&self, state: PlaybackState);
    
    /// Update song information and notify listeners
    fn update_song(&self);
    
    /// Update position information and notify listeners
    fn update_position(&self);
    
    /// Convert to Any for dynamic casting
    fn as_any(&self) -> &dyn std::any::Any;
}

/// List of commands that should only be logged at debug level
const IGNORED_COMMANDS: &[&str] = &[
    "playlist open",
    "playlist pause",
    "playlist newsong",
    "playlist jump",
    "button",
    "menustatus",
    "server currentSong",
    "material-skin",
    "prefset plugin.fulltext",
    "prefset server currentSong",
    "mixer",
    "scanner notify progress",
    "listen 1"
];

/// Helper function to check if a command matches any of the ignored commands
fn is_ignored_command(cmd_parts: &[String]) -> bool {
    if cmd_parts.is_empty() {
        return false;
    }
    
    // Join the command parts to create the full command string
    let full_command = cmd_parts.join(" ");
    
    // Check if the full command starts with any of the ignored commands
    IGNORED_COMMANDS.iter().any(|ignored| full_command.starts_with(ignored))
}

/// LMSListener connects to the Logitech Media Server CLI interface on port 9090
/// and logs all messages received from the server
pub struct LMSListener {
    /// Server address (hostname or IP)
    server_address: String,
    
    /// Player ID (MAC address)
    player_id: String,
    
    /// Running flag to control the listener thread
    running: Arc<AtomicBool>,
    
    /// Thread handle for the listener
    thread_handle: Option<thread::JoinHandle<()>>,
    
    /// Reference to the parent audio controller
    controller: WeakAudioController,
    
    /// Last time displaynotify was processed (to avoid duplicate events)
    last_display_notify: Arc<RwLock<Option<SystemTime>>>,
}

impl LMSListener {
    /// Create a new LMS CLI listener
    /// 
    /// # Arguments
    /// * `server` - Server address (hostname or IP)
    /// * `player_id` - Player ID (MAC address)
    /// * `controller` - Reference to the parent audio controller
    pub fn new(server: &str, player_id: &str, controller: WeakAudioController) -> Self {
        Self {
            server_address: server.to_string(),
            player_id: player_id.to_string(),
            running: Arc::new(AtomicBool::new(false)),
            thread_handle: None,
            controller,
            last_display_notify: Arc::new(RwLock::new(None)),
        }
    }
    
    /// Start the listener thread
    pub fn start(&mut self) {
        // Check if already running
        if self.running.load(Ordering::SeqCst) {
            debug!("LMSListener already running");
            return;
        }
        
        self.running.store(true, Ordering::SeqCst);
        let server = self.server_address.clone();
        let player_id = self.player_id.clone();
        let running = self.running.clone();
        let controller = self.controller.clone();
        let last_display_notify = self.last_display_notify.clone();
        
        self.thread_handle = Some(thread::spawn(move || {
            // Main connection loop - try to reconnect if connection fails
            while running.load(Ordering::SeqCst) {
                match Self::connect_and_listen(&server, &player_id, running.clone(), controller.clone(), last_display_notify.clone()) {
                    Ok(_) => {
                        // Connection closed normally, try to reconnect after a delay
                        if running.load(Ordering::SeqCst) {
                            warn!("LMS CLI connection closed, reconnecting in 5 seconds...");
                            thread::sleep(Duration::from_secs(5));
                        }
                    },
                    Err(e) => {
                        // Connection failed, try again after a delay
                        error!("Failed to connect to LMS CLI: {}", e);
                        
                        if running.load(Ordering::SeqCst) {
                            warn!("Will retry LMS CLI connection in 10 seconds...");
                            thread::sleep(Duration::from_secs(10));
                        }
                    }
                };
            }
            
            debug!("LMSListener thread exiting");
        }));
        
        debug!("LMSListener started for server {} and player {}", self.server_address, self.player_id);
    }
    
    /// Parse an LMS event string into MAC address and command components
    /// 
    /// # Arguments
    /// * `event` - Raw event string from LMS CLI
    /// 
    /// # Returns
    /// A tuple containing:
    /// - Optional MAC address if present in the event
    /// - Vector of command components
    fn parse_lms_event(event: &str) -> (Option<String>, Vec<String>) {
        // Split the event into components
        let components: Vec<&str> = event.split_whitespace().collect();
        
        if components.is_empty() {
            return (None, Vec::new());
        }
        
        // Check if the first component looks like a MAC address
        // LMS encodes colons as %3A in the CLI
        let first = components[0];
        let is_mac_addr = first.contains("%3A") || first.contains(":");
        
        if is_mac_addr {
            // Try to decode the URL-encoded MAC address
            match decode(first) {
                Ok(mac) => {
                    // Return the MAC and the rest of the components
                    let mac_str = mac.to_string();
                    let cmd_parts: Vec<String> = components[1..].iter()
                        .map(|&s| decode(s).unwrap_or_else(|_| s.to_string().into()).to_string())
                        .collect();
                    
                    (Some(mac_str), cmd_parts)
                },
                Err(_) => {
                    // If decoding failed, return as-is
                    (Some(first.to_string()), components[1..].iter().map(|&s| s.to_string()).collect())
                }
            }
        } else {
            // No MAC address, return all components
            (None, components.iter().map(|&s| s.to_string()).collect())
        }
    }
    
    /// Connect to the server and listen for messages
    fn connect_and_listen(server: &str, player_id: &str, running: Arc<AtomicBool>, controller: WeakAudioController, last_display_notify: Arc<RwLock<Option<SystemTime>>>) -> Result<(), String> {
        // Connect to the LMS CLI on port 9090
        let address = format!("{}:9090", server);
        debug!("Connecting to LMS CLI at {}", address);
        
        let stream = match TcpStream::connect(&address) {
            Ok(s) => s,
            Err(e) => return Err(format!("Failed to connect to LMS CLI: {}", e)),
        };
        
        // Set read timeout to allow checking the running flag periodically
        if let Err(e) = stream.set_read_timeout(Some(Duration::from_secs(1))) {
            return Err(format!("Failed to set read timeout: {}", e));
        }
        
        // Subscribe to server events
        debug!("Subscribing to LMS events for player {}", player_id);
        let mut write_stream = stream.try_clone().map_err(|e| format!("Failed to clone TCP stream: {}", e))?;
        
        // Send the listen command to start receiving events
        if let Err(e) = write_stream.write_all(b"listen 1\n") {
            return Err(format!("Failed to send listen command: {}", e));
        }
        
        // Create a buffered reader for reading lines from the stream
        let reader = BufReader::new(stream);
        
        // Read lines until the connection is closed or the running flag is set to false
        info!("Connected to LMS CLI, receiving events...");
        
        for line in reader.lines() {
            if !running.load(Ordering::SeqCst) {
                debug!("LMSListener thread stopping");
                break;
            }
            
            match line {
                Ok(line) => {
                    // Parse the event
                    let (mac_opt, cmd_parts) = Self::parse_lms_event(&line);
                    
                    // Only update last_seen timestamp if the MAC address matches our player_id
                    if let Some(mac_addr) = &mac_opt {
                        // Use the MAC address helper to compare addresses case-insensitively
                        if crate::helpers::macaddress::mac_equal_ignore_case(mac_addr, player_id) {
                            // Notify the audio controller that we've seen activity for our player
                            if let Some(controller) = controller.upgrade() {
                                controller.seen();
                                trace!("Updated last_seen timestamp for player {}", player_id);
                            }
                        } else {
                            // Skip processing events for other players
                            trace!("Skipping event for different player: {}", mac_addr);
                            continue;
                        }
                    } else if !cmd_parts.is_empty() {
                        // For server-wide events without a specific player, continue processing
                        trace!("Processing server-wide event");
                    } else {
                        // Skip empty events
                        trace!("Skipping empty event");
                        continue;
                    }
                    
                    // Log the event with structured information
                    if let Some(mac_addr) = mac_opt {
                        if cmd_parts.is_empty() {
                            warn!("LMS event: MAC={}", mac_addr);
                        } else {
                            if is_ignored_command(&cmd_parts) {
                                debug!("Ignored LMS event: Player {} {}", mac_addr, cmd_parts.join(" "));
                                continue;
                            }
                            let cmd = &cmd_parts[0];
                            let args = if cmd_parts.len() > 1 {
                                cmd_parts[1..].join(" ")
                            } else {
                                String::new()
                            };
                            
                            match cmd.as_str() {
                                "playlist" => {
                                    if cmd_parts.len() > 1 {
                                        let all_args = cmd_parts[1..].join(" ");
                                        match cmd_parts[1].as_str() {
                                            "newsong" => {
                                                let song_title = if cmd_parts.len() > 2 { &cmd_parts[2] } else { "Unknown" };
                                                warn!("LMS event: Player {} started new song: {} (full command: playlist {})", 
                                                    mac_addr, song_title, all_args);
                                            },
                                            "pause" => {
                                                // is sent twice: as playlist pause (here) and as pause (below)
                                                // only use the pause event
                                            },
                                            "shuffle" => {
                                                // Handle shuffle mode changes
                                                if cmd_parts.len() > 2 {
                                                    let mode = &cmd_parts[2];
                                                    let shuffle_enabled = mode != "0";
                                                    debug!("LMS event: Player {} shuffle mode changed to {} ({})",
                                                          mac_addr, mode, if shuffle_enabled { "on" } else { "off" });
                                                          
                                                    // Notify the controller about the shuffle mode change
                                                    if let Some(ctrl) = controller.upgrade() {
                                                        // Use this AudioControllerRef trait object to access the BasePlayerController
                                                        // and notify about shuffle mode changes
                                                        if let Some(lms_controller) = ctrl.as_any().downcast_ref::<crate::players::lms::lmsaudio::LMSAudioController>() {
                                                            // Use the public method instead of directly accessing the private base field
                                                            lms_controller.notify_random_mode(shuffle_enabled);
                                                            debug!("Notified random mode change: {}", shuffle_enabled);
                                                        } else {
                                                            error!("Failed to downcast controller to LMSAudioController");
                                                        }
                                                    } else {
                                                        error!("Failed to upgrade controller reference for shuffle event");
                                                    }
                                                }
                                            },
                                            "repeat" => {
                                                // Handle repeat mode changes
                                                if cmd_parts.len() > 2 {
                                                    let mode = &cmd_parts[2];
                                                    debug!("LMS event: Player {} repeat mode changed to {}", mac_addr, mode);
                                                    
                                                    // Notify the controller about the loop mode change
                                                    if let Some(ctrl) = controller.upgrade() {
                                                        // Convert the mode to LoopMode enum and notify
                                                        if let Some(lms_controller) = ctrl.as_any().downcast_ref::<crate::players::lms::lmsaudio::LMSAudioController>() {
                                                            let loop_mode = match mode.as_str() {
                                                                "0" => crate::data::LoopMode::None,
                                                                "1" => crate::data::LoopMode::Track,
                                                                "2" => crate::data::LoopMode::Playlist,
                                                                _ => {
                                                                    debug!("Unknown repeat mode: {}", mode);
                                                                    crate::data::LoopMode::None
                                                                }
                                                            };
                                                            lms_controller.notify_loop_mode(loop_mode);
                                                            debug!("Notified loop mode change: {:?}", loop_mode);
                                                        } else {
                                                            error!("Failed to downcast controller to LMSAudioController");
                                                        }
                                                    } else {
                                                        error!("Failed to upgrade controller reference for repeat event");
                                                    }
                                                }
                                            },
                                            _ => {
                                                warn!("LMS event: Player {} {} {}", mac_addr, cmd, all_args);
                                            }
                                        }
                                    } else {
                                        warn!("LMS event: Player {} {}", mac_addr, cmd);
                                    }
                                },
                                "pause" => {
                                    let all_args = cmd_parts[1..].join(" ");
                                    let is_paused = cmd_parts.len() > 1 && cmd_parts[1] == "1";
                                    let state = if is_paused { "paused" } else { "resumed" };
                                    debug!("LMS event: Player {} {} (full command: pause {})", 
                                        mac_addr, state, all_args);
                                    
                                    // Notify the audio controller about the state change
                                    match controller.upgrade() {
                                        Some(ctrl) => {
                                            debug!("Sending state change to controller: {}", state);
                                            if is_paused {
                                                ctrl.state_changed(PlaybackState::Paused);
                                            } else {
                                                ctrl.state_changed(PlaybackState::Playing);
                                            }
                                        },
                                        None => {
                                            error!("Failed to upgrade controller reference - state change will not be sent!");
                                        }
                                    }
                                },
                                "client" => {
                                    let all_args = cmd_parts[1..].join(" ");
                                    if cmd_parts.len() > 1 {
                                        warn!("LMS event: Player {} client {} (full command: client {})", 
                                            mac_addr, cmd_parts[1], all_args);
                                        
                                        // Handle disconnect and reconnect events with state changes
                                        match cmd_parts[1].as_str() {
                                            "disconnect" => {
                                                debug!("LMS player disconnected - updating state");
                                                if let Some(ctrl) = controller.upgrade() {
                                                    ctrl.state_changed(PlaybackState::Disconnected);
                                                } else {
                                                    error!("Failed to upgrade controller reference on disconnect");
                                                }
                                            },
                                            "reconnect" => {
                                                debug!("LMS player reconnected - updating state to Stopped");
                                                if let Some(ctrl) = controller.upgrade() {
                                                    ctrl.state_changed(PlaybackState::Stopped);
                                                } else {
                                                    error!("Failed to upgrade controller reference on reconnect");
                                                }
                                            },
                                            _ => {} // Other client events, no state change needed
                                        }
                                    } else {
                                        warn!("LMS event: Player {} client event", mac_addr);
                                    }
                                },
                                "prefset" => {
                                    let all_args = cmd_parts[1..].join(" ");
                                    if cmd_parts.len() > 2 {
                                        // Handle server-level prefset commands
                                        if cmd_parts[1] == "server" && cmd_parts.len() > 3 {
                                            match cmd_parts[2].as_str() {
                                                "repeat" => {
                                                    // Parse the repeat mode (0=off, 1=song, 2=playlist)
                                                    if let Ok(mode) = cmd_parts[3].parse::<u8>() {
                                                        debug!("LMS server event: Setting repeat mode to {} (full command: prefset {})",
                                                            mode, all_args);
                                                        
                                                        // Find the loop mode based on the numeric value
                                                        let loop_mode = match mode {
                                                            0 => crate::data::LoopMode::None,
                                                            1 => crate::data::LoopMode::Track,
                                                            2 => crate::data::LoopMode::Playlist,
                                                            _ => {
                                                                debug!("Unknown repeat mode: {}", mode);
                                                                crate::data::LoopMode::None
                                                            }
                                                        };
                                                        
                                                        // Notify all controllers about this change,
                                                        // as it's a server-wide setting
                                                        if let Some(ctrl) = controller.upgrade() {
                                                            if let Some(lms_controller) = ctrl.as_any().downcast_ref::<crate::players::lms::lmsaudio::LMSAudioController>() {
                                                                lms_controller.notify_loop_mode(loop_mode);
                                                                debug!("Notified server-wide loop mode change: {:?}", loop_mode);
                                                            }
                                                        }
                                                    }
                                                },
                                                "shuffle" => {
                                                    // Parse the shuffle mode (0=off, 1=on)
                                                    if let Ok(mode) = cmd_parts[3].parse::<u8>() {
                                                        debug!("LMS server event: Setting shuffle mode to {} (full command: prefset {})",
                                                            mode, all_args);
                                                        
                                                        // Convert to boolean: anything non-zero is considered "on"
                                                        let shuffle_enabled = mode != 0;
                                                        
                                                        // Notify all controllers about this change,
                                                        // as it's a server-wide setting
                                                        if let Some(ctrl) = controller.upgrade() {
                                                            if let Some(lms_controller) = ctrl.as_any().downcast_ref::<crate::players::lms::lmsaudio::LMSAudioController>() {
                                                                lms_controller.notify_random_mode(shuffle_enabled);
                                                                debug!("Notified server-wide shuffle mode change: {}", shuffle_enabled);
                                                            }
                                                        }
                                                    }
                                                },
                                                _ => {
                                                    // Other server settings
                                                    warn!("LMS server event: Setting {} {} = {} (full command: prefset {})", 
                                                        cmd_parts[1], cmd_parts[2],
                                                        if cmd_parts.len() > 3 { &cmd_parts[3] } else { "" }, 
                                                        all_args);
                                                }
                                            }
                                        } else {
                                            warn!("LMS server event: Setting {} = {} (full command: prefset {})", 
                                                cmd_parts[1], 
                                                if cmd_parts.len() > 2 { &cmd_parts[2] } else { "" },
                                                all_args);
                                        }
                                    } else {
                                        warn!("LMS server event: prefset {} (full command: {})", 
                                            all_args, line);
                                    }
                                },
                                "displaynotify" => {
                                    // Display notification event - indicates updates to the player display
                                    // This is a good opportunity to refresh song and position information
                                    let now = SystemTime::now();
                                    let mut last_notify = last_display_notify.write();
                                    
                                    let should_skip = if let Some(last_time) = *last_notify {
                                        match now.duration_since(last_time) {
                                            Ok(duration) => duration < Duration::from_millis(200),
                                            Err(_) => {
                                                debug!("Clock skew detected, not skipping displaynotify");
                                                false
                                            }
                                        }
                                    } else {
                                        false
                                    };
                                    
                                    if should_skip {
                                        debug!("Skipping duplicate displaynotify event");
                                        continue;
                                    }
                                    
                                    *last_notify = Some(now);
                                    
                                    debug!("LMS event: Display notification received, updating song and position");
                                    
                                    if let Some(ctrl) = controller.upgrade() {
                                        // Update song and position information
                                        ctrl.update_song();
                                        ctrl.update_position();
                                    } else {
                                        error!("Failed to upgrade controller reference for display notification");
                                    }
                                },
                                "power" => {
                                    let all_args = cmd_parts[1..].join(" ");
                                    let is_powered = cmd_parts.len() > 1 && cmd_parts[1] == "1";
                                    let power_state = if is_powered { "on" } else { "off" };
                                    warn!("LMS event: Player {} power {} (full command: power {})", 
                                        mac_addr, power_state, all_args);
                                    
                                    // Update player state based on power status
                                    if let Some(ctrl) = controller.upgrade() {
                                        if is_powered {
                                            // When powered on, set to Stopped - actual playback state will follow
                                            warn!("Player powered on - updating state to Stopped");
                                            ctrl.state_changed(PlaybackState::Stopped);
                                        } else {
                                            // When powered off, set to Disconnected
                                            warn!("Player powered off - updating state to Disconnected");
                                            ctrl.state_changed(PlaybackState::Disconnected);
                                        }
                                    } else {
                                        error!("Failed to upgrade controller reference for power event");
                                    }
                                },
                                "time" => {
                                    let position = if cmd_parts.len() > 1 { &cmd_parts[1] } else { "unknown" };
                                    debug!("LMS event: Player {} position update: {} seconds", mac_addr, position);
                                    
                                    // Update player position when time events are received
                                    if let Some(ctrl) = controller.upgrade() {
                                        ctrl.update_position();
                                    } else {
                                        error!("Failed to upgrade controller reference for time event");
                                    }
                                },
                                _ => {
                                    // Default formatting for other events
                                    warn!("Unknown LMS event: Player {} {} {}", mac_addr, cmd, args);
                                }
                            }
                        }
                    } else {
                        // Server-wide events without a specific player
                        if !cmd_parts.is_empty() {
                            if is_ignored_command(&cmd_parts) {
                                debug!("Ignored LMS server event: {}", cmd_parts.join(" "));
                                continue;
                            }
                            let cmd = &cmd_parts[0];
                            let args = if cmd_parts.len() > 1 {
                                cmd_parts[1..].join(" ")
                            } else {
                                String::new()
                            };
                            
                            match cmd.as_str() {
                                "prefset" => {
                                    let all_args = cmd_parts[1..].join(" ");
                                    if cmd_parts.len() > 2 {
                                        // Handle server-level prefset commands
                                        if cmd_parts[1] == "server" && cmd_parts.len() > 3 {
                                            match cmd_parts[2].as_str() {
                                                "repeat" => {
                                                    // Parse the repeat mode (0=off, 1=song, 2=playlist)
                                                    if let Ok(mode) = cmd_parts[3].parse::<u8>() {
                                                        warn!("LMS server event: Setting repeat mode to {} (full command: prefset {})",
                                                            mode, all_args);
                                                        
                                                        // Find the loop mode based on the numeric value
                                                        let loop_mode = match mode {
                                                            0 => crate::data::LoopMode::None,
                                                            1 => crate::data::LoopMode::Track,
                                                            2 => crate::data::LoopMode::Playlist,
                                                            _ => {
                                                                debug!("Unknown repeat mode: {}", mode);
                                                                crate::data::LoopMode::None
                                                            }
                                                        };
                                                        
                                                        // Notify all controllers about this change,
                                                        // as it's a server-wide setting
                                                        if let Some(ctrl) = controller.upgrade() {
                                                            if let Some(lms_controller) = ctrl.as_any().downcast_ref::<crate::players::lms::lmsaudio::LMSAudioController>() {
                                                                lms_controller.notify_loop_mode(loop_mode);
                                                                debug!("Notified server-wide loop mode change: {:?}", loop_mode);
                                                            }
                                                        }
                                                    }
                                                },
                                                "shuffle" => {
                                                    // Parse the shuffle mode (0=off, 1=on)
                                                    if let Ok(mode) = cmd_parts[3].parse::<u8>() {
                                                        warn!("LMS server event: Setting shuffle mode to {} (full command: prefset {})",
                                                            mode, all_args);
                                                        
                                                        // Convert to boolean: anything non-zero is considered "on"
                                                        let shuffle_enabled = mode != 0;
                                                        
                                                        // Notify all controllers about this change,
                                                        // as it's a server-wide setting
                                                        if let Some(ctrl) = controller.upgrade() {
                                                            if let Some(lms_controller) = ctrl.as_any().downcast_ref::<crate::players::lms::lmsaudio::LMSAudioController>() {
                                                                lms_controller.notify_random_mode(shuffle_enabled);
                                                                debug!("Notified server-wide shuffle mode change: {}", shuffle_enabled);
                                                            }
                                                        }
                                                    }
                                                },
                                                _ => {
                                                    // Other server settings
                                                    warn!("LMS server event: Setting {} {} = {} (full command: prefset {})", 
                                                        cmd_parts[1], cmd_parts[2],
                                                        if cmd_parts.len() > 3 { &cmd_parts[3] } else { "" }, 
                                                        all_args);
                                                }
                                            }
                                        } else {
                                            warn!("LMS server event: Setting {} = {} (full command: prefset {})", 
                                                cmd_parts[1], 
                                                if cmd_parts.len() > 2 { &cmd_parts[2] } else { "" },
                                                all_args);
                                        }
                                    } else {
                                        warn!("LMS server event: prefset {} (full command: {})", 
                                            all_args, line);
                                    }
                                },
                                "artworkspec" => {
                                    let all_args = cmd_parts[1..].join(" ");
                                    warn!("LMS server event: Artwork spec update (full command: artworkspec {})",
                                        all_args);
                                },
                                _ => {
                                    // Default formatting for other events
                                    let all_args = cmd_parts[1..].join(" ");
                                    warn!("LMS server event: {} {} (full command: {} {})", 
                                        cmd, args, cmd, all_args);
                                }
                            }
                        } else {
                            warn!("LMS event: Empty command");
                        }
                    }
                },
                Err(e) => {
                    // Check if it's a timeout (would be io::ErrorKind::TimedOut or WouldBlock)
                    if e.kind() == std::io::ErrorKind::TimedOut || e.kind() == std::io::ErrorKind::WouldBlock {
                        // This is normal due to the read timeout, just continue
                        continue;
                    }
                    
                    // Real error, report it and exit the loop
                    error!("Error reading from LMS CLI: {}", e);
                    return Err(format!("Connection error: {}", e));
                }
            }
        }
        
        warn!("LMS CLI connection closed");
        Ok(())
    }
    
    /// Stop the listener thread
    pub fn stop(&mut self) {
        self.running.store(false, Ordering::SeqCst);
        debug!("Stopping LMSListener");
        
        // Wait for the thread to finish
        if let Some(handle) = self.thread_handle.take() {
            if let Err(e) = handle.join() {
                error!("Error joining LMSListener thread: {:?}", e);
            }
        }
        
        debug!("LMSListener stopped");
    }
}

impl Drop for LMSListener {
    fn drop(&mut self) {
        self.stop();
    }
}