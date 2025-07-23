use std::env;
use std::path::PathBuf;

fn print_usage() {
    println!("show-settingsdb: Dumps the contents of a settings database in key|value format");
    println!();
    println!("Usage:");
    println!("  show-settingsdb [OPTIONS] [PATH]");
    println!();
    println!("Options:");
    println!("  --help, -h        Show this help message");
    println!();
    println!("Arguments:");
    println!("  PATH              Path to the settings database directory");
    println!();
    println!("If no path is specified, defaults to \"/var/lib/audiocontrol/db\"");
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Parse command line arguments
    let args: Vec<String> = env::args().collect();

    // Default path
    let mut db_path = PathBuf::from("/var/lib/audiocontrol/db");

    // Process arguments
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--help" | "-h" => {
                print_usage();
                return Ok(());
            }
            arg if arg.starts_with('-') => {
                eprintln!("Unknown option: {}", arg);
                eprintln!("Use --help for usage information");
                std::process::exit(1);
            }
            path => {
                db_path = PathBuf::from(path);
            }
        }
        i += 1;
    }

    // Check if the path exists
    if !db_path.exists() {
        eprintln!("Error: Database directory does not exist: {:?}", db_path);
        return Err(format!("Database directory not found: {:?}", db_path).into());
    }

    // Check if this looks like a Sled database by checking for the existence of the conf file
    let conf_path = db_path.join("conf");
    if !conf_path.exists() {
        eprintln!(
            "Error: Not a valid Sled database (missing 'conf' file): {:?}",
            db_path
        );
        return Err(format!("Not a valid Sled database at: {:?}", db_path).into());
    }

    // Open the database
    let db = match sled::open(&db_path) {
        Ok(db) => db,
        Err(e) => {
            eprintln!("Failed to open database: {}", e);
            return Err(Box::new(e));
        }
    };

    // Dump all key-value pairs
    println!("Settings database contents (key|value format):");
    println!("Database path: {:?}", db_path);
    println!("Total entries: {}", db.len());
    println!();

    // Iterate through all entries
    for item in db.iter() {
        match item {
            Ok((key, value)) => {
                // Convert key and value to strings
                let key_str = match String::from_utf8(key.to_vec()) {
                    Ok(s) => s,
                    Err(_) => {
                        // If the key is not valid UTF-8, skip it or show as hex
                        format!("<binary key: {:?}>", key.as_ref())
                    }
                };

                let value_str = match String::from_utf8(value.to_vec()) {
                    Ok(s) => s,
                    Err(_) => {
                        // If the value is not valid UTF-8, show it as hex representation
                        format!("<binary value: {} bytes>", value.len())
                    }
                };

                println!("{}|{}", key_str, value_str);
            }
            Err(e) => {
                eprintln!("Error reading entry: {}", e);
            }
        }
    }

    Ok(())
}
