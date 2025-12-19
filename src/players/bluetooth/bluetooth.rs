use crate::players::player_controller::{BasePlayerController, PlayerController};
use crate::data::{PlayerCapability, PlayerCapabilitySet, Song, LoopMode, PlaybackState, PlayerCommand, PlayerState, Track};
use delegate::delegate;
use std::sync::{Arc, RwLock, Mutex};
use log::{debug, info, warn, error};
use std::any::Any;
use std::collections::HashMap;
use dbus::blocking::Connection;
use dbus::blocking::stdintf::org_freedesktop_dbus::{Properties, ObjectManager};
use dbus::arg::RefArg;
use std::time::{Duration, SystemTime};
use std::thread;
use std::sync::atomic::Ordering;

/// Bluetooth player controller implementation
/// This controller interfaces with Bluetooth audio devices via D-Bus using BlueZ MediaPlayer1 interface
pub struct BluetoothPlayerController {
    /// Base controller
    base: BasePlayerController,
    
    /// D-Bus connection (using Mutex instead of RwLock for thread safety)
    connection: Arc<Mutex<Option<Connection>>>,
    
    /// Current song information
    current_song: Arc<RwLock<Option<Song>>>,

    /// Current player state
    current_state: Arc<RwLock<PlayerState>>,
    
    /// Bluetooth device address (MAC address) - None means auto-discover
    device_address: Arc<RwLock<Option<String>>>,
    
    /// D-Bus object path for the MediaPlayer1 interface
    player_path: Arc<RwLock<Option<String>>>,
    
    /// Device name (friendly name)
    device_name: Arc<RwLock<Option<String>>>,
    
    /// Background thread handle for device scanning
    scan_thread: Arc<RwLock<Option<std::thread::JoinHandle<()>>>>,
    
    /// Flag to stop scanning thread
    stop_scanning: Arc<std::sync::atomic::AtomicBool>,
    
    /// Background thread handle for status polling
    poll_thread: Arc<RwLock<Option<std::thread::JoinHandle<()>>>>,
    
    /// Flag to stop polling thread
    stop_polling: Arc<std::sync::atomic::AtomicBool>,
    
    /// Whether this controller was created in auto-discover mode
    is_auto_discover: bool,
}

// Manually implement Clone for BluetoothPlayerController
impl Clone for BluetoothPlayerController {
    fn clone(&self) -> Self {
        BluetoothPlayerController {
            base: self.base.clone(),
            connection: Arc::clone(&self.connection),
            current_song: Arc::clone(&self.current_song),
            current_state: Arc::clone(&self.current_state),
            device_address: Arc::clone(&self.device_address),
            player_path: Arc::clone(&self.player_path),
            device_name: Arc::clone(&self.device_name),
            scan_thread: Arc::new(RwLock::new(None)),
            stop_scanning: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            poll_thread: Arc::new(RwLock::new(None)),
            stop_polling: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            is_auto_discover: self.is_auto_discover,
        }
    }
}

impl Drop for BluetoothPlayerController {
    fn drop(&mut self) {
        // Signal both threads to stop
        self.stop_scanning.store(true, Ordering::Relaxed);
        self.stop_polling.store(true, Ordering::Relaxed);
        
        // Wait for the scanning thread to finish
        if let Ok(mut guard) = self.scan_thread.write() {
            if let Some(handle) = guard.take() {
                let _ = handle.join();
            }
        }
        
        // Wait for the polling thread to finish
        if let Ok(mut guard) = self.poll_thread.write() {
            if let Some(handle) = guard.take() {
                let _ = handle.join();
            }
        }
        
        debug!("BluetoothPlayerController dropped");
    }
}

impl BluetoothPlayerController {
    /// Create a new BluetoothPlayerController with auto-discovery
    pub fn new() -> Self {
        Self::new_with_address(None)
    }
    
    /// Create a new BluetoothPlayerController with a specific device address
    pub fn new_with_address(device_address: Option<String>) -> Self {
        let player_id = match &device_address {
            Some(addr) => addr.clone(),
            None => "auto-discover".to_string(),
        };
        
        let base = BasePlayerController::with_player_info("bluetooth", &player_id);
        
        // Set initial capabilities
        let capabilities = PlayerCapabilitySet::from_slice(&[
            PlayerCapability::Play,
            PlayerCapability::Pause,
            PlayerCapability::Stop,
            PlayerCapability::Next,
            PlayerCapability::Previous,
        ]);
        base.set_capabilities_set(capabilities, false);
        
        let is_auto_discover = device_address.is_none();
        
        let controller = BluetoothPlayerController {
            base,
            connection: Arc::new(Mutex::new(None)),
            current_song: Arc::new(RwLock::new(None)),
            current_state: Arc::new(RwLock::new(PlayerState::new())),
            device_address: Arc::new(RwLock::new(device_address.clone())),
            player_path: Arc::new(RwLock::new(None)),
            device_name: Arc::new(RwLock::new(None)),
            scan_thread: Arc::new(RwLock::new(None)),
            stop_scanning: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            poll_thread: Arc::new(RwLock::new(None)),
            stop_polling: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            is_auto_discover,
        };
        
        info!("Created BluetoothPlayerController with address: {:?}", device_address);
        
        // If no specific device address is given, start auto-discovery
        if device_address.is_none() {
            info!("Starting auto-discovery for Bluetooth devices");
            controller.start_scanning_thread();
        } else {
            // Try to find the specific device immediately
            controller.find_player_path();
        }
        
        controller
    }
    
