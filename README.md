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

## Architecture

AudioControl/Rust uses a player controller abstraction to handle different audio player backends uniformly. The AudioController acts as a manager for multiple PlayerController instances and provides a unified interface for client applications.

## Configuration

ACR uses a JSON configuration file to define its behavior. The configuration file specifies settings for:

- Player backends (MPD, Librespot, etc.)
- API server settings
- Cache locations
- Plugin settings

### Configuration File Locations

ACR requires a valid configuration file to run. By default, it looks for:

1. Path specified with the `-c` command line argument
2. `acr.json` in the current directory

When installed as a system service, ACR uses `/etc/acr/acr.json` as its configuration file. 
If this file doesn't exist, the installation script will copy the sample configuration 
from `/etc/acr/acr.json.sample` automatically.

### Cache Directories

By default, ACR uses these relative paths for cache directories:
- `cache/attributes` - For metadata and other attributes
- `cache/images` - For image files like album covers

If absolute paths are specified in the configuration file (starting with `/`), those exact paths will be used. Otherwise, paths are relative to the current working directory.

When running as a system service, the working directory is `/etc/acr`, so cache directories will be created there if relative paths are used.

### Directory Structure

When installed as a system service, ACR uses the following directory structure:
- `/etc/acr` - Configuration files and default cache location
- `/var/acr` - Variable data directory for runtime files
- `/usr/bin/acr` - The executable binary

Both `/etc/acr` and `/var/acr` directories are owned by the `acr` user and group.

### Command Line Options

ACR supports the following command line options:

- `-c <path>`: Specifies the path to the configuration file
- `--debug`: Enables debug-level logging

## License

This project is licensed under the MIT License. See the `debian/copyright` file for more details.