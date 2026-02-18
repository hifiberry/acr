use std::time::Instant;
use std::sync::Arc;
use parking_lot::Mutex;

/// PlayerProgress tracks the current playback position and automatically
/// updates it when the player is in a playing state.
#[derive(Debug, Clone)]
pub struct PlayerProgress {
    inner: Arc<Mutex<PlayerProgressInner>>,
}

#[derive(Debug)]
struct PlayerProgressInner {
    /// Current position in seconds
    position: f64,
    /// Whether the player is currently playing
    is_playing: bool,
    /// Timestamp when the position was last updated
    last_update: Instant,
}

impl PlayerProgress {
    /// Create a new PlayerProgress instance with position 0 and not playing
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(PlayerProgressInner {
                position: 0.0,
                is_playing: false,
                last_update: Instant::now(),
            })),
        }
    }

    /// Set the current position in seconds
    /// Position must be non-negative
    pub fn set_position(&self, position: f64) {
        if position < 0.0 {
            return; // Ignore negative positions
        }

        let mut inner = self.inner.lock();
        inner.position = position;
        inner.last_update = Instant::now();
    }

    /// Get the current position in seconds
    /// If playing, this will return an updated position based on elapsed time
    pub fn get_position(&self) -> f64 {
        let mut inner = self.inner.lock();
        
        if inner.is_playing {
            // Update position based on elapsed time
            let now = Instant::now();
            let elapsed = now.duration_since(inner.last_update);
            inner.position += elapsed.as_secs_f64();
            inner.last_update = now;
        }
        
        inner.position
    }

    /// Set the playing state
    /// When set to true, position will start auto-incrementing
    /// When set to false, position will remain static
    pub fn set_playing(&self, playing: bool) {
        let mut inner = self.inner.lock();
        
        if inner.is_playing != playing {
            // Update position to current time before changing state
            if inner.is_playing {
                let now = Instant::now();
                let elapsed = now.duration_since(inner.last_update);
                inner.position += elapsed.as_secs_f64();
            }
            
            inner.is_playing = playing;
            inner.last_update = Instant::now();
        }
    }

    /// Get the current playing state
    pub fn is_playing(&self) -> bool {
        let inner = self.inner.lock();
        inner.is_playing
    }

    /// Reset position to 0 and stop playing
    pub fn reset(&self) {
        let mut inner = self.inner.lock();
        inner.position = 0.0;
        inner.is_playing = false;
        inner.last_update = Instant::now();
    }
}

impl Default for PlayerProgress {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn test_new_player_progress() {
        let progress = PlayerProgress::new();
        assert_eq!(progress.get_position(), 0.0);
        assert!(!progress.is_playing());
    }

    #[test]
    fn test_set_get_position() {
        let progress = PlayerProgress::new();
        
        // Set position to 10.5 seconds
        progress.set_position(10.5);
        assert_eq!(progress.get_position(), 10.5);
        
        // Set position to 0
        progress.set_position(0.0);
        assert_eq!(progress.get_position(), 0.0);
        
        // Set position to another value
        progress.set_position(123.45);
        assert_eq!(progress.get_position(), 123.45);
    }

    #[test]
    fn test_negative_position_ignored() {
        let progress = PlayerProgress::new();
        
        // Set initial position
        progress.set_position(10.0);
        assert_eq!(progress.get_position(), 10.0);
        
        // Try to set negative position - should be ignored
        progress.set_position(-5.0);
        assert_eq!(progress.get_position(), 10.0);
    }

    #[test]
    fn test_playing_state() {
        let progress = PlayerProgress::new();
        
        // Initially not playing
        assert!(!progress.is_playing());
        
        // Set playing to true
        progress.set_playing(true);
        assert!(progress.is_playing());
        
        // Set playing to false
        progress.set_playing(false);
        assert!(!progress.is_playing());
    }