    /// Initialize D-Bus connection
    fn ensure_dbus_connection(&self) -> bool {
        let mut conn_guard = match self.connection.lock() {
            Ok(guard) => guard,
            Err(_) => {
                error!("Failed to acquire lock for D-Bus connection");
                return false;
            }
        };
        
        if conn_guard.is_none() {
            match Connection::new_system() {
                Ok(conn) => {
                    debug!("Established D-Bus system connection");
                    *conn_guard = Some(conn);
                    true
                }
                Err(e) => {
                    error!("Failed to connect to D-Bus system bus: {}", e);
                    false
                }
            }
        } else {
            true
        }
    }
    
    /// Find all available Bluetooth devices with MediaPlayer1 interface
    fn discover_bluetooth_devices(&self) -> Vec<(String, String)> {
        let mut devices = Vec::new();
        
        if !self.ensure_dbus_connection() {
            return devices;
        }
        
        let conn_guard = match self.connection.lock() {
            Ok(guard) => guard,
            Err(_) => return devices,
        };
        
        let conn = match conn_guard.as_ref() {
            Some(c) => c,
            None => return devices,
        };
        
        // Get the BlueZ object manager to enumerate all objects
        let proxy = conn.with_proxy("org.bluez", "/", Duration::from_millis(5000));
        
        // Try to get all managed objects
        if let Ok(objects) = proxy.get_managed_objects() {
            for (path, interfaces) in objects {
                // Look for MediaPlayer1 interfaces
                if interfaces.contains_key("org.bluez.MediaPlayer1") {
                    // Extract device address from path
                    // Path format: /org/bluez/hci0/dev_XX_XX_XX_XX_XX_XX/player0
                    if let Some(device_part) = path.strip_prefix("/org/bluez/hci0/dev_") {
                        if let Some(addr_part) = device_part.split('/').next() {
                            // Convert XX_XX_XX_XX_XX_XX back to XX:XX:XX:XX:XX:XX
                            let device_address = addr_part.replace('_', ":");
                            
                            // Get device name
                            let device_path = format!("/org/bluez/hci0/dev_{}", addr_part);
                            let device_proxy = conn.with_proxy("org.bluez", &device_path, Duration::from_millis(1000));
                            
                            let device_name = device_proxy.get::<String>("org.bluez.Device1", "Name")
                                .unwrap_or_else(|_| device_address.clone());
                            
                            debug!("Found Bluetooth device with MediaPlayer1: {} ({})", device_name, device_address);
                            devices.push((device_address, device_name));
                        }
                    }
                }
            }
        }
        
        devices
    }
    /// Find the active player path for a given device address
    /// This scans for MediaPlayer1 interfaces (player0, player1, player2, etc.)
    fn find_active_player(&self, device_address: &str) -> Option<String> {
        if !self.ensure_dbus_connection() {
            return None;
        }
        
        // Convert MAC address format from 80:B9:89:1E:B5:6F to 80_B9_89_1E_B5_6F
        let device_path_part = device_address.replace(":", "_");
        
        // Use ObjectManager to find the actual player path (player index may vary: player0, player1, player2, etc.)
        let conn_guard = match self.connection.lock() {
            Ok(guard) => guard,
            Err(_) => return None,
        };
        
        if let Some(conn) = conn_guard.as_ref() {
            let proxy = conn.with_proxy("org.bluez", "/", Duration::from_millis(5000));
            
            // Get all managed objects and find the MediaPlayer1 for our device
            if let Ok(objects) = proxy.get_managed_objects() {
                let device_prefix = format!("/org/bluez/hci0/dev_{}/player", device_path_part);
                
                for (path, interfaces) in objects {
                    // Look for MediaPlayer1 interface under our device path
                    if path.starts_with(&device_prefix) && interfaces.contains_key("org.bluez.MediaPlayer1") {
                        debug!("Found MediaPlayer1 at path: {}", path);
                        return Some(path.to_string());
                    }
                }
                
                debug!("MediaPlayer1 not found for device {}", device_address);
                None
            } else {
                debug!("Failed to get managed objects from BlueZ");
                None
            }
        } else {
            None
        }
    }
    
