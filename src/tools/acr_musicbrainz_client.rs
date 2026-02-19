use clap::{Arg, Command};
use log::{error, info, warn};
use std::fs;

use audiocontrol::helpers::musicbrainz::{self, is_mbid};
use audiocontrol::helpers::artistsplitter::DEFAULT_ARTIST_SEPARATORS;

fn main() {
    // Initialize logging
    env_logger::init();

    // Setup command line interface
    let matches = Command::new("acr_musicbrainz_client")
        .about("MusicBrainz API client for direct API calls")
        .version("1.0")
        .arg(
            Arg::new("config")
                .short('c')
                .long("config")
                .value_name("FILE")
                .help("Configuration file path")
                .required(false)
                .default_value("/etc/audiocontrol/audiocontrol.json"),
        )
        .arg(
            Arg::new("artist-name")
                .short('a')
                .long("artist-name")
                .value_name("NAME")
                .help("Look up artist by name")
                .required(false),
        )
        .arg(
            Arg::new("artist-mbid")
                .short('m')
                .long("artist-mbid")
                .value_name("MBID")
                .help("Look up artist by MusicBrainz ID")
                .required(false),
        )
        .arg(
            Arg::new("album-mbid")
                .short('b')
                .long("album-mbid")
                .value_name("MBID")
                .help("Look up album by MusicBrainz ID")
                .required(false),
        )
        .arg(
            Arg::new("split")
                .short('s')
                .long("split")
                .value_name("ARTIST_NAME")
                .help("Check if artist name contains multiple artists (comma-separated)")
                .required(false),
        )
        .arg(
            Arg::new("verbose")
                .short('v')
                .long("verbose")
                .help("Enable verbose output")
                .required(false)
                .action(clap::ArgAction::SetTrue),
        )
        .get_matches();

    // Load configuration
    let config_path = matches.get_one::<String>("config").unwrap();
    println!("Loading configuration from: {}", config_path);
    
    let config = match load_config_file(config_path) {
        Ok(config) => {
            println!("Configuration loaded successfully");
            config
        }
        Err(e) => {
            error!("Failed to load configuration: {}", e);
            std::process::exit(1);
        }
    };
    
    // Initialize MusicBrainz module
    musicbrainz::initialize_from_config(&config);
    
    // Check if MusicBrainz is enabled
    if !musicbrainz::is_enabled() {
        warn!("MusicBrainz lookups are disabled in configuration");
        println!("Warning: MusicBrainz lookups are disabled in configuration");
        println!("Enable MusicBrainz in your configuration file to use this tool.");
        std::process::exit(1);
    } else {
        info!("MusicBrainz lookups are enabled");
        println!("MusicBrainz lookups are enabled");
    }
    
    // Check which operation to perform
    let verbose = matches.get_flag("verbose");
    if verbose {
        println!("Verbose mode enabled");
    }
    
    // Handle artist name lookup
    if let Some(artist_name) = matches.get_one::<String>("artist-name") {
        lookup_artist_by_name(artist_name, verbose);
        return;
    }
    
    // Handle artist MBID lookup  
    if let Some(artist_mbid) = matches.get_one::<String>("artist-mbid") {
        lookup_artist_by_mbid(artist_mbid, verbose);
        return;
    }
    
    // Handle album MBID lookup
    if let Some(album_mbid) = matches.get_one::<String>("album-mbid") {
        lookup_album_by_mbid(album_mbid, verbose);
        return;
    }
    
    // Handle artist name splitting
    if let Some(split_name) = matches.get_one::<String>("split") {
        split_artist_name(split_name, verbose);
        return;
    }
    
    // If no specific operation was requested, show help
    println!("No operation specified. Use --help for available options.");
    show_examples();
}

fn load_config_file(config_path: &str) -> Result<serde_json::Value, String> {
    let config_content = fs::read_to_string(config_path)
        .map_err(|e| format!("Failed to read config file '{}': {}", config_path, e))?;
    
    serde_json::from_str(&config_content)
        .map_err(|e| format!("Failed to parse config file '{}': {}", config_path, e))
}

