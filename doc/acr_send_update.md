# Using acr_send_update Tool

The `acr_send_update` tool is a command-line utility for sending player state updates to the AudioControl REST API. It provides a simple way to update the state of a player without writing any code or directly interacting with the API.

## Overview

The tool allows you to send updates for various player attributes:
- Song information (artist, title, album, duration)
- Playback position
- Playback state (playing, paused, stopped)
- Loop mode
- Shuffle state

Updates are sent to the AudioControl API endpoint using HTTP POST requests.

## Usage

```
acr_send_update [OPTIONS] <PLAYER_NAME>
```

### Arguments

- `<PLAYER_NAME>`: The name/identifier of the player to update

### Options

| Option | Description |
|--------|-------------|
| `--artist <ARTIST>` | Set the artist name for the current song |
| `--title <TITLE>` | Set the title for the current song |
| `--album <ALBUM>` | Set the album name for the current song |
| `--length <LENGTH>` | Set the song duration in seconds |
| `--position <POSITION>` | Set the current playback position in seconds |
| `--state <STATE>` | Set the playback state (Playing, Paused, Stopped) |
| `--loop-mode <LOOP_MODE>` | Set the loop mode (None, Track, Playlist) |
| `--shuffle <SHUFFLE>` | Set the shuffle state (true or false) |
| `--baseurl <BASEURL>` | Specify the AudioControl API base URL (default: `http://localhost:1080/api`) |

## Examples

### Update Song Information

```bash
acr_send_update my-player --artist "The Beatles" --title "Let It Be" --album "Let It Be" --length 243.5
```

This will update the current song information for the player "my-player".

### Update Playback State

```bash
acr_send_update my-player --state Playing
```

This will set the player state to "Playing".

### Update Playback Position

```bash
acr_send_update my-player --position 120.5
```

This will set the current playback position to 120.5 seconds.

### Multiple Updates at Once

```bash
acr_send_update my-player --artist "Queen" --title "Bohemian Rhapsody" --state Playing --position 45.2
```

This will update the song information, playback state, and position in a single request.

### Update Loop and Shuffle Settings

```bash
acr_send_update my-player --loop-mode Playlist --shuffle true
```

This will set the loop mode to "Playlist" and enable shuffle.

### Using a Different API Host

```bash
acr_send_update --baseurl "http://192.168.1.100:1080/api" my-player --state Paused
```

This will send the update to a different AudioControl API host.

## Response

When an update is successfully sent, the tool will display:
- The URL to which the update was sent
- The JSON payload that was sent
- A success message with the HTTP status code

Example:
```
Sending event to: http://localhost:1080/api/player/my-player/update
Payload: {
  "type": "state_changed",
  "state": "playing"
}
Event sent successfully. Status: 200
```

Note: The tool sends individual events for each type of update. If you specify multiple update types (e.g., both song information and playback state), multiple events will be sent sequentially.

## Integration with Other Systems

The `acr_send_update` tool can be used in scripts or as part of other systems to integrate with the AudioControl API. For example:

### Shell Script Integration

```bash
#!/bin/bash
# Example script that updates player state based on external events

# Update song when a file is played
function on_file_play() {
    acr_send_update my-player --artist "$1" --title "$2" --album "$3" --state Playing
}

# Update playback state
function on_state_change() {
    acr_send_update my-player --state "$1"
}

# Call these functions from your application logic
on_file_play "Artist Name" "Song Title" "Album Name"
```

### Cron Job for Regular Updates

```bash
# Update playback position every 10 seconds
*/10 * * * * acr_send_update my-player --position $(get_current_position_command)
```

## Error Handling

If the API call fails, the tool will display:
- The error message
- The HTTP status code (if the request was received but failed)
- The response body from the server (if available)

## Notes

- If no update options are specified, the tool will display "No updates to send" and exit.
- Updates are sent as JSON in the format expected by the AudioControl API.
- Multiple attributes can be updated in a single call.
- The tool requires network access to the AudioControl API server.
