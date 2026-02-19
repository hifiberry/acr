use std::collections::HashMap;
use std::fs;
use std::path::Path;
use log::{debug, info, warn, LevelFilter};
use serde::{Deserialize, Serialize};
use env_logger::{Builder, Target, WriteStyle};
use std::io::Write;

/// Available logging subsystems in audiocontrol
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LoggingSubsystem {
    /// Main application logging
    #[serde(rename = "main")]
    Main,
    /// API server logging
    #[serde(rename = "api")]
    Api,
    /// Player controllers (MPD, RAAT, Librespot, etc.)
    #[serde(rename = "players")]
    Players,
    /// Cache system (attribute and image cache)
    #[serde(rename = "cache")]
    Cache,
    /// Music metadata services (MusicBrainz, TheAudioDB, Last.fm)
    #[serde(rename = "metadata")]
    Metadata,
    /// Spotify integration
    #[serde(rename = "spotify")]
    Spotify,
    /// WebSocket connections
    #[serde(rename = "websocket")]
    WebSocket,
    /// Library management
    #[serde(rename = "library")]
    Library,
    /// Security and authentication
    #[serde(rename = "security")]
    Security,
    /// HTTP client operations
    #[serde(rename = "http")]
    Http,
    /// Network operations
    #[serde(rename = "network")]
    Network,
    /// Database operations
    #[serde(rename = "database")]
    Database,
    /// File I/O operations
    #[serde(rename = "io")]
    Io,
    /// Event handling and notifications
    #[serde(rename = "events")]
    Events,
    /// Configuration loading and parsing
    #[serde(rename = "config")]
    Config,
    /// Plugin system
    #[serde(rename = "plugins")]
    Plugins,
    /// Third-party dependencies
    #[serde(rename = "deps")]
    Dependencies,
}

impl LoggingSubsystem {
    /// Get the module prefix for this subsystem
    pub fn module_prefix(&self) -> &'static str {
        match self {
            LoggingSubsystem::Main => "audiocontrol",
            LoggingSubsystem::Api => "audiocontrol::api",
            LoggingSubsystem::Players => "audiocontrol::players,audiocontrol::players::mpd::libraryloader,audiocontrol::players::lms::libraryloader",
            LoggingSubsystem::Cache => "audiocontrol::helpers::attributecache,audiocontrol::helpers::imagecache",
            LoggingSubsystem::Metadata => "audiocontrol::helpers::musicbrainz,audiocontrol::helpers::theaudiodb,audiocontrol::helpers::lastfm",
            LoggingSubsystem::Spotify => "audiocontrol::helpers::spotify",
            LoggingSubsystem::WebSocket => "audiocontrol::api::websocket,rocket_ws",
            LoggingSubsystem::Library => "audiocontrol::data::library",
            LoggingSubsystem::Security => "audiocontrol::helpers::security_store",
            LoggingSubsystem::Http => "audiocontrol::helpers::http_client,reqwest,hyper",
            LoggingSubsystem::Network => "tokio,mio",
            LoggingSubsystem::Database => "sled",
            LoggingSubsystem::Io => "audiocontrol::helpers::stream_helper",
            LoggingSubsystem::Events => "audiocontrol::audiocontrol::eventbus",
            LoggingSubsystem::Config => "audiocontrol::config",
            LoggingSubsystem::Plugins => "audiocontrol::plugins",
            LoggingSubsystem::Dependencies => "rocket,serde",
        }
    }

    /// Get all available subsystems
    pub fn all() -> Vec<LoggingSubsystem> {
        vec![
            LoggingSubsystem::Main,
            LoggingSubsystem::Api,
            LoggingSubsystem::Players,
            LoggingSubsystem::Cache,
            LoggingSubsystem::Metadata,
            LoggingSubsystem::Spotify,
            LoggingSubsystem::WebSocket,
            LoggingSubsystem::Library,
            LoggingSubsystem::Security,
            LoggingSubsystem::Http,
            LoggingSubsystem::Network,
            LoggingSubsystem::Database,
            LoggingSubsystem::Io,
            LoggingSubsystem::Events,
            LoggingSubsystem::Config,
            LoggingSubsystem::Plugins,
            LoggingSubsystem::Dependencies,
        ]
    }
}

/// Logging configuration structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingConfig {
    /// Global log level (error, warn, info, debug, trace)
    #[serde(default = "default_log_level")]
    pub level: String,
    
    /// Target for log output (stdout, stderr, file)
    #[serde(default = "default_target")]
    pub target: String,
    
    /// Log file path (when target is "file")
    pub file_path: Option<String>,
    
    /// Whether to include timestamps
    #[serde(default = "default_timestamps")]
    pub timestamps: bool,
    
    /// Whether to use colored output
    #[serde(default = "default_colors")]
    pub colors: bool,
    
    /// Subsystem-specific log levels
    #[serde(default, deserialize_with = "deserialize_subsystems")]
    pub subsystems: HashMap<String, String>,
    
    /// Whether to include module paths in log output
    #[serde(default = "default_module_path")]
    pub include_module_path: bool,
    
    /// Whether to include line numbers in log output
    #[serde(default = "default_line_numbers")]
    pub include_line_numbers: bool,
    
    /// Custom environment variable overrides
    #[serde(default)]
    pub env_overrides: HashMap<String, String>,
}

fn default_log_level() -> String {
    "info".to_string()
}

fn default_target() -> String {
    "stdout".to_string()
}

fn default_timestamps() -> bool {
    true
}

fn default_colors() -> bool {
    true
}

fn default_module_path() -> bool {
    false
}

fn default_line_numbers() -> bool {
    false
}

/// Custom deserializer for subsystems that filters out keys starting with underscore
fn deserialize_subsystems<'de, D>(deserializer: D) -> Result<HashMap<String, String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let raw_map = HashMap::<String, String>::deserialize(deserializer)?;
    let filtered_map = raw_map
        .into_iter()
        .filter(|(key, _)| !key.starts_with('_'))
        .collect();
    Ok(filtered_map)
}

impl Default for LoggingConfig {
    fn default() -> Self {
        LoggingConfig {
            level: default_log_level(),
            target: default_target(),
            file_path: None,
            timestamps: default_timestamps(),
            colors: default_colors(),
            subsystems: HashMap::new(),
            include_module_path: default_module_path(),
            include_line_numbers: default_line_numbers(),
            env_overrides: HashMap::new(),
        }
    }
}

impl LoggingConfig {
    /// Load logging configuration from a file
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self, String> {
        let content = fs::read_to_string(path.as_ref())
            .map_err(|e| format!("Failed to read logging config file: {}", e))?;
        
        let config: LoggingConfig = serde_json::from_str(&content)
            .map_err(|e| format!("Failed to parse logging config: {}", e))?;
        
        Ok(config)
    }
    
    /// Load logging configuration from JSON string
    pub fn from_json(json: &str) -> Result<Self, String> {
        serde_json::from_str(json)
            .map_err(|e| format!("Failed to parse logging config JSON: {}", e))
    }
    
    /// Save logging configuration to a file
    pub fn save_to_file<P: AsRef<Path>>(&self, path: P) -> Result<(), String> {
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| format!("Failed to serialize logging config: {}", e))?;
        
        fs::write(path.as_ref(), json)
            .map_err(|e| format!("Failed to write logging config file: {}", e))?;
        
