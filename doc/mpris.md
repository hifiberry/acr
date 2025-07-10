# MPRIS Player Controller

This module provides MPRIS (Media Player Remote Interfacing Specification) support for AudioControl, allowing it to control media players that implement the MPRIS D-Bus interface.

## Overview

MPRIS is a standard D-Bus interface specification for media players on Linux and other Unix-like systems. This controller allows AudioControl to communicate with any MPRIS-compatible media player such as:

- VLC Media Player
- Spotify
- Rhythmbox
- Audacious
- Clementine
- And many others

## Features

The MPRIS PlayerController supports:

- **Playback Control**: Play, Pause, Stop, PlayPause
- **Navigation**: Next track, Previous track
- **Seeking**: Seek by offset, Set absolute position
- **Shuffle and Loop**: Control shuffle and loop modes
- **Volume Control**: Set player volume
- **Metadata**: Retrieve current track information
- **State Monitoring**: Get current playback state and position

## Platform Support

MPRIS support is only available on Unix-like systems (Linux, macOS, BSD) where D-Bus is available. It is not supported on Windows.

## Configuration

To use an MPRIS player in AudioControl, add it to your `audiocontrol.json` configuration:

```json
{
  "players": [
    {
      "mpris": {
        "enable": true,
        "bus_name": "org.mpris.MediaPlayer2.vlc"
      }
    }
  ]
}
```

### Configuration Parameters

- `enable`: Boolean flag to enable/disable the player (default: true)
- `bus_name`: The D-Bus name of the MPRIS player (required)

### Finding Available Players

Use the `audiocontrol_list_mpris_players` tool to discover available MPRIS players:

```bash
audiocontrol_list_mpris_players
```

This tool will:
- List all currently running MPRIS players
- Show their capabilities
- Display current status and metadata
- Provide sample configuration

## Common Bus Names

Here are some common MPRIS bus names:

- VLC: `org.mpris.MediaPlayer2.vlc`
- Spotify: `org.mpris.MediaPlayer2.spotify`
- Rhythmbox: `org.mpris.MediaPlayer2.rhythmbox`
- Audacious: `org.mpris.MediaPlayer2.audacious`
- Clementine: `org.mpris.MediaPlayer2.clementine`

## Example Configurations

### Multiple Players

```json
{
  "players": [
    {
      "mpris": {
        "enable": true,
        "bus_name": "org.mpris.MediaPlayer2.vlc"
      }
    },
    {
      "mpris": {
        "enable": true,
        "bus_name": "org.mpris.MediaPlayer2.spotify"
      }
    }
  ]
}
```

### Mixed Player Types

```json
{
  "players": [
    {
      "mpd": {
        "enable": true,
        "host": "localhost",
        "port": 6600
      }
    },
    {
      "mpris": {
        "enable": true,
        "bus_name": "org.mpris.MediaPlayer2.vlc"
      }
    }
  ]
}
```

## API Integration

MPRIS players are fully integrated with the AudioControl API:

- Appear in `/api/players` endpoint
- Support commands via `/api/player/<name>/command/<command>`
- Provide status via `/api/now-playing`
- Expose capabilities in the API response

## Supported Commands

The following commands are supported via the API:

- `play` - Start playback
- `pause` - Pause playback  
- `playpause` - Toggle play/pause
- `stop` - Stop playback
- `next` - Skip to next track
- `previous` - Go to previous track
- `seek:<seconds>` - Seek by offset
- `set_random:true|false` - Enable/disable shuffle
- `set_loop:none|track|playlist` - Set loop mode
- `kill` - Not supported (MPRIS players can't be "killed")

## Limitations

- **Queue Management**: MPRIS doesn't typically expose queue information, so `get_queue()` returns an empty list
- **Player Control**: Can't start/stop the media player application itself
- **Library Access**: No library browsing capabilities (depends on the specific player)
- **Platform Specific**: Only works on systems with D-Bus support

## Troubleshooting

### Player Not Found

If you get "Failed to find MPRIS player" errors:

1. Ensure the media player is running
2. Check that the player supports MPRIS
3. Verify the bus name is correct using `audiocontrol_list_mpris_players`
4. Make sure D-Bus is running on your system

### Permission Issues

Some systems may require specific D-Bus permissions. Check your system's D-Bus configuration if you encounter permission errors.

### Connection Lost

MPRIS connections are automatically re-established when the player restarts. If a player is closed and reopened, the controller will reconnect automatically.

## Dependencies

The MPRIS controller requires:
- `mpris` crate (v2.0.1)
- D-Bus system bus access
- Unix-like operating system

## Development

The MPRIS controller is implemented in `src/players/mpris/mod.rs` and follows the same patterns as other AudioControl player controllers, implementing the `PlayerController` trait.
