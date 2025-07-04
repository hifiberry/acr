# AudioControl Logging Configuration

AudioControl supports comprehensive logging configuration through JSON configuration files and command-line options. This document describes the available logging subsystems and configuration options.

## Configuration File

The logging configuration uses JSON format and can be specified using the `--log-config` command line option:

```bash
audiocontrol --log-config /etc/audiocontrol/logging.json
```

If no configuration file is specified, AudioControl will look for default files in this order:
1. `/etc/audiocontrol/logging.json`
2. `logging.json` (in current directory)
3. `config/logging.json`

## Configuration Structure

```json
{
  "level": "info",
  "target": "stdout",
  "file_path": "/var/log/audiocontrol.log",
  "timestamps": true,
  "colors": true,
  "include_module_path": false,
  "include_line_numbers": false,
  "subsystems": {
    "players": "debug",
    "cache": "warn",
    "network": "error"
  },
  "env_overrides": {
    "RUST_BACKTRACE": "1"
  }
}
```

## Configuration Options

### Global Options

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| `level` | string | `"info"` | Global log level (off, error, warn, info, debug, trace) |
| `target` | string | `"stdout"` | Output target (stdout, stderr, file) |
| `file_path` | string | null | Log file path (when target is "file") |
| `timestamps` | boolean | `true` | Include timestamps in log output |
| `colors` | boolean | `true` | Use colored output (when supported) |
| `include_module_path` | boolean | `false` | Include Rust module paths in log output |
| `include_line_numbers` | boolean | `false` | Include source file and line numbers |
| `subsystems` | object | `{}` | Subsystem-specific log levels |
| `env_overrides` | object | `{}` | Environment variable overrides |

### Log Levels

Available log levels in order of verbosity:
- `off` - No logging
- `error` - Only errors
- `warn` - Warnings and errors
- `info` - Informational messages, warnings, and errors
- `debug` - Debug messages and all above
- `trace` - All messages including very verbose tracing

## Available Logging Subsystems

### Core Application Subsystems

#### `main`
- **Description**: Main application logic and startup/shutdown
- **Modules**: `audiocontrol`
- **Typical Messages**: Application startup, shutdown, main event loop

#### `api`
- **Description**: REST API server and HTTP endpoints
- **Modules**: `audiocontrol::api`
- **Typical Messages**: HTTP requests, API responses, endpoint routing

#### `players`
- **Description**: Audio player controllers (MPD, RAAT, Librespot, LMS)
- **Modules**: `audiocontrol::players`
- **Typical Messages**: Player state changes, command execution, connection status

#### `cache`
- **Description**: Caching system for metadata and images
- **Modules**: `audiocontrol::helpers::attributecache`, `audiocontrol::helpers::imagecache`
- **Typical Messages**: Cache hits/misses, cache operations, cleanup

#### `metadata`
- **Description**: Music metadata services integration
- **Modules**: `audiocontrol::helpers::musicbrainz`, `audiocontrol::helpers::theaudiodb`, `audiocontrol::helpers::lastfm`
- **Typical Messages**: API requests to metadata services, data parsing

#### `spotify`
- **Description**: Spotify integration and authentication
- **Modules**: `audiocontrol::helpers::spotify`
- **Typical Messages**: OAuth flow, API requests, token management

#### `websocket`
- **Description**: WebSocket connections and real-time updates
- **Modules**: `audiocontrol::api::websocket`, `rocket_ws`
- **Typical Messages**: WebSocket connections, message broadcasting

#### `library`
- **Description**: Music library management and scanning
- **Modules**: `audiocontrol::data::library`
- **Typical Messages**: Library scans, track indexing, metadata updates

#### `security`
- **Description**: Security store and sensitive data handling
- **Modules**: `audiocontrol::helpers::security_store`
- **Typical Messages**: Encryption operations, key management

### Infrastructure Subsystems

#### `http`
- **Description**: HTTP client operations and network requests
- **Modules**: `audiocontrol::helpers::http_client`, `reqwest`, `hyper`
- **Typical Messages**: HTTP requests, response parsing, connection errors