    /// Static helper for checking and updating active player in the polling thread
    fn check_and_update_active_player(
        player_path: &Arc<RwLock<Option<String>>>,
        connection: &Arc<Mutex<Option<Connection>>>,
        device_address: &Arc<RwLock<Option<String>>>,
    ) {
        let current_path = match player_path.read() {
            Ok(guard) => guard.clone(),
            Err(_) => return,
        };
        
        let device_addr = match device_address.read() {
            Ok(guard) => guard.clone(),
            Err(_) => return,
        };
        
        // If we have a stored path, check if it's still valid
        if let Some(path) = current_path {
            let conn_guard = match connection.lock() {
                Ok(guard) => guard,
                Err(_) => return,
            };
            
            if let Some(conn) = conn_guard.as_ref() {
                let proxy = conn.with_proxy("org.bluez", "/", Duration::from_millis(5000));
                
                // Check if the current path still exists
                if let Ok(objects) = proxy.get_managed_objects() {
                    if objects.contains_key(&dbus::Path::from(path.clone())) {
                        // Current player path is still valid
                        return;
                    } else {
                        debug!("Current player path {} no longer exists, searching for new player", path);
                    }
                } else {
                    debug!("Failed to get managed objects from BlueZ");
                    return;
                }
            } else {
                return;
            }
        }
        
        // Current path is invalid or doesn't exist, try to find a new player
        if let Some(addr) = device_addr {
            // Find active player using static helper
            if let Some(new_path) = Self::find_active_player_static(connection, &addr) {
                info!("Found new active player at path: {}", new_path);
                if let Ok(mut guard) = player_path.write() {
                    *guard = Some(new_path);
                }
            }
        }
    }
    
    /// Static helper to find active player (for use in polling thread)
    fn find_active_player_static(
        connection: &Arc<Mutex<Option<Connection>>>,
        device_address: &str,
    ) -> Option<String> {
        // Convert MAC address format from 80:B9:89:1E:B5:6F to 80_B9_89_1E_B5_6F
        let device_path_part = device_address.replace(":", "_");
        
        let conn_guard = match connection.lock() {
            Ok(guard) => guard,
            Err(_) => return None,
        };
        
        if let Some(conn) = conn_guard.as_ref() {
            let proxy = conn.with_proxy("org.bluez", "/", Duration::from_millis(5000));
            
            // Get all managed objects and find the MediaPlayer1 for our device
            if let Ok(objects) = proxy.get_managed_objects() {
                let device_prefix = format!("/org/bluez/hci0/dev_{}/player", device_path_part);
                
                for (path, interfaces) in objects {
                    // Look for MediaPlayer1 interface under our device path
                    if path.starts_with(&device_prefix) && interfaces.contains_key("org.bluez.MediaPlayer1") {
                        debug!("Found MediaPlayer1 at path: {}", path);
                        return Some(path.to_string());
                    }
                }
                
                debug!("MediaPlayer1 not found for device {}", device_address);
                None
            } else {
                debug!("Failed to get managed objects from BlueZ");
                None
            }
        } else {
            None
        }
    }
    
    /// Check if the currently stored active player is still available
    /// If not, attempt to find a new player (e.g., player0 -> player1 transition)
    fn check_active_player(&self) -> bool {
        if !self.ensure_dbus_connection() {
            return false;
        }
        
        let current_path = match self.player_path.read() {
            Ok(guard) => guard.clone(),
            Err(_) => return false,
        };
        
        let device_address = match self.device_address.read() {
            Ok(guard) => guard.clone(),
            Err(_) => return false,
        };
        
        // If we have a stored path, check if it's still valid
        if let Some(path) = current_path {
            let conn_guard = match self.connection.lock() {
                Ok(guard) => guard,
                Err(_) => return false,
            };
            
            if let Some(conn) = conn_guard.as_ref() {
                let proxy = conn.with_proxy("org.bluez", "/", Duration::from_millis(5000));
                
                // Check if the current path still exists
                if let Ok(objects) = proxy.get_managed_objects() {
                    if objects.contains_key(&dbus::Path::from(path.clone())) {
                        // Current player path is still valid
                        return true;
                    } else {
                        debug!("Current player path {} no longer exists, searching for new player", path);
                    }
                } else {
                    debug!("Failed to get managed objects from BlueZ");
                    return false;
                }
            } else {
                return false;
            }
        }
        
        // Current path is invalid or doesn't exist, try to find a new player
        if let Some(addr) = device_address {
            if let Some(new_path) = self.find_active_player(&addr) {
                info!("Found new active player at path: {}", new_path);
                if let Ok(mut guard) = self.player_path.write() {
                    *guard = Some(new_path);
                }
                return true;
            }
        }
        
        false
    }
    