fn lookup_artist_by_name(artist_name: &str, verbose: bool) {
    println!("\n=== Artist Name Lookup ===");
    println!("Artist: {}", artist_name);
    println!("Making direct API call to MusicBrainz...");
    
    if verbose {
        println!("Artist separators checked: {:?}", DEFAULT_ARTIST_SEPARATORS);
    }
    
    // Check if the artist name contains separators
    let contains_separators = DEFAULT_ARTIST_SEPARATORS.iter().any(|sep| artist_name.contains(sep));
    if contains_separators {
        println!("Note: Artist name contains potential separators: {}", 
                 DEFAULT_ARTIST_SEPARATORS.iter()
                     .filter(|sep| artist_name.contains(*sep))
                     .map(|s| format!("'{}'", s))
                     .collect::<Vec<_>>()
                     .join(", "));
    }
    
    // Make direct API call using ureq
    match make_artist_search_api_call(artist_name, verbose) {
        Ok(response) => {
            println!("✓ API call successful");
            if verbose {
                println!("Raw response: {}", response);
            }
            parse_and_display_artist_search_response(&response, artist_name, verbose);
        }
        Err(e) => {
            println!("✗ API call failed: {}", e);
        }
    }
}

fn lookup_artist_by_mbid(mbid: &str, verbose: bool) {
    println!("\n=== Artist MBID Lookup ===");
    println!("MBID: {}", mbid);
    
    // Validate MBID format
    if !is_mbid(mbid) {
        println!("✗ Invalid MBID format. MusicBrainz IDs should be in UUID format:");
        println!("  Example: 5b11f4ce-a62d-471e-81fc-a69a8278c7da");
        return;
    }
    
    println!("✓ MBID format is valid");
    println!("Making direct API call to MusicBrainz...");
    
    if verbose {
        println!("MusicBrainz URL: https://musicbrainz.org/artist/{}", mbid);
    }
    
    // Make direct API call
    match make_artist_lookup_api_call(mbid, verbose) {
        Ok(response) => {
            println!("✓ API call successful");
            if verbose {
                println!("Raw response: {}", response);
            }
            parse_and_display_artist_lookup_response(&response, mbid, verbose);
        }
        Err(e) => {
            println!("✗ API call failed: {}", e);
        }
    }
}

fn lookup_album_by_mbid(mbid: &str, verbose: bool) {
    println!("\n=== Album MBID Lookup ===");
    println!("MBID: {}", mbid);
    
    // Validate MBID format
    if !is_mbid(mbid) {
        println!("✗ Invalid MBID format. MusicBrainz IDs should be in UUID format:");
        println!("  Example: 5b11f4ce-a62d-471e-81fc-a69a8278c7da");
        return;
    }
    
    println!("✓ MBID format is valid");
    println!("Making direct API call to MusicBrainz...");
    
    if verbose {
        println!("MusicBrainz URL: https://musicbrainz.org/release/{}", mbid);
    }
    
    // Make direct API call
    match make_album_lookup_api_call(mbid, verbose) {
        Ok(response) => {
            println!("✓ API call successful");
            if verbose {
                println!("Raw response: {}", response);
            }
            parse_and_display_album_lookup_response(&response, mbid, verbose);
        }
        Err(e) => {
            println!("✗ API call failed: {}", e);
        }
    }
}

fn split_artist_name(artist_name: &str, verbose: bool) {
    println!("\n=== Artist Name Splitting ===");
    println!("Artist: {}", artist_name);
    
    // Check for separators
    let found_separators: Vec<&str> = DEFAULT_ARTIST_SEPARATORS.iter()
        .filter(|sep| artist_name.contains(*sep))
        .cloned()
        .collect();
    
    if found_separators.is_empty() {
        println!("✓ No separators found in artist name");
        println!("  Separators checked: {:?}", DEFAULT_ARTIST_SEPARATORS);
        return;
    }
    
    println!("ℹ Found separators: {:?}", found_separators);
    
    // Split the artist name using the found separators
    let split_artists = split_artist_with_separators(artist_name, &found_separators);
    
    if split_artists.len() > 1 {
        println!("✓ Artist name appears to contain multiple artists:");
        for (i, artist) in split_artists.iter().enumerate() {
            println!("  {}: {}", i + 1, artist.trim());
        }
        
        if verbose {
            println!("\nTesting individual artists with direct API calls:");
            for artist in &split_artists {
                let trimmed_artist = artist.trim();
                println!("  Checking '{}':", trimmed_artist);
                match make_artist_search_api_call(trimmed_artist, false) {
                    Ok(response) => {
                        let found = !response.contains("\"artists\":[]");
                        if found {
                            println!("    ✓ Found in MusicBrainz");
                        } else {
                            println!("    ✗ Not found in MusicBrainz");
                        }
                    }
                    Err(e) => {
                        println!("    ✗ API error: {}", e);
                    }
                }
            }
        }
    } else {
        println!("✗ Artist name does not appear to contain multiple artists");
    }
}

