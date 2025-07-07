use std::env;
use std::path::PathBuf;

fn print_usage() {
    println!("show-cache: Dumps the contents of an attribute cache database in key|value format");
    println!();
    println!("Usage:");
    println!("  show-cache [OPTIONS] [PATH]");
    println!();
    println!("Options:");
    println!("  --help, -h        Show this help message");
    println!();
    println!("Arguments:");
    println!("  PATH              Path to the cache database directory");
    println!();
    println!("If no path is specified, defaults to \"/var/lib/audiocontrol/cache/attributes\"");
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Parse command line arguments
    let args: Vec<String> = env::args().collect();

    // Default path
    let mut db_path = PathBuf::from("/var/lib/audiocontrol/cache/attributes");

    // Process arguments
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--help" | "-h" => {
                print_usage();
                return Ok(());
            }
            arg => {
                // If it doesn't start with a dash, assume it's a direct path argument
                if !arg.starts_with('-') {
                    db_path = PathBuf::from(arg);
                    i += 1;
                } else {
                    eprintln!("Error: Unknown option: {}", arg);
                    print_usage();
                    return Err(format!("Unknown option: {}", arg).into());
                }
            }
        }
    }

    println!("Opening cache database at: {:?}", db_path);

    // Check if the directory exists before trying to open the database
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
    println!("Cache contents (key|value format):");
    println!("----------------------------------");

    let mut count = 0;

    for result in db.iter() {
        match result {
            Ok((key_bytes, value_bytes)) => {
                // Convert key to string if possible
                let key = match std::str::from_utf8(&key_bytes) {
                    Ok(s) => s.to_string(),
                    Err(_) => format!("<binary: {:?}>", key_bytes),
                };

                // Try to parse value as JSON for better display
                let value = match serde_json::from_slice::<serde_json::Value>(&value_bytes) {
                    Ok(json) => json.to_string(),
                    Err(_) => {
                        // If not valid JSON, try to display as string
                        match std::str::from_utf8(&value_bytes) {
                            Ok(s) => s.to_string(),
                            Err(_) => format!("<binary: {:?}>", value_bytes),
                        }
                    }
                };

                // Print in key|value format
                println!("{}|{}", key, value);
                count += 1;
            }
            Err(e) => {
                eprintln!("Error reading entry: {}", e);
            }
        }
    }

    println!("----------------------------------");
    println!("Total entries: {}", count);

    Ok(())
}
