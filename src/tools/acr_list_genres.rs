use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use clap::{Arg, Command};
use serde_json::Value;

fn print_usage() {
    println!("list-genres: Lists all genres from the artist metadata cache");
    println!();
    println!("Shows each genre with the number of artists that have it, sorted by");
    println!("artist count in descending order. Comparison is case-insensitive.");
    println!();
    println!("Usage:");
    println!("  audiocontrol_list_genres [OPTIONS]");
    println!();
    println!("Options:");
    println!("  -c, --config <FILE>    Configuration file path");
    println!("                         (default: /etc/audiocontrol/audiocontrol.json)");
    println!("  -h, --help             Show this help message");
    println!();
    println!("Example output:");
    println!("  hip-hop: 10");
    println!("  metal: 20");
    println!("  test: 4");
}

fn load_config(config_path: &str) -> Result<Value, Box<dyn std::error::Error>> {
    let config_content = fs::read_to_string(config_path)
        .map_err(|e| format!("Failed to read config file '{}': {}", config_path, e))?;
    
    let config: Value = serde_json::from_str(&config_content)
        .map_err(|e| format!("Failed to parse config file '{}': {}", config_path, e))?;
    
    Ok(config)
}

fn get_cache_path_from_config(config: &Value) -> String {
    // Try to get the attribute cache path from configuration
    if let Some(cache_config) = config.get("cache") {
        if let Some(cache_path) = cache_config
            .get("attribute_cache_path")
            .and_then(|v| v.as_str())
        {
            return cache_path.to_string();
        }
    }
    
    // Default path if not found in config
    "/var/lib/audiocontrol/cache/attributes".to_string()
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let matches = Command::new("audiocontrol_list_genres")
        .about("Lists all genres from the artist metadata cache")
        .arg(
            Arg::new("config")
                .short('c')
                .long("config")
                .value_name("FILE")
                .help("Configuration file path")
                .default_value("/etc/audiocontrol/audiocontrol.json")
        )
        .arg(
            Arg::new("help")
                .short('h')
                .long("help")
                .help("Show help message")
                .action(clap::ArgAction::Help)
        )
        .get_matches();

    let config_path = matches.get_one::<String>("config").unwrap();
    
    // Load configuration to determine cache path
    let cache_path = if PathBuf::from(config_path).exists() {
        match load_config(config_path) {
            Ok(config) => get_cache_path_from_config(&config),
            Err(e) => {
                eprintln!("Warning: {}", e);
                eprintln!("Using default cache path.");
                "/var/lib/audiocontrol/cache/attributes".to_string()
            }
        }
    } else {
        eprintln!("Warning: Config file '{}' not found. Using default cache path.", config_path);
        "/var/lib/audiocontrol/cache/attributes".to_string()
    };

    let db_path = PathBuf::from(&cache_path);
    
    // Check if the directory exists
    if !db_path.exists() {
        eprintln!("Error: Cache directory does not exist: {}", cache_path);
        eprintln!("Make sure audiocontrol has been running and has populated the cache.");
        return Err(format!("Cache directory not found: {}", cache_path).into());
    }

    // Check if this looks like a Sled database
    let conf_path = db_path.join("conf");
    if !conf_path.exists() {
        eprintln!("Error: Not a valid Sled database (missing 'conf' file): {}", cache_path);
        return Err(format!("Not a valid Sled database at: {}", cache_path).into());
    }

    // Open the database
    let db = sled::open(&db_path)
        .map_err(|e| format!("Failed to open cache database: {}", e))?;

    // HashMap to count genres (case-insensitive)
    let mut genre_counts: HashMap<String, u32> = HashMap::new();
    let mut processed_artists = 0;
    let mut artists_with_genres = 0;

    // Iterate through all entries in the cache
    for result in db.iter() {
        match result {
            Ok((key_bytes, value_bytes)) => {
                // Convert key to string if possible
                let key = match std::str::from_utf8(&key_bytes) {
                    Ok(s) => s,
                    Err(_) => continue, // Skip binary keys
                };

                // Only process artist metadata entries
                if !key.starts_with("artist::metadata::") {
                    continue;
                }

                processed_artists += 1;

                // Try to parse the cached artist metadata
                match serde_json::from_slice::<Value>(&value_bytes) {
                    Ok(artist_data) => {
                        // Check if this artist has genres directly in the root level
                        if let Some(genres_array) = artist_data.get("genres") {
                            if let Some(genres) = genres_array.as_array() {
                                if !genres.is_empty() {
                                    artists_with_genres += 1;
                                }
                                
                                // Count each genre (case-insensitive)
                                for genre_value in genres {
                                    if let Some(genre_str) = genre_value.as_str() {
                                        if !genre_str.trim().is_empty() {
                                            // Convert to lowercase for case-insensitive comparison
                                            let genre_lower = genre_str.trim().to_lowercase();
                                            *genre_counts.entry(genre_lower).or_insert(0) += 1;
                                        }
                                    }
                                }
                            }
                        }
                    }
                    Err(_) => {
                        // Skip entries that can't be parsed as JSON
                        continue;
                    }
                }
            }
            Err(e) => {
                eprintln!("Warning: Error reading cache entry: {}", e);
                continue;
            }
        }
    }

    // Sort genres by count (descending) and then by name (ascending) for stable ordering
    let total_unique_genres = genre_counts.len();
    let mut sorted_genres: Vec<(String, u32)> = genre_counts.into_iter().collect();
    sorted_genres.sort_by(|a, b| {
        // First sort by count (descending)
        match b.1.cmp(&a.1) {
            std::cmp::Ordering::Equal => {
                // If counts are equal, sort by genre name (ascending)
                a.0.cmp(&b.0)
            }
            other => other,
        }
    });

    // Display results
    if sorted_genres.is_empty() {
        println!("No genres found in the cache.");
        if processed_artists == 0 {
            println!("The cache appears to be empty or contains no artist data.");
        } else {
            println!("Processed {} artist entries, but none had genre metadata.", processed_artists);
            println!("This might indicate that genre enrichment hasn't been performed yet.");
        }
    } else {
        for (genre, count) in sorted_genres {
            println!("{}: {}", genre, count);
        }
        
        // Print summary
        eprintln!();
        eprintln!("Summary:");
        eprintln!("  Total unique genres: {}", total_unique_genres);
        eprintln!("  Artists with genres: {}", artists_with_genres);
        eprintln!("  Total artists processed: {}", processed_artists);
    }

    Ok(())
}