fn make_artist_search_api_call(artist_name: &str, verbose: bool) -> Result<String, String> {
    let encoded_name = urlencoding::encode(artist_name);
    let url = format!(
        "https://musicbrainz.org/ws/2/artist?query=artist:{}&fmt=json&limit=3",
        encoded_name
    );
    
    if verbose {
        println!("API URL: {}", url);
    }
    
    let response = ureq::get(&url)
        .set("User-Agent", "HifiBerry-ACR/1.0 (https://www.hifiberry.com/)")
        .call()
        .map_err(|e| format!("HTTP request failed: {}", e))?
        .into_string()
        .map_err(|e| format!("Failed to read response: {}", e))?;
    
    Ok(response)
}

fn make_artist_lookup_api_call(mbid: &str, verbose: bool) -> Result<String, String> {
    let url = format!("https://musicbrainz.org/ws/2/artist/{}?fmt=json", mbid);
    
    if verbose {
        println!("API URL: {}", url);
    }
    
    let response = ureq::get(&url)
        .set("User-Agent", "HifiBerry-ACR/1.0 (https://www.hifiberry.com/)")
        .call()
        .map_err(|e| format!("HTTP request failed: {}", e))?
        .into_string()
        .map_err(|e| format!("Failed to read response: {}", e))?;
    
    Ok(response)
}

fn make_album_lookup_api_call(mbid: &str, verbose: bool) -> Result<String, String> {
    let url = format!("https://musicbrainz.org/ws/2/release/{}?fmt=json", mbid);
    
    if verbose {
        println!("API URL: {}", url);
    }
    
    let response = ureq::get(&url)
        .set("User-Agent", "HifiBerry-ACR/1.0 (https://www.hifiberry.com/)")
        .call()
        .map_err(|e| format!("HTTP request failed: {}", e))?
        .into_string()
        .map_err(|e| format!("Failed to read response: {}", e))?;
    
    Ok(response)
}

fn parse_and_display_artist_search_response(response: &str, artist_name: &str, verbose: bool) {
    match serde_json::from_str::<serde_json::Value>(response) {
        Ok(json) => {
            if let Some(artists) = json.get("artists").and_then(|a| a.as_array()) {
                if artists.is_empty() {
                    println!("✗ No artists found for '{}'", artist_name);
                } else {
                    println!("✓ Found {} artist(s):", artists.len());
                    for (i, artist) in artists.iter().enumerate() {
                        let name = artist.get("name").and_then(|n| n.as_str()).unwrap_or("Unknown");
                        let id = artist.get("id").and_then(|i| i.as_str()).unwrap_or("Unknown");
                        let score = artist.get("score").and_then(|s| s.as_i64()).unwrap_or(0);
                        
                        println!("  {}: {} (MBID: {}, Score: {})", i + 1, name, id, score);
                        
                        if verbose {
                            if let Some(disambiguation) = artist.get("disambiguation").and_then(|d| d.as_str()) {
                                if !disambiguation.is_empty() {
                                    println!("      Disambiguation: {}", disambiguation);
                                }
                            }
                            if let Some(country) = artist.get("country").and_then(|c| c.as_str()) {
                                println!("      Country: {}", country);
                            }
                            println!("      URL: https://musicbrainz.org/artist/{}", id);
                        }
                    }
                }
            } else {
                println!("✗ Invalid response format from MusicBrainz");
            }
        }
        Err(e) => {
            println!("✗ Failed to parse JSON response: {}", e);
            if verbose {
                println!("Raw response: {}", response);
            }
        }
    }
}

