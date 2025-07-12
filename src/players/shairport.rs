use crate::players::mpris::MprisPlayerController;
use crate::players::player_controller::PlayerController;
use crate::data::PlayerCapabilitySet;
use std::time::Duration;
use log::{debug, info};

/// ShairportSync player controller implementation
/// This controller extends the MPRIS controller specifically for ShairportSync players
pub struct ShairportSyncPlayerController {
    /// Base MPRIS controller
    mpris_controller: MprisPlayerController,
}

impl ShairportSyncPlayerController {
    /// Create a new ShairportSync player controller
    pub fn new() -> Self {
        Self::new_with_poll_interval(Duration::from_secs_f64(1.0))
    }
    
    /// Create a new ShairportSync player controller with configurable polling interval
    pub fn new_with_poll_interval(poll_interval: Duration) -> Self {
        debug!("Creating new ShairportSyncPlayerController with poll interval: {:?}", poll_interval);
        
        // Create MPRIS controller with the ShairportSync bus name
        let mpris_controller = MprisPlayerController::new_with_poll_interval(
            "org.mpris.MediaPlayer2.ShairportSync", 
            poll_interval
        );
        
        let controller = Self {
            mpris_controller,
        };
        
        // Set ShairportSync-specific capabilities
        controller.set_shairport_capabilities();
        
        info!("Created ShairportSync player controller");
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
}

// Implement Clone by delegating to the inner MPRIS controller
impl Clone for ShairportSyncPlayerController {
    fn clone(&self) -> Self {
        Self {
            mpris_controller: self.mpris_controller.clone(),
        }
    }
}

// Delegate all PlayerController methods to the inner MPRIS controller
impl PlayerController for ShairportSyncPlayerController {
    fn get_capabilities(&self) -> PlayerCapabilitySet {
        self.mpris_controller.get_capabilities()
    }
    
    fn get_player_name(&self) -> String {
        "ShairportSync".to_string()
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
    
    fn send_command(&self, command: crate::data::PlayerCommand) -> bool {
        self.mpris_controller.send_command(command)
    }
    
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    
    fn start(&self) -> bool {
        info!("Starting ShairportSync player controller");
        self.mpris_controller.start()
    }
    
    fn stop(&self) -> bool {
        info!("Stopping ShairportSync player controller");
        self.mpris_controller.stop()
    }
}