        Ok(())
    }
    
    /// Convert string log level to LevelFilter
    fn parse_log_level(level: &str) -> LevelFilter {
        match level.to_lowercase().as_str() {
            "off" => LevelFilter::Off,
            "error" => LevelFilter::Error,
            "warn" => LevelFilter::Warn,
            "info" => LevelFilter::Info,
            "debug" => LevelFilter::Debug,
            "trace" => LevelFilter::Trace,
            _ => {
                eprintln!("Warning: Unknown log level '{}', defaulting to 'info'", level);
                LevelFilter::Info
            }
        }
    }
    
    /// Build the environment filter string for env_logger
    pub fn build_filter_string(&self) -> String {
        let mut filter_parts = Vec::new();
        
        // Set global default level
        let global_level = &self.level;
        filter_parts.push(global_level.clone());
        
        // Collect all filter entries first
        let mut all_filters = Vec::new();
        
        // Add subsystem-specific levels
        for (subsystem_name, level) in &self.subsystems {
            if let Some(subsystem) = self.parse_subsystem(subsystem_name) {
                let module_prefixes = subsystem.module_prefix();
                for prefix in module_prefixes.split(',') {
                    all_filters.push((prefix.trim().to_string(), level.clone()));
                }
            } else {
                // Allow custom module specifications
                all_filters.push((subsystem_name.clone(), level.clone()));
            }
        }
        
        // Resolve conflicts: if same module path appears multiple times, use most verbose level
        let resolved_filters = self.resolve_filter_conflicts(all_filters);
        
        // Sort by module path length (shorter first, so more specific paths come last)
        let mut sorted_filters: Vec<_> = resolved_filters.into_iter().collect();
        sorted_filters.sort_by_key(|(path, _)| path.len());
        
        // Add sorted filters to filter_parts
        for (path, level) in sorted_filters {
            filter_parts.push(format!("{}={}", path, level));
        }
        
        filter_parts.join(",")
    }
    
    /// Resolve conflicts when the same module path has multiple log levels
    /// Returns the most verbose (debug-like) level and warns about conflicts
    fn resolve_filter_conflicts(&self, filters: Vec<(String, String)>) -> HashMap<String, String> {
        let mut path_levels: HashMap<String, Vec<String>> = HashMap::new();
        
        // Group all levels for each path
        for (path, level) in filters {
            path_levels.entry(path).or_default().push(level);
        }
        
        let mut resolved = HashMap::new();
        
        for (path, levels) in path_levels {
            if levels.len() > 1 {
                // Multiple levels for same path - find most verbose
                let most_verbose = self.find_most_verbose_level(&levels);
                warn!("Conflicting log levels for module '{}': {:?}. Using most verbose: '{}'", 
                      path, levels, most_verbose);
                resolved.insert(path, most_verbose);
            } else {
                // Single level - use as is
                resolved.insert(path, levels.into_iter().next().unwrap());
            }
        }
        
        resolved
    }
    
    /// Find the most verbose (debug-like) log level from a list of levels
    fn find_most_verbose_level(&self, levels: &[String]) -> String {
        // Order from least to most verbose
        let verbosity_order = ["off", "error", "warn", "info", "debug", "trace"];
        
        let mut most_verbose_index = 0;
        let mut most_verbose_level = "off".to_string();
        
        for level in levels {
            let level_lower = level.to_lowercase();
            if let Some(index) = verbosity_order.iter().position(|&l| l == level_lower) {
                if index >= most_verbose_index {
                    most_verbose_index = index;
                    most_verbose_level = level_lower.clone();
                }
            }
        }
        
        most_verbose_level
    }
    
    /// Parse subsystem name to enum
    fn parse_subsystem(&self, name: &str) -> Option<LoggingSubsystem> {
        match name.to_lowercase().as_str() {
            "main" => Some(LoggingSubsystem::Main),
            "api" => Some(LoggingSubsystem::Api),
            "players" => Some(LoggingSubsystem::Players),
            "cache" => Some(LoggingSubsystem::Cache),
            "metadata" => Some(LoggingSubsystem::Metadata),
            "spotify" => Some(LoggingSubsystem::Spotify),
            "websocket" => Some(LoggingSubsystem::WebSocket),
            "library" => Some(LoggingSubsystem::Library),
            "security" => Some(LoggingSubsystem::Security),
            "http" => Some(LoggingSubsystem::Http),
            "network" => Some(LoggingSubsystem::Network),
            "database" => Some(LoggingSubsystem::Database),
            "io" => Some(LoggingSubsystem::Io),
            "events" => Some(LoggingSubsystem::Events),
            "config" => Some(LoggingSubsystem::Config),
            "plugins" => Some(LoggingSubsystem::Plugins),
            "deps" | "dependencies" => Some(LoggingSubsystem::Dependencies),
            _ => None,
        }
    }
    
    /// Initialize the logger with this configuration
    pub fn initialize_logger(&self) -> Result<(), String> {
        // Set environment variables from overrides
        for (key, value) in &self.env_overrides {
            std::env::set_var(key, value);
        }
        
        let filter_string = self.build_filter_string();
        debug!("Using logging filter: {}", filter_string);
        
        let mut builder = Builder::new();
        
        // Parse environment variables if they exist
        builder.parse_env("RUST_LOG");
        
        // Set the filter directly
        builder.filter(None, Self::parse_log_level(&self.level));
        
        // Collect all subsystem filters first
        let mut all_filters = Vec::new();
        
        // Add subsystem-specific filters
        for (subsystem_name, level) in &self.subsystems {
            if let Some(subsystem) = self.parse_subsystem(subsystem_name) {
                let module_prefixes = subsystem.module_prefix();
                for prefix in module_prefixes.split(',') {
                    all_filters.push((prefix.trim().to_string(), level.clone()));
                }
            } else {
                // Allow custom module specifications
                all_filters.push((subsystem_name.clone(), level.clone()));
            }
        }
        
        // Resolve conflicts: if same module path appears multiple times, use most verbose level
        let resolved_filters = self.resolve_filter_conflicts(all_filters);
        
        // Sort by module path length (shorter first, so more specific paths are applied last)
        let mut sorted_filters: Vec<_> = resolved_filters.into_iter().collect();
        sorted_filters.sort_by_key(|(path, _)| path.len());
        
        // Apply sorted filters to builder
        for (path, level) in sorted_filters {
            let level_filter = Self::parse_log_level(&level);
            builder.filter(Some(&path), level_filter);
        }
        
        // Configure timestamps
        if !self.timestamps {
            builder.format_timestamp(None);
        }
        
        // Configure colors
        let write_style = if self.colors {
            WriteStyle::Auto
        } else {
            WriteStyle::Never
        };
        builder.write_style(write_style);
        
        // Configure output target
        match self.target.to_lowercase().as_str() {
            "stdout" => {
                builder.target(Target::Stdout);
            }
            "stderr" => {
                builder.target(Target::Stderr);
            }
            "file" => {
                if let Some(_file_path) = &self.file_path {
                    // For file output, we need to set up a custom target
                    // env_logger doesn't directly support file output, so we'll use stderr
                    // and recommend using shell redirection or systemd logging
                    builder.target(Target::Stderr);
                    warn!("File logging target specified but env_logger doesn't support direct file output. Use shell redirection or systemd journal instead.");
                } else {
                    return Err("File target specified but no file_path provided".to_string());
                }
            }
            _ => {
                return Err(format!("Unknown logging target: {}", self.target));
            }
        }
        
        // Configure module path and line numbers
        let include_module_path = self.include_module_path;
        let include_line_numbers = self.include_line_numbers;
        let timestamps = self.timestamps;
        
        builder.format(move |buf, record| {
            let mut output = String::new();
            
            if timestamps {
                output.push_str(&format!("[{}] ", chrono::Local::now().format("%Y-%m-%d %H:%M:%S")));
            }
            
            output.push_str(&format!("[{}] ", record.level()));
            
            if include_module_path {
                if let Some(module) = record.module_path() {
                    output.push_str(&format!("[{}] ", module));
                }
            }
            
            if include_line_numbers {
                if let (Some(file), Some(line)) = (record.file(), record.line()) {
                    output.push_str(&format!("[{}:{}] ", file, line));
                }
            }
            
            output.push_str(&format!("{}", record.args()));
            
            writeln!(buf, "{}", output)
        });
        
        // Initialize the logger
        builder.try_init()
            .map_err(|e| format!("Failed to initialize logger: {}", e))?;
        
        info!("Logging initialized with filter: {}", filter_string);
        Ok(())
    }
    
    /// Create a sample configuration file
    pub fn create_sample_config() -> Self {
        let mut config = LoggingConfig::default();
        
        // Add some example subsystem configurations
        config.subsystems.insert("players".to_string(), "debug".to_string());
        config.subsystems.insert("cache".to_string(), "warn".to_string());
        config.subsystems.insert("network".to_string(), "error".to_string());
        config.subsystems.insert("deps".to_string(), "warn".to_string());
        
        // Add some example environment overrides
        config.env_overrides.insert("RUST_BACKTRACE".to_string(), "1".to_string());
        
        config
    }
}

