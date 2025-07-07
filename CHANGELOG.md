# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.4.6] - 2025-07-07

### Added

- Added new `audiocontrol_notify_librespot` tool for librespot integration
- Added support for `seeked` events in librespot notification tool
- Enhanced librespot event handling with position tracking
- Added comprehensive CLI testing examples with environment variables
- Updated documentation with librespot integration examples
- Added man page for `audiocontrol_notify_librespot` tool

## [0.4.5] - 2025-07-07

### Changed

- Refactored `audiocontrol_send_update` CLI tool to use explicit subcommands
- Added `--quiet` and `--verbose` output options to `audiocontrol_send_update`
- Enhanced librespot player with configurable API event support
- Added `enable_api_updates` configuration option for librespot player
- Improved event propagation and notification handling in librespot
- Updated CLI documentation with new subcommand usage examples

### Fixed

- Fixed librespot event handling to preserve song info during state updates

## [0.4.4] - 2025-07-07

### Added

- Enhanced logging system with module-specific log level configuration
- Added API endpoint `/api/audiodb/mbid/<mbid>` for TheAudioDB integration testing

### Changed

- Moved example web application from `example-app/` to `example/web/`
- Updated Debian package configuration for new example directory structure
- Removed version numbers from man pages following best practices

### Fixed

- Fixed unused import warnings in stream helper module

## [0.4.3] - 2025-07-04

### Added

- Improved logging system with JSON-based configuration support
- Added flexible logging configuration with subsystem-specific log levels
- Added support for `--log-config` command line option in systemd service
- Added environment variable overrides for logging configuration
- Added comprehensive logging documentation and sample configurations
- Enhanced logging subsystem mappings for better debugging
- Added production-ready logging configuration for deployment

### Fixed

- Fixed logging initialization issues and improved error handling

## [0.4.2] - 2025-06-30

### Added

- Added systemd unit integration for librespot and RAAT backends
- Librespot and RAAT controllers now check if systemd units are active
- Added 'systemd_unit' configuration option (defaults: 'librespot', 'raat')
- Improved error handling with non-blocking systemd checks
- Enhanced player initialization with service status validation

## [0.4.1] - 2025-06-25

### Changed

- Renamed package from hifiberry-acr to hifiberry-audiocontrol
- Updated documentation to reflect new package name
- Renamed binary from acr to audiocontrol

## [0.4.0] - 2025-06-20

### Added

- Changed build system to sbuild
- Streamlined config file syntax with unified services section
- Added Spotify support
- Added Lyrion media server support
- Integrated Last.fm functionality
- Enhanced messaging system
- Implemented new caching system for improved performance
- Added enhanced metadata capabilities (artist images, album art)
- Improved WebSocket API for real-time updates
- Enhanced library management system

### Fixed

- Initial release fixes and improvements
