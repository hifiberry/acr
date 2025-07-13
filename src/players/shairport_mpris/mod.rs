use crate::players::mpris::MprisPlayerController;
use crate::players::player_controller::PlayerController;
use crate::data::{PlayerCapabilitySet, PlayerCommand};
use std::time::Duration;
use log::{debug, info, warn, error};

/// ShairportSync MPRIS player controller implementation
/// This controller extends the MPRIS controller specifically for ShairportSync players
pub struct ShairportMprisPlayerController {
    /// Base MPRIS controller
    mpris_controller: MprisPlayerController,
    
    /// Optional systemd service name for AirPlay 2 mode (where MPRIS controls don't work)
    systemd_service: Option<String>,
}

impl ShairportMprisPlayerController {
    /// Create a new ShairportSync MPRIS player controller
    pub fn new() -> Self {
        Self::new_with_config(Duration::from_secs_f64(1.0), None)
    }
    
    /// Create a new ShairportSync MPRIS player controller with configurable polling interval
    pub fn new_with_poll_interval(poll_interval: Duration) -> Self {
        Self::new_with_config(poll_interval, None)
    }
    
    /// Create a new ShairportSync MPRIS player controller with full configuration
    pub fn new_with_config(poll_interval: Duration, systemd_service: Option<String>) -> Self {
        debug!("Creating new ShairportMprisPlayerController with poll interval: {:?}, systemd_service: {:?}", 
               poll_interval, systemd_service);
        
        // Create MPRIS controller with the ShairportSync bus name
        let mpris_controller = MprisPlayerController::new_with_poll_interval(
            "org.mpris.MediaPlayer2.ShairportSync", 
            poll_interval
        );
        
        let controller = Self {
            mpris_controller,
            systemd_service: systemd_service.clone(),
        };
        
        // Set ShairportSync-specific capabilities
        controller.set_shairport_capabilities();
        
        info!("Created ShairportSync MPRIS player controller with systemd_service: {:?}", systemd_service);
        controller
    }
    
    /// Set capabilities specific to ShairportSync
    fn set_shairport_capabilities(&self) {
        debug!("Setting ShairportSync-specific capabilities");
        
        // ShairportSync typically supports these capabilities:
        // - Play/Pause controls (from AirPlay client)
        // - Next/Previous (if client supports it)
        // - Volume control
        // - Position reporting (but not seeking)
        // Note: ShairportSync often runs on system bus and may have limited seek support
        // We need to access the base through the MPRIS controller's public interface
        // Since the base field is private, we'll let the MPRIS controller handle the default capabilities
        // and they should be appropriate for ShairportSync as well
    }
    
    /// Restart the systemd service (used for AirPlay 2 mode where MPRIS controls don't work)
    fn restart_systemd_service(&self) -> bool {
        if let Some(ref service_name) = self.systemd_service {
            info!("Restarting systemd service: {}", service_name);
            
            // Use systemctl to restart the service
            match std::process::Command::new("systemctl")
                .arg("restart")
                .arg(service_name)
                .output()
            {
                Ok(output) => {
                    if output.status.success() {
                        info!("Successfully restarted systemd service: {}", service_name);
                        true
                    } else {
                        let stderr = String::from_utf8_lossy(&output.stderr);
                        error!("Failed to restart systemd service {}: {}", service_name, stderr);
                        false
                    }
                }
                Err(e) => {
                    error!("Failed to execute systemctl restart {}: {}", service_name, e);
                    false
                }
            }
        } else {
            warn!("No systemd service configured for ShairportSync restart");
            false
        }
    }
}

// Implement Clone by delegating to the inner MPRIS controller
impl Clone for ShairportMprisPlayerController {
    fn clone(&self) -> Self {
        Self {
            mpris_controller: self.mpris_controller.clone(),
            systemd_service: self.systemd_service.clone(),
        }
    }
}

// Delegate all PlayerController methods to the inner MPRIS controller
impl PlayerController for ShairportMprisPlayerController {
    fn get_capabilities(&self) -> PlayerCapabilitySet {
        self.mpris_controller.get_capabilities()
    }
    
    fn get_player_name(&self) -> String {
        "shairport-mpris".to_string()
    }
    
    fn get_player_id(&self) -> String {
        self.mpris_controller.get_player_id()
    }
    
    fn has_library(&self) -> bool {
        false // ShairportSync is an AirPlay receiver, no library
    }
    
    fn supports_api_events(&self) -> bool {
        self.mpris_controller.supports_api_events()
    }
    
    fn get_last_seen(&self) -> Option<std::time::SystemTime> {
        self.mpris_controller.get_last_seen()
    }
    
    fn receive_update(&self, update: crate::data::PlayerUpdate) -> bool {
        self.mpris_controller.receive_update(update)
    }
    
    fn get_metadata(&self) -> Option<std::collections::HashMap<String, serde_json::Value>> {
        self.mpris_controller.get_metadata()
    }
    
    fn get_playback_state(&self) -> crate::data::PlaybackState {
        self.mpris_controller.get_playback_state()
    }
    
    fn get_song(&self) -> Option<crate::data::Song> {
        self.mpris_controller.get_song()
    }
    
    fn get_queue(&self) -> Vec<crate::data::Track> {
        self.mpris_controller.get_queue()
    }
    
    fn get_shuffle(&self) -> bool {
        self.mpris_controller.get_shuffle()
    }
    
    fn get_loop_mode(&self) -> crate::data::LoopMode {
        self.mpris_controller.get_loop_mode()
    }
    
    fn get_position(&self) -> Option<f64> {
        self.mpris_controller.get_position()
    }
    
    fn send_command(&self, command: PlayerCommand) -> bool {
        // If systemd service is configured, use systemd restart for play/pause commands
        // This is useful for AirPlay 2 mode where MPRIS controls don't work properly
        if self.systemd_service.is_some() {
            match command {
                PlayerCommand::Play | PlayerCommand::Pause | PlayerCommand::PlayPause => {
                    info!("Using systemd restart for command: {:?} (AirPlay 2 mode)", command);
                    return self.restart_systemd_service();
                }
                _ => {
                    // For other commands, try MPRIS first but don't fail if it doesn't work
                    debug!("Attempting MPRIS command: {:?}", command);
                    self.mpris_controller.send_command(command)
                }
            }
        } else {
            // Normal MPRIS mode
            self.mpris_controller.send_command(command)
        }
    }
    
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    
    fn start(&self) -> bool {
        info!("Starting ShairportSync MPRIS player controller");
        self.mpris_controller.start()
    }
    
    fn stop(&self) -> bool {
        info!("Stopping ShairportSync MPRIS player controller");
        self.mpris_controller.stop()
    }
}
