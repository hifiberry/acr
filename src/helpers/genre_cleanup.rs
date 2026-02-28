use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use serde::{Deserialize, Serialize};
use log::{debug, warn};
use once_cell::sync::Lazy;
use parking_lot::Mutex;

/// Configuration for genre cleanup
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct GenreConfig {
    #[serde(rename = "_comment", default, skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
    #[serde(rename = "_ignore", default)]
    pub ignore: Vec<String>,
    #[serde(default)]
    pub mappings: HashMap<String, String>,
}

impl Default for GenreConfig {
    fn default() -> Self {
        GenreConfig {
            comment: None,
            ignore: Vec::new(),
            mappings: HashMap::new(),
        }
    }
}

/// Genre cleanup service that consolidates and normalizes genre tags
pub struct GenreCleanup {
    ignore_set: HashSet<String>,
    mapping_lowercase: HashMap<String, String>,
    /// Merged effective config (for API inspection/serialization)
    pub effective_config: GenreConfig,
    /// System config path (for reload)
    system_config_path: Option<PathBuf>,
    /// User config path (for read/write)
    pub user_path: PathBuf,
}

// Global instance
static GENRE_CLEANUP: Lazy<Mutex<Option<GenreCleanup>>> = Lazy::new(|| Mutex::new(None));

/// Returns the standard user config path: $HOME/.config/audiocontrol/genres.json
pub fn user_config_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    PathBuf::from(home).join(".config/audiocontrol/genres.json")
}

/// Merge system and user configs: user entries win for mappings, ignore lists are unioned
fn merge_configs(system: Option<&GenreConfig>, user: Option<&GenreConfig>) -> GenreConfig {
    let mut merged = GenreConfig::default();

    if let Some(sys) = system {
        merged.mappings.extend(sys.mappings.clone());
        for ig in &sys.ignore {
            if !merged.ignore.contains(ig) {
                merged.ignore.push(ig.clone());
            }
        }
    }

    if let Some(usr) = user {
        // User mappings override system mappings
        merged.mappings.extend(usr.mappings.clone());
        for ig in &usr.ignore {
            if !merged.ignore.contains(ig) {
                merged.ignore.push(ig.clone());
            }
        }
    }

    merged
}

impl GenreCleanup {
    /// Create a new GenreCleanup instance from a config object, with explicit paths
    pub fn from_configs(
        system_config: Option<GenreConfig>,
        user_config: Option<GenreConfig>,
        system_config_path: Option<PathBuf>,
        user_path: PathBuf,
    ) -> Self {
        let effective = merge_configs(system_config.as_ref(), user_config.as_ref());

        let ignore_set: HashSet<String> = effective.ignore.iter()
            .map(|s| s.to_lowercase())
            .collect();

        let mapping_lowercase: HashMap<String, String> = effective.mappings.iter()
            .map(|(k, v)| (k.to_lowercase(), v.clone()))
            .collect();

        debug!("Genre cleanup initialized with {} ignore entries and {} mappings",
               ignore_set.len(), mapping_lowercase.len());

        GenreCleanup {
            ignore_set,
            mapping_lowercase,
            effective_config: effective,
            system_config_path,
            user_path,
        }
    }

    /// Create a new GenreCleanup instance from a config file (legacy, no merge)
    pub fn from_config_file<P: AsRef<Path>>(config_path: P) -> Result<Self, Box<dyn std::error::Error>> {
        let config_content = fs::read_to_string(config_path.as_ref())
            .map_err(|e| format!("Failed to read genre config file: {}", e))?;
        let config: GenreConfig = serde_json::from_str(&config_content)
            .map_err(|e| format!("Failed to parse genre config JSON: {}", e))?;
        Ok(Self::from_configs(
            Some(config),
            None,
            Some(config_path.as_ref().to_path_buf()),
            user_config_path(),
        ))
    }

    /// Create a new GenreCleanup instance from a config object (legacy, no merge)
    pub fn from_config(config: GenreConfig) -> Result<Self, Box<dyn std::error::Error>> {
        Ok(Self::from_configs(Some(config), None, None, user_config_path()))
    }

