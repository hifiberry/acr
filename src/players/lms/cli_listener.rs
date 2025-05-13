use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::sync::{Arc, atomic::{AtomicBool, Ordering}, Weak};
use std::thread;
use std::time::Duration;
use log::{warn, debug, error, trace};
use urlencoding::decode;

// Forward declaration to avoid circular dependency
type WeakAudioController = Weak<dyn AudioControllerRef>;

/// Interface for interacting with the Audio Controller
pub trait AudioControllerRef: Send + Sync {
    /// Notify that an event was seen from this player
    fn seen(&self);
}

/// List of commands that should only be logged at debug level
const IGNORED_COMMANDS: &[&str] = &[
    "open",
];

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
        
        self.thread_handle = Some(thread::spawn(move || {
            // Main connection loop - try to reconnect if connection fails
            while running.load(Ordering::SeqCst) {
                match Self::connect_and_listen(&server, &player_id, running.clone(), controller.clone()) {
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
    fn connect_and_listen(server: &str, player_id: &str, running: Arc<AtomicBool>, controller: WeakAudioController) -> Result<(), String> {
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
        
        // Enable the TCP keep-alive option to detect connection drops
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        if let Err(e) = stream.set_keepalive(Some(Duration::from_secs(30))) {
            warn!("Failed to set TCP keepalive: {}", e);
            // Non-fatal, continue anyway
        }
        
        #[cfg(target_os = "windows")]
        {
            // On Windows, the keep-alive API is different
            warn!("TCP keepalive not implemented for Windows");
            // Non-fatal, continue anyway
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
        warn!("Connected to LMS CLI, receiving events...");
        
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
                        }
                    }
                    
                    // Log the event with structured information
                    if let Some(mac_addr) = mac_opt {
                        if cmd_parts.is_empty() {
                            warn!("LMS event: MAC={}", mac_addr);
                        } else {
                            let cmd = &cmd_parts[0];
                            if IGNORED_COMMANDS.contains(&cmd.as_str()) {
                                debug!("Ignored LMS event: Player {} {}", mac_addr, cmd);
                                continue;
                            }
                            let args = if cmd_parts.len() > 1 {
                                cmd_parts[1..].join(" ")
                            } else {
                                String::new()
                            };
                            
                            match cmd.as_str() {
                                "playlist" => {
                                    if cmd_parts.len() > 1 {
                                        match cmd_parts[1].as_str() {
                                            "newsong" => {
                                                let song_title = if cmd_parts.len() > 2 { &cmd_parts[2] } else { "Unknown" };
                                                warn!("LMS event: Player {} started new song: {}", mac_addr, song_title);
                                            },
                                            "pause" => {
                                                let state = if cmd_parts.len() > 2 && cmd_parts[2] == "1" { "paused" } else { "resumed" };
                                                warn!("LMS event: Player {} {}", mac_addr, state);
                                            },
                                            "stop" => {
                                                warn!("LMS event: Player {} stopped", mac_addr);
                                            },
                                            _ => {
                                                warn!("LMS event: Player {} {} {}", mac_addr, cmd, args);
                                            }
                                        }
                                    } else {
                                        warn!("LMS event: Player {} {}", mac_addr, cmd);
                                    }
                                },
                                "pause" => {
                                    let state = if cmd_parts.len() > 1 && cmd_parts[1] == "1" { "paused" } else { "resumed" };
                                    warn!("LMS event: Player {} {}", mac_addr, state);
                                },
                                "client" => {
                                    if cmd_parts.len() > 1 {
                                        warn!("LMS event: Player {} client {}", mac_addr, cmd_parts[1]);
                                    } else {
                                        warn!("LMS event: Player {} client event", mac_addr);
                                    }
                                },
                                "prefset" => {
                                    if cmd_parts.len() > 2 {
                                        warn!("LMS event: Player {} setting {} = {}", mac_addr, cmd_parts[1], 
                                              if cmd_parts.len() > 2 { &cmd_parts[2] } else { "" });
                                    } else {
                                        warn!("LMS event: Player {} {}", mac_addr, line);
                                    }
                                },
                                "displaynotify" | "menustatus" => {
                                    warn!("LMS event: Player {} UI update", mac_addr);
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
                            let cmd = &cmd_parts[0];
                            if IGNORED_COMMANDS.contains(&cmd.as_str()) {
                                debug!("Ignored LMS server event: {}", cmd);
                                continue;
                            }
                            let args = if cmd_parts.len() > 1 {
                                cmd_parts[1..].join(" ")
                            } else {
                                String::new()
                            };
                            
                            match cmd.as_str() {
                                "prefset" => {
                                    if cmd_parts.len() > 2 {
                                        warn!("LMS server event: Setting {} = {}", cmd_parts[1], 
                                              if cmd_parts.len() > 2 { &cmd_parts[2] } else { "" });
                                    } else {
                                        warn!("LMS server event: {}", line);
                                    }
                                },
                                "artworkspec" => {
                                    warn!("LMS server event: Artwork spec update");
                                },
                                _ => {
                                    // Default formatting for other events
                                    warn!("LMS server event: {} {}", cmd, args);
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