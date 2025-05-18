# AudioControl/Rust (ACR)

AudioControl/Rust (ACR) is the next-generation audio control software for HiFiBerry devices, designed as the successor to [audiocontrol2](https://github.com/hifiberry/audiocontrol2). This Rust implementation offers improved performance, reliability, and a more modular architecture compared to its Python predecessor.

## Why a Rewrite?

As the original audiocontrol2 project grew in scope and complexity, it became increasingly difficult to maintain:

- The Python codebase suffered from runtime type errors and fragility in production
- Dynamic typing led to hard-to-diagnose issues that would often only appear at runtime
- The lack of strict interfaces made it challenging to ensure consistent behavior across different player implementations
- Concurrency issues and race conditions became more common as more features were added
- The plugin architecture, while flexible, became unwieldy as the number of plugins increased
- I wanted to learn Rust ;-)

This Rust implementation addresses these issues through strong static typing, a trait-based architecture, and better concurrency management, providing a more robust foundation for future development.

## Features

- Multi-player management with seamless switching between audio sources
- Unified interface for controlling different player backends (MPD, etc.)
- Event-based notification system for player state changes
- Clean separation between audio player control and user interfaces
- Last.fm integration for scrobbling and "now playing" updates
- Secure credential storage with AES-GCM encryption

## TODO

- Lyrics support

## Architecture

AudioControl/Rust uses a player controller abstraction to handle different audio player backends uniformly. The AudioController acts as a manager for multiple PlayerController instances and provides a unified interface for client applications.

## Configuration

ACR uses a JSON configuration file to define its behavior. The configuration file specifies settings for:

- Player backends (MPD, Librespot, etc.)
- API server settings
- Cache locations
- Plugin settings

### Configuration File Locations

ACR requires a valid configuration file to run. The configuration file is looked up in this order:

1. Path specified with the `-c` command line argument
2. `acr.json` in the current directory

When installed as a system service:

- The configuration file is located at `/etc/acr/acr.json`
- If this file doesn't exist during installation, it's automatically created from `/etc/acr/acr.json.sample`
- A default configuration is also available at `/usr/share/acr/acr-default.json`

### Cache Directories

ACR uses these paths for caching data:

- `/etc/acr/cache/attributes` - For metadata and other attributes
- `/etc/acr/cache/images` - For image files like album covers

These paths are automatically created during installation with the correct permissions.

> **Note:** If you're manually editing the configuration file, always use absolute paths starting with `/` to avoid any path resolution issues.

### Directory Structure

When installed as a system service, ACR uses the following directory structure:

- `/etc/acr` - Configuration files and cache location
  - `/etc/acr/acr.json` - Main configuration file
  - `/etc/acr/acr.json.sample` - Sample configuration file
  - `/etc/acr/cache/` - Cache directories
- `/var/acr` - Variable data directory for runtime files
- `/usr/bin/acr` - The executable binary
- `/usr/share/acr/acr-default.json` - Default configuration (backup)

Both `/etc/acr` and `/var/acr` directories are owned by the `acr` user and group, which is automatically created during installation.

### Command Line Options

ACR supports the following command line options:

- `-c <path>`: Specifies the path to the configuration file
- `--debug`: Enables debug-level logging

## Additional Documentation

- [Last.fm Integration](doc/lastfm.md)
- [API Documentation](doc/api.md)
- [Caching](doc/caching.md)
- [Library Management](doc/library.md)
- [WebSocket Support](doc/websocket.md)

## License

This project is licensed under the MIT License. See the `debian/copyright` file for more details.