    /// Clean up a single genre string
    pub fn clean_genre(&self, genre: &str) -> Option<String> {
        let genre_lower = genre.trim().to_lowercase();

        if self.ignore_set.contains(&genre_lower) {
            debug!("Ignoring genre: {}", genre);
            return None;
        }

        if let Some(mapped_genre) = self.mapping_lowercase.get(&genre_lower) {
            debug!("Mapped genre '{}' to '{}'\n", genre, mapped_genre);
            return Some(mapped_genre.clone());
        }

        Some(genre.trim().to_string())
    }

    /// Clean up a list of genres, removing duplicates and applying mappings
    pub fn clean_genres(&self, genres: Vec<String>) -> Vec<String> {
        let mut cleaned_genres = HashSet::new();

        for genre in genres {
            if let Some(cleaned) = self.clean_genre(&genre) {
                cleaned_genres.insert(cleaned);
            }
        }

        let mut result: Vec<String> = cleaned_genres.into_iter().collect();
        result.sort();
        result
    }

    /// Clean up genres from a slice of strings
    pub fn clean_genres_slice(&self, genres: &[String]) -> Vec<String> {
        self.clean_genres(genres.to_vec())
    }

    /// Reload from the same paths (re-reads system and user config files)
    fn reload(&mut self) {
        let system_config = self.system_config_path.as_ref().and_then(|p| {
            if p.exists() {
                fs::read_to_string(p).ok()
                    .and_then(|s| serde_json::from_str::<GenreConfig>(&s).ok())
            } else {
                None
            }
        });

        let user_config = if self.user_path.exists() {
            fs::read_to_string(&self.user_path).ok()
                .and_then(|s| serde_json::from_str::<GenreConfig>(&s).ok())
        } else {
            None
        };

        let effective = merge_configs(system_config.as_ref(), user_config.as_ref());

        self.ignore_set = effective.ignore.iter().map(|s| s.to_lowercase()).collect();
        self.mapping_lowercase = effective.mappings.iter()
            .map(|(k, v)| (k.to_lowercase(), v.clone()))
            .collect();
        self.effective_config = effective;
    }
}

/// Initialize the global genre cleanup instance
pub fn initialize_genre_cleanup() -> Result<(), Box<dyn std::error::Error>> {
    initialize_genre_cleanup_with_config(None)
}

/// Initialize the global genre cleanup instance with an optional configuration
pub fn initialize_genre_cleanup_with_config(config: Option<&serde_json::Value>) -> Result<(), Box<dyn std::error::Error>> {
    let mut system_config_path: Option<PathBuf> = None;
    let mut system_config: Option<GenreConfig> = None;

    // Try configured path first
    if let Some(config_value) = config {
        if let Some(genre_config) = crate::config::get_service_config(config_value, "genre_cleanup") {
            if let Some(path_str) = genre_config.get("config_path").and_then(|p| p.as_str()) {
                let path = Path::new(path_str);
                if path.exists() {
                    match fs::read_to_string(path).and_then(|s| {
                        serde_json::from_str::<GenreConfig>(&s)
                            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
                    }) {
                        Ok(cfg) => {
                            debug!("Loaded system genre config from configured path: {}", path_str);
                            system_config_path = Some(path.to_path_buf());
                            system_config = Some(cfg);
                        }
                        Err(e) => warn!("Failed to load genre config from configured path {}: {}", path_str, e),
                    }
                }
            }
        }
    }

    // Fall back to default system config path
    if system_config.is_none() {
        let default_path = Path::new("/etc/audiocontrol/genres.json");
        if default_path.exists() {
            match fs::read_to_string(default_path).and_then(|s| {
                serde_json::from_str::<GenreConfig>(&s)
                    .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
            }) {
                Ok(cfg) => {
                    debug!("Loaded system genre config from default path");
                    system_config_path = Some(default_path.to_path_buf());
                    system_config = Some(cfg);
                }
                Err(e) => warn!("Failed to load genre config from default path: {}", e),
            }
        }
    }

    // Load user config (always from user home path)
    let u_path = user_config_path();
    let user_config: Option<GenreConfig> = if u_path.exists() {
        match fs::read_to_string(&u_path).and_then(|s| {
            serde_json::from_str::<GenreConfig>(&s)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
        }) {
            Ok(cfg) => {
                debug!("Loaded user genre config from {}", u_path.display());
                Some(cfg)
            }
            Err(e) => {
                warn!("Failed to load user genre config from {}: {}", u_path.display(), e);
                None
            }
        }
    } else {
        None
    };

    if system_config.is_none() && user_config.is_none() {
        warn!("No genre config found in system or user locations — genre cleanup disabled");
        return Err("Genre cleanup configuration not found".into());
    }

    let cleanup = GenreCleanup::from_configs(system_config, user_config, system_config_path, u_path);
    let mut global = GENRE_CLEANUP.lock();
    *global = Some(cleanup);
    Ok(())
}

