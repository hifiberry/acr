use std::process::{Command, Stdio};
use std::io;
use log::{debug, error};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum SystemdError {
    #[error("Failed to execute systemctl command: {0}")]
    CommandFailed(#[from] io::Error),
    #[error("Systemctl command returned non-zero exit code: {code}")]
    NonZeroExit { code: i32 },
    #[error("Failed to parse systemctl output")]
    ParseError,
    #[error("Unit '{unit}' does not exist")]
    UnitNotFound { unit: String },
    #[error("Systemd is not available on this system")]
    SystemdNotAvailable,
}

pub type Result<T> = std::result::Result<T, SystemdError>;

/// Status of a systemd unit
#[derive(Debug, Clone, PartialEq)]
pub enum UnitStatus {
    Active,
    Inactive,
    Failed,
    Activating,
    Deactivating,
    Unknown(String),
}

impl From<&str> for UnitStatus {
    fn from(status: &str) -> Self {
        match status.trim() {
            "active" => UnitStatus::Active,
            "inactive" => UnitStatus::Inactive,
            "failed" => UnitStatus::Failed,
            "activating" => UnitStatus::Activating,
            "deactivating" => UnitStatus::Deactivating,
            other => UnitStatus::Unknown(other.to_string()),
        }
    }
}

impl std::fmt::Display for UnitStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            UnitStatus::Active => write!(f, "active"),
            UnitStatus::Inactive => write!(f, "inactive"),
            UnitStatus::Failed => write!(f, "failed"),
            UnitStatus::Activating => write!(f, "activating"),
            UnitStatus::Deactivating => write!(f, "deactivating"),
            UnitStatus::Unknown(s) => write!(f, "{}", s),
        }
    }
}

/// Information about a systemd unit
#[derive(Debug, Clone)]
pub struct UnitInfo {
    pub name: String,
    pub status: UnitStatus,
    pub enabled: bool,
    pub exists: bool,
}

/// Helper for interacting with systemd units
pub struct SystemdHelper {
    use_user_mode: bool,
}

impl SystemdHelper {
    /// Create a new SystemdHelper for system-wide units
    pub fn new() -> Self {
        Self {
            use_user_mode: false,
        }
    }

    /// Create a new SystemdHelper for user units
    pub fn new_user() -> Self {
        Self {
            use_user_mode: true,
        }
    }

    /// Check if systemd is available on the system
    pub fn is_available(&self) -> bool {
        let mut cmd = Command::new("systemctl");
        if self.use_user_mode {
            cmd.arg("--user");
        }
        cmd.arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null());

