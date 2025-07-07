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

### audiocontrol_notify_librespot

The `audiocontrol_notify_librespot` tool is designed to be called by librespot on player events. It reads event information from environment variables and sends corresponding updates to the audiocontrol API.

**Key Features:**

- Processes librespot player events automatically
- Reads event data from environment variables
- Sends structured updates to audiocontrol API
- Supports all major librespot event types
- Configurable output verbosity
- Automatic event type detection

**Usage:**

```bash
audiocontrol_notify_librespot [OPTIONS]
```

**Options:**

- `--baseurl <URL>` - API base URL (default: `http://127.0.0.1:1080/api`)
- `--player-name <NAME>` - Player name for API calls (default: `librespot`)
- `--verbose, -v` - Enable verbose output with full request details
- `--quiet, -q` - Suppress all output
- `--help` - Show help information

**Supported Events:**

- `track_changed` - New song/track information
- `playing` - Playback started
- `paused` - Playback paused
- `seeked` - Playback position changed
- `shuffle_changed` - Shuffle mode changed
- `repeat_changed` - Repeat/loop mode changed

**Environment Variables:**

The tool reads the following environment variables set by librespot:

- `PLAYER_EVENT` - Event type
- `NAME` - Track title
- `ARTISTS` - Track artist(s)
- `ALBUM` - Album name
- `DURATION_MS` - Track duration in milliseconds
- `URI` - Spotify URI
- `POSITION_MS` - Current playback position in milliseconds
- `SHUFFLE` - Shuffle state ("true"/"false")
- `REPEAT` - Repeat enabled ("true"/"false")
- `REPEAT_TRACK` - Track repeat enabled ("true"/"false")

**Librespot Configuration:**

To use this tool with librespot, configure it as the onevent handler:

```bash
librespot --onevent /usr/bin/audiocontrol_notify_librespot [other options]
```

Or in librespot configuration file:

```ini
onevent = "/usr/bin/audiocontrol_notify_librespot"
```

**Example Output:**

```bash
# Normal output (default)
audiocontrol_notify_librespot
# Output: Received event: track_changed

# Verbose output
audiocontrol_notify_librespot --verbose
# Output: 
# Received event: track_changed
# Sending event to: http://127.0.0.1:1080/api/player/librespot/update
# Payload: {
#   "type": "song_changed",
#   "song": {
#     "title": "Teenage Kicks",
#     "artist": "The Undertones",
#     "album": "The Undertones",
#     "duration": 148.16,
#     "uri": "spotify:track:5TZcyH9biCPfH8WDiPk8WA"
#   }
# }
# Event sent successfully. Status: 200

# Quiet output (no output)
audiocontrol_notify_librespot --quiet
# (no output)
```

**Integration Examples:**

```bash
# Use with custom API endpoint
audiocontrol_notify_librespot --baseurl http://192.168.1.100:1080/api

# Use with custom player name
audiocontrol_notify_librespot --player-name spotify-connect

# Debugging with verbose output
audiocontrol_notify_librespot --verbose > /var/log/librespot-events.log
```

**Testing Examples with Environment Variables:**

You can test the tool manually by setting environment variables:

```bash
# Test track changed event
export PLAYER_EVENT="track_changed"
export NAME="Teenage Kicks"
export ARTISTS="The Undertones"
export ALBUM="The Undertones"
export DURATION_MS="148160"
export URI="spotify:track:5TZcyH9biCPfH8WDiPk8WA"
export NUMBER="5"
export DISC_NUMBER="1"
export COVERS="https://i.scdn.co/image/ab67616d0000b27340c0d9f7af61bf0543eaf75c"
audiocontrol_notify_librespot --verbose

# Test playing event
export PLAYER_EVENT="playing"
export POSITION_MS="0"
export TRACK_ID="5TZcyH9biCPfH8WDiPk8WA"
audiocontrol_notify_librespot --verbose

# Test paused event
export PLAYER_EVENT="paused"
export POSITION_MS="72434"
export TRACK_ID="5TZcyH9biCPfH8WDiPk8WA"
audiocontrol_notify_librespot --verbose

# Test seeked event
export PLAYER_EVENT="seeked"
export POSITION_MS="106192"
export TRACK_ID="5TZcyH9biCPfH8WDiPk8WA"
audiocontrol_notify_librespot --verbose

# Test shuffle changed event
export PLAYER_EVENT="shuffle_changed"
export SHUFFLE="true"
audiocontrol_notify_librespot --verbose

# Test repeat changed event (track repeat)
export PLAYER_EVENT="repeat_changed"
export REPEAT="true"
export REPEAT_TRACK="true"
audiocontrol_notify_librespot --verbose

# Test repeat changed event (playlist repeat)
export PLAYER_EVENT="repeat_changed"
export REPEAT="true"
export REPEAT_TRACK="false"
audiocontrol_notify_librespot --verbose

# Test repeat disabled
export PLAYER_EVENT="repeat_changed"
export REPEAT="false"
export REPEAT_TRACK="false"
audiocontrol_notify_librespot --verbose
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
