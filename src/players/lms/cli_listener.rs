use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
use std::thread;
use std::time::Duration;
use log::{warn, debug, error};

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
}

impl LMSListener {
    /// Create a new LMS CLI listener
    /// 
    /// # Arguments
    /// * `server` - Server address (hostname or IP)
    /// * `player_id` - Player ID (MAC address)
    pub fn new(server: &str, player_id: &str) -> Self {
        Self {
            server_address: server.to_string(),
            player_id: player_id.to_string(),
            running: Arc::new(AtomicBool::new(false)),
            thread_handle: None,
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
        
        self.thread_handle = Some(thread::spawn(move || {
            // Main connection loop - try to reconnect if connection fails
            while running.load(Ordering::SeqCst) {
                match Self::connect_and_listen(&server, &player_id, running.clone()) {
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
    
    /// Connect to the server and listen for messages
    fn connect_and_listen(server: &str, player_id: &str, running: Arc<AtomicBool>) -> Result<(), String> {
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
                    // Log the received line
                    warn!("LMS event: {}", line);
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