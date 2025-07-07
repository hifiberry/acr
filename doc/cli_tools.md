# CLI Tools

AudioControl REST (ACR) includes several command-line tools for interacting with the system. These tools can be useful for debugging, testing, or integrating ACR with other systems.

## Available Tools

### audiocontrol_send_update

The `audiocontrol_send_update` tool allows you to send player state updates to the AudioControl API from the command line using a subcommand-based interface.

[Detailed Documentation](acr_send_update.md)

**Key Features:**

- Subcommand-based interface for precise control
- Update song information (artist, title, album, duration, URI)
- Update playback state and position
- Update loop mode and shuffle settings
- Send updates to any ACR instance
- Configurable output verbosity (quiet, normal, verbose)

**Usage:**

```bash
audiocontrol_send_update [OPTIONS] <PLAYER_NAME> <SUBCOMMAND>
```

**Options:**

- `--baseurl <URL>` - API base URL (default: `http://localhost:1080/api`)
- `--verbose, -v` - Enable verbose output with JSON payloads
- `--quiet, -q` - Suppress all output
- `--help` - Show help information

**Subcommands:**

#### song

Update song information and optionally set playback state:

```bash
# Update song with basic information
audiocontrol_send_update spotify song --title "Bohemian Rhapsody" --artist "Queen"

# Update song with full metadata
audiocontrol_send_update spotify song \
  --title "Comfortably Numb" \
  --artist "Pink Floyd" \
  --album "The Wall" \
  --length 382.5 \
  --uri "spotify:track:4gMgiXfqyzZLMhsksGmbQV"

# Update song and set state to Paused
audiocontrol_send_update spotify song \
  --title "Hotel California" \
  --artist "Eagles" \
  --state Paused
```

#### state

Update playback state:

```bash
# Set player to playing
audiocontrol_send_update spotify state Playing

# Pause playback
audiocontrol_send_update spotify state Paused

# Stop playback
audiocontrol_send_update spotify state Stopped
```

#### shuffle

Update shuffle/random mode:

```bash
# Enable shuffle
audiocontrol_send_update spotify shuffle true

# Disable shuffle
audiocontrol_send_update spotify shuffle false
```

#### loop

Update loop/repeat mode:

```bash
# Enable track repeat
audiocontrol_send_update spotify loop track

# Enable playlist repeat
audiocontrol_send_update spotify loop playlist

# Disable repeat
audiocontrol_send_update spotify loop none
```

#### position

Update playback position:

```bash
# Set position to 120 seconds
audiocontrol_send_update spotify position 120.5

# Set position to beginning
audiocontrol_send_update spotify position 0
```

**Output Control Examples:**

```bash
# Normal output (default)
audiocontrol_send_update spotify state Playing
# Output:
# Sending event to: http://localhost:1080/api/player/spotify/update
# Event sent successfully. Status: 200

# Verbose output (shows JSON payload)
audiocontrol_send_update --verbose spotify state Playing
# Output:
# Sending event to: http://localhost:1080/api/player/spotify/update
# Payload: {
#   "state": "playing",
#   "type": "state_changed"
# }
# Event sent successfully. Status: 200

# Quiet output (no output on success)
audiocontrol_send_update --quiet spotify state Playing
# (no output)
```

**Integration Examples:**

```bash
# Send multiple updates in sequence
audiocontrol_send_update --quiet spotify song \
  --title "Stairway to Heaven" \
  --artist "Led Zeppelin" \
  --album "Led Zeppelin IV"

audiocontrol_send_update --quiet spotify state Playing
audiocontrol_send_update --quiet spotify position 45.2

# Use with remote server
audiocontrol_send_update --baseurl http://192.168.1.100:1080/api \
  spotify song --title "Imagine" --artist "John Lennon"
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
