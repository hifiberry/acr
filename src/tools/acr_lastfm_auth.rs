use acr::helpers::lastfm::{LastfmClient, LastfmCredentials};
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
}

fn main() -> Result<(), Box<dyn Error>> {
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
    } else {
        // Require API key and secret
        let api_key = match args.api_key {
            Some(key) => key,
            None => {
                println!("API key is required for authentication. Use --api-key option.");
                return Ok(());
            }
        };

        let api_secret = match args.api_secret {
            Some(secret) => secret,
            None => {
                println!("API secret is required for authentication. Use --api-secret option.");
                return Ok(());
            }
        };

        // Initialize the Last.fm client
        LastfmClient::initialize(api_key, api_secret)?;

        // Get a client instance
        let mut client = LastfmClient::get_instance()?;

        // Start authentication flow
        let auth_url = client.get_auth_url()?;

        println!("\nTo authenticate with Last.fm, please:");
        println!("1. Visit this URL in your browser: {}", auth_url);
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
