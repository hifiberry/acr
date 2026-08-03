// Configuration utilities for ACR
// 
// This module provides utilities for reading configuration values with backward compatibility
// support for the migration from top-level service configuration to the new "services" subtree.

use log::{debug, info, warn};
use std::fs;
use std::path::Path;

/// Helper function to get service configuration with backward compatibility
/// 
/// This function first tries to find the service in the new "services" structure,
/// then falls back to the old top-level structure for backward compatibility.
/// 
/// # Arguments
/// * `config` - The configuration JSON object
/// * `service_name` - The name of the service to look up (e.g., "spotify", "lastfm", etc.)
/// 
/// # Returns
/// * `Option<&serde_json::Value>` - The service configuration if found, None otherwise
/// 
/// # Example
/// ```rust
/// use serde_json::json;
/// use audiocontrol::config::get_service_config;
/// 
/// // For a config with new structure:
/// let config = json!({
///   "services": {
///     "spotify": { "enable": true }
///   }
/// });
/// 
/// if let Some(spotify_config) = get_service_config(&config, "spotify") {
///     assert_eq!(spotify_config["enable"], true);
/// }
/// 
/// // For old structure (backward compatibility):
/// let old_config = json!({
///   "spotify": { "enable": false }
/// });
/// 
/// if let Some(spotify_config) = get_service_config(&old_config, "spotify") {
///     assert_eq!(spotify_config["enable"], false);
/// }
/// ```
pub fn get_service_config<'a>(config: &'a serde_json::Value, service_name: &str) -> Option<&'a serde_json::Value> {
    // First, try to find the service in the new "services" structure
    if let Some(services) = config.get("services") {
        if let Some(service_config) = services.get(service_name) {
            debug!("Found {} configuration in services section", service_name);
            return Some(service_config);
        }
    }
    
    // Fall back to the old top-level structure for backward compatibility
    if let Some(service_config) = config.get(service_name) {
        debug!("Found {} configuration at top level (legacy structure)", service_name);
        return Some(service_config);
    }
    
    // Service configuration not found
    debug!("No {} configuration found in either services section or top level", service_name);
    None
}

/// Merge player configurations from a `players.d/` include directory.
///
/// Scans `<config_dir>/players.d/` for `*.json` files and appends each
/// player entry to the `"players"` array in the main config. Files are
/// loaded in alphabetical order.
///
/// Each file may contain either a single player object (e.g.
/// `{"generic": {"name": "my-player", ...}}`) or an array of player
/// objects.
///
/// If the directory does not exist, this is a no-op. Malformed files
/// are skipped with a warning.
pub fn merge_player_includes(config: &mut serde_json::Value, config_dir: &Path) {
    let players_d = config_dir.join("players.d");
    if !players_d.is_dir() {
        debug!("No players.d directory at {}, skipping", players_d.display());
        return;
    }

    // Ensure config has a players array
    if config.get("players").is_none() {
        config["players"] = serde_json::Value::Array(vec![]);
    }

    // Read and sort *.json files
    let mut files: Vec<_> = match fs::read_dir(&players_d) {
        Ok(entries) => entries
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().map_or(false, |ext| ext == "json"))
            .collect(),
        Err(e) => {
            warn!("Failed to read players.d directory: {}", e);
            return;
        }
    };
    files.sort_by_key(|e| e.file_name());

    for entry in files {
        let path = entry.path();
        match fs::read_to_string(&path) {
            Ok(content) => match serde_json::from_str::<serde_json::Value>(&content) {
                Ok(value) => {
                    let items = if value.is_array() {
                        value.as_array().unwrap().clone()
                    } else if value.is_object() {
                        vec![value]
                    } else {
                        warn!("Skipping {}: not a JSON object or array", path.display());
                        continue;
                    };
                    // Tag each item so from_json() knows it came from an include
                    let tagged: Vec<_> = items.into_iter().map(|mut item| {
                        if let Some(obj) = item.as_object_mut() {
                            obj.insert("_from_include".to_string(),
                                       serde_json::Value::String(path.display().to_string()));
                        }
                        item
                    }).collect();
                    let count = tagged.len();
                    if let Some(players) = config["players"].as_array_mut() {
                        players.extend(tagged);
                    }
                    info!("Loaded {} player(s) from {}", count, path.display());
                }
                Err(e) => warn!("Failed to parse {}: {}", path.display(), e),
            },
            Err(e) => warn!("Failed to read {}: {}", path.display(), e),
        }
    }
}

/// Player types that used to be shipped in the `players` array of
/// audiocontrol.json and are now provided by per-package include files in
/// players.d. Entries of these types are removed from an existing config on
/// upgrade so the player is not defined twice - merge_player_includes()
/// appends without deduplicating, and from_json() would create one controller
/// per entry.
const MIGRATED_PLAYER_TYPES: &[&str] = &[
    "mpd", "raat", "librespot", "lms", "bluetooth", "_mpris", "shairport",
];

