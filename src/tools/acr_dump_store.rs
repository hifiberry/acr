use acr::helpers::security_store::{SecurityStore};
use std::fs;
use std::path::PathBuf;
use clap::Parser;
use log::{info, error};
use serde_json::Value;

#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
struct Args {
    /// Path to the security_store.json file
    #[clap(short, long, value_parser)]
    store_path: Option<PathBuf>,

    /// Encryption key to decrypt the store. If not provided, values will be masked.
    #[clap(short, long)]
    key: Option<String>,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let args = Args::parse();

    let store_path = args.store_path.unwrap_or_else(|| {
        info!("No store path provided, using default: secrets/security_store.json");
        PathBuf::from("secrets/security_store.json")
    });

    if !store_path.exists() {
        error!("Security store file not found at: {}", store_path.display());
        return Err(Box::new(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "Security store file not found",
        )));
    }
    
    let encryption_key_to_use: Option<String> = args.key;

    if let Some(enc_key) = encryption_key_to_use {
        info!("Attempting to decrypt store with provided key...");
        match SecurityStore::initialize(&enc_key, Some(store_path.clone())) {
            Ok(_) => {
                info!("SecurityStore initialized successfully.");
                match SecurityStore::get_all_keys() {
                    Ok(keys) => {
                        if keys.is_empty() {
                            info!("Security store is empty.");
                        } else {
                            info!("Found {} keys. Decrypted values:", keys.len());
                            for key_name in keys {
                                match SecurityStore::get(&key_name) {
                                    Ok(value) => println!("{}: {}", key_name, value),
                                    Err(e) => error!("Failed to get/decrypt key '{}': {}", key_name, e),
                                }
                            }
                        }
                    }
                    Err(e) => {
                        error!("Failed to get keys from security store: {}", e);
                        println!("Could not retrieve keys. The store might be corrupted or the key incorrect.");
                    }
                }
            }
            Err(e) => {
                error!("Failed to initialize SecurityStore with key: {}", e);
                println!("Could not initialize the security store. Is the key correct?");
                // Fallback to dumping raw if initialization fails with a key
                dump_raw_store(&store_path)?;
            }
        }
    } else {
        info!("No encryption key provided. Dumping keys with masked values...");
        dump_raw_store(&store_path)?;
    }

    Ok(())
}

fn dump_raw_store(store_path: &PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    let content = fs::read_to_string(store_path)?;
    let json_data: Value = serde_json::from_str(&content)?;

    if let Some(values) = json_data.get("values").and_then(|v| v.as_object()) {
        if values.is_empty() {
            info!("Security store (raw) is empty or has no 'values' map.");
        } else {
            info!("Found {} keys (raw). Values are masked:", values.len());
            for (key, _value) in values {
                println!("{}: ***", key);
            }
        }
    } else {
        error!("Could not find 'values' map in the security store JSON.");
        println!("The store file format seems incorrect or does not contain a 'values' map.");
    }
    Ok(())
}