    /// Find the MediaPlayer1 object path for the device
    fn find_player_path(&self) -> Option<String> {
        if !self.ensure_dbus_connection() {
            return None;
        }
        
        // Get current device address
        let device_address = match self.device_address.read() {
            Ok(guard) => guard.clone(),
            Err(_) => return None,
        };
        
        // If no specific device address, try to discover one
        let device_address = match device_address {
            Some(addr) => addr,
            None => {
                // Auto-discover first available device
                let discovered = self.discover_bluetooth_devices();
                if let Some((addr, name)) = discovered.first() {
                    info!("Auto-discovered Bluetooth device: {} ({})", name, addr);
                    
                    // Update our stored address and name
                    if let Ok(mut guard) = self.device_address.write() {
                        *guard = Some(addr.clone());
                    }
                    if let Ok(mut guard) = self.device_name.write() {
                        *guard = Some(name.clone());
                    }
                    
                    addr.clone()
                } else {
                    debug!("No Bluetooth devices with MediaPlayer1 found");
                    return None;
                }
            }
        };
        
        // Use the new find_active_player function
        self.find_active_player(&device_address)
    }
    
    /// Get device friendly name  
    fn get_device_name(&self) -> Option<String> {
        if !self.ensure_dbus_connection() {
            return None;
        }
        
        let conn_guard = match self.connection.lock() {
            Ok(guard) => guard,
            Err(_) => return None,
        };
        
        let conn = conn_guard.as_ref()?;
        
        let device_address = match self.device_address.read() {
            Ok(guard) => guard.clone(),
            Err(_) => return None,
        };
        
        let device_address = device_address?;
        let device_path_part = device_address.replace(":", "_");
        let device_path = format!("/org/bluez/hci0/dev_{}", device_path_part);
        
        let proxy = conn.with_proxy("org.bluez", &device_path, Duration::from_millis(1000));
        
        // Try to get the Name property using D-Bus property interface
        use dbus::blocking::stdintf::org_freedesktop_dbus::Properties;
        
        match proxy.get::<String>("org.bluez.Device1", "Name") {
            Ok(name) => {
                debug!("Device name: {}", name);
                Some(name)
            }
            Err(e) => {
                debug!("Failed to get device name: {}", e);
                None
            }
        }
    }
    
    /// Update current song from D-Bus
    fn update_song_from_dbus(&self) {
        // Check if the active player is still valid before querying
        self.check_active_player();
        
        let player_path = match self.player_path.read() {
            Ok(guard) => guard.clone(),
            Err(_) => return,
        };
        
        let player_path = match player_path {
            Some(path) => path,
            None => {
                // Try to find the player path
                if let Some(path) = self.find_player_path() {
                    if let Ok(mut guard) = self.player_path.write() {
                        *guard = Some(path.clone());
                    }
                    path
                } else {
                    return;
                }
            }
        };
        
        if !self.ensure_dbus_connection() {
            return;
        }
        
        let conn_guard = match self.connection.lock() {
            Ok(guard) => guard,
            Err(_) => return,
        };
        
        let conn = match conn_guard.as_ref() {
            Some(c) => c,
            None => return,
        };
        
        let proxy = conn.with_proxy("org.bluez", &player_path, Duration::from_millis(1000));
        
        // Use D-Bus Properties interface to get Track information
        if let Ok(track_info) = proxy.get::<HashMap<String, dbus::arg::Variant<Box<dyn dbus::arg::RefArg>>>>("org.bluez.MediaPlayer1", "Track") {
            let mut metadata = HashMap::new();
            let mut title = None;
            let mut artist = None;
            let mut album = None;
            let mut duration = None;
            
            for (key, variant) in track_info {
                match key.as_str() {
                    "Title" => {
                        if let Some(val) = variant.as_str() {
                            title = Some(val.to_string());
                            metadata.insert("title".to_string(), serde_json::Value::String(val.to_string()));
                        }
                    }
                    "Artist" => {
                        if let Some(val) = variant.as_str() {
                            artist = Some(val.to_string());
                            metadata.insert("artist".to_string(), serde_json::Value::String(val.to_string()));
                        }
                    }
                    "Album" => {
                        if let Some(val) = variant.as_str() {
                            album = Some(val.to_string());
                            metadata.insert("album".to_string(), serde_json::Value::String(val.to_string()));
                        }
                    }
                    "Duration" => {
                        if let Some(val) = variant.as_u64() {
                            // Duration is in microseconds, convert to seconds
                            let duration_secs = val as f64 / 1_000_000.0;
                            duration = Some(duration_secs);
                            metadata.insert("duration".to_string(), serde_json::Value::Number(
                                serde_json::Number::from_f64(duration_secs).unwrap_or(serde_json::Number::from(0))
                            ));
                        }
                    }
                    _ => {
                        // Store other metadata as strings
                        if let Some(val) = variant.as_str() {
                            metadata.insert(key.to_lowercase(), serde_json::Value::String(val.to_string()));
                        }
                    }
                }
            }
            
            // Create song if we have at least a title
            if let Some(title) = title {
                let song = Song {
                    title: Some(title),
                    artist,
                    album,
                    duration,
                    metadata,
                    ..Default::default()
                };
                
                if let Ok(mut guard) = self.current_song.write() {
                    *guard = Some(song);
                    debug!("Updated Bluetooth song information");
                }
            }
        }
    }
    
