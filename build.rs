use base64::{engine::general_purpose::STANDARD, Engine as _};
use md5;
use std::collections::HashMap;
use std::env;
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::Path;

fn main() {
    println!("cargo:rerun-if-changed=secrets.txt");
    println!("cargo:rerun-if-changed=config/secrets.txt");
    println!("cargo:rerun-if-changed=../secrets.txt");

    // Log all secrets found during build
    println!("cargo:warning=SECRETS FOUND DURING BUILD:");

    let mut secrets = HashMap::new();
    // Check for secrets in various possible locations
    check_secrets_file("secrets.txt", &mut secrets);
    check_secrets_file("config/secrets.txt", &mut secrets);
    check_secrets_file("../secrets.txt", &mut secrets);

    // Look for environment variables with secret-like names
    check_environment_secrets(&mut secrets);

    // Generate Rust code with the secrets
    generate_secrets_file(&secrets);
}

fn check_secrets_file(filename: &str, secrets: &mut HashMap<String, String>) {
    let path = Path::new(filename);
    if path.exists() {
        if let Ok(file) = File::open(path) {
            let reader = BufReader::new(file);
            for line in reader.lines() {
                if let Ok(line) = line {
                    let trimmed = line.trim();
                    if trimmed.is_empty() || trimmed.starts_with('#') {
                        continue;
                    }
                    if let Some(pos) = line.find('=') {
                        let key = line[..pos].trim().to_string();
                        let value = line[pos + 1..].trim().to_string();
                        secrets.insert(key.clone(), value.clone());
                    }
                }
            }
        }
    }
}

fn check_environment_secrets(secrets: &mut HashMap<String, String>) {
    let secret_prefixes = [
        "API_",
        "TOKEN_",
        "SECRET_",
        "PASSWORD_",
        "AUTH_",
        "CREDENTIAL_",
        "KEY_",
    ];
    for (key, value) in env::vars() {
        let upper_key = key.to_uppercase();
        for prefix in &secret_prefixes {
            if upper_key.starts_with(prefix)
                || upper_key.contains("_SECRET_")
                || upper_key.contains("_API_KEY")
            {
                secrets.insert(key.clone(), value.clone());
            }
        }
    }
}

fn obfuscate(s: &str) -> String {
    let reversed: String = s.chars().rev().collect();
    STANDARD.encode(reversed)
}