/// Initialize logging from a configuration file path
pub fn initialize_logging_from_file<P: AsRef<Path>>(config_path: P) -> Result<(), String> {
    let config = LoggingConfig::from_file(config_path)?;
    config.initialize_logger()
}

/// Initialize logging with default configuration
pub fn initialize_default_logging() -> Result<(), String> {
    let config = LoggingConfig::default();
    config.initialize_logger()
}

/// Initialize logging from command line arguments and optional config file
pub fn initialize_logging_with_args(args: &[String], config_file: Option<&Path>) -> Result<(), String> {
    // Check for debug flag in command line arguments
    let debug_mode = args.iter().any(|arg| arg == "--debug" || arg == "-d");
    let verbose_mode = args.iter().any(|arg| arg == "--verbose" || arg == "-v");
    
    // Try to load configuration from file first
    let mut config = if let Some(config_path) = config_file {
        if config_path.exists() {
            LoggingConfig::from_file(config_path)?
        } else {
            return Err(format!("Logging config file {:?} not found", config_path));
        }
    } else {
        LoggingConfig::default()
    };
    
    // Override log level based on command line flags
    if debug_mode {
        config.level = "debug".to_string();
        info!("Debug mode enabled via command line");
    } else if verbose_mode {
        config.level = "debug".to_string();
        info!("Verbose mode enabled via command line");
    }
    
    config.initialize_logger()
}
