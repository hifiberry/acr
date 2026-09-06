use clap::{Parser, Subcommand};
use serde_json::{json, Value};
use std::error::Error;

#[derive(Parser, Debug)]
#[clap(author, version, about = "AudioControl favourites management tool", long_about = None)]
struct Args {
    /// AudioControl API base URL
    #[clap(long, default_value = "http://localhost:1080")]
    url: String,

    /// Enable verbose output
    #[clap(long, short = 'v', help = "Enable verbose output")]
    verbose: bool,

    /// Suppress all output except errors
    #[clap(long, short = 'q', help = "Quiet mode - suppress all output except errors")]
    quiet: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Check if a song is marked as favourite
    ///
    /// Example: audiocontrol_favourites check --artist "Artist Name" --title "Song Title"
    Check {
        /// Artist name
        #[clap(long, short = 'a', help = "Artist name")]
        artist: String,

        /// Song title
        #[clap(long, short = 't', help = "Song title")]
        title: String,
    },
    /// Add a song to favourites
    ///
    /// Example: audiocontrol_favourites add --artist "Artist Name" --title "Song Title"
    Add {
        /// Artist name
        #[clap(long, short = 'a', help = "Artist name")]
        artist: String,

        /// Song title
        #[clap(long, short = 't', help = "Song title")]
        title: String,
    },
    /// Remove a song from favourites
    ///
    /// Example: audiocontrol_favourites remove --artist "Artist Name" --title "Song Title"
    Remove {
        /// Artist name
        #[clap(long, short = 'a', help = "Artist name")]
        artist: String,

        /// Song title
        #[clap(long, short = 't', help = "Song title")]
        title: String,
    },
    /// List available favourite providers and their status
    ///
    /// Example: audiocontrol_favourites providers
    Providers,
}

fn print_verbose(args: &Args, message: &str) {
    if args.verbose && !args.quiet {
        println!("{}", message);
    }
}

fn print_info(args: &Args, message: &str) {
    if !args.quiet {
        println!("{}", message);
    }
}

/// Make an HTTP GET request
fn http_get(url: &str) -> Result<String, Box<dyn Error>> {
    let response = ureq::get(url).call()?;
    let body = response.into_string()?;
    Ok(body)
}

/// Make an HTTP POST request with JSON data
fn http_post(url: &str, json_data: &Value) -> Result<String, Box<dyn Error>> {
    let response = ureq::post(url)
        .set("Content-Type", "application/json")
        .send_string(&json_data.to_string())?;
    let body = response.into_string()?;
    Ok(body)
}

/// Make an HTTP DELETE request with JSON data
fn http_delete(url: &str, json_data: &Value) -> Result<String, Box<dyn Error>> {
    let response = ureq::request("DELETE", url)
        .set("Content-Type", "application/json")
        .send_string(&json_data.to_string())?;
    let body = response.into_string()?;
    Ok(body)
}

/// Check if a song is favourite
fn check_favourite(args: &Args, artist: &str, title: &str) -> Result<(), Box<dyn Error>> {
    let url = format!("{}/api/favourites/is_favourite?artist={}&title={}", 
                      args.url, 
                      urlencoding::encode(artist), 
                      urlencoding::encode(title));
    
    print_verbose(args, &format!("Checking favourite status at: {}", url));
    
    let response_text = http_get(&url)?;
    print_verbose(args, &format!("Response: {}", response_text));
    
    let response: Value = serde_json::from_str(&response_text)?;
    
    // Handle both direct response and wrapped response formats
    let result = if let Some(ok_result) = response.get("Ok").and_then(|v| v.as_object()) {
        // Response is wrapped in "Ok" field
        ok_result
    } else if let Some(result) = response.as_object() {
        // Direct response format
        result
    } else {
        return Err("Invalid response format".into());
    };
    
    if let Some(is_favourite) = result.get("is_favourite").and_then(|v| v.as_bool()) {
        if is_favourite {
            print_info(args, &format!("✓ '{}' by '{}' is marked as favourite", title, artist));
        } else {
            print_info(args, &format!("✗ '{}' by '{}' is not marked as favourite", title, artist));
        }
        
        if let Some(providers) = result.get("providers").and_then(|v| v.as_array()) {
            print_verbose(args, &format!("Available providers: {:?}", providers));
        }
    } else if let Some(error) = result.get("error").and_then(|v| v.as_str()) {
        return Err(format!("API error: {}", error).into());
    } else {
        return Err("Unexpected response format".into());
    }
    
    Ok(())
}

/// Add a song to favourites
fn add_favourite(args: &Args, artist: &str, title: &str) -> Result<(), Box<dyn Error>> {
    let url = format!("{}/api/favourites/add", args.url);
    let json_data = json!({
        "artist": artist,
        "title": title
    });
    
    print_verbose(args, &format!("Adding favourite at: {}", url));
    print_verbose(args, &format!("Request data: {}", json_data));
    
    let response_text = http_post(&url, &json_data)?;
    print_verbose(args, &format!("Response: {}", response_text));
    
    let response: Value = serde_json::from_str(&response_text)?;
    
    // Handle both direct response and wrapped response formats
    let result = if let Some(ok_result) = response.get("Ok").and_then(|v| v.as_object()) {
        // Response is wrapped in "Ok" field
        ok_result
    } else if let Some(result) = response.as_object() {
        // Direct response format
        result
    } else if let Some(err_result) = response.get("Err").and_then(|v| v.as_object()) {
        // Error response wrapped in "Err" field
        if let Some(error) = err_result.get("error").and_then(|v| v.as_str()) {
            return Err(format!("API error: {}", error).into());
        } else {
            return Err("API returned error".into());
        }
    } else {
        return Err("Invalid response format".into());
    };
    
    if let Some(success) = result.get("success").and_then(|v| v.as_bool()) {
        if success {
            if let Some(message) = result.get("message").and_then(|v| v.as_str()) {
                print_info(args, &format!("✓ {}", message));
            } else {
                print_info(args, &format!("✓ Successfully added '{}' by '{}' to favourites", title, artist));
            }
            
            if let Some(updated_providers) = result.get("updated_providers").and_then(|v| v.as_array()) {
                if !updated_providers.is_empty() {
                    print_verbose(args, &format!("Updated providers: {:?}", updated_providers));
                }
            }
        } else {
            return Err("Failed to add favourite".into());
        }
    } else if let Some(error) = result.get("error").and_then(|v| v.as_str()) {
        return Err(format!("API error: {}", error).into());
    } else {
        return Err("Unexpected response format".into());
    }
    
    Ok(())
}