fn generate_secrets_file(secrets: &HashMap<String, String>) {
    let out_dir = env::var("OUT_DIR").unwrap();
    let dest_path = Path::new(&out_dir).join("generated_secrets.rs");
    let hash_path = Path::new(&out_dir).join("secrets_hash.txt");
    
    // Calculate hash of current secrets
    let mut secrets_string = String::new();
    let mut sorted_secrets: Vec<_> = secrets.iter().collect();
    sorted_secrets.sort_by_key(|(k, _)| *k);
    for (key, value) in sorted_secrets {
        secrets_string.push_str(&format!("{}={}\n", key, value));
    }
    let current_hash = format!("{:x}", md5::compute(&secrets_string));
    
    // Check if secrets have changed since last generation
    let mut should_generate = !dest_path.exists();
    if let Ok(stored_hash) = fs::read_to_string(&hash_path) {
        if stored_hash.trim() != current_hash {
            should_generate = true;
        }
    } else {
        should_generate = true;
    }
    
    if !should_generate {
        println!("cargo:warning=Secrets file up to date, skipping generation");
        return;
    }
    
    println!("cargo:warning=Generating secrets file...");
    
    let mut content = String::new();
    content.push_str("// AUTO-GENERATED FILE - DO NOT EDIT\n");
    content.push_str("// This file is generated by build.rs\n\n");
    content.push_str("use base64::{engine::general_purpose::STANDARD, Engine as _};\n\n");
    content.push_str("fn r#do(s: &str) -> String {\n    let decoded = STANDARD.decode(s).unwrap_or_default();\n    let reversed: String = String::from_utf8(decoded).unwrap_or_default().chars().rev().collect();\n    reversed\n}\n\n");
    // Common secrets as obfuscated base64 constants, fallback to "unknown"
    // Try different common key names for each secret
    let lastfm_key = secrets
        .get("LASTFM_APIKEY")
        .or_else(|| secrets.get("LASTFM_API_KEY"))
        .map(|s| s.as_str())
        .unwrap_or("unknown");
    let lastfm_secret = secrets
        .get("LASTFM_APISECRET")
        .or_else(|| secrets.get("LASTFM_API_SECRET"))
        .map(|s| s.as_str())
        .unwrap_or("unknown");

    let artistdb_key = secrets
        .get("ARTISTDB_APIKEY")
        .or_else(|| secrets.get("THEAUDIODB_APIKEY"))
        .or_else(|| secrets.get("THEAUDIODB_API_KEY"))
        .map(|s| s.as_str())
        .unwrap_or("unknown");

    let encryption_key = secrets
        .get("SECRETS_ENCRYPTION_KEY")
        .or_else(|| secrets.get("SECURITY_KEY"))
        .map(|s| s.as_str())
        .unwrap_or("unknown");
    // Spotify OAuth credentials
    let spotify_oauth_url = secrets
        .get("SPOTIFY_OAUTH_URL")
        .map(|s| s.as_str())
        .unwrap_or("unknown");

    let spotify_proxy_secret = secrets
        .get("SPOTIFY_PROXY_SECRET")
        .map(|s| s.as_str())
        .unwrap_or("unknown");

    // Obfuscate the keys
    let lastfm_key_obf = obfuscate(lastfm_key);
    let lastfm_secret_obf = obfuscate(lastfm_secret);
    let artistdb_key_obf = obfuscate(artistdb_key);
    let encryption_key_obf = obfuscate(encryption_key);
    let spotify_oauth_url_obf = obfuscate(spotify_oauth_url);
    let spotify_proxy_secret_obf = obfuscate(spotify_proxy_secret);
    content.push_str(&format!(
        "pub const LASTFM_API_KEY_OBF: &str = \"{}\";\n",
        lastfm_key_obf
    ));
    content.push_str(&format!(
        "pub const LASTFM_API_SECRET_OBF: &str = \"{}\";\n",
        lastfm_secret_obf
    ));
    content.push_str(&format!(
        "pub const ARTISTDB_API_KEY_OBF: &str = \"{}\";\n",
        artistdb_key_obf
    ));
    content.push_str(&format!(
        "pub const SECRETS_ENCRYPTION_KEY_OBF: &str = \"{}\";\n",
        encryption_key_obf
    ));
    content.push_str(&format!(
        "pub const SPOTIFY_OAUTH_URL_OBF: &str = \"{}\";\n",
        spotify_oauth_url_obf
    ));
    content.push_str(&format!(
        "pub const SPOTIFY_PROXY_SECRET_OBF: &str = \"{}\";\n",
        spotify_proxy_secret_obf
    ));
    content.push_str("\n#[allow(unused_mut)]\npub fn get_all_secrets_obfuscated() -> std::collections::HashMap<String, String> {\n");
    content.push_str("    let mut map = std::collections::HashMap::new();\n");
    for (key, value) in secrets {
        let obf = obfuscate(value);
        content.push_str(&format!(
            "    map.insert(\"{}\".to_string(), \"{}\".to_string());\n",
            key, obf
        ));
    }
    content.push_str("    map\n}\n");
    // For compatibility, also provide the deobfuscated constants
    content.push_str(&format!(
        "\npub fn lastfm_api_key() -> String {{ r#do(LASTFM_API_KEY_OBF) }}\n"
    ));
    content.push_str(&format!(
        "pub fn lastfm_api_secret() -> String {{ r#do(LASTFM_API_SECRET_OBF) }}\n"
    ));
    content.push_str(&format!(
        "pub fn artistdb_api_key() -> String {{ r#do(ARTISTDB_API_KEY_OBF) }}\n"
    ));
    content.push_str(&format!(
        "pub fn secrets_encryption_key() -> String {{ r#do(SECRETS_ENCRYPTION_KEY_OBF) }}\n"
    ));
    content.push_str(&format!(
        "pub fn spotify_oauth_url() -> String {{ r#do(SPOTIFY_OAUTH_URL_OBF) }}\n"
    ));
    content.push_str(&format!(
        "pub fn spotify_proxy_secret() -> String {{ r#do(SPOTIFY_PROXY_SECRET_OBF) }}\n"
    ));
    fs::write(&dest_path, content).unwrap();
    
    // Save the hash of current secrets for future comparison
    fs::write(&hash_path, current_hash).unwrap();
    
    println!("cargo:rerun-if-changed=build.rs");
}