    /// Send a D-Bus method call to the MediaPlayer1 interface
    fn send_dbus_command(&self, method: &str) -> bool {
        let player_path = match self.player_path.read() {
            Ok(guard) => guard.clone(),
            Err(_) => return false,
        };
        
        let player_path = match player_path {
            Some(path) => path,
            None => {
                if let Some(path) = self.find_player_path() {
                    if let Ok(mut guard) = self.player_path.write() {
                        *guard = Some(path.clone());
                    }
                    path
                } else {
                    let addr = self.device_address.read().map(|guard| guard.clone()).unwrap_or(None);
                    warn!("No MediaPlayer1 found for device {:?}", addr);
                    return false;
                }
            }
        };
        
        if !self.ensure_dbus_connection() {
            return false;
        }
        
        let conn_guard = match self.connection.lock() {
            Ok(guard) => guard,
            Err(_) => return false,
        };
        
        let conn = match conn_guard.as_ref() {
            Some(c) => c,
            None => return false,
        };
        
        let proxy = conn.with_proxy("org.bluez", &player_path, Duration::from_millis(5000));
        
        match proxy.method_call("org.bluez.MediaPlayer1", method, ()) {
            Ok(()) => {
                debug!("Successfully sent {} command to Bluetooth device", method);
                true
            }
            Err(e) => {
                warn!("Failed to send {} command to Bluetooth device: {}", method, e);
                false
            }
        }
    }
    
