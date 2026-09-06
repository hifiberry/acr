use std::time::Duration;
use std::thread;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use log::{debug, warn, info};

/// Retry mechanism with exponential backoff
/// 
/// This helper provides retry functionality with increasing intervals:
/// 1s, 2s, 4s, 8s, 15s, 30s, 60s (max)
pub struct RetryHandler {
    /// Current attempt number (0-based)
    attempt: usize,
    /// Maximum number of attempts before giving up
    max_attempts: Option<usize>,
    /// Custom retry intervals (if None, uses default exponential backoff)
    custom_intervals: Option<Vec<Duration>>,
}

impl RetryHandler {
    /// Create a new retry handler with default exponential backoff
    pub fn new() -> Self {
        Self {
            attempt: 0,
            max_attempts: None,
            custom_intervals: None,
        }
    }
    
    /// Create a new retry handler with a maximum number of attempts
    pub fn with_max_attempts(max_attempts: usize) -> Self {
        Self {
            attempt: 0,
            max_attempts: Some(max_attempts),
            custom_intervals: None,
        }
    }
    
    /// Create a new retry handler with custom intervals
    pub fn with_intervals(intervals: Vec<Duration>) -> Self {
        Self {
            attempt: 0,
            max_attempts: Some(intervals.len()),
            custom_intervals: Some(intervals),
        }
    }
    
    /// Create a retry handler with the standard intervals for connection retries
    /// Uses: 1s, 2s, 4s, 8s, 15s, 30s, 60s
    pub fn connection_retry() -> Self {
        let intervals = vec![
            Duration::from_secs(1),
            Duration::from_secs(2),
            Duration::from_secs(4),
            Duration::from_secs(8),
            Duration::from_secs(15),
            Duration::from_secs(30),
            Duration::from_secs(60),
        ];
        Self::with_intervals(intervals)
    }
    
    /// Get the current attempt number (0-based)
    pub fn attempt(&self) -> usize {
        self.attempt
    }
    
    /// Check if we should continue retrying
    pub fn should_retry(&self) -> bool {
        if let Some(max) = self.max_attempts {
            self.attempt < max
        } else {
            true // Retry indefinitely if no max is set
        }
    }
    
    /// Get the delay for the current attempt
    pub fn get_delay(&self) -> Duration {
        if let Some(ref intervals) = self.custom_intervals {
            // Use custom intervals, clamping to the last interval if we exceed the list
            let index = std::cmp::min(self.attempt, intervals.len() - 1);
            intervals[index]
        } else {
            // Use exponential backoff: 1s, 2s, 4s, 8s, 16s, 32s, 60s (max)
            let base_delay = 2_u64.pow(self.attempt as u32);
            let delay_secs = std::cmp::min(base_delay, 60);
            Duration::from_secs(delay_secs)
        }
    }
    
    /// Wait for the current retry interval
    /// Returns true if we should continue, false if interrupted by the running flag
    pub fn wait(&mut self, running: Option<&Arc<AtomicBool>>) -> bool {
        let delay = self.get_delay();
        debug!("Retry attempt {}: waiting {:?} before next attempt", self.attempt + 1, delay);
        
        // If we have a running flag, check it periodically during the wait
        if let Some(running_flag) = running {
            let check_interval = Duration::from_millis(100);
            let mut remaining = delay;
            
            while remaining > Duration::from_millis(0) {
                if !running_flag.load(Ordering::SeqCst) {
                    debug!("Retry interrupted by shutdown signal");
                    return false;
                }
                
                let sleep_time = std::cmp::min(check_interval, remaining);
                thread::sleep(sleep_time);
                remaining = remaining.saturating_sub(sleep_time);
            }
        } else {
            // Simple sleep without interruption checking
            thread::sleep(delay);
        }
        
        self.attempt += 1;
        true
    }
    
    /// Reset the retry counter
    pub fn reset(&mut self) {
        debug!("Resetting retry counter");
        self.attempt = 0;
    }
    
    /// Execute a closure with retry logic
    /// 
    /// # Arguments
    /// * `operation` - The operation to retry
    /// * `running` - Optional running flag to check for shutdown
    /// * `operation_name` - Name for logging purposes
    /// 
    /// # Returns
    /// * `Some(T)` if the operation succeeded
    /// * `None` if all retries were exhausted or interrupted
    pub fn execute_with_retry<T, F>(
        &mut self,
        mut operation: F,
        running: Option<&Arc<AtomicBool>>,
        operation_name: &str,
    ) -> Option<T>
    where
        F: FnMut() -> Option<T>,
    {
        info!("Starting {} with retry logic", operation_name);
        
        loop {
            // Check if we should stop due to shutdown signal
            if let Some(running_flag) = running {
                if !running_flag.load(Ordering::SeqCst) {
                    debug!("{} interrupted by shutdown signal", operation_name);
                    return None;
                }
            }
            
            // Try the operation
            debug!("Attempting {} (attempt {})", operation_name, self.attempt + 1);
            if let Some(result) = operation() {
                info!("{} succeeded on attempt {}", operation_name, self.attempt + 1);
                return Some(result);
            }
            
            // Check if we should retry
            if !self.should_retry() {
                warn!("{} failed after {} attempts, giving up", operation_name, self.attempt + 1);
                return None;
            }
            
            // Wait before retrying
            if !self.wait(running) {
                return None; // Interrupted
            }
        }
    }
}

impl Default for RetryHandler {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_default_retry_intervals() {
        let mut retry = RetryHandler::new();
        
        // Test the first few intervals
        assert_eq!(retry.get_delay(), Duration::from_secs(1));
        retry.attempt += 1;
        assert_eq!(retry.get_delay(), Duration::from_secs(2));
        retry.attempt += 1;
        assert_eq!(retry.get_delay(), Duration::from_secs(4));
        retry.attempt += 1;
        assert_eq!(retry.get_delay(), Duration::from_secs(8));
    }
    
    #[test]
    fn test_connection_retry_intervals() {
        let mut retry = RetryHandler::connection_retry();
        
        let expected = vec![1, 2, 4, 8, 15, 30, 60];
        for (i, &expected_secs) in expected.iter().enumerate() {
            assert_eq!(retry.get_delay(), Duration::from_secs(expected_secs));
            if i < expected.len() - 1 {
                retry.attempt += 1;
            }
        }
        
        // Test that we don't go beyond the last interval
        retry.attempt += 1;
        assert_eq!(retry.get_delay(), Duration::from_secs(60));
    }
    
    #[test]
    fn test_max_attempts() {
        let retry = RetryHandler::with_max_attempts(3);
        
        assert!(retry.should_retry()); // attempt 0
        
        let mut retry = retry;
        retry.attempt = 2;
        assert!(retry.should_retry()); // attempt 2
        
        retry.attempt = 3;
        assert!(!retry.should_retry()); // attempt 3, should not retry
    }
    
    #[test]
    fn test_reset() {
        let mut retry = RetryHandler::new();
        retry.attempt = 5;
        retry.reset();
        assert_eq!(retry.attempt, 0);
    }
}
