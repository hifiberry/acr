//! External metadata providers, cover art, accounts and the caches that
//! serve them. Knows nothing about players.

pub mod albumupdater;
pub mod artist_store;
pub mod artistsplitter;
pub mod artistupdater;
pub mod coverart;
pub mod coverart_providers;
pub mod external_coverart;
pub mod fanarttv;
pub mod favourites;
pub mod image_meta;
pub mod lastfm;
pub mod lastfm_worker;
pub mod library_enricher;
pub mod musicbrainz;
pub mod now_playing;
pub mod resolver;
pub mod security_store;
pub mod spotify;
pub mod theaudiodb;
pub mod title_order;
pub mod api;
pub mod secrets;

use acr_types::config::get_service_config;
use acr_types::Artist;
use log::{debug, info, warn};

/// Trait for services that can update artist metadata.
pub trait ArtistUpdater {
    /// Update an artist with additional metadata from a service
    ///
    /// # Arguments
    /// * `artist` - The artist to update
    ///
    /// # Returns
    /// The updated artist with additional metadata
    fn update_artist(&self, artist: Artist) -> Artist;
}

/// Bring up the metadata providers that read one configuration document.
///
/// Phase 0 runs this crate inside the player daemon, so this is what `main`
/// calls in place of the six `initialize_*` helpers it used to hold. Phase 1
/// calls the same function from the metadata daemon's own `main`, against the
/// same configuration keys.
///
/// The order is the order `main` used, and it is load-bearing in one place
/// worth naming: `Spotify::set_global_config` has to run before
/// `initialize_spotify`, because the client reads the stored document while it
/// initialises.
///
/// Three other pieces of metadata start-up deliberately stay at their own call
/// sites in `main` rather than joining this function, because moving them
/// would change *when* they run relative to code that is not in this crate:
/// the security store comes before the attribute cache and the settings
/// database, the favourite providers come after the settings database and the
/// volume control, and the cover art providers are registered only once the
/// `AudioController` exists.
pub fn initialize_in_process(config: &serde_json::Value) {
    initialize_musicbrainz(config);
    initialize_theaudiodb(config);
    initialize_fanarttv(config);
    initialize_external_coverart(config);
    initialize_lastfm(config);
    if let Some(spotify_config) = get_service_config(config, "spotify") {
        spotify::Spotify::set_global_config(spotify_config);
    }
    initialize_spotify(config);
}

// Helper function to initialize MusicBrainz
fn initialize_musicbrainz(config: &serde_json::Value) {
    musicbrainz::initialize_from_config(config);
    info!("MusicBrainz initialized successfully");
}

// Helper function to initialize TheAudioDB
fn initialize_theaudiodb(config: &serde_json::Value) {
    theaudiodb::initialize_from_config(config);
    info!("TheAudioDB initialized successfully");
}

// Helper function to initialize external cover art endpoints
fn initialize_external_coverart(config: &serde_json::Value) {
    external_coverart::initialize_from_config(config);
    info!("External cover art initialized successfully");
}

// Helper function to initialize FanArt.tv
fn initialize_fanarttv(config: &serde_json::Value) {
    fanarttv::initialize_from_config(config);
    info!("FanArt.tv initialized successfully");
}

// Helper function to initialize Last.fm
fn initialize_lastfm(config: &serde_json::Value) {
    if let Some(lastfm_config) = get_service_config(config, "lastfm") {
        // Check if enabled flag exists and is set to true
        let enabled = lastfm_config
            .get("enable")
            .and_then(|v| v.as_bool())
            .unwrap_or(false); // Default to disabled if not specified

        if enabled {
            // Initialize with default API credentials
            if let Err(e) = lastfm::LastfmClient::initialize_with_defaults() {
                warn!("Failed to initialize Last.fm client: {}", e);
                return;
            }

            // Log Last.fm connection status
            match lastfm::LastfmClient::get_instance() {
                Ok(client) => {
                    if client.is_authenticated() {
                        if let Some(username) = client.get_username() {
                            info!("Last.fm connected as user: {}", username);
                        } else {
                            // This case should ideally not happen if is_authenticated is true
                            warn!("Last.fm is authenticated but username is not available.");
                        }
                    } else {
                        info!("Last.fm is not connected. User needs to authenticate.");
                    }
                }
                Err(e) => {
                    // This might happen if initialization failed silently or was never called
                    warn!(
                        "Could not get Last.fm client instance to check status: {}",
                        e
                    );
                }
            }
            info!("Last.fm initialized successfully"); // This message might be redundant now or could be rephrased
        } else {
            info!("Last.fm integration is disabled");
        }
    } else {
        debug!("No Last.fm configuration found, Last.fm features will be unavailable.");
    }
}

