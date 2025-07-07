# Using acr_send_update Tool

The `acr_send_update` tool is a command-line utility for sending player state updates to the AudioControl REST API. It provides a simple way to update the state of a player without writing any code or directly interacting with the API.

## Overview

The tool uses a subcommand-based interface where each command corresponds to a specific type of update. This ensures that only the relevant event is sent for each update type, providing more precise control over player state changes.

The tool allows you to send updates for various player attributes:

- Song information (artist, title, album, duration, URI)
- Playback state (playing, paused, stopped, etc.)
- Playback position
- Loop mode
- Shuffle state

Updates are sent to the AudioControl API endpoint using HTTP POST requests.

## Usage

```bash
acr_send_update [OPTIONS] <PLAYER_NAME> <COMMAND>
```

### Arguments

- `<PLAYER_NAME>`: The name/identifier of the player to update

### Global Options

| Option | Description |
|--------|-------------|
| `--baseurl <BASEURL>` | Specify the AudioControl API base URL (default: `http://localhost:1080/api`) |
| `-h, --help` | Display help information and exit |
| `-V, --version` | Display version information and exit |

### Commands

#### `song` - Update Song Information

Updates the current song information and optionally sets the playback state.

```bash
acr_send_update <PLAYER_NAME> song [OPTIONS]
```

| Option | Description |
|--------|-------------|
| `--artist <ARTIST>` | Set the artist name for the current song |
| `--title <TITLE>` | Set the title for the current song |
| `--album <ALBUM>` | Set the album name for the current song |
| `--length <LENGTH>` | Set the song duration in seconds |
| `--uri <URI>` | Set the stream URI for the current song |
| `--state <STATE>` | Set the playback state (default: Playing) |

#### `state` - Update Playback State

Updates the current playback state.

```bash
acr_send_update <PLAYER_NAME> state <STATE>
```

Valid states: `Playing`, `Paused`, `Stopped`, `Killed`, `Disconnected`, `Unknown`

#### `shuffle` - Update Shuffle Setting

Updates the shuffle setting.

```bash
acr_send_update <PLAYER_NAME> shuffle <ENABLED>
```

Values: `true` or `false`

#### `loop` - Update Loop Mode

Updates the loop mode.

```bash
acr_send_update <PLAYER_NAME> loop <MODE>
```

Valid modes: `None`, `Track`, `Playlist`

#### `position` - Update Playback Position

Updates the current playback position.

```bash
acr_send_update <PLAYER_NAME> position <POSITION>
```

Position is specified in seconds.

## Examples

### Update Song Information

```bash
acr_send_update my-player song --artist "The Beatles" --title "Let It Be" --album "Let It Be" --length 243.5
```

This will update the current song information for the player "my-player".

### Update Song with Playback State

```bash
acr_send_update my-player song --artist "Queen" --title "Bohemian Rhapsody" --state Playing
```

This will update the song information and set the playback state to "Playing".

### Update Playback State

```bash
acr_send_update my-player state Playing
```

This will set the player state to "Playing".

### Update Shuffle Setting

```bash
acr_send_update my-player shuffle true
```

This will enable shuffle mode.

### Update Loop Mode

```bash
acr_send_update my-player loop Playlist
```

This will set the loop mode to "Playlist".

### Update Playback Position

```bash
acr_send_update my-player position 120.5
```

This will set the current playback position to 120.5 seconds.

### Using a Different API Host

```bash
acr_send_update --baseurl "http://192.168.1.100:1080/api" my-player state Paused
```

This will send the update to a different AudioControl API host.

## Response

When an update is successfully sent, the tool will display:

- The URL to which the update was sent
- The JSON payload that was sent
- A success message with the HTTP status code

Example:

```json
Sending event to: http://localhost:1080/api/player/my-player/update
Payload: {
  "type": "state_changed",
  "state": "playing"
}
Event sent successfully. Status: 200
```

Note: The tool sends a single event for each command. Each subcommand generates the appropriate event type for that specific update.

## Integration with Other Systems

The `acr_send_update` tool can be used in scripts or as part of other systems to integrate with the AudioControl API. For example:

### Shell Script Integration

```bash
#!/bin/bash
# Example script that updates player state based on external events

# Update song when a file is played
function on_file_play() {
    acr_send_update my-player song --artist "$1" --title "$2" --album "$3" --state Playing
}

# Update playback state
function on_state_change() {
    acr_send_update my-player state "$1"
}

# Call these functions from your application logic
on_file_play "Artist Name" "Song Title" "Album Name"
```

### Cron Job for Regular Updates

```bash
# Update playback position every 10 seconds
*/10 * * * * acr_send_update my-player position $(get_current_position_command)
```

## Error Handling

If the API call fails, the tool will display:

- The error message
- The HTTP status code (if the request was received but failed)
- The response body from the server (if available)

## Notes

- Each command sends a single, specific event to the API
- The `song` command can optionally set the playback state (defaults to "Playing")
- Updates are sent as JSON in the format expected by the AudioControl API
- The tool requires network access to the AudioControl API server
