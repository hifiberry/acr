use crate::config::get_service_config;
use log::{debug, info, error};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use ureq;

/// Global configurator API URL
static CONFIGURATOR_URL: RwLock<String> = RwLock::new(String::new());

/// Default configurator URL
const DEFAULT_CONFIGURATOR_URL: &str = "http://localhost:1081";

/// System information response from configurator API
#[derive(Debug, Deserialize, Serialize)]
pub struct SystemInfo {
    #[serde(default)]
    pub pi_model: Option<PiModel>,
    #[serde(default)]
    pub hat_info: Option<HatInfo>,
    #[serde(default)]
    pub soundcard: Option<Soundcard>,
    #[serde(default)]
    pub system: Option<SystemDetails>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct PiModel {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct HatInfo {
    #[serde(default)]
    pub vendor: Option<String>,
    #[serde(default)]
    pub product: Option<String>,
    #[serde(default)]
    pub uuid: Option<String>,
    #[serde(default)]
    pub vendor_card: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Soundcard {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub volume_control: Option<String>,
    #[serde(default)]
    pub hardware_index: Option<u32>,
    #[serde(default)]
    pub output_channels: Option<u32>,
    #[serde(default)]
    pub input_channels: Option<u32>,
    #[serde(default)]
    pub features: Option<Vec<String>>,
    #[serde(default)]
    pub hat_name: Option<String>,
    #[serde(default)]
    pub supports_dsp: Option<bool>,
    #[serde(default)]
    pub card_type: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct SystemDetails {
    #[serde(default)]
    pub uuid: Option<String>,
    #[serde(default)]
    pub hostname: Option<String>,
    #[serde(default)]
    pub pretty_hostname: Option<String>,
}

/// Initialize the configurator helper from configuration
pub fn initialize_from_config(config: &serde_json::Value) {
    if let Some(configurator_config) = get_service_config(config, "configurator") {
        // Get URL if provided, otherwise use default
        let url = configurator_config.get("url")
            .and_then(|v| v.as_str())
            .unwrap_or(DEFAULT_CONFIGURATOR_URL);
        
        {
            let mut url_guard = CONFIGURATOR_URL.write();
            *url_guard = url.to_string();
        }

        info!("Configurator API initialized, URL: {}", url);
    } else {
        // Default URL if not in config
        {
            let mut url_guard = CONFIGURATOR_URL.write();
            *url_guard = DEFAULT_CONFIGURATOR_URL.to_string();
        }
        info!("Configurator configuration not found, using default URL: {}", DEFAULT_CONFIGURATOR_URL);
    }
}

/// Check if configurator API is enabled (always true)
pub fn is_enabled() -> bool {
    true
}

/// Get the configured configurator URL
pub fn get_url() -> String {
    let url_guard = CONFIGURATOR_URL.read();
    if url_guard.is_empty() {
        DEFAULT_CONFIGURATOR_URL.to_string()
    } else {
        url_guard.clone()
    }
}

/// Get system information from configurator API
/// 
/// # Returns
/// * `Result<SystemInfo, String>` - System information or error message
pub fn get_system_info() -> Result<SystemInfo, String> {
    let url = format!("{}/api/v1/systeminfo", get_url());
    
    debug!("Getting system information from configurator API: {}", url);
    
    // Make the HTTP request
    match ureq::get(&url).call() {
        Ok(response) => {
            let status = response.status();
            if status == 200 {
                match response.into_string() {
                    Ok(body) => {
                        debug!("Received system info response: {}", body);
                        match serde_json::from_str::<SystemInfo>(&body) {
                            Ok(system_info) => {
                                debug!("Successfully parsed system info");
                                Ok(system_info)
                            }
                            Err(e) => {
                                error!("Failed to parse system info JSON: {}", e);
                                Err(format!("Failed to parse system info response: {}", e))
                            }
                        }
                    }
                    Err(e) => {
                        error!("Failed to read response body: {}", e);
                        Err(format!("Failed to read response body: {}", e))
                    }
                }
            } else {
                error!("Configurator API returned error status: {}", status);
                Err(format!("Configurator API returned status {}", status))
            }
        }
        Err(e) => {
            error!("Failed to connect to configurator API at {}: {}", url, e);
            Err(format!("Failed to connect to configurator API: {}", e))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use parking_lot::Mutex;

    // Use a mutex to ensure tests run sequentially to avoid state conflicts
    static TEST_MUTEX: Mutex<()> = Mutex::new(());

    #[test]
    fn test_initialize_from_config_with_url() {
        let _guard = TEST_MUTEX.lock();
        
        let config = json!({
            "services": {
                "configurator": {
                    "url": "http://test.example.com:1081"
                }
            }
        });

        initialize_from_config(&config);
        
        assert!(is_enabled()); // Always enabled
        assert_eq!(get_url(), "http://test.example.com:1081");
    }

    #[test]
    fn test_initialize_from_config_default() {
        let _guard = TEST_MUTEX.lock();
        
        let config = json!({
            "services": {}
        });

        initialize_from_config(&config);
        
        assert!(is_enabled()); // Always enabled
        assert_eq!(get_url(), DEFAULT_CONFIGURATOR_URL);
    }

    #[test]
    fn test_is_enabled_always_true() {
        // Configurator is always enabled
        assert!(is_enabled());
    }

    #[test]
    fn test_system_info_deserialization() {
        let json_response = r#"{
            "pi_model": {
                "name": "Raspberry Pi 4 Model B Rev 1.4",
                "version": "4"
            },
            "hat_info": {
                "vendor": "HiFiBerry",
                "product": "DAC+ Pro",
                "uuid": "12345678-1234-1234-1234-123456789abc",
                "vendor_card": "HiFiBerry:DAC+ Pro"
            },
            "soundcard": {
                "name": "DAC+ Pro",
                "volume_control": "Digital",
                "hardware_index": 0,
                "output_channels": 2,
                "input_channels": 0,
                "features": ["usehwvolume"],
                "hat_name": "DAC+ Pro",
                "supports_dsp": false,
                "card_type": ["DAC"]
            },
            "system": {
                "uuid": "abcd1234-5678-90ef-1234-567890abcdef",
                "hostname": "hifiberry-player",
                "pretty_hostname": "HiFiBerry Music Player"
            },
            "status": "success"
        }"#;

        let system_info: SystemInfo = serde_json::from_str(json_response).unwrap();
        
        assert_eq!(system_info.status, Some("success".to_string()));
        assert!(system_info.pi_model.is_some());
        assert!(system_info.hat_info.is_some());
        assert!(system_info.soundcard.is_some());
        assert!(system_info.system.is_some());
        
        let pi_model = system_info.pi_model.unwrap();
        assert_eq!(pi_model.name, Some("Raspberry Pi 4 Model B Rev 1.4".to_string()));
        assert_eq!(pi_model.version, Some("4".to_string()));
        
        let system = system_info.system.unwrap();
        assert_eq!(system.hostname, Some("hifiberry-player".to_string()));
        assert_eq!(system.pretty_hostname, Some("HiFiBerry Music Player".to_string()));
    }

    #[test]
    fn test_system_info_error_deserialization() {
        let json_response = r#"{
            "pi_model": {
                "name": "unknown",
                "version": "unknown"
            },
            "hat_info": {
                "vendor": null,
                "product": null,
                "uuid": null,
                "vendor_card": "unknown:unknown"
            },
            "soundcard": {
                "name": "unknown",
                "volume_control": null,
                "hardware_index": null,
                "output_channels": 0,
                "input_channels": 0,
                "features": [],
                "hat_name": null,
                "supports_dsp": false,
                "card_type": []
            },
            "system": {
                "uuid": null,
                "hostname": null,
                "pretty_hostname": null
            },
            "status": "error",
            "error": "Failed to collect system info"
        }"#;

        let system_info: SystemInfo = serde_json::from_str(json_response).unwrap();
        
        assert_eq!(system_info.status, Some("error".to_string()));
        assert_eq!(system_info.error, Some("Failed to collect system info".to_string()));
        assert!(system_info.pi_model.is_some());
        assert!(system_info.system.is_some());
        
        let system = system_info.system.unwrap();
        assert_eq!(system.hostname, None);
        assert_eq!(system.pretty_hostname, None);
    }
}