/// Get the global genre cleanup instance
pub fn get_genre_cleanup() -> parking_lot::MutexGuard<'static, Option<GenreCleanup>> {
    GENRE_CLEANUP.lock()
}

/// Returns the current effective (merged) config for API inspection
pub fn get_effective_config() -> Option<GenreConfig> {
    let guard = GENRE_CLEANUP.lock();
    guard.as_ref().map(|c| c.effective_config.clone())
}

/// Returns the user config from disk (what the user has explicitly set)
pub fn get_user_config() -> GenreConfig {
    let u_path = user_config_path();
    if u_path.exists() {
        match fs::read_to_string(&u_path)
            .and_then(|s| serde_json::from_str::<GenreConfig>(&s)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e)))
        {
            Ok(cfg) => return cfg,
            Err(e) => warn!("Failed to read user genre config: {}", e),
        }
    }
    GenreConfig::default()
}

/// Save a new user config to disk and reload the global instance
pub fn save_user_config(config: GenreConfig) -> Result<(), Box<dyn std::error::Error>> {
    let u_path = user_config_path();
    if let Some(parent) = u_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(&config)?;
    fs::write(&u_path, json)?;
    debug!("Saved user genre config to {}", u_path.display());

    // Reload the global instance
    let mut guard = GENRE_CLEANUP.lock();
    if let Some(ref mut cleanup) = *guard {
        cleanup.reload();
        debug!("Reloaded genre cleanup after user config save");
    } else {
        // Not initialized yet — initialize now
        drop(guard);
        initialize_genre_cleanup()?;
    }
    Ok(())
}

/// Add or update a mapping in the user config
pub fn set_genre_mapping(from: String, to: String) -> Result<(), Box<dyn std::error::Error>> {
    let mut cfg = get_user_config();
    cfg.mappings.insert(from, to);
    save_user_config(cfg)
}

/// Remove a mapping from the user config
pub fn delete_genre_mapping(from: &str) -> Result<(), Box<dyn std::error::Error>> {
    let mut cfg = get_user_config();
    cfg.mappings.remove(from);
    save_user_config(cfg)
}

/// Add a genre to the user ignore list
pub fn add_genre_ignore(genre: String) -> Result<(), Box<dyn std::error::Error>> {
    let mut cfg = get_user_config();
    if !cfg.ignore.contains(&genre) {
        cfg.ignore.push(genre);
    }
    save_user_config(cfg)
}

/// Remove a genre from the user ignore list
pub fn remove_genre_ignore(genre: &str) -> Result<(), Box<dyn std::error::Error>> {
    let mut cfg = get_user_config();
    cfg.ignore.retain(|g| g != genre);
    save_user_config(cfg)
}

/// Clean up genres using the global instance
pub fn clean_genres_global(genres: Vec<String>) -> Vec<String> {
    let cleanup_guard = GENRE_CLEANUP.lock();
    if let Some(ref cleanup) = *cleanup_guard {
        cleanup.clean_genres(genres)
    } else {
        let mut unique_genres: Vec<String> = genres.into_iter().collect::<HashSet<_>>().into_iter().collect();
        unique_genres.sort();
        unique_genres
    }
}