#### `network`
- **Description**: Low-level network operations
- **Modules**: `tokio`, `mio`
- **Typical Messages**: Socket operations, async I/O, connection management

#### `database`
- **Description**: Database operations (Sled embedded database)
- **Modules**: `sled`
- **Typical Messages**: Database transactions, file I/O, compaction

#### `io`
- **Description**: File and stream I/O operations
- **Modules**: `audiocontrol::helpers::stream_helper`
- **Typical Messages**: File operations, pipe handling, stream management

#### `events`
- **Description**: Event handling and notification system
- **Modules**: `audiocontrol::audiocontrol::eventbus`
- **Typical Messages**: Event dispatching, listener notifications

#### `config`
- **Description**: Configuration loading and parsing
- **Modules**: `audiocontrol::config`
- **Typical Messages**: Config file parsing, validation errors

#### `plugins`
- **Description**: Plugin system and extensions
- **Modules**: `audiocontrol::plugins`
- **Typical Messages**: Plugin loading, event filtering

#### `deps` / `dependencies`
- **Description**: Third-party library messages
- **Modules**: `rocket`, `serde`, and other dependencies
- **Typical Messages**: Framework operations, serialization

## Command Line Options

### Logging-Related Flags

- `--debug` or `-d`: Set global log level to debug
- `--verbose` or `-v`: Set global log level to debug (same as --debug)
- `--log-config <path>`: Specify logging configuration file

### Examples

```bash
# Use debug logging
audiocontrol --debug

# Use custom logging configuration
audiocontrol --log-config /etc/audiocontrol/logging.json

# Combine with other options
audiocontrol -c /etc/audiocontrol/config.json --log-config /etc/audiocontrol/logging.json
```

## Configuration Examples

### Development Configuration
```json
{
  "level": "debug",
  "target": "stdout",
  "timestamps": true,
  "colors": true,
  "include_module_path": true,
  "include_line_numbers": true,
  "subsystems": {
    "players": "trace",
    "api": "debug",
    "cache": "debug",
    "deps": "warn"
  }
}
```

### Production Configuration
```json
{
  "level": "info",
  "target": "stderr",
  "timestamps": true,
  "colors": false,
  "include_module_path": false,
  "include_line_numbers": false,
  "subsystems": {
    "players": "info",
    "cache": "warn",
    "network": "error",
    "deps": "error",
    "database": "warn"
  },
  "env_overrides": {
    "RUST_BACKTRACE": "0"
  }
}
```

### Debugging Specific Issues

#### Player Connection Issues
```json
{
  "level": "warn",
  "subsystems": {
    "players": "trace",
    "network": "debug",
    "io": "debug"
  }
}
```

#### Cache Performance Issues
```json
{
  "level": "info",
  "subsystems": {
    "cache": "trace",
    "database": "debug",
    "io": "debug"
  }
}
```

#### API Problems
```json
{
  "level": "info",
  "subsystems": {
    "api": "trace",
    "websocket": "debug",
    "http": "debug"
  }
}
```

## Environment Variables

You can also control logging through environment variables:

- `RUST_LOG`: Standard Rust logging filter string
- `RUST_BACKTRACE`: Enable backtraces (0, 1, or full)

The configuration file's `env_overrides` section allows you to set these automatically.

## Systemd Integration

When running under systemd, logs are automatically captured by the journal. You can view them with:

```bash
# View all audiocontrol logs
journalctl -u audiocontrol

# Follow logs in real-time
journalctl -u audiocontrol -f

# Filter by log level
journalctl -u audiocontrol -p err

# View logs since last boot
journalctl -u audiocontrol -b
```

## File Logging

While the logging system supports file output configuration, it's recommended to use systemd journal logging or shell redirection instead:

```bash
# Redirect to file
audiocontrol 2>/var/log/audiocontrol.log

# Use systemd journal
# (automatic when running as a service)
```

## Custom Module Filters

You can also specify custom module filters in the `subsystems` section using full module paths:

```json
{
  "subsystems": {
    "audiocontrol::players::mpd": "trace",
    "audiocontrol::helpers::spotify": "debug",
    "reqwest": "warn",
    "hyper": "error"
  }
}
```

This provides fine-grained control over logging for specific components.
