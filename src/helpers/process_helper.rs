use log::{debug, info, warn};
use std::process::Command;
use std::io;

/// Systemd actions
#[derive(Debug, Clone, PartialEq)]
pub enum SystemdAction {
    Start,
    Stop,
    Restart,
}

impl std::fmt::Display for SystemdAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SystemdAction::Start => write!(f, "start"),
            SystemdAction::Stop => write!(f, "stop"),
            SystemdAction::Restart => write!(f, "restart"),
        }
    }
}

/// Kill processes by name using platform-appropriate commands
/// 
/// # Arguments
/// * `process_name` - The name of the process to kill
/// * `force` - If true, sends SIGKILL (TERM on Unix) or force kill on Windows
/// 
/// # Returns
/// * `Ok(true)` if the kill command was executed successfully
/// * `Ok(false)` if no processes were found or killed
/// * `Err(io::Error)` if the command failed to execute
pub fn pkill(process_name: &str, force: bool) -> Result<bool, io::Error> {
    info!("Attempting to kill process: {} (force: {})", process_name, force);

    #[cfg(unix)]
    {
        let signal = if force { "-KILL" } else { "-TERM" };
        
        debug!("Using pkill with signal {} for process: {}", signal, process_name);
        
        let output = Command::new("pkill")
            .arg(signal)
            .arg("-f")  // Match against full command line
            .arg(process_name)
            .output()?;

        if output.status.success() {
            info!("Successfully sent {} signal to process: {}", signal, process_name);
            Ok(true)
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            if output.status.code() == Some(1) {
                // Exit code 1 means no processes found
                info!("No processes found matching: {}", process_name);
                Ok(false)
            } else {
                warn!("pkill command failed with exit code: {:?}, stderr: {}", 
                      output.status.code(), stderr);
                Ok(false)
            }
        }
    }

    #[cfg(windows)]
    {
        let force_flag = if force { "/F" } else { "" };
        
        debug!("Using taskkill {} for process: {}", force_flag, process_name);
        
        // Build command args
        let mut args = vec!["/IM", process_name];
        if force {
            args.push("/F");
        }
        
        let output = Command::new("taskkill")
            .args(&args)
            .output()?;

        if output.status.success() {
            info!("Successfully killed process: {}", process_name);
            return Ok(true);
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            
            // Check if it's a "process not found" error
            if stderr.contains("not found") || stdout.contains("not found") {
                info!("No processes found matching: {}", process_name);
                return Ok(false);
            } else {
                warn!("taskkill command failed with exit code: {:?}, stderr: {}, stdout: {}", 
                      output.status.code(), stderr, stdout);
                return Ok(false);
            }
        }
    }
}

/// Manage systemd units (Linux only)
/// 
/// # Arguments
/// * `unit_name` - The name of the systemd unit
/// * `action` - The action to perform (start, stop, restart)
/// 
/// # Returns
/// * `Ok(true)` if the systemd command was executed successfully
/// * `Ok(false)` if the command failed but executed
/// * `Err(io::Error)` if the command failed to execute or systemd is not available
pub fn systemd(unit_name: &str, action: SystemdAction) -> Result<bool, io::Error> {
    info!("Attempting to {} systemd unit: {}", action, unit_name);

    #[cfg(unix)]
    {
        debug!("Using systemctl {} for unit: {}", action, unit_name);
        
        let output = Command::new("systemctl")
            .arg(action.to_string())
            .arg(unit_name)
            .output()?;

        if output.status.success() {
            info!("Successfully executed systemctl {} for unit: {}", action, unit_name);
            Ok(true)
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            
            warn!("systemctl {} failed for unit: {}, exit code: {:?}, stderr: {}, stdout: {}", 
                  action, unit_name, output.status.code(), stderr, stdout);
            Ok(false)
        }
    }

    #[cfg(not(unix))]
    {
        error!("Systemd is only available on Unix-like systems");
        Err(io::Error::new(io::ErrorKind::Unsupported, 
                          "Systemd is only available on Unix-like systems"))
    }
}

/// Check if a systemd unit is active (Linux only)
/// 
/// # Arguments
/// * `unit_name` - The name of the systemd unit
/// 
/// # Returns
/// * `Ok(true)` if the unit is active
/// * `Ok(false)` if the unit is not active
/// * `Err(io::Error)` if the command failed to execute or systemd is not available
pub fn is_systemd_unit_active(unit_name: &str) -> Result<bool, io::Error> {
    debug!("Checking if systemd unit is active: {}", unit_name);

    #[cfg(unix)]
    {
        let output = Command::new("systemctl")
            .arg("is-active")
            .arg("--quiet")
            .arg(unit_name)
            .output()?;

        // Exit code 0 means active, non-zero means inactive or failed
        let is_active = output.status.success();
        debug!("Systemd unit {} is active: {}", unit_name, is_active);
        Ok(is_active)
    }

    #[cfg(not(unix))]
    {
        error!("Systemd is only available on Unix-like systems");
        Err(io::Error::new(io::ErrorKind::Unsupported, 
                          "Systemd is only available on Unix-like systems"))
    }
}

/// Check if systemctl command is available on the system
/// 
/// # Returns
/// * `true` if systemctl is available
/// * `false` if systemctl is not available
pub fn is_systemd_available() -> bool {
    #[cfg(unix)]
    {
        match Command::new("which")
            .arg("systemctl")
            .output() {
        Ok(output) => {
            let available = output.status.success();
            debug!("Systemd available: {}", available);
            available
        }
        Err(_) => {
            debug!("Cannot check systemd availability");
            false
        }
        }
    }

    #[cfg(not(unix))]
    {
        false
    }
}

/// Get the status of a systemd unit (Linux only)
/// 
/// # Arguments
/// * `unit_name` - The name of the systemd unit
/// 
/// # Returns
/// * `Ok(String)` containing the status output
/// * `Err(io::Error)` if the command failed to execute or systemd is not available
pub fn get_systemd_unit_status(unit_name: &str) -> Result<String, io::Error> {
    debug!("Getting status of systemd unit: {}", unit_name);

    #[cfg(unix)]
    {
        let output = Command::new("systemctl")
            .arg("status")
            .arg(unit_name)
            .output()?;

        let status = String::from_utf8_lossy(&output.stdout);
        debug!("Systemd unit {} status: {}", unit_name, status);
        Ok(status.to_string())
    }

    #[cfg(not(unix))]
    {
        error!("Systemd is only available on Unix-like systems");
        Err(io::Error::new(io::ErrorKind::Unsupported, 
                          "Systemd is only available on Unix-like systems"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_systemd_action_display() {
        assert_eq!(SystemdAction::Start.to_string(), "start");
        assert_eq!(SystemdAction::Stop.to_string(), "stop");
        assert_eq!(SystemdAction::Restart.to_string(), "restart");
    }

    #[test]
    fn test_systemd_available() {
        // Just test that the function doesn't panic
        let _available = is_systemd_available();
    }

    #[cfg(unix)]
    #[test]
    fn test_pkill_nonexistent_process() {
        // Test with a process that should not exist
        let result = pkill("nonexistent_process_12345", false);
        assert!(result.is_ok());
        // Should return false for no processes found
        assert_eq!(result.unwrap(), false);
    }

    #[cfg(windows)]
    #[test]
    fn test_pkill_nonexistent_process() {
        // Test with a process that should not exist
        let result = pkill("nonexistent_process_12345.exe", false);
        assert!(result.is_ok());
        // Should return false for no processes found
        assert_eq!(result.unwrap(), false);
    }
}
