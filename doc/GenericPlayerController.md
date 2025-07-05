# GenericPlayerController

The `GenericPlayerController` is a configurable player implementation that can be controlled entirely through API events. It maintains internal state and can be used to represent external players or services that are controlled through the ACR API.

## Features

- **Configurable**: Multiple instances can be created with different configurations
- **API-Controlled**: State is managed through API events  
- **Stateful**: Maintains internal state for song, position, playback state, etc.
- **Extensible**: Supports custom capabilities and metadata
- **Multi-Instance**: Multiple players can run simultaneously with different names

## Quick Start

1. **Create a configuration file** (e.g., `generic_config.json`):
```json
{
  "my_player": {
    "type": "generic",
    "name": "my_player",
    "display_name": "My Player",
    "enable": true,
    "supports_api_events": true,
    "capabilities": ["play", "pause", "stop", "next", "previous"],
    "initial_state": "stopped"
  }
}
```

2. **Start ACR with the configuration**:
```bash
./audiocontrol --config generic_config.json
```

3. **Control the player via API**:
```bash
# Update the song
curl -X POST "http://localhost:3000/api/player/my_player/update" \
  -H "Content-Type: application/json" \
  -d '{
    "type": "song_changed",
    "song": {
      "title": "My Song",
      "artist": "My Artist",
      "album": "My Album"
    }
  }'

# Change playback state
curl -X POST "http://localhost:3000/api/player/my_player/update" \
  -H "Content-Type: application/json" \
  -d '{
    "type": "state_changed",
    "state": "playing"
  }'
```

## Configuration Options

| Option | Type | Required | Default | Description |
|--------|------|----------|---------|-------------|
| `name` | string | Yes | - | Unique identifier for the player |
| `display_name` | string | No | `name` | Human-readable name |
| `enable` | boolean | No | `true` | Whether the player is enabled |
| `supports_api_events` | boolean | No | `true` | Whether to accept API events |
| `capabilities` | array | No | `["play", "pause", "stop", "next", "previous"]` | Supported capabilities |
| `initial_state` | string | No | `"stopped"` | Initial playback state |
| `shuffle` | boolean | No | `false` | Initial shuffle state |
| `loop_mode` | string | No | `"none"` | Initial loop mode |

## Capabilities

The following capabilities are supported:

- `play` - Can start playback
- `pause` - Can pause playback  
- `stop` - Can stop playback
- `next` - Can skip to next track
- `previous` - Can skip to previous track
- `seek` - Can seek within track
- `shuffle` - Can toggle shuffle mode
- `loop` - Can set loop mode
- `queue` - Can manage queue
- `volume` - Can control volume

## API Events

The player responds to the following event types:

### State Change
```json
{
  "type": "state_changed",
  "state": "playing"  // "playing", "paused", "stopped"
}
```

### Song Change
```json
{
  "type": "song_changed",
  "song": {
    "title": "Song Title",
    "artist": "Artist Name", 
    "album": "Album Name",
    "duration": 240.5,
    "uri": "https://example.com/song.mp3"
  }
}
```

### Position Change
```json
{
  "type": "position_changed",
  "position": 120.5  // seconds
}
```

### Shuffle Change
```json
{
  "type": "shuffle_changed",
  "shuffle": true
}
```

### Loop Mode Change
```json
{
  "type": "loop_mode_changed",
  "loop_mode": "playlist"  // "none", "song", "playlist"
}
```

### Queue Change
```json
{
  "type": "queue_changed",
  "queue": [
    {
      "title": "Track 1",
      "artist": "Artist 1",
      "track_number": 1
    }
  ]
}
```

## Multiple Players

You can configure multiple generic players:

```json
{
  "living_room": {
    "type": "generic",
    "name": "living_room", 
    "display_name": "Living Room Player",
    "capabilities": ["play", "pause", "stop", "volume"]
  },
  "bedroom": {
    "type": "generic",
    "name": "bedroom",
    "display_name": "Bedroom Player", 
    "capabilities": ["play", "pause", "stop"]
  }
}
```

Each has its own API endpoint:
- `POST /api/player/living_room/update`
- `POST /api/player/bedroom/update`

## Use Cases

- **External Player Integration**: Control external media players through ACR
- **Custom Implementations**: Implement custom playback logic
- **Testing**: Create mock players for testing
- **Bridging**: Bridge between different audio systems
- **Remote Control**: Control remote devices through ACR's API

## Examples

See `example-config-generic.json` for a complete configuration example.

Use the `audiocontrol_player_event_client` tool to easily send events:

```bash
# Build the client tool
cargo build --bin audiocontrol_player_event_client

# Send a song change event
./target/debug/audiocontrol_player_event_client my_player song-changed \
  --title "Song Title" --artist "Artist Name" --album "Album Name"

# Change playback state  
./target/debug/audiocontrol_player_event_client my_player state-changed playing
```

## Client Tool

A dedicated command-line client tool is available for sending events to generic players:

- **Binary name**: `audiocontrol_player_event_client`
- **Documentation**: [Player Event Client Documentation](player-event-client.md)
- **Location**: `src/tools/acr_player_event_client.rs`

## API Documentation

For complete API documentation, see `doc/api.md`.