/// Clean up a single genre using the global instance
pub fn clean_genre_global(genre: &str) -> Option<String> {
    let cleanup_guard = GENRE_CLEANUP.lock();
    if let Some(ref cleanup) = *cleanup_guard {
        cleanup.clean_genre(genre)
    } else {
        Some(genre.trim().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;
    use std::io::Write;

    #[test]
    fn test_genre_cleanup_basic() {
        let config = GenreConfig {
            comment: Some("Test config".to_string()),
            ignore: vec!["seen live".to_string(), "80s".to_string()],
            mappings: {
                let mut map = HashMap::new();
                map.insert("hip hop".to_string(), "hip-hop".to_string());
                map.insert("heavy metal".to_string(), "heavy metal".to_string());
                map.insert("thrash metal".to_string(), "thrash metal".to_string());
                map
            },
        };

        let cleanup = GenreCleanup::from_config(config).unwrap();

        assert_eq!(cleanup.clean_genre("seen live"), None);
        assert_eq!(cleanup.clean_genre("80s"), None);
        assert_eq!(cleanup.clean_genre("hip hop"), Some("hip-hop".to_string()));
        assert_eq!(cleanup.clean_genre("Hip Hop"), Some("hip-hop".to_string()));
        assert_eq!(cleanup.clean_genre("jazz"), Some("jazz".to_string()));
    }

    #[test]
    fn test_genre_cleanup_list() {
        let config = GenreConfig {
            comment: None,
            ignore: vec!["seen live".to_string()],
            mappings: {
                let mut map = HashMap::new();
                map.insert("hip hop".to_string(), "hip-hop".to_string());
                map.insert("rap".to_string(), "hip-hop".to_string());
                map
            },
        };

        let cleanup = GenreCleanup::from_config(config).unwrap();

        let input = vec![
            "hip hop".to_string(),
            "rap".to_string(),
            "jazz".to_string(),
            "seen live".to_string(),
            "hip hop".to_string(),
        ];

        let result = cleanup.clean_genres(input);
        assert_eq!(result.len(), 2);
        assert!(result.contains(&"hip-hop".to_string()));
        assert!(result.contains(&"jazz".to_string()));
    }

    #[test]
    fn test_config_from_file() {
        let config_json = r#"{
            "_comment": "Test config",
            "_ignore": ["seen live", "80s"],
            "mappings": {
                "hip hop": "hip-hop",
                "heavy metal": "heavy metal"
            }
        }"#;

        let mut temp_file = NamedTempFile::new().unwrap();
        temp_file.write_all(config_json.as_bytes()).unwrap();

        let cleanup = GenreCleanup::from_config_file(temp_file.path()).unwrap();
        assert_eq!(cleanup.clean_genre("seen live"), None);
        assert_eq!(cleanup.clean_genre("hip hop"), Some("hip-hop".to_string()));
    }

    #[test]
    fn test_merge_configs() {
        let system = GenreConfig {
            comment: None,
            ignore: vec!["seen live".to_string()],
            mappings: {
                let mut m = HashMap::new();
                m.insert("rock n roll".to_string(), "Rock".to_string());
                m.insert("hip hop".to_string(), "Hip-Hop".to_string());
                m
            },
        };
        let user = GenreConfig {
            comment: None,
            ignore: vec!["promo".to_string()],
            mappings: {
                let mut m = HashMap::new();
                // user overrides hip hop
                m.insert("hip hop".to_string(), "Hip Hop".to_string());
                m
            },
        };

        let merged = merge_configs(Some(&system), Some(&user));

        // System ignore + user ignore
        assert!(merged.ignore.contains(&"seen live".to_string()));
        assert!(merged.ignore.contains(&"promo".to_string()));

        // User override wins
        assert_eq!(merged.mappings.get("hip hop"), Some(&"Hip Hop".to_string()));
        // System-only mapping preserved
        assert_eq!(merged.mappings.get("rock n roll"), Some(&"Rock".to_string()));
    }
}
