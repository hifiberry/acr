use acr::helpers::lastfm::{LastfmClient, LastfmCredentials, default_lastfm_api_key, default_lastfm_api_secret};
use clap::Parser;
use std::error::Error;
use std::fs::File;
use std::io;
use std::io::{Read, Write};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[clap(author, version, about = "Last.fm Authentication Tool", long_about = None)]
struct Args {
    /// API Key (required for initial authentication)
    #[clap(long)]
    api_key: Option<String>,

    /// API Secret (required for initial authentication)
    #[clap(long)]
    api_secret: Option<String>,

    /// Path to save or load credentials file
    #[clap(long, default_value = "lastfm_credentials.json")]
    credentials_file: PathBuf,

    /// Authenticate with saved credentials
    #[clap(long)]
    use_saved: bool,
    
    /// Use default credentials from the compiled application
    #[clap(long)]
    use_defaults: bool,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let args = Args::parse();

    // Check if we're loading saved credentials
    if args.use_saved {
        println!(
            "Attempting to load saved Last.fm credentials from: {}",
            args.credentials_file.display()
        );

        // Load saved credentials
        let mut file = File::open(&args.credentials_file)
            .map_err(|e| format!("Failed to open credentials file: {}", e))?;

        let mut contents = String::new();
        file.read_to_string(&mut contents)
            .map_err(|e| format!("Failed to read credentials file: {}", e))?;

        let credentials: LastfmCredentials = serde_json::from_str(&contents)
            .map_err(|e| format!("Failed to parse credentials: {}", e))?;

        // Initialize with saved credentials
        LastfmClient::from_credentials(credentials)?;

        // Get client instance
        let client = LastfmClient::get_instance()?;

        if client.is_authenticated() {
            println!("Successfully authenticated with Last.fm!");
            println!(
                "Username: {}",
                client
                    .get_username()
                    .unwrap_or_else(|| "Unknown".to_string())
            );
        } else {
            println!("Credentials loaded but not authenticated!");
            println!("Please run the authentication flow with API key and secret.");
        }
    } else if args.use_defaults {
        // Use default credentials compiled into the application
        println!("Using default Last.fm API credentials from compilation");
        
        // Check if we have non-default credentials
        if default_lastfm_api_key() == "YOUR_API_KEY_HERE" || default_lastfm_api_secret() == "YOUR_API_SECRET_HERE" {
            println!("Error: No valid default credentials found. Please compile with valid credentials or provide them explicitly.");
            println!("Make sure you have a secrets.txt file with LASTFM_APIKEY and LASTFM_APISECRET defined.");
            return Ok(());
        }
        
        println!("API Key: {}", if default_lastfm_api_key().len() > 4 {
            format!("{}...", &default_lastfm_api_key()[0..4])
        } else {
            "Invalid".to_string()
        });

        // Initialize with default credentials
        LastfmClient::initialize_with_defaults()?;
        
        // Get a client instance and continue with authentication
        let mut client = LastfmClient::get_instance()?;
        
        // Start authentication flow
        let auth_url_tuple = client.get_auth_url().await?;
        
        println!("\nTo authenticate with Last.fm, please:");
        println!("1. Visit this URL in your browser: {}", auth_url_tuple.0);
        println!("2. Log in to your Last.fm account if necessary");
        println!("3. Authorize this application");
        println!("4. Return here and press Enter when completed");
        
        // Wait for user to authorize
        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        
        // Now try to get the session key
        match client.get_session() {
            Ok((session_key, username)) => {
                println!("\nAuthentication successful!");
                println!("Username: {}", username);
                println!("Session key: {}", session_key);
                
                // Save credentials to file
                let credentials = client.get_credentials();
                let json = serde_json::to_string_pretty(&credentials)?;
                
                let mut file = File::create(&args.credentials_file)?;
                file.write_all(json.as_bytes())?;
                
                println!(
                    "\nCredentials saved to: {}",
                    args.credentials_file.display()
                );
                println!("You can use these credentials in the future with --use-saved");
            }
            Err(e) => {
                println!("\nAuthentication failed: {}", e);
                println!("Make sure you authorized the application before pressing Enter.");
            }
        }
    } else {
        // Require API key and secret
        let api_key = match args.api_key {
            Some(key) => key,
            None => {
                println!("API key is required for authentication. Use --api-key option.");
                println!("Alternatively, use --use-defaults to use credentials from compilation.");
                return Ok(());
            }
        };

        let api_secret = match args.api_secret {
            Some(secret) => secret,
            None => {
                println!("API secret is required for authentication. Use --api-secret option.");
                println!("Alternatively, use --use-defaults to use credentials from compilation.");
                return Ok(());
            }
        };

        // Initialize the Last.fm client
        LastfmClient::initialize(api_key, api_secret)?;

        // Get a client instance
        let mut client = LastfmClient::get_instance()?;

        // Start authentication flow
        let auth_url_tuple = client.get_auth_url().await?;

        println!("\nTo authenticate with Last.fm, please:");
        println!("1. Visit this URL in your browser: {}", auth_url_tuple.0);
        println!("2. Log in to your Last.fm account if necessary");
        println!("3. Authorize this application");
        println!("4. Return here and press Enter when completed");

        // Wait for user to authorize
        let mut input = String::new();
        io::stdin().read_line(&mut input)?;

        // Now try to get the session key
        match client.get_session() {
            Ok((session_key, username)) => {
                println!("\nAuthentication successful!");
                println!("Username: {}", username);
                println!("Session key: {}", session_key);

                // Save credentials to file
                let credentials = client.get_credentials();
                let json = serde_json::to_string_pretty(&credentials)?;

                let mut file = File::create(&args.credentials_file)?;
                file.write_all(json.as_bytes())?;

                println!(
                    "\nCredentials saved to: {}",
                    args.credentials_file.display()
                );
                println!("You can use these credentials in the future with --use-saved");
            }
            Err(e) => {
                println!("\nAuthentication failed: {}", e);
                println!("Make sure you authorized the application before pressing Enter.");
            }
        }
    }

    Ok(())
}
