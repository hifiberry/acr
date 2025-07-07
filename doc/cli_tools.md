# CLI Tools

AudioControl REST (ACR) includes several command-line tools for interacting with the system. These tools can be useful for debugging, testing, or integrating ACR with other systems.

## Available Tools

### acr_send_update

The `acr_send_update` tool allows you to send player state updates to the AudioControl API from the command line using a subcommand-based interface.

[Detailed Documentation](acr_send_update.md)

**Key Features:**

- Subcommand-based interface for precise control
- Update song information (artist, title, album, duration, URI)
- Update playback state and position
- Update loop mode and shuffle settings
- Send updates to any ACR instance

**Example:**

```bash
audiocontrol_send_update generic song --artist "Pink Floyd" --title "Comfortably Numb" --state Playing
```

### acr_dumpcache

The `acr_dumpcache` tool allows you to inspect and manage the ACR caching system.

**Key Features:**
- List cached items
- View cache metadata
- Clear specific cache entries
- Generate cache statistics

**Example:**
```bash
acr_dumpcache --list-keys
```

### acr_lms_client

The `acr_lms_client` tool provides a command-line interface for interacting with Logitech Media Server instances that are connected to ACR. It is mostly used to debug the connection to and database of the media server.

**Key Features:**
- Query player status
- View server information
- List available players
- Test connectivity

**Example:**
```bash
acr_lms_client --server 192.168.1.100 --list-players
```

### acr_lastfm_auth

The `acr_lastfm_auth` tool provides a command-line interface for authenticating with Last.fm using their desktop authentication flow. This tool helps set up Last.fm integration for scrobbling and other Last.fm features.

[Detailed Documentation](acr_lastfm_auth.md)

**Key Features:**
- Desktop authentication flow
- Credential storage and management
- Reuse of stored credentials

**Example:**
```bash
# Initial authentication
acr_lastfm_auth --api-key YOUR_API_KEY --api-secret YOUR_API_SECRET

# Using saved credentials
acr_lastfm_auth --use-saved
```

## Building the Tools

All tools are built automatically when you build the ACR project:

```bash
cargo build
```

The compiled binaries will be available in the `target/debug/` or `target/release/` directory, depending on your build configuration. All tools follow the naming pattern `acr_*` for consistent identification.

## Integration Examples

These tools can be integrated into scripts, cron jobs, or other systems to automate tasks or extend ACR functionality.

**Example Script:**
```bash
#!/bin/bash
# Script to update player state based on external process

# Check if a specific process is running
if pgrep -x "spotify" > /dev/null; then
    # Update player state to indicate Spotify is active
    audiocontrol_send_update --state Playing spotify-player
else
    # Update player state to indicate Spotify is not active
    audiocontrol_send_update --state Stopped spotify-player
fi
```

## Additional Resources

- [ACR API Documentation](api.md)
- [WebSocket Documentation](websocket.md) for real-time updates
