# CLI Tools

AudioControl REST (ACR) includes several command-line tools for interacting with the system. These tools can be useful for debugging, testing, or integrating ACR with other systems.

## Available Tools

### acr_send_update

The `acr_send_update` tool allows you to send player state updates to the AudioControl API from the command line.

[Detailed Documentation](acr_send_update.md)

**Key Features:**
- Update song information (artist, title, album, duration)
- Update playback state and position
- Update loop mode and shuffle settings
- Send updates to any ACR instance

**Example:**
```bash
acr_send_update my-player --artist "Pink Floyd" --title "Comfortably Numb" --state Playing
```

### dumpcache

The `dumpcache` tool allows you to inspect and manage the ACR caching system.

**Key Features:**
- List cached items
- View cache metadata
- Clear specific cache entries
- Generate cache statistics

**Example:**
```bash
dumpcache --list-keys
```

### lms_client

The `lms_client` tool provides a command-line interface for interacting with Logitech Media Server instances that are connected to ACR. It is mostly used to debug the connection to and database of the media server.

**Key Features:**
- Query player status
- View server information
- List available players
- Test connectivity

**Example:**
```bash
lms_client --server 192.168.1.100 --list-players
```

## Building the Tools

All tools are built automatically when you build the ACR project:

```bash
cargo build
```

The compiled binaries will be available in the `target/debug/` or `target/release/` directory, depending on your build configuration.

## Integration Examples

These tools can be integrated into scripts, cron jobs, or other systems to automate tasks or extend ACR functionality.

**Example Script:**
```bash
#!/bin/bash
# Script to update player state based on external process

# Check if a specific process is running
if pgrep -x "spotify" > /dev/null; then
    # Update player state to indicate Spotify is active
    acr_send_update spotify-player --state Playing
else
    # Update player state to indicate Spotify is not active
    acr_send_update spotify-player --state Stopped
fi
```

## Additional Resources

- [ACR API Documentation](api.md)
- [WebSocket Documentation](websocket.md) for real-time updates
