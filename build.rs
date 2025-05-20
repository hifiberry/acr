use std::env;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

fn main() {
    println!("cargo:rerun-if-changed=secrets.txt");
    
    // Log all secrets found during build
    println!("cargo:warning=SECRETS FOUND DURING BUILD:");
    
    // Check for secrets in various possible locations
    check_secrets_file("secrets.txt");
    check_secrets_file("config/secrets.txt");
    check_secrets_file("../secrets.txt");
    
    // Look for environment variables with secret-like names
    check_environment_secrets();
}

fn check_secrets_file(filename: &str) {
    let path = Path::new(filename);
    
    if path.exists() {
        println!("cargo:warning=Found secrets file: {}", path.display());
        
        if let Ok(file) = File::open(path) {
            let reader = BufReader::new(file);
            let mut count = 0;
            
            for line in reader.lines() {
                if let Ok(line) = line {
                    // Skip empty lines and comments
                    let trimmed = line.trim();
                    if trimmed.is_empty() || trimmed.starts_with('#') {
                        continue;
                    }
                    
                    // Try to parse as key=value
                    if let Some(pos) = line.find('=') {
                        let key = line[..pos].trim();
                        let value = line[pos+1..].trim();
                        
                        // Show full key but mask the value
                        let masked_value = mask_value(value);
                        println!("cargo:warning=Secret: {}={}", key, masked_value);
                        count += 1;
                    }
                }
            }
            
            println!("cargo:warning=Read {} secrets from {}", count, path.display());
        } else {
            println!("cargo:warning=Failed to open secrets file: {}", path.display());
        }
    }
}

fn check_environment_secrets() {
    let secret_prefixes = ["API_", "TOKEN_", "SECRET_", "PASSWORD_", "AUTH_", "CREDENTIAL_", "KEY_"];
    let mut found = false;
    
    println!("cargo:warning=Checking environment variables for secrets...");
    
    for (key, value) in env::vars() {
        let upper_key = key.to_uppercase();
        for prefix in &secret_prefixes {
            if upper_key.starts_with(prefix) || upper_key.contains("_SECRET_") || upper_key.contains("_API_KEY") {
                // Show full key but mask the value
                let masked_value = mask_value(&value);
                println!("cargo:warning=Environment secret: {}={}", key, masked_value);
                found = true;
            }
        }
    }
    
    if !found {
        println!("cargo:warning=No environment secrets found with common prefixes");
    }
}

// Mask a value by showing only the first 3 characters followed by asterisks
fn mask_value(value: &str) -> String {
    if value.len() <= 3 {
        return "***".to_string();
    }
    
    format!("{}***", &value[0..3])
}