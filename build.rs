use std::env;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

fn main() {
    // Tell Cargo to rebuild if this build script changes
    println!("cargo:rerun-if-changed=build.rs");
    
    // Tell Cargo to rebuild if the secrets file changes
    println!("cargo:rerun-if-changed=secrets.txt");
    
    // Check for secrets.txt and extract Last.fm API credentials
    read_lastfm_secrets();
}

fn read_lastfm_secrets() {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let secrets_path = Path::new(&manifest_dir).join("secrets.txt");
    
    if !secrets_path.exists() {
        println!("No secrets.txt file found, using placeholder values for Last.fm API credentials");
        return;
    }
    
    println!("Found secrets.txt, reading Last.fm API credentials");
    
    match File::open(&secrets_path) {
        Ok(file) => {
            let reader = BufReader::new(file);
            
            for line in reader.lines() {
                if let Ok(line) = line {
                    if line.trim().is_empty() || line.trim().starts_with("//") {
                        continue;
                    }
                    
                    if let Some((key, value)) = line.split_once('=') {
                        let key = key.trim();
                        let value = value.trim();
                          match key {
                            "LASTFM_APIKEY" => {
                                println!("cargo:rustc-env=LASTFM_APIKEY={}", value);
                            },                            "LASTFM_APISECRET" => {
                                println!("cargo:rustc-env=LASTFM_APISECRET={}", value);
                            },
                            "ARTISTDB_APIKEY" => {
                                println!("cargo:rustc-env=ARTISTDB_APIKEY={}", value);
                            },
                            "SECRETS_ENCRYPTION_KEY" => {
                                println!("cargo:rustc-env=SECRETS_ENCRYPTION_KEY={}", value);
                            },
                            _ => {} // Ignore other keys
                        }
                    }
                }
            }
        },
        Err(e) => {
            println!("Failed to open secrets.txt: {}", e);
        }
    }
}