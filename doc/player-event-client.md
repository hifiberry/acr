# Player Event Client

A command-line tool for sending events to the AudioControl player API. This tool is designed to work with the generic player API and can send various types of events to control player state.

## Installation

Build the tool with Cargo:

```bash
cargo build --bin audiocontrol_player_event_client
```

The binary will be available at `target/debug/audiocontrol_player_event_client` (or `target/release/` for release builds).

## Usage

```bash
audiocontrol_player_event_client [OPTIONS] <PLAYER_NAME> <COMMAND>
```

### Options

- `--host <HOST>`: AudioControl host URL (default: `http://localhost:3000`)

### Commands

#### State Change

Send a playback state change event:

```bash
audiocontrol_player_event_client my_player state-changed playing
audiocontrol_player_event_client my_player state-changed paused
audiocontrol_player_event_client my_player state-changed stopped
```

#### Song Change

Send a song change event:

```bash
audiocontrol_player_event_client my_player song-changed \
  --title "Song Title" \
  --artist "Artist Name" \
  --album "Album Name" \
  --duration 240.5 \
  --uri "https://example.com/song.mp3"
```

Only `--title` is required; other fields are optional.

#### Position Change

Send a position change event:

```bash
audiocontrol_player_event_client my_player position-changed 120.5
```

#### Shuffle Change

Send a shuffle state change event:

```bash
audiocontrol_player_event_client my_player shuffle-changed true
audiocontrol_player_event_client my_player shuffle-changed false
```

#### Loop Mode Change

Send a loop mode change event:

```bash
audiocontrol_player_event_client my_player loop-mode-changed none
audiocontrol_player_event_client my_player loop-mode-changed song
audiocontrol_player_event_client my_player loop-mode-changed playlist
```

#### Queue Change

Send a queue change event from a JSON file:

```bash
audiocontrol_player_event_client my_player queue-changed --file queue.json
```

Or inline JSON:

```bash
audiocontrol_player_event_client my_player queue-changed \
  --json '[{"title": "Track 1", "artist": "Artist 1"}]'
```

Example queue JSON file (`queue.json`):

```json
[
  {
    "title": "Track 1",
    "artist": "Artist 1", 
    "track_number": 1,
    "uri": "file:///path/to/track1.mp3"
  },
  {
    "title": "Track 2",
    "artist": "Artist 2",
    "track_number": 2,
    "uri": "file:///path/to/track2.mp3"
  }
]
```

#### Custom Events

Send custom JSON events:

```bash
audiocontrol_player_event_client my_player custom \
  '{"type": "custom_event", "data": {"custom_field": "value"}}'
```

## Examples

### Basic Song Update

```bash
# Update current song
audiocontrol_player_event_client bedroom_speaker song-changed \
  --title "Bohemian Rhapsody" \
  --artist "Queen" \
  --album "A Night at the Opera" \
  --duration 355.0

# Start playback
audiocontrol_player_event_client bedroom_speaker state-changed playing
```

### Position Updates

```bash
# Seek to 2 minutes
audiocontrol_player_event_client bedroom_speaker position-changed 120.0
```

### Shuffle and Loop

```bash
# Enable shuffle
audiocontrol_player_event_client bedroom_speaker shuffle-changed true

# Set loop mode to playlist
audiocontrol_player_event_client bedroom_speaker loop-mode-changed playlist
```

### Queue Management

```bash
# Update the entire queue
audiocontrol_player_event_client bedroom_speaker queue-changed --file playlist.json
```

### Using with Different Hosts

```bash
# Connect to a remote AudioControl instance
audiocontrol_player_event_client --host http://192.168.1.100:3000 \
  living_room_speaker state-changed playing
```

## Integration Examples

### Shell Scripts

Create a script to update now playing:

```bash
#!/bin/bash
# update_now_playing.sh

PLAYER_NAME="$1"
TITLE="$2"
ARTIST="$3"
ALBUM="$4"

audiocontrol_player_event_client "$PLAYER_NAME" song-changed \
  --title "$TITLE" \
  --artist "$ARTIST" \
  --album "$ALBUM"

audiocontrol_player_event_client "$PLAYER_NAME" state-changed playing
```

### Automation

Use with media players or automation systems:

```bash
# Example: Update from MPD
mpc current -f "%title%|%artist%|%album%" | while IFS='|' read -r title artist album; do
  audiocontrol_player_event_client mpd_generic song-changed \
    --title "$title" \
    --artist "$artist" \
    --album "$album"
done
```

## Error Handling

The tool provides clear error messages and uses appropriate exit codes:

- **Exit code 0**: Success
- **Exit code 1**: Network error, HTTP error, or JSON parsing error
- **Exit code 2**: Command-line argument error

Example output on success:

```text
✓ Event sent successfully. Status: 200
```

Example output on error:

```text
✗ Error sending request: Connection Failed
```

## Related Documentation

- [GenericPlayerController Documentation](../doc/GenericPlayerController.md)
- [API Documentation](../doc/api.md)
- [AudioControl Configuration Guide](../README.md)