/// Remove player entries that have moved to players.d.
///
/// Returns true when the config was modified. Anything not in
/// MIGRATED_PLAYER_TYPES is kept, so a hand-added player survives. The
/// `players` key is dropped entirely when nothing is left.
pub fn strip_migrated_players(config: &mut serde_json::Value) -> bool {
    let Some(players) = config.get("players").and_then(|v| v.as_array()) else {
        return false;
    };

    let kept: Vec<serde_json::Value> = players
        .iter()
        .filter(|entry| {
            let player_type = entry
                .as_object()
                .and_then(|obj| obj.keys().find(|k| k.as_str() != "_from_include"));
            match player_type {
                Some(t) => !MIGRATED_PLAYER_TYPES.contains(&t.as_str()),
                None => true,
            }
        })
        .cloned()
        .collect();

    if kept.len() == players.len() {
        return false;
    }

    if let Some(obj) = config.as_object_mut() {
        if kept.is_empty() {
            obj.remove("players");
        } else {
            obj.insert("players".to_string(), serde_json::Value::Array(kept));
        }
    }
    true
}

/// Apply strip_migrated_players to a config file in place.
///
/// Writes a .bak copy before changing anything and leaves the file untouched
/// when there is nothing to remove. Returns true when the file was rewritten.
pub fn migrate_config_file(path: &Path) -> Result<bool, String> {
    let content = fs::read_to_string(path)
        .map_err(|e| format!("cannot read {}: {}", path.display(), e))?;
    let mut config: serde_json::Value = serde_json::from_str(&content)
        .map_err(|e| format!("cannot parse {}: {}", path.display(), e))?;

    if !strip_migrated_players(&mut config) {
        return Ok(false);
    }

    let backup = path.with_extension("json.bak");
    fs::write(&backup, &content)
        .map_err(|e| format!("cannot write {}: {}", backup.display(), e))?;

    let serialized = serde_json::to_string_pretty(&config)
        .map_err(|e| format!("cannot serialize config: {}", e))?;
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, serialized)
        .map_err(|e| format!("cannot write {}: {}", tmp.display(), e))?;
    fs::rename(&tmp, path)
        .map_err(|e| format!("cannot replace {}: {}", path.display(), e))?;

    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::TempDir;

    #[test]
    fn test_no_players_d_directory() {
        let tmp = TempDir::new().unwrap();
        let mut config = json!({"players": []});
        merge_player_includes(&mut config, tmp.path());
        assert_eq!(config["players"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn test_single_player_object() {
        let tmp = TempDir::new().unwrap();
        let players_d = tmp.path().join("players.d");
        fs::create_dir(&players_d).unwrap();
        fs::write(
            players_d.join("test.json"),
            r#"{"generic": {"name": "test-player", "enable": true}}"#,
        ).unwrap();

        let mut config = json!({"players": []});
        merge_player_includes(&mut config, tmp.path());

        let players = config["players"].as_array().unwrap();
        assert_eq!(players.len(), 1);
        assert_eq!(players[0]["generic"]["name"], "test-player");
    }

    #[test]
    fn test_array_of_players() {
        let tmp = TempDir::new().unwrap();
        let players_d = tmp.path().join("players.d");
        fs::create_dir(&players_d).unwrap();
        fs::write(
            players_d.join("multi.json"),
            r#"[{"generic": {"name": "player-a"}}, {"generic": {"name": "player-b"}}]"#,
        ).unwrap();

        let mut config = json!({"players": []});
        merge_player_includes(&mut config, tmp.path());

        let players = config["players"].as_array().unwrap();
        assert_eq!(players.len(), 2);
        assert_eq!(players[0]["generic"]["name"], "player-a");
        assert_eq!(players[1]["generic"]["name"], "player-b");
    }

    #[test]
    fn test_alphabetical_order() {
        let tmp = TempDir::new().unwrap();
        let players_d = tmp.path().join("players.d");
        fs::create_dir(&players_d).unwrap();
        fs::write(
            players_d.join("20-second.json"),
            r#"{"generic": {"name": "second"}}"#,
        ).unwrap();
        fs::write(
            players_d.join("10-first.json"),
            r#"{"generic": {"name": "first"}}"#,
        ).unwrap();

        let mut config = json!({"players": []});
        merge_player_includes(&mut config, tmp.path());

        let players = config["players"].as_array().unwrap();
        assert_eq!(players.len(), 2);
        assert_eq!(players[0]["generic"]["name"], "first");
        assert_eq!(players[1]["generic"]["name"], "second");
    }

    #[test]
    fn test_appends_to_existing_players() {
        let tmp = TempDir::new().unwrap();
        let players_d = tmp.path().join("players.d");
        fs::create_dir(&players_d).unwrap();
        fs::write(
            players_d.join("extra.json"),
            r#"{"generic": {"name": "extra"}}"#,
        ).unwrap();

        let mut config = json!({"players": [{"mpd": {"enable": true}}]});
        merge_player_includes(&mut config, tmp.path());

        let players = config["players"].as_array().unwrap();
        assert_eq!(players.len(), 2);
        assert!(players[0]["mpd"].is_object());
        assert_eq!(players[1]["generic"]["name"], "extra");
    }

    #[test]
    fn test_creates_players_array_if_missing() {
        let tmp = TempDir::new().unwrap();
        let players_d = tmp.path().join("players.d");
        fs::create_dir(&players_d).unwrap();
        fs::write(
            players_d.join("player.json"),
            r#"{"generic": {"name": "new"}}"#,
        ).unwrap();

        let mut config = json!({});
        merge_player_includes(&mut config, tmp.path());

        let players = config["players"].as_array().unwrap();
        assert_eq!(players.len(), 1);
        assert_eq!(players[0]["generic"]["name"], "new");
    }

    #[test]
    fn test_skips_non_json_files() {
        let tmp = TempDir::new().unwrap();
        let players_d = tmp.path().join("players.d");
        fs::create_dir(&players_d).unwrap();
        fs::write(players_d.join("notes.txt"), "not json").unwrap();
        fs::write(
            players_d.join("player.json"),
            r#"{"generic": {"name": "real"}}"#,
        ).unwrap();

        let mut config = json!({"players": []});
        merge_player_includes(&mut config, tmp.path());

        let players = config["players"].as_array().unwrap();
        assert_eq!(players.len(), 1);
    }

    #[test]
    fn test_skips_malformed_json() {
        let tmp = TempDir::new().unwrap();
        let players_d = tmp.path().join("players.d");
        fs::create_dir(&players_d).unwrap();
        fs::write(players_d.join("bad.json"), "not valid {{{").unwrap();
        fs::write(
            players_d.join("good.json"),
            r#"{"generic": {"name": "ok"}}"#,
        ).unwrap();

        let mut config = json!({"players": []});
        merge_player_includes(&mut config, tmp.path());

        let players = config["players"].as_array().unwrap();
        assert_eq!(players.len(), 1);
        assert_eq!(players[0]["generic"]["name"], "ok");
    }

    #[test]
    fn test_strip_migrated_players_removes_shipped_types() {
        let mut config = json!({
            "players": [
                {"mpd": {"enable": true}},
                {"librespot": {"enable": true}},
                {"shairport": {"enable": true}},
                {"raat": {"enable": true}},
                {"lms": {"enable": true}},
                {"bluetooth": {"enable": true}},
                {"_mpris": {"enable": true}}
            ]
        });
        assert!(strip_migrated_players(&mut config));
        assert!(config.get("players").is_none(),
                "players key should be dropped when empty: {}", config);
    }

    #[test]
    fn test_strip_migrated_players_keeps_custom_players() {
        let mut config = json!({
            "players": [
                {"mpd": {"enable": true}},
                {"generic": {"name": "my-player", "enable": true}}
            ]
        });
        assert!(strip_migrated_players(&mut config));
        let players = config["players"].as_array().unwrap();
        assert_eq!(players.len(), 1);
        assert!(players[0].get("generic").is_some());
    }

    #[test]
    fn test_strip_migrated_players_is_idempotent() {
        let mut config = json!({"players": [{"generic": {"name": "keep"}}]});
        assert!(!strip_migrated_players(&mut config),
                "already-migrated config must report no change");
    }

    #[test]
    fn test_strip_migrated_players_without_players_key() {
        let mut config = json!({"services": {}});
        assert!(!strip_migrated_players(&mut config));
        assert!(config.get("services").is_some());
    }

    #[test]
    fn test_migrate_config_file_round_trip() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("audiocontrol.json");
        fs::write(&path, r#"{"players":[{"mpd":{"enable":true}},{"generic":{"name":"keep"}}],"services":{}}"#).unwrap();

        assert!(migrate_config_file(&path).unwrap());

        let result: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        let players = result["players"].as_array().unwrap();
        assert_eq!(players.len(), 1);
        assert!(players[0].get("generic").is_some());
        assert!(result.get("services").is_some(), "unrelated keys preserved");
        assert!(path.with_extension("json.bak").exists(), "backup written");
    }

    #[test]
    fn test_migrate_config_file_leaves_malformed_json_alone() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("audiocontrol.json");
        fs::write(&path, "{not json").unwrap();

        assert!(migrate_config_file(&path).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "{not json");
    }
}