    #[test]
    fn test_position_increments_when_playing() {
        let progress = PlayerProgress::new();
        
        // Set initial position
        progress.set_position(0.0);
        assert_eq!(progress.get_position(), 0.0);
        
        // Start playing
        progress.set_playing(true);
        
        // Wait a bit and check position has increased
        thread::sleep(Duration::from_millis(100));
        let position1 = progress.get_position();
        assert!(position1 > 0.0);
        assert!(position1 < 1.0); // Should be less than 1 second
        
        // Wait a bit more and check position increased further
        thread::sleep(Duration::from_millis(100));
        let position2 = progress.get_position();
        assert!(position2 > position1);
        assert!(position2 < 1.0); // Should still be less than 1 second
    }

    #[test]
    fn test_position_stops_incrementing_when_not_playing() {
        let progress = PlayerProgress::new();
        
        // Set initial position and start playing
        progress.set_position(0.0);
        progress.set_playing(true);
        
        // Wait a bit
        thread::sleep(Duration::from_millis(100));
        let position1 = progress.get_position();
        assert!(position1 > 0.0);
        
        // Stop playing
        progress.set_playing(false);
        
        // Wait a bit more
        thread::sleep(Duration::from_millis(100));
        let position2 = progress.get_position();
        
        // Position should be approximately the same (within a small tolerance)
        assert!((position2 - position1).abs() < 0.01);
    }

    #[test]
    fn test_position_updates_correctly_over_time() {
        let progress = PlayerProgress::new();
        
        // Set initial position and start playing
        progress.set_position(5.0);
        progress.set_playing(true);
        
        // Wait approximately 1 second
        thread::sleep(Duration::from_millis(1000));
        
        let position = progress.get_position();
        // Position should be approximately 6.0 (5.0 + 1.0 second)
        // Allow some tolerance for timing variations
        assert!(position > 5.8);
        assert!(position < 6.2);
    }

    #[test]
    fn test_multiple_get_position_calls_when_playing() {
        let progress = PlayerProgress::new();
        
        // Set initial position and start playing
        progress.set_position(0.0);
        progress.set_playing(true);
        
        // Get position multiple times with small delays
        let pos1 = progress.get_position();
        thread::sleep(Duration::from_millis(100));
        let pos2 = progress.get_position();
        thread::sleep(Duration::from_millis(100));
        let pos3 = progress.get_position();
        
        // Each position should be greater than the previous
        assert!(pos2 > pos1);
        assert!(pos3 > pos2);
    }

    #[test]
    fn test_reset() {
        let progress = PlayerProgress::new();
        
        // Set some position and start playing
        progress.set_position(42.0);
        progress.set_playing(true);
        
        // Wait a bit
        thread::sleep(Duration::from_millis(100));
        
        // Reset
        progress.reset();
        
        // Should be back to initial state
        assert_eq!(progress.get_position(), 0.0);
        assert!(!progress.is_playing());
    }

    #[test]
    fn test_set_playing_multiple_times() {
        let progress = PlayerProgress::new();
        
        // Set playing to true multiple times
        progress.set_playing(true);
        progress.set_playing(true);
        assert!(progress.is_playing());
        
        // Set playing to false multiple times
        progress.set_playing(false);
        progress.set_playing(false);
        assert!(!progress.is_playing());
    }

    #[test]
    fn test_concurrent_access() {
        let progress = PlayerProgress::new();
        progress.set_position(0.0);
        progress.set_playing(true);
        
        // Clone progress for use in thread
        let progress_clone = progress.clone();
        
        // Start a thread that continuously updates position
        let handle = thread::spawn(move || {
            for i in 0..10 {
                progress_clone.set_position(i as f64);
                thread::sleep(Duration::from_millis(10));
            }
        });
        
        // Main thread continuously reads position
        for _ in 0..10 {
            let _pos = progress.get_position();
            thread::sleep(Duration::from_millis(10));
        }
        
        handle.join().unwrap();
        
        // Should not panic or cause data races
        assert!(progress.get_position() >= 0.0);
    }

    #[test]
    fn test_default_implementation() {
        let progress = PlayerProgress::default();
        assert_eq!(progress.get_position(), 0.0);
        assert!(!progress.is_playing());
    }
}