// Helper function to initialize Spotify
fn initialize_spotify(config: &serde_json::Value) {
    info!("Starting Spotify initialization");

    if let Some(spotify_config) = get_service_config(config, "spotify") {
        // Check if enabled flag exists and is set to true
        let enabled = spotify_config
            .get("enable")
            .and_then(|v| v.as_bool())
            .unwrap_or(false); // Default to disabled if not specified

        info!("Spotify enabled in config: {}", enabled);

        if enabled {
            // Get custom OAuth URL and proxy secret if specified in config
            let oauth_url = spotify_config
                .get("oauth_url")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());

            let proxy_secret = spotify_config
                .get("proxy_secret")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());

            info!(
                "Config values - OAuth URL present: {}, proxy secret present: {}",
                oauth_url.is_some(),
                proxy_secret.is_some()
            );

            // Initialize with values from config or fall back to defaults
            let init_result = match (oauth_url, proxy_secret) {
                (Some(url), Some(secret)) if !url.is_empty() && !secret.is_empty() => {
                    info!(
                        "Initializing Spotify with configuration from audiocontrol.json, URL: '{}'",
                        url
                    );
                    spotify::Spotify::initialize(url, secret)
                }
                _ => {
                    info!(
                        "No valid Spotify config in audiocontrol.json, falling back to secrets.txt"
                    );
                    spotify::Spotify::initialize_with_defaults()
                }
            };
            if let Err(e) = init_result {
                warn!("Failed to initialize Spotify client: {}", e);

                // Additional logging to help diagnose the issue
                info!(
                    "Checking default OAuth URL directly: '{}'",
                    spotify::default_spotify_oauth_url()
                );

                return;
            }

            // Log Spotify connection status
            match spotify::Spotify::get_instance() {
                Ok(client) => {
                    if client.has_valid_tokens() {
                        info!("Spotify is connected with valid tokens");
                    } else {
                        info!("Spotify is not connected. User needs to authenticate.");
                    }
                }
                Err(e) => {
                    warn!(
                        "Could not get Spotify client instance to check status: {}",
                        e
                    );
                }
            }
            info!("Spotify initialized successfully");
        } else {
            info!("Spotify integration is disabled");
        }
    } else {
        debug!("No Spotify configuration found, Spotify features will be unavailable.");
    }
}

/// Print the status of the secrets compiled into this binary.
///
/// This is what `audiocontrol --check-secrets` prints. It reports on the
/// metadata crate's own compiled-in credentials, so it lives with them.
pub fn check_secrets_status() {
    println!("AudioControl - Compiled Secrets Status");
    println!("=====================================");

    // Get all compiled secrets
    let secrets_map = secrets::get_all_secrets_obfuscated();

    if secrets_map.is_empty() {
        println!("❌ No secrets compiled into binary");
        println!("   This binary was compiled without any secrets configured.");
        println!("   External API integrations will not work unless configured at runtime.");
        return;
    }

    println!("✅ Secrets compiled into binary: {}", secrets_map.len());
    println!();

    // Check specific known secrets
    let known_secrets = vec![
        ("LASTFM_APIKEY", "Last.fm API integration"),
        ("LASTFM_API_KEY", "Last.fm API integration"),
        ("LASTFM_APISECRET", "Last.fm API secret"),
        ("LASTFM_API_SECRET", "Last.fm API secret"),
        ("ARTISTDB_APIKEY", "TheAudioDB API integration"),
        ("THEAUDIODB_APIKEY", "TheAudioDB API integration"),
        ("THEAUDIODB_API_KEY", "TheAudioDB API integration"),
        ("SECRETS_ENCRYPTION_KEY", "Security store encryption"),
        ("SECURITY_KEY", "Security store encryption"),
        ("SPOTIFY_OAUTH_URL", "Spotify OAuth integration"),
        ("SPOTIFY_PROXY_SECRET", "Spotify proxy authentication"),
    ];

    println!("Known Integration Status:");
    println!("------------------------");

    let mut found_any = false;
    for (key, description) in known_secrets {
        if secrets_map.contains_key(key) {
            println!("✅ {} - {}", key, description);
            found_any = true;
        }
    }

    if !found_any {
        println!("⚠️  No known integration secrets found");
        println!(
            "   Available keys: {}",
            secrets_map.keys().cloned().collect::<Vec<_>>().join(", ")
        );
    }

    println!();
    println!("API Service Status:");
    println!("------------------");

    // Test specific service functions
    let lastfm_key = secrets::lastfm_api_key();
    let audiodb_key = secrets::artistdb_api_key();
    let encryption_key = secrets::secrets_encryption_key();
    let spotify_oauth = secrets::spotify_oauth_url();
    let spotify_secret = secrets::spotify_proxy_secret();

    println!(
        "🔑 Last.fm API: {}",
        if lastfm_key != "unknown" {
            "✅ Available"
        } else {
            "❌ Not configured"
        }
    );
    println!(
        "🔑 TheAudioDB API: {}",
        if audiodb_key != "unknown" {
            "✅ Available"
        } else {
            "❌ Not configured"
        }
    );
    println!(
        "🔑 Security Store: {}",
        if encryption_key != "unknown" {
            "✅ Available"
        } else {
            "❌ Not configured"
        }
    );
    println!(
        "🔑 Spotify OAuth: {}",
        if spotify_oauth != "unknown" {
            "✅ Available"
        } else {
            "❌ Not configured"
        }
    );
    println!(
        "🔑 Spotify Proxy: {}",
        if spotify_secret != "unknown" {
            "✅ Available"
        } else {
            "❌ Not configured"
        }
    );

    println!();
    println!("Note: This shows compile-time secrets only. Runtime configuration");
    println!("      may override these values or provide additional secrets.");
}
