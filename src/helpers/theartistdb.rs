use std::sync::atomic::{AtomicBool, Ordering};
use log::{info, debug, warn, error};
use lazy_static::lazy_static;
use reqwest;
use std::sync::Mutex;
use serde_json::{Value, json};
use std::time::Duration;

/// Global flag to indicate if TheArtistDB lookups are enabled
static THEARTISTDB_ENABLED: AtomicBool = AtomicBool::new(false);

/// API key storage for TheArtistDB
#[derive(Default)]
struct TheArtistDBConfig {
    api_key: String,
}

// Global singleton for TheArtistDB configuration
lazy_static! {
    static ref THEARTISTDB_CONFIG: Mutex<TheArtistDBConfig> = Mutex::new(TheArtistDBConfig::default());
}

/// Initialize TheArtistDB module from configuration
pub fn initialize_from_config(config: &serde_json::Value) {
    if let Some(artistdb_config) = config.get("theartistdb") {
        // Check if enabled flag exists and is set to true
        let enabled = artistdb_config.get("enable")
            .and_then(|v| v.as_bool())
            .unwrap_or(true); // Default to enabled if not specified
        
        THEARTISTDB_ENABLED.store(enabled, Ordering::SeqCst);
        
        // Get API key if provided
        if let Some(api_key) = artistdb_config.get("api_key").and_then(|v| v.as_str()) {
            if let Ok(mut config) = THEARTISTDB_CONFIG.lock() {
                config.api_key = api_key.to_string();
                if !api_key.is_empty() {
                    info!("TheArtistDB API key configured");
                } else {
                    warn!("Empty TheArtistDB API key provided");
                }
            } else {
                error!("Failed to acquire lock on TheArtistDB configuration");
            }
        } else {
            warn!("No API key found for TheArtistDB in configuration");
        }
        
        let status = if enabled { "enabled" } else { "disabled" };
        info!("TheArtistDB lookup {}", status);
    } else {
        // Default to disabled if not in config
        THEARTISTDB_ENABLED.store(false, Ordering::SeqCst);
        debug!("TheArtistDB configuration not found, lookups disabled");
    }
}

/// Check if TheArtistDB lookups are enabled
pub fn is_enabled() -> bool {
    THEARTISTDB_ENABLED.load(Ordering::SeqCst)
}

/// Get the configured API key
pub fn get_api_key() -> Option<String> {
    if let Ok(config) = THEARTISTDB_CONFIG.lock() {
        if config.api_key.is_empty() {
            None
        } else {
            Some(config.api_key.clone())
        }
    } else {
        None
    }
}

/// Look up artist information from TheArtistDB by MusicBrainz ID
/// 
/// # Arguments
/// * `mbid` - MusicBrainz ID of the artist to look up
/// 
/// # Returns
/// * `Result<serde_json::Value, String>` - Artist information or error message
pub fn lookup_mbid(mbid: &str) -> Result<serde_json::Value, String> {
    if !is_enabled() {
        return Err("TheArtistDB lookups are disabled".to_string());
    }
    
    let api_key = match get_api_key() {
        Some(key) => {
            if key.is_empty() {
                return Err("No API key configured for TheArtistDB".to_string());
            }
            key
        },
        None => return Err("No API key configured for TheArtistDB".to_string()),
    };

    debug!("Looking up artist with MBID {}", mbid);
    
    // Construct the API URL
    let url = format!(
        "https://www.theaudiodb.com/api/v1/json/{}/artist-mb.php?i={}", 
        api_key, 
        mbid
    );
    
    // Create a client with a reasonable timeout
    let client = match reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(10))
        .build() {
            Ok(c) => c,
            Err(e) => return Err(format!("Failed to create HTTP client: {}", e)),
        };
    
    // Make the request
    debug!("Making request to TheArtistDB API for MBID {}", mbid);
    let response = match client.get(&url).send() {
        Ok(resp) => resp,
        Err(e) => return Err(format!("Failed to send request to TheArtistDB: {}", e)),
    };
    
    // Check if the request was successful
    if !response.status().is_success() {
        return Err(format!(
            "TheArtistDB API returned error code: {}", 
            response.status()
        ));
    }
    
    // Parse the response as JSON
    match response.json::<Value>() {
        Ok(json_data) => {
            // Check if the artists array exists and is not empty
            if let Some(artists) = json_data.get("artists") {
                if artists.is_null() || (artists.as_array().map_or(true, |a| a.is_empty())) {
                    debug!("No artist found with MBID {}", mbid);
                    return Err(format!("No artist found with MBID {}", mbid));
                }
                
                debug!("Successfully retrieved artist data for MBID {}", mbid);
                Ok(json_data)
            } else {
                debug!("Invalid response format from TheArtistDB");
                Ok(json!({ "artists": null }))
            }
        },
        Err(e) => Err(format!("Failed to parse TheArtistDB response: {}", e))
    }
}