        match cmd.status() {
            Ok(status) => status.success(),
            Err(_) => false,
        }
    }

    /// Check if a unit exists in systemd
    pub fn unit_exists(&self, unit_name: &str) -> Result<bool> {
        if !self.is_available() {
            return Err(SystemdError::SystemdNotAvailable);
        }

        let mut cmd = Command::new("systemctl");
        if self.use_user_mode {
            cmd.arg("--user");
        }
        cmd.args(["list-unit-files", unit_name, "--no-legend", "--no-pager"]);

        debug!("Checking if unit exists: {}", unit_name);
        
        match cmd.output() {
            Ok(output) => {
                if output.status.success() {
                    let output_str = String::from_utf8_lossy(&output.stdout);
                    let exists = !output_str.trim().is_empty();
                    debug!("Unit {} exists: {}", unit_name, exists);
                    Ok(exists)
                } else {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    debug!("systemctl list-unit-files failed: {}", stderr);
                    Ok(false)
                }
            }
            Err(e) => {
                error!("Failed to execute systemctl: {}", e);
                Err(SystemdError::CommandFailed(e))
            }
        }
    }

    /// Get the status of a unit
    pub fn get_unit_status(&self, unit_name: &str) -> Result<UnitStatus> {
        if !self.is_available() {
            return Err(SystemdError::SystemdNotAvailable);
        }

        let mut cmd = Command::new("systemctl");
        if self.use_user_mode {
            cmd.arg("--user");
        }
        cmd.args(["is-active", unit_name]);

        debug!("Getting status for unit: {}", unit_name);

        match cmd.output() {
            Ok(output) => {
                let output_str = String::from_utf8_lossy(&output.stdout);
                let status = UnitStatus::from(output_str.as_ref());
                debug!("Unit {} status: {}", unit_name, status);
                Ok(status)
            }
            Err(e) => {
                error!("Failed to execute systemctl is-active: {}", e);
                Err(SystemdError::CommandFailed(e))
            }
        }
    }

    /// Check if a unit is enabled
    pub fn is_unit_enabled(&self, unit_name: &str) -> Result<bool> {
        if !self.is_available() {
            return Err(SystemdError::SystemdNotAvailable);
        }

        let mut cmd = Command::new("systemctl");
        if self.use_user_mode {
            cmd.arg("--user");
        }
        cmd.args(["is-enabled", unit_name]);

        debug!("Checking if unit is enabled: {}", unit_name);

        match cmd.output() {
            Ok(output) => {
                let enabled = output.status.success();
                debug!("Unit {} enabled: {}", unit_name, enabled);
                Ok(enabled)
            }
            Err(e) => {
                error!("Failed to execute systemctl is-enabled: {}", e);
                Err(SystemdError::CommandFailed(e))
            }
        }
    }

    /// Check if a unit is active
    pub fn is_unit_active(&self, unit_name: &str) -> Result<bool> {
        match self.get_unit_status(unit_name)? {
            UnitStatus::Active => Ok(true),
            _ => Ok(false),
        }
    }

    /// Get comprehensive information about a unit
    pub fn get_unit_info(&self, unit_name: &str) -> Result<UnitInfo> {
        let exists = self.unit_exists(unit_name)?;
        
        if !exists {
            return Ok(UnitInfo {
                name: unit_name.to_string(),
                status: UnitStatus::Unknown("not-found".to_string()),
                enabled: false,
                exists: false,
            });
        }

        let status = self.get_unit_status(unit_name)?;
        let enabled = self.is_unit_enabled(unit_name).unwrap_or(false);

        Ok(UnitInfo {
            name: unit_name.to_string(),
            status,
            enabled,
            exists: true,
        })
    }

    /// Start a unit
    pub fn start_unit(&self, unit_name: &str) -> Result<()> {
        if !self.is_available() {
            return Err(SystemdError::SystemdNotAvailable);
        }

        let mut cmd = Command::new("systemctl");
        if self.use_user_mode {
            cmd.arg("--user");
        }
        cmd.args(["start", unit_name]);

        debug!("Starting unit: {}", unit_name);

        match cmd.status() {
            Ok(status) => {
                if status.success() {
                    debug!("Successfully started unit: {}", unit_name);
                    Ok(())
                } else {
                    let code = status.code().unwrap_or(-1);
                    error!("Failed to start unit {}: exit code {}", unit_name, code);
                    Err(SystemdError::NonZeroExit { code })
                }
            }
            Err(e) => {
                error!("Failed to execute systemctl start: {}", e);
                Err(SystemdError::CommandFailed(e))
            }
        }
    }

    /// Stop a unit
    pub fn stop_unit(&self, unit_name: &str) -> Result<()> {
        if !self.is_available() {
            return Err(SystemdError::SystemdNotAvailable);
        }

        let mut cmd = Command::new("systemctl");
        if self.use_user_mode {
            cmd.arg("--user");
        }
        cmd.args(["stop", unit_name]);

        debug!("Stopping unit: {}", unit_name);

        match cmd.status() {
            Ok(status) => {
                if status.success() {
                    debug!("Successfully stopped unit: {}", unit_name);
                    Ok(())
                } else {
                    let code = status.code().unwrap_or(-1);
                    error!("Failed to stop unit {}: exit code {}", unit_name, code);
                    Err(SystemdError::NonZeroExit { code })
                }
            }
            Err(e) => {
                error!("Failed to execute systemctl stop: {}", e);
                Err(SystemdError::CommandFailed(e))
            }
        }
    }

    /// Restart a unit
    pub fn restart_unit(&self, unit_name: &str) -> Result<()> {
        if !self.is_available() {
            return Err(SystemdError::SystemdNotAvailable);
        }

        let mut cmd = Command::new("systemctl");
        if self.use_user_mode {
            cmd.arg("--user");
        }
        cmd.args(["restart", unit_name]);

        debug!("Restarting unit: {}", unit_name);

        match cmd.status() {
            Ok(status) => {
                if status.success() {
                    debug!("Successfully restarted unit: {}", unit_name);
                    Ok(())
                } else {
                    let code = status.code().unwrap_or(-1);
                    error!("Failed to restart unit {}: exit code {}", unit_name, code);
                    Err(SystemdError::NonZeroExit { code })
                }
            }
            Err(e) => {
                error!("Failed to execute systemctl restart: {}", e);
                Err(SystemdError::CommandFailed(e))
            }
        }
    }

    /// Enable a unit
    pub fn enable_unit(&self, unit_name: &str) -> Result<()> {
        if !self.is_available() {
            return Err(SystemdError::SystemdNotAvailable);
        }

        let mut cmd = Command::new("systemctl");
        if self.use_user_mode {
            cmd.arg("--user");
        }
        cmd.args(["enable", unit_name]);

        debug!("Enabling unit: {}", unit_name);

        match cmd.status() {
            Ok(status) => {
                if status.success() {
                    debug!("Successfully enabled unit: {}", unit_name);
                    Ok(())
                } else {
                    let code = status.code().unwrap_or(-1);
                    error!("Failed to enable unit {}: exit code {}", unit_name, code);
                    Err(SystemdError::NonZeroExit { code })
                }
            }
            Err(e) => {
                error!("Failed to execute systemctl enable: {}", e);
                Err(SystemdError::CommandFailed(e))
            }
        }
    }

    /// Disable a unit
    pub fn disable_unit(&self, unit_name: &str) -> Result<()> {
        if !self.is_available() {
            return Err(SystemdError::SystemdNotAvailable);
        }

        let mut cmd = Command::new("systemctl");
        if self.use_user_mode {
            cmd.arg("--user");
        }
        cmd.args(["disable", unit_name]);

        debug!("Disabling unit: {}", unit_name);

        match cmd.status() {
            Ok(status) => {
                if status.success() {
                    debug!("Successfully disabled unit: {}", unit_name);
                    Ok(())
                } else {
                    let code = status.code().unwrap_or(-1);
                    error!("Failed to disable unit {}: exit code {}", unit_name, code);
                    Err(SystemdError::NonZeroExit { code })
                }
            }
            Err(e) => {
                error!("Failed to execute systemctl disable: {}", e);
                Err(SystemdError::CommandFailed(e))
            }
        }
    }

    /// Reload systemd daemon configuration
    pub fn daemon_reload(&self) -> Result<()> {
        if !self.is_available() {
            return Err(SystemdError::SystemdNotAvailable);
        }

        let mut cmd = Command::new("systemctl");
        if self.use_user_mode {
            cmd.arg("--user");
        }
        cmd.arg("daemon-reload");

        debug!("Reloading systemd daemon");

        match cmd.status() {
            Ok(status) => {
                if status.success() {
                    debug!("Successfully reloaded systemd daemon");
                    Ok(())
                } else {
                    let code = status.code().unwrap_or(-1);
                    error!("Failed to reload systemd daemon: exit code {}", code);
                    Err(SystemdError::NonZeroExit { code })
                }
            }
            Err(e) => {
                error!("Failed to execute systemctl daemon-reload: {}", e);
                Err(SystemdError::CommandFailed(e))
            }
        }
    }
}

