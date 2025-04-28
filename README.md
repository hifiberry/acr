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

## License

This project is licensed under the MIT License. See the `debian/copyright` file for more details.