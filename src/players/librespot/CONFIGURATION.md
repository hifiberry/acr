# Librespot Configuration Example

This example shows how to configure the Librespot player with both pipe reading and API endpoint processing.

## Configuration Options

The Librespot player supports both traditional event pipe reading and modern API endpoint processing. You can enable one or both methods depending on your setup.

### Configuration Fields

- `event_pipe`: Path to the event pipe (default: `/var/run/librespot/events_pipe`)
- `process_name`: Path to the librespot executable (default: `/usr/bin/librespot`)
- `reopen_event_pipe`: Whether to reopen the pipe when it closes (default: `true`)
- `systemd_unit`: Name of the systemd unit to check (optional)
- `enable_pipe_reader`: Enable reading from event pipe (default: `true`)
- `enable_api_processor`: Enable API endpoint processing (default: `true`)

### Example Configurations

#### Both Pipe and API Enabled (Default)
```json
{
  "services": {
    "players": {
      "librespot": {
        "enable": true,
        "event_pipe": "/var/run/librespot/events_pipe",
        "process_name": "/usr/bin/librespot",
        "reopen_event_pipe": true,
        "systemd_unit": "librespot",
        "enable_pipe_reader": true,
        "enable_api_processor": true
      }
    }
  }
}
```

#### API Endpoint Only
```json
{
  "services": {
    "players": {
      "librespot": {
        "enable": true,
        "enable_pipe_reader": false,
        "enable_api_processor": true
      }
    }
  }
}
```

#### Event Pipe Only (Traditional)
```json
{
  "services": {
    "players": {
      "librespot": {
        "enable": true,
        "event_pipe": "/var/run/librespot/events_pipe",
        "reopen_event_pipe": true,
        "enable_pipe_reader": true,
        "enable_api_processor": false
      }
    }
  }
}
```

#### Custom Event Pipe Path
```json
{
  "services": {
    "players": {
      "librespot": {
        "enable": true,
        "event_pipe": "/tmp/spotify_events",
        "process_name": "/opt/librespot/bin/librespot",
        "systemd_unit": "custom-librespot",
        "enable_pipe_reader": true,
        "enable_api_processor": true
      }
    }
  }
}
```

## Usage

### Using the API Endpoint

When `enable_api_processor` is `true`, you can send events to:
- **Endpoint**: `POST /api/player/librespot/update`
- **Content-Type**: `application/json`

The event format is the same as used by the pipe reader. For detailed information about the event format, see `EVENT_PIPE_FORMAT.md`.

### Example API Usage

```bash
# Send a track change event
curl -X POST http://localhost:1080/api/player/librespot/update \
  -H "Content-Type: application/json" \
  -d '{
    "event": "track_changed",
    "NAME": "Your Song",
    "ARTISTS": "Artist Name",
    "ALBUM": "Album Name",
    "DURATION_MS": "240000",
    "TRACK_ID": "spotify:track:example"
  }'

# Send a playback state change
curl -X POST http://localhost:1080/api/player/librespot/update \
  -H "Content-Type: application/json" \
  -d '{
    "event": "playing",
    "POSITION_MS": "30000",
    "TRACK_ID": "spotify:track:example"
  }'
```

## Benefits

### API Endpoint Benefits
- **Reliability**: No issues with named pipes being unavailable
- **Security**: Can be secured with authentication
- **Network Access**: Can receive events from remote sources
- **Error Handling**: HTTP response codes provide immediate feedback
- **Integration**: Easy to integrate with web services and applications

### Event Pipe Benefits
- **Low Latency**: Direct file system communication
- **Low Overhead**: No HTTP parsing overhead
- **Traditional**: Works with existing librespot configurations
- **Local Only**: Secure by default (local file system access only)

## Migration

If you're currently using only pipe reading, you can:

1. **Add API support**: Set `enable_api_processor: true` while keeping `enable_pipe_reader: true`
2. **Test API functionality**: Send test events to verify everything works
3. **Gradually migrate**: Update your event sources to use the API endpoint
4. **Remove pipe reading**: Set `enable_pipe_reader: false` once API is fully implemented

Both methods can run simultaneously without conflict.