fn parse_and_display_artist_lookup_response(response: &str, mbid: &str, verbose: bool) {
    match serde_json::from_str::<serde_json::Value>(response) {
        Ok(json) => {
            let name = json.get("name").and_then(|n| n.as_str()).unwrap_or("Unknown");
            println!("✓ Artist found: {}", name);
            
            if verbose {
                if let Some(disambiguation) = json.get("disambiguation").and_then(|d| d.as_str()) {
                    if !disambiguation.is_empty() {
                        println!("  Disambiguation: {}", disambiguation);
                    }
                }
                if let Some(country) = json.get("country").and_then(|c| c.as_str()) {
                    println!("  Country: {}", country);
                }
                if let Some(type_name) = json.get("type").and_then(|t| t.as_str()) {
                    println!("  Type: {}", type_name);
                }
                if let Some(begin_area) = json.get("begin-area").and_then(|ba| ba.get("name")).and_then(|n| n.as_str()) {
                    println!("  Begin area: {}", begin_area);
                }
                println!("  URL: https://musicbrainz.org/artist/{}", mbid);
            }
        }
        Err(e) => {
            println!("✗ Failed to parse JSON response: {}", e);
            if verbose {
                println!("Raw response: {}", response);
            }
        }
    }
}

fn parse_and_display_album_lookup_response(response: &str, mbid: &str, verbose: bool) {
    match serde_json::from_str::<serde_json::Value>(response) {
        Ok(json) => {
            let title = json.get("title").and_then(|t| t.as_str()).unwrap_or("Unknown");
            println!("✓ Album found: {}", title);
            
            if verbose {
                if let Some(date) = json.get("date").and_then(|d| d.as_str()) {
                    println!("  Date: {}", date);
                }
                if let Some(country) = json.get("country").and_then(|c| c.as_str()) {
                    println!("  Country: {}", country);
                }
                if let Some(status) = json.get("status").and_then(|s| s.as_str()) {
                    println!("  Status: {}", status);
                }
                if let Some(packaging) = json.get("packaging").and_then(|p| p.as_str()) {
                    println!("  Packaging: {}", packaging);
                }
                println!("  URL: https://musicbrainz.org/release/{}", mbid);
            }
        }
        Err(e) => {
            println!("✗ Failed to parse JSON response: {}", e);
            if verbose {
                println!("Raw response: {}", response);
            }
        }
    }
}

fn split_artist_with_separators(artist_name: &str, separators: &[&str]) -> Vec<String> {
    let mut parts = vec![artist_name.to_string()];
    
    for separator in separators {
        let mut new_parts = Vec::new();
        for part in parts {
            if part.contains(separator) {
                new_parts.extend(part.split(separator).map(|s| s.to_string()));
            } else {
                new_parts.push(part);
            }
        }
        parts = new_parts;
    }
    
    parts.into_iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

fn show_examples() {
    println!("\n=== Usage Examples ===");
    println!();
    println!("# Look up artist by name:");
    println!("audiocontrol_musicbrainz_client --artist-name \"The Beatles\"");
    println!();
    println!("# Look up artist by MusicBrainz ID:");
    println!("audiocontrol_musicbrainz_client --artist-mbid \"b10bbbfc-cf9e-42e0-be17-e2c3e1d2600d\"");
    println!();
    println!("# Look up album by MusicBrainz ID:");
    println!("audiocontrol_musicbrainz_client --album-mbid \"5b11f4ce-a62d-471e-81fc-a69a8278c7da\"");
    println!();
    println!("# Check if artist name contains multiple artists:");
    println!("audiocontrol_musicbrainz_client --split \"John Williams & London Symphony Orchestra\"");
    println!();
    println!("# Verbose output:");
    println!("audiocontrol_musicbrainz_client --artist-name \"Queen\" --verbose");
    println!();
    println!("# Use custom config file:");
    println!("audiocontrol_musicbrainz_client --config /path/to/config.json --artist-name \"Pink Floyd\"");
    println!();
}