    /// Start background scanning for devices
    fn start_scanning_thread(&self) {
        // Don't start if we already have a device
        if let Ok(guard) = self.device_address.read() {
            if guard.is_some() {
                return;
            }
        }
        
        let device_address = Arc::clone(&self.device_address);
        let device_name = Arc::clone(&self.device_name);
        let player_path = Arc::clone(&self.player_path);
        let stop_flag = Arc::clone(&self.stop_scanning);
        let connection = Arc::clone(&self.connection);
        
        let handle = thread::spawn(move || {
            info!("Starting Bluetooth device scanning thread");
            
            while !stop_flag.load(std::sync::atomic::Ordering::Relaxed) {
                // Check if we still need to scan
                if let Ok(guard) = device_address.read() {
                    if guard.is_some() {
                        // We found a device, stop scanning
                        break;
                    }
                }
                
                // Try to discover devices
                if let Ok(conn_guard) = connection.lock() {
                    if let Some(conn) = conn_guard.as_ref() {
                        // Simplified discovery logic for background thread
                        let proxy = conn.with_proxy("org.bluez", "/", Duration::from_millis(2000));
                        
                        if let Ok(objects) = proxy.get_managed_objects() {
                            for (path, interfaces) in objects {
                                if interfaces.contains_key("org.bluez.MediaPlayer1") {
                                    if let Some(device_part) = path.strip_prefix("/org/bluez/hci0/dev_") {
                                        if let Some(addr_part) = device_part.split('/').next() {
                                            let discovered_address = addr_part.replace('_', ":");
                                            
                                            // Get device name
                                            let device_path = format!("/org/bluez/hci0/dev_{}", addr_part);
                                            let device_proxy = conn.with_proxy("org.bluez", &device_path, Duration::from_millis(1000));
                                            
                                            let discovered_name = device_proxy.get::<String>("org.bluez.Device1", "Name")
                                                .unwrap_or_else(|_| discovered_address.clone());
                                            
                                            info!("Background scan found Bluetooth device: {} ({})", discovered_name, discovered_address);
                                            
                                            // Update stored values
                                            if let Ok(mut guard) = device_address.write() {
                                                *guard = Some(discovered_address);
                                            }
                                            if let Ok(mut guard) = device_name.write() {
                                                *guard = Some(discovered_name);
                                            }
                            if let Ok(mut guard) = player_path.write() {
                                *guard = Some(path.to_string());
                            }                                            // Found a device, stop scanning
                                            return;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                
                // Wait 5 seconds before next scan
                thread::sleep(Duration::from_secs(5));
            }
            
            debug!("Bluetooth scanning thread stopped");
        });
        
        if let Ok(mut guard) = self.scan_thread.write() {
            *guard = Some(handle);
        }
    }
    
    /// Manually trigger a rescan for devices
    pub fn rescan(&self) {
        debug!("Manually triggering Bluetooth device rescan");
        
        // Clear current device info to force rediscovery
        if let Ok(mut guard) = self.device_address.write() {
            *guard = None;
        }
        if let Ok(mut guard) = self.player_path.write() {
            *guard = None;
        }
        if let Ok(mut guard) = self.device_name.write() {
            *guard = None;
        }
        
        // Try to find a device immediately
        self.find_player_path();
    }
    


    /// Poll and update playback state
    fn poll_playback_state(
        proxy: &dbus::blocking::Proxy<&dbus::blocking::Connection>,
        current_state: &Arc<RwLock<PlayerState>>,
        current_song: &Arc<RwLock<Option<Song>>>,
        base: &BasePlayerController,
    ) {
        if let Ok(status) = proxy.get::<String>("org.bluez.MediaPlayer1", "Status") {
            let new_state = match status.as_str() {
                "playing" => PlaybackState::Playing,
                "paused" => PlaybackState::Paused,
                "stopped" => PlaybackState::Stopped,
                _ => PlaybackState::Unknown,
            };
            
            // Update state if changed
            if let Ok(mut state_guard) = current_state.write() {
                let old_state = state_guard.state;
                if old_state != new_state {
                    debug!("Bluetooth playback state changed: {:?} -> {:?}", old_state, new_state);
                    state_guard.state = new_state;
                    base.notify_state_changed(new_state);
                    
                    // When becoming active (starts playing), just notify about current song
                    if new_state == PlaybackState::Playing && old_state != PlaybackState::Playing {
                        debug!("Bluetooth player became active");
                        
                        // Notify about current song when becoming active
                        if let Ok(song_guard) = current_song.read() {
                            if let Some(ref song) = *song_guard {
                                base.notify_song_changed(Some(song));
                            }
                        }
                    }
                }
            }
            
            // Mark as alive
            base.alive();
        }
    }

    /// Poll and update track information
    fn poll_track_information(
        proxy: &dbus::blocking::Proxy<&dbus::blocking::Connection>,
        current_song: &Arc<RwLock<Option<Song>>>,
        base: &BasePlayerController,
    ) {
        if let Ok(track_data) = proxy.get::<HashMap<String, dbus::arg::Variant<Box<dyn RefArg>>>>("org.bluez.MediaPlayer1", "Track") {
            let title = track_data.get("Title")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            
            let artist = track_data.get("Artist")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            
            let album = track_data.get("Album")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            
            let duration = track_data.get("Duration")
                .and_then(|v| v.as_u64())
                .map(|d| d as f64 / 1000.0); // Convert ms to seconds
            
            // Create new song if we have track data
            if title.is_some() || artist.is_some() || album.is_some() {
                let new_song = Song {
                    title: title.clone(),
                    artist: artist.clone(),
                    album: album.clone(),
                    duration: duration,
                    ..Song::default()
                };
                
                // Update song if changed
                if let Ok(mut song_guard) = current_song.write() {
                    let song_changed = song_guard.as_ref().map(|s| {
                        s.title != new_song.title || 
                        s.artist != new_song.artist || 
                        s.album != new_song.album
                    }).unwrap_or(true);
                    
                    if song_changed {
                        info!("Bluetooth track changed: {:?} - {:?} ({:?})", 
                               new_song.artist, new_song.title, new_song.album);
                        *song_guard = Some(new_song.clone());
                        base.notify_song_changed(Some(&new_song));
                        
                        // Also mark as alive when song changes
                        base.alive();
                    }
                }
            }
        }
    }

    /// Poll and update position information
    fn poll_position_information(
        proxy: &dbus::blocking::Proxy<&dbus::blocking::Connection>,
        current_state: &Arc<RwLock<PlayerState>>,
        base: &BasePlayerController,
    ) {
        if let Ok(position) = proxy.get::<u32>("org.bluez.MediaPlayer1", "Position") {
            let position_seconds = position as f64 / 1000.0;
            
            // Update position in player state
            if let Ok(mut state_guard) = current_state.write() {
                if (state_guard.position.unwrap_or(0.0) - position_seconds).abs() > 1.0 {
                    state_guard.position = Some(position_seconds);
                    base.notify_position_changed(position_seconds);
                }
            }
        }
    }

    /// Main polling loop logic
    fn run_polling_loop(
        player_path: Arc<RwLock<Option<String>>>,
        connection: Arc<Mutex<Option<Connection>>>,
        current_song: Arc<RwLock<Option<Song>>>,
        current_state: Arc<RwLock<PlayerState>>,
        stop_flag: Arc<std::sync::atomic::AtomicBool>,
        base: BasePlayerController,
        device_address: Arc<RwLock<Option<String>>>,
    ) {
        info!("Starting Bluetooth status polling thread");
        
        let mut last_no_path_warning = SystemTime::UNIX_EPOCH;
        
        while !stop_flag.load(Ordering::Relaxed) {
            // Check if the active player is still available before polling
            // This handles transitions like player0 -> player1 -> player2
            Self::check_and_update_active_player(&player_path, &connection, &device_address);
            
            // Get current player path
            let path = if let Ok(guard) = player_path.read() {
                guard.clone()
            } else {
                thread::sleep(Duration::from_secs(2));
                continue;
            };
            
            if let Some(ref path_str) = path {
                if let Ok(conn_guard) = connection.lock() {
                    if let Some(ref conn) = *conn_guard {
                        let proxy = conn.with_proxy("org.bluez", path_str, Duration::from_millis(1000));
                        
                        // Poll different aspects of the player state
                        debug!("Polling Bluetooth player state at {}", path_str);
                        Self::poll_playback_state(&proxy, &current_state, &current_song, &base);
                        Self::poll_track_information(&proxy, &current_song, &base);
                        Self::poll_position_information(&proxy, &current_state, &base);
                    }
                }
            } else {
                // Only log this message every 10 seconds to avoid spam
                if let Ok(elapsed) = SystemTime::now().duration_since(last_no_path_warning) {
                    if elapsed >= Duration::from_secs(10) {
                        debug!("No player path available for polling");
                        last_no_path_warning = SystemTime::now();
                    }
                }
            }
            
            // Poll every 2 seconds
            thread::sleep(Duration::from_secs(2));
        }
        
        debug!("Bluetooth polling thread stopped");
    }

    /// Start the status polling thread
    fn start_polling_thread(&self) {
        debug!("Starting Bluetooth status polling thread");
        
        let player_path = Arc::clone(&self.player_path);
        let connection = Arc::clone(&self.connection);
        let current_song = Arc::clone(&self.current_song);
        let current_state = Arc::clone(&self.current_state);
        let stop_flag = Arc::clone(&self.stop_polling);
        let base = self.base.clone();
        let device_address = Arc::clone(&self.device_address);
        
        let handle = thread::spawn(move || {
            Self::run_polling_loop(player_path, connection, current_song, current_state, stop_flag, base, device_address);
        });
        
        if let Ok(mut guard) = self.poll_thread.write() {
            *guard = Some(handle);
        }
    }
    fn get_playback_status(&self) -> PlaybackState {
        let player_path = match self.player_path.read() {
            Ok(guard) => guard.clone(),
            Err(_) => return PlaybackState::Unknown,
        };
        
        let player_path = match player_path {
            Some(path) => path,
            None => return PlaybackState::Unknown,
        };
        
        if !self.ensure_dbus_connection() {
            return PlaybackState::Unknown;
        }
        
        let conn_guard = match self.connection.lock() {
            Ok(guard) => guard,
            Err(_) => return PlaybackState::Unknown,
        };
        
        let conn = match conn_guard.as_ref() {
            Some(c) => c,
            None => return PlaybackState::Unknown,
        };
        
        let proxy = conn.with_proxy("org.bluez", &player_path, Duration::from_millis(1000));
        
        match proxy.get::<String>("org.bluez.MediaPlayer1", "Status") {
            Ok(status) => {
                match status.as_str() {
                    "playing" => PlaybackState::Playing,
                    "paused" => PlaybackState::Paused,
                    "stopped" => PlaybackState::Stopped,
                    _ => PlaybackState::Unknown,
                }
            }
            Err(_) => PlaybackState::Unknown,
        }
    }
}

impl PlayerController for BluetoothPlayerController {
    delegate! {
        to self.base {
            fn get_capabilities(&self) -> PlayerCapabilitySet;
            fn get_last_seen(&self) -> Option<SystemTime>;
        }
    }
    
    fn get_song(&self) -> Option<Song> {
        // Update song information from D-Bus before returning
        self.update_song_from_dbus();
        
        match self.current_song.read() {
            Ok(guard) => guard.clone(),
            Err(_) => None,
        }
    }
    
    fn get_queue(&self) -> Vec<Track> {
        // Bluetooth devices typically don't expose queue information via D-Bus
        Vec::new()
    }
    
    fn get_loop_mode(&self) -> LoopMode {
        // Most Bluetooth devices don't expose loop mode via D-Bus
        LoopMode::None
    }
    
    fn get_playback_state(&self) -> PlaybackState {
        let state = self.get_playback_status();
        
        // Update our internal state
        if let Ok(mut guard) = self.current_state.write() {
            guard.state = state;
        }
        
        // Mark as alive
        self.base.alive();
        
        state
    }
    
    fn get_position(&self) -> Option<f64> {
        // Most Bluetooth devices don't expose precise position via D-Bus
        None
    }
    
    fn get_shuffle(&self) -> bool {
        // Most Bluetooth devices don't expose shuffle state via D-Bus
        false
    }
    
    fn get_player_name(&self) -> String {
        "bluetooth".to_string()
    }
    
    fn get_aliases(&self) -> Vec<String> {
        vec!["bluetooth".to_string(), "bluez".to_string(), "bt".to_string()]
    }
    
    fn get_player_id(&self) -> String {
        // If controller was created in auto-discover mode, always return "bluetooth:auto-discover"
        // If controller was created with specific device, return "bluetooth:<device_address>"
        
        if self.is_auto_discover {
            "bluetooth:auto-discover".to_string()
        } else {
            // For specific device controllers, return the configured address with bluetooth prefix
            let device_address = if let Ok(guard) = self.device_address.read() {
                guard.clone().unwrap_or_else(|| "unknown".to_string())
            } else {
                "unknown".to_string()
            };
            format!("bluetooth:{}", device_address)
        }
    }
    
    fn send_command(&self, command: PlayerCommand) -> bool {
        info!("Sending command to Bluetooth device: {}", command);
        
        // Update player path if needed
        if self.player_path.read().unwrap().is_none() {
            if let Some(path) = self.find_player_path() {
                if let Ok(mut guard) = self.player_path.write() {
                    *guard = Some(path);
                }
            }
        }
        
        match command {
            PlayerCommand::Play => self.send_dbus_command("Play"),
            PlayerCommand::Pause => self.send_dbus_command("Pause"),
            PlayerCommand::PlayPause => {
                // Determine current state and toggle
                match self.get_playback_state() {
                    PlaybackState::Playing => self.send_dbus_command("Pause"),
                    _ => self.send_dbus_command("Play"),
                }
            }
            PlayerCommand::Stop => self.send_dbus_command("Stop"),
            PlayerCommand::Next => self.send_dbus_command("Next"),
            PlayerCommand::Previous => self.send_dbus_command("Previous"),
            _ => {
                warn!("Unsupported command for Bluetooth device: {}", command);
                false
            }
        }
    }
    
    fn as_any(&self) -> &dyn Any {
        self
    }
    
    fn start(&self) -> bool {
        let addr = self.device_address.read().map(|guard| guard.clone()).unwrap_or(None);
        info!("Starting Bluetooth player controller for device: {:?}", addr);
        
        // Initialize D-Bus connection
        if !self.ensure_dbus_connection() {
            error!("Failed to initialize D-Bus connection");
            return false;
        }
        
        // Try to find the player path
        if let Some(path) = self.find_player_path() {
            if let Ok(mut guard) = self.player_path.write() {
                *guard = Some(path);
            }
            let addr = self.device_address.read().map(|guard| guard.clone()).unwrap_or(None);
            info!("Found MediaPlayer1 interface for device: {:?}", addr);
        } else {
            let addr = self.device_address.read().map(|guard| guard.clone()).unwrap_or(None);
            warn!("MediaPlayer1 interface not found for device: {:?}", addr);
            // Don't return false here as the device might connect later
        }
        
        // Always start polling thread - it will wait for a device if none is available yet
        self.start_polling_thread();
        
        // Get device name
        if let Some(name) = self.get_device_name() {
            if let Ok(mut guard) = self.device_name.write() {
                *guard = Some(name);
            }
        }
        
        // Mark as alive
        self.base.alive();
        
        true
    }
    
    fn stop(&self) -> bool {
        let addr = self.device_address.read().map(|guard| guard.clone()).unwrap_or(None);
        info!("Stopping Bluetooth player controller for device: {:?}", addr);
        
        // Signal polling thread to stop
        self.stop_polling.store(true, Ordering::Relaxed);
        
        // Wait for polling thread to finish
        if let Ok(mut guard) = self.poll_thread.write() {
            if let Some(handle) = guard.take() {
                let _ = handle.join();
            }
        }
        
        // Clear connection
        if let Ok(mut guard) = self.connection.lock() {
            *guard = None;
        }
        
        // Clear player path
        if let Ok(mut guard) = self.player_path.write() {
            *guard = None;
        }
        
        true
    }
}