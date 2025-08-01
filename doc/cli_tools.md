# CLI Tools

AudioControl REST (Audiocontrol) includes several command-line tools for interacting with the system. These tools can be useful for debugging, testing, or integrating Audiocontrol with other systems.

## Available Tools

### audiocontrol_send_update

The `audiocontrol_send_update` tool allows you to send player state updates to the AudioControl API from the command line using a subcommand-based interface.

[Detailed Documentation](audiocontrol_send_update.md)

**Key Features:**

- Subcommand-based interface for precise control
- Update song information (artist, title, album, duration, URI)
- Update playback state and position
- Update loop mode and shuffle settings
- Send updates to any Audiocontrol instance
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

### audiocontrol_lms_client

The `audiocontrol_lms_client` tool provides a command-line interface for interacting with Logitech Media Server instances that are connected to Audiocontrol. It is mostly used to debug the connection to and database of the media server.

**Key Features:**

- Query player status
- View server information
- List available players
- Test connectivity

**Example:**

```bash
audiocontrol_lms_client --server 192.168.1.100 --list-players
```

### audiocontrol_dump_store

The `audiocontrol_dump_store` tool allows you to inspect the contents of the Audiocontrol data store (settings database and other persistent data).

**Key Features:**

- View stored settings and configuration data
- Inspect database contents
- Debug data storage issues

**Example:**

```bash
audiocontrol_dump_store
```

### audiocontrol_musicbrainz_client

The `audiocontrol_musicbrainz_client` tool provides a command-line interface for querying the MusicBrainz database, which is used for music metadata enrichment.

**Key Features:**

- Query artist information from MusicBrainz
- Test MusicBrainz API connectivity
- Debug metadata lookup issues

**Example:**

```bash
audiocontrol_musicbrainz_client --artist "The Beatles"
```

### audiocontrol_list_mpris_players

The `audiocontrol_list_mpris_players` tool lists all available MPRIS (Media Player Remote Interfacing Specification) players on the system.

**Key Features:**

- Discover MPRIS-capable media players
- Show player names and bus addresses
- Test MPRIS connectivity

**Example:**

```bash
audiocontrol_list_mpris_players
```

### audiocontrol_get_mpris_state

The `audiocontrol_get_mpris_state` tool retrieves the current state of a specific MPRIS player.

**Key Features:**

- Get current playback state
- Retrieve song metadata
- Check player capabilities

**Example:**

```bash
audiocontrol_get_mpris_state --player org.mpris.MediaPlayer2.spotify
```

### audiocontrol_monitor_mpris_state

The `audiocontrol_monitor_mpris_state` tool continuously monitors MPRIS player state changes.

**Key Features:**

- Real-time monitoring of player state
- Track song changes and playback events
- Debug MPRIS integration issues

**Example:**

```bash
audiocontrol_monitor_mpris_state --player org.mpris.MediaPlayer2.spotify
```

### audiocontrol_listen_shairportsync

The `audiocontrol_listen_shairportsync` tool monitors Shairport Sync events for AirPlay integration.

**Key Features:**

- Listen for AirPlay connection events
- Monitor metadata changes
- Debug AirPlay integration

**Example:**

```bash
audiocontrol_listen_shairportsync
```

### audiocontrol_favourites

The `audiocontrol_favourites` tool provides a command-line interface for managing favourite songs across multiple providers (LocalDB, Last.fm, etc.). This tool interacts directly with the Favourites API.

**Key Features:**

- Check if songs are marked as favourites
- Add songs to favourites across all enabled providers
- Remove songs from favourites across all enabled providers
- List available favourite providers and their status
- Verbose output for debugging API interactions
- Quiet mode for scripting

**Usage:**

```bash
audiocontrol_favourites [OPTIONS] <COMMAND>
```

**Global Options:**

- `--url <URL>` - AudioControl API base URL (default: `http://localhost:1080`)
- `--verbose, -v` - Enable verbose output with API request/response details
- `--quiet, -q` - Suppress all output except errors
- `--help` - Show help information

**Commands:**

#### check

Check if a song is marked as favourite:

```bash
# Check if a song is favourite
audiocontrol_favourites check --artist "The Beatles" --title "Hey Jude"

# With verbose output (shows API URL and JSON response)
audiocontrol_favourites --verbose check --artist "Queen" --title "Bohemian Rhapsody"
```

**Output:**
```
✓ 'Hey Jude' by 'The Beatles' is marked as favourite
```

#### add

Add a song to favourites across all enabled providers:

```bash
# Add a song to favourites
audiocontrol_favourites add --artist "Pink Floyd" --title "Comfortably Numb"

# With verbose output to see which providers were updated
audiocontrol_favourites --verbose add --artist "Led Zeppelin" --title "Stairway to Heaven"
```

**Output:**
```
✓ Added 'Comfortably Numb' by 'Pink Floyd' to favourites
```

#### remove

Remove a song from favourites across all enabled providers:

```bash
# Remove a song from favourites
audiocontrol_favourites remove --artist "The Beatles" --title "Hey Jude"

# Quiet mode for scripting (no output on success)
audiocontrol_favourites --quiet remove --artist "Queen" --title "Bohemian Rhapsody"
```

**Output:**
```
✓ Removed 'Hey Jude' by 'The Beatles' from favourites
```

#### providers

List available favourite providers and their status:

```bash
# List all providers
audiocontrol_favourites providers

# With verbose output to see additional details
audiocontrol_favourites --verbose providers
```

**Output:**
```
Favourite Providers: 2 enabled out of 2 total

  User settings (settingsdb): ✓ Enabled
  Last.fm (lastfm): ✓ Enabled
```

**API Response Handling:**

The tool automatically handles the API response format where successful responses are wrapped in `"Ok"` and errors in `"Err"`:

- **Successful response**: `{"Ok": {"is_favourite": true, "providers": ["lastfm"]}}`
- **Error response**: `{"Err": {"error": "Missing required parameters"}}`

Note: The `providers` array contains only the providers where the song is actually marked as favourite, not all enabled providers.

**Integration Examples:**

```bash
#!/bin/bash
# Script to mark currently playing song as favourite

# Get current song info (assumes you have a way to get this)
ARTIST="The Beatles"
TITLE="Hey Jude"

# Add to favourites quietly (no output unless error)
audiocontrol_favourites --quiet add --artist "$ARTIST" --title "$TITLE"

if [ $? -eq 0 ]; then
    echo "Successfully added to favourites"
else
    echo "Failed to add to favourites"
fi
```

**Remote Server Usage:**

```bash
# Connect to remote AudioControl instance
audiocontrol_favourites --url http://192.168.1.100:1080 \
  check --artist "David Bowie" --title "Heroes"
```

## Building the Tools

All tools are built automatically when you build the Audiocontrol project:

```bash
cargo build
```

The compiled binaries will be available in the `target/debug/` or `target/release/` directory, depending on your build configuration. All tools follow the naming pattern `audiocontrol_*` for consistent identification.

## Integration Examples

These tools can be integrated into scripts, cron jobs, or other systems to automate tasks or extend Audiocontrol functionality.

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

- [Audiocontrol API Documentation](api.md)
- [WebSocket Documentation](websocket.md) for real-time updates