impl Default for SystemdHelper {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_unit_status_from_str() {
        assert_eq!(UnitStatus::from("active"), UnitStatus::Active);
        assert_eq!(UnitStatus::from("inactive"), UnitStatus::Inactive);
        assert_eq!(UnitStatus::from("failed"), UnitStatus::Failed);
        assert_eq!(UnitStatus::from("activating"), UnitStatus::Activating);
        assert_eq!(UnitStatus::from("deactivating"), UnitStatus::Deactivating);
        assert_eq!(UnitStatus::from("unknown"), UnitStatus::Unknown("unknown".to_string()));
    }

    #[test]
    fn test_unit_status_display() {
        assert_eq!(format!("{}", UnitStatus::Active), "active");
        assert_eq!(format!("{}", UnitStatus::Inactive), "inactive");
        assert_eq!(format!("{}", UnitStatus::Failed), "failed");
        assert_eq!(format!("{}", UnitStatus::Activating), "activating");
        assert_eq!(format!("{}", UnitStatus::Deactivating), "deactivating");
        assert_eq!(format!("{}", UnitStatus::Unknown("test".to_string())), "test");
    }

    #[test]
    fn test_systemd_helper_creation() {
        let helper = SystemdHelper::new();
        assert!(!helper.use_user_mode);

        let user_helper = SystemdHelper::new_user();
        assert!(user_helper.use_user_mode);
    }

    #[test]
    fn test_default_systemd_helper() {
        let helper = SystemdHelper::default();
        assert!(!helper.use_user_mode);
    }
}