/// Remove a song from favourites
fn remove_favourite(args: &Args, artist: &str, title: &str) -> Result<(), Box<dyn Error>> {
    let url = format!("{}/api/favourites/remove", args.url);
    let json_data = json!({
        "artist": artist,
        "title": title
    });
    
    print_verbose(args, &format!("Removing favourite at: {}", url));
    print_verbose(args, &format!("Request data: {}", json_data));
    
    let response_text = http_delete(&url, &json_data)?;
    print_verbose(args, &format!("Response: {}", response_text));
    
    let response: Value = serde_json::from_str(&response_text)?;
    
    // Handle both direct response and wrapped response formats
    let result = if let Some(ok_result) = response.get("Ok").and_then(|v| v.as_object()) {
        // Response is wrapped in "Ok" field
        ok_result
    } else if let Some(result) = response.as_object() {
        // Direct response format
        result
    } else if let Some(err_result) = response.get("Err").and_then(|v| v.as_object()) {
        // Error response wrapped in "Err" field
        if let Some(error) = err_result.get("error").and_then(|v| v.as_str()) {
            return Err(format!("API error: {}", error).into());
        } else {
            return Err("API returned error".into());
        }
    } else {
        return Err("Invalid response format".into());
    };
    
    if let Some(success) = result.get("success").and_then(|v| v.as_bool()) {
        if success {
            if let Some(message) = result.get("message").and_then(|v| v.as_str()) {
                print_info(args, &format!("✓ {}", message));
            } else {
                print_info(args, &format!("✓ Successfully removed '{}' by '{}' from favourites", title, artist));
            }
            
            if let Some(updated_providers) = result.get("updated_providers").and_then(|v| v.as_array()) {
                if !updated_providers.is_empty() {
                    print_verbose(args, &format!("Updated providers: {:?}", updated_providers));
                }
            }
        } else {
            return Err("Failed to remove favourite".into());
        }
    } else if let Some(error) = result.get("error").and_then(|v| v.as_str()) {
        return Err(format!("API error: {}", error).into());
    } else {
        return Err("Unexpected response format".into());
    }
    
    Ok(())
}

/// List favourite providers
fn list_providers(args: &Args) -> Result<(), Box<dyn Error>> {
    let url = format!("{}/api/favourites/providers", args.url);
    
    print_verbose(args, &format!("Getting providers at: {}", url));
    
    let response_text = http_get(&url)?;
    print_verbose(args, &format!("Response: {}", response_text));
    
    let response: Value = serde_json::from_str(&response_text)?;
    
    // Handle both direct response and wrapped response formats
    let result = if let Some(ok_result) = response.get("Ok").and_then(|v| v.as_object()) {
        // Response is wrapped in "Ok" field
        ok_result
    } else if let Some(result) = response.as_object() {
        // Direct response format (most likely for this endpoint)
        result
    } else {
        return Err("Invalid response format".into());
    };
    
    if let Some(enabled_count) = result.get("enabled_count").and_then(|v| v.as_u64()) {
        if let Some(total_providers) = result.get("total_providers").and_then(|v| v.as_u64()) {
            print_info(args, &format!("Favourite Providers: {} enabled out of {} total", enabled_count, total_providers));
        }
    }
    
    if let Some(providers) = result.get("providers").and_then(|v| v.as_array()) {
        print_info(args, "");
        for provider in providers {
            if let Some(provider_obj) = provider.as_object() {
                if let (Some(name), Some(display_name), Some(enabled)) = (
                    provider_obj.get("name").and_then(|v| v.as_str()),
                    provider_obj.get("display_name").and_then(|v| v.as_str()),
                    provider_obj.get("enabled").and_then(|v| v.as_bool())
                ) {
                    let status = if enabled { "✓ Enabled" } else { "✗ Disabled" };
                    print_info(args, &format!("  {} ({}): {}", display_name, name, status));
                    
                    if let Some(favorite_count) = provider_obj.get("favorite_count").and_then(|v| v.as_u64()) {
                        print_verbose(args, &format!("    Favourites: {}", favorite_count));
                    }
                }
            }
        }
    }
    
    if let Some(enabled_providers) = result.get("enabled_providers").and_then(|v| v.as_array()) {
        if !enabled_providers.is_empty() {
            print_verbose(args, &format!("\nEnabled provider names: {:?}", enabled_providers));
        }
    }
    
    Ok(())
}

fn main() -> Result<(), Box<dyn Error>> {
    let args = Args::parse();
    
    match &args.command {
        Commands::Check { artist, title } => {
            check_favourite(&args, artist, title)?;
        }
        Commands::Add { artist, title } => {
            add_favourite(&args, artist, title)?;
        }
        Commands::Remove { artist, title } => {
            remove_favourite(&args, artist, title)?;
        }
        Commands::Providers => {
            list_providers(&args)?;
        }
    }
    
    Ok(())
}
