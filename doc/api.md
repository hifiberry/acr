# Audio Control REST API Documentation

This document describes the REST API endpoints available in the Audio Control REST (ACR) service.

## Table of Contents

- [Base Information](#base-information)
- [Events](#events)
  - [Player Events](#player-events)
- [Core API](#core-api)
  - [Get API Version](#get-api-version)
- [Player API](#player-api)
  - [Get Current Player](#get-current-player)
  - [List Available Players](#list-available-players)
  - [Send Command to Active Player](#send-command-to-active-player)
  - [Send Command to Specific Player](#send-command-to-specific-player)
  - [Player Event Update](#player-event-update)
  - [Get Now Playing Information](#get-now-playing-information)
  - [Get Player Queue](#get-player-queue)
  - [Get Player Metadata](#get-player-metadata)
  - [Get Specific Player Metadata Key](#get-specific-player-metadata-key)
- [Volume Control API](#volume-control-api)
  - [Get Volume Information](#get-volume-information)
  - [Get Current Volume State](#get-current-volume-state)
  - [Set Volume Level](#set-volume-level)
  - [Increase Volume](#increase-volume)
  - [Decrease Volume](#decrease-volume)
  - [Mute/Unmute Volume](#muteunmute-volume)
- [Plugin API](#plugin-api)
  - [List Available Plugins](#list-available-plugins)
  - [Get Plugin Information](#get-plugin-information)
- [Library API](#library-api)
  - [Get Library Information](#get-library-information)
  - [Search Library](#search-library)
  - [Browse Artists](#browse-artists)
  - [Browse Albums](#browse-albums)
  - [Browse Album Tracks](#browse-album-tracks)
  - [Browse Tracks](#browse-tracks)
  - [Browse Playlists](#browse-playlists)
  - [Browse Playlist Tracks](#browse-playlist-tracks)
  - [Browse Genres](#browse-genres)
  - [Browse Files](#browse-files)
  - [Get Library Statistics](#get-library-statistics)
- [External Services API](#external-services-api)
  - [MusicBrainz Integration](#musicbrainz-integration)
  - [TheAudioDB Integration](#theaudiodb-integration)
  - [Last.fm Integration](#lastfm-integration)
  - [Favourites Management](#favourites-management)
- [Lyrics API](#lyrics-api)
  - [Get Lyrics by Song ID](#get-lyrics-by-song-id)
  - [Get Lyrics by Metadata](#get-lyrics-by-metadata)
  - [MPD Integration](#mpd-integration)
- [M3U Playlist API](#m3u-playlist-api)
  - [Parse M3U Playlist](#parse-m3u-playlist)
- [Data Structures](#data-structures)
  - [Album](#album)
  - [Track](#track)
  - [Artist](#artist)
  - [Playlist](#playlist)
  - [Genre](#genre)
  - [File](#file)
- [Generic Player Controller](#generic-player-controller)
  - [Configuration](#configuration)
  - [Event Handling](#event-handling)
  - [Command Processing](#command-processing)
- [Cover Art API](#cover-art-api)
  - [URL-Safe Base64 Encoding](#url-safe-base64-encoding)
  - [Get Cover Art for Artist](#get-cover-art-for-artist)
  - [Get Cover Art for Song](#get-cover-art-for-song)
  - [Get Cover Art for Album](#get-cover-art-for-album)
  - [Get Cover Art for Album with Year](#get-cover-art-for-album-with-year)
  - [Get Cover Art from URL](#get-cover-art-from-url)
  - [List Cover Art Methods and Providers](#list-cover-art-methods-and-providers)
  - [Cover Art Response Format](#cover-art-response-format)
  - [Error Handling](#error-handling)
  - [Provider Registration](#provider-registration)

## Base Information

- **Base URL**: `http://<device-ip>:1080`
- **API Prefix**: All endpoints are prefixed with `/api`
- **Content Type**: All responses are in JSON format
- **Version**: As per current package version

## Events

The ACR system uses an event-based architecture to communicate state changes between components. Events can be monitored via WebSockets or server-sent events (SSE).

For detailed information about WebSocket communication, message formats, and event types, see the [WebSocket API documentation](websocket.md).

### Player Events

These events are emitted when a player's state changes:

- `StateChanged` - Player state has changed (playing, paused, stopped, etc.)
- `SongChanged` - Current song has changed
- `LoopModeChanged` - Loop mode has changed
- `CapabilitiesChanged` - Player capabilities have changed
- `PositionChanged` - Playback position has changed
- `DatabaseUpdating` - Database is being updated
- `QueueChanged` - Queue content has changed (note: many players might not actively emit this event when their queue changes)

Note: Not all players actively emit all event types. In particular, queue changes might not be detected automatically for some player implementations. In this case, manual polling of the queue endpoint might be necessary.

## Core API

### Get API Version

Retrieves the current version of the API.

- **Endpoint**: `/api/version`
- **Method**: GET
- **Response**:
  ```json
  {
    "version": "x.y.z"
  }
  ```

#### Example
```bash
curl http://<device-ip>:1080/api/version
```

## Player API

### Get Current Player

Retrieves information about the currently active player.

- **Endpoint**: `/api/player`
- **Method**: GET
- **Response**:
  ```json
  {
    "name": "player-name",
    "id": "player-id",
    "state": "Playing|Paused|Stopped|Unknown",
    "last_seen": "2023-01-01T12:00:00Z" // ISO 8601 format, null if not available
  }
  ```

#### Example
```bash
curl http://<device-ip>:1080/api/player
```

### List Available Players

Retrieves a list of all available audio players.

- **Endpoint**: `/api/players`
- **Method**: GET
- **Response**:
  ```json
  {
    "players": [
      {
        "name": "player-name",
        "id": "player-id",
        "state": "Playing|Paused|Stopped|Unknown",
        "is_active": true,
        "has_library": true,
        "last_seen": "2023-01-01T12:00:00Z"
      }
    ]
  }
  ```

#### Example
```bash
curl http://<device-ip>:1080/api/players
```

### Send Command to Active Player

Sends a playback command to the currently active player.

- **Endpoint**: `/api/player/active/send/<command>`
- **Method**: POST
- **Path Parameters**:
  - `command` (string): The command to send. Supported values:
    - Simple commands: `play`, `pause`, `playpause`, `stop`, `next`, `previous`, `kill`
    - Parameterized commands:
      - `set_loop:none|track|playlist`
      - `seek:<position>` (position in seconds)
      - `set_random:true|false` (or `on|off`, `1|0`)
- **Response**:
  ```json
  {
    "success": true,
    "message": "Command 'play' sent successfully to active player"
  }
  ```
- **Error Response** (400 Bad Request, 500 Internal Server Error):
  ```json
  {
    "success": false,
    "message": "Error message"
  }
  ```

#### Examples
```bash
# Simple command
curl -X POST http://<device-ip>:1080/api/player/active/send/play

# Stop playback
curl -X POST http://<device-ip>:1080/api/player/active/send/stop

# Play/pause toggle
curl -X POST http://<device-ip>:1080/api/player/active/send/playpause

# Next track
curl -X POST http://<device-ip>:1080/api/player/active/send/next

# Set loop mode to playlist
curl -X POST http://<device-ip>:1080/api/player/active/send/set_loop:playlist

# Seek to 30 seconds
curl -X POST http://<device-ip>:1080/api/player/active/send/seek:30.0

# Enable shuffle
curl -X POST http://<device-ip>:1080/api/player/active/send/set_random:true
```

### Send Command to Specific Player

Sends a playback command to a specific player by name.

- **Endpoint**: `/api/player/<player-name>/command/<command>`
- **Method**: POST
- **Path Parameters**:
  - `player-name` (string): The name of the target player. You can use "active" to target the currently active player.
  - `command` (string): The command to send (same options as above, plus the following):    - Queue management commands:
      - `add_track` - Adds a track to the queue. Requires JSON body with `uri` field. Optional `title` and `coverart_url` fields for future use.
      - `remove_track:<position>` - Removes a track at the specified position from the queue.
      - `clear_queue` - Clears the entire queue.
      - `play_queue_index:<index>` - Plays the track at the specified index position in the queue.
- **Request Body** (for `add_track` command):
  ```json
  {
    "uri": "string (required)",
    "title": "string (optional, future use)",
    "coverart_url": "string (optional, future use)"
  }
  ```
- **Response**: Same as "Send Command to Active Player"
- **Error Response** (400 Bad Request, 404 Not Found, 500 Internal Server Error): Same structure as above

#### Examples
```bash
# Play on a specific player
curl -X POST http://<device-ip>:1080/api/player/spotify/command/play

# Pause a specific player
curl -X POST http://<device-ip>:1080/api/player/raat/command/pause

# Send a command to the currently active player (alternative to /api/player/active/send/)
curl -X POST http://<device-ip>:1080/api/player/active/command/play

# Add a track to the queue using JSON body
curl -X POST http://<device-ip>:1080/api/player/mpd/command/add_track \
  -H "Content-Type: application/json" \
  -d '{"uri": "https://example.com/song.mp3"}'

# Add a track with optional metadata (future use)
curl -X POST http://<device-ip>:1080/api/player/mpd/command/add_track \
  -H "Content-Type: application/json" \
  -d '{
    "uri": "file:///path/to/song.mp3",
    "title": "Custom Song Title",
    "coverart_url": "https://example.com/cover.jpg"
  }'

# Remove a track from the queue at position 2
curl -X POST http://<device-ip>:1080/api/player/lms/command/remove_track:2

# Clear the entire queue
curl -X POST http://<device-ip>:1080/api/player/lms/command/clear_queue

# Play the track at index 3 in the queue
curl -X POST http://<device-ip>:1080/api/player/lms/command/play_queue_index:3

# Error examples for add_track command:

# Missing JSON body (returns 400 Bad Request)
curl -X POST http://<device-ip>:1080/api/player/mpd/command/add_track
# Response: {"success": false, "message": "Invalid command: add_track - add_track command requires JSON body with 'uri' field"}

# Invalid JSON structure (returns 400 Bad Request)
curl -X POST http://<device-ip>:1080/api/player/mpd/command/add_track \
  -H "Content-Type: application/json" \
  -d '{"url": "https://example.com/song.mp3"}'
# Response: {"success": false, "message": "Invalid command: add_track - add_track command requires JSON body with 'uri' field"}
```

### Player Event Update

Receives player events via API endpoint. This endpoint allows external systems to send event notifications to players that support API event processing.

**Purpose**: External systems (like Spotify Connect, RAAT bridges, or other audio services) can use this endpoint to inform players about events that occurred elsewhere, such as track changes, playback state changes, or other player-related events.

- **Endpoint**: `/api/player/<player-name>/update`
- **Method**: POST
- **Content-Type**: `application/json`
- **Request Body**: JSON event data in a format specific to the player
- **Response**:

  ```json
  {
    "success": true,
    "message": "Event processed successfully"
  }
  ```

- **Error Response** (400 Bad Request, 500 Internal Server Error):

  ```json
  {
    "success": false,
    "message": "Error message"
  }
  ```

**Note**: Not all players support API event processing. Currently, only Librespot implements this functionality.

#### Player Event API Examples

```bash
# Send a track_changed event to Librespot
curl -X POST http://<device-ip>:1080/api/player/librespot/update \
  -H "Content-Type: application/json" \
  -d '{
    "event": "track_changed",
    "NAME": "Bohemian Rhapsody",
    "ARTISTS": "Queen",
    "ALBUM": "A Night at the Opera",
    "DURATION_MS": "354000",
    "TRACK_ID": "spotify:track:4uLU6hMCjMI75M1A2tKUQC"
  }'

# Send a playing event to Librespot
curl -X POST http://<device-ip>:1080/api/player/librespot/update \
  -H "Content-Type: application/json" \
  -d '{
    "event": "playing",
    "POSITION_MS": "30000",
    "TRACK_ID": "spotify:track:4uLU6hMCjMI75M1A2tKUQC"
  }'

# Try to send an event to a player that doesn't support API events
curl -X POST http://<device-ip>:1080/api/player/mpd/update \
  -H "Content-Type: application/json" \
  -d '{
    "event": "some_event"
  }'
# Response: {"success": false, "message": "Player 'mpd' does not support API event processing"}
```

### Get Now Playing Information

Retrieves information about the currently playing track and player status.

- **Endpoint**: `/api/now-playing`
- **Method**: GET
- **Response**:
  ```json
  {
    "player": {
      "name": "player-name",
      "id": "player-id",
      "state": "Playing|Paused|Stopped|Unknown",
      "is_active": true,
      "has_library": true,
      "last_seen": "2023-01-01T12:00:00Z"
    },
    "song": {
      // Song details (title, artist, album, etc.)
      // May be null if no song is playing
    },
    "state": "Playing|Paused|Stopped|Unknown",
    "shuffle": true,
    "loop_mode": "None|Track|Playlist",
    "position": 123.45 // Current position in seconds, may be null
  }
  ```

#### Example
```bash
curl http://<device-ip>:1080/api/now-playing
```

### Get Player Queue

Retrieves the current queue for a specific player.

- **Endpoint**: `/api/player/<player-name>/queue`
- **Method**: GET
- **Path Parameters**:
  - `player-name` (string): The name of the player. You can use "active" to target the currently active player.
- **Response**:
  ```json
  {
    "player": "player-name",
    "queue": [
      // Track objects in the queue
    ]
  }
  ```
- **Error Response** (404 Not Found): Error message if player not found
- **Note**: While some players will emit `QueueChanged` events when their queue is modified (such as when tracks are added, removed, or reordered), many player implementations might not actively inform about these updates. If you're building a UI that displays queue content, you may need to periodically poll this endpoint to ensure the display remains current.

#### Example
```bash
curl http://<device-ip>:1080/api/player/mpd/queue

# Get queue for the currently active player
curl http://<device-ip>:1080/api/player/active/queue
```

### Get Player Metadata

Retrieves all metadata for a specific player.

- **Endpoint**: `/api/player/<player-name>/meta`
- **Method**: GET
- **Path Parameters**:
  - `player-name` (string): The name of the player. You can use "active" to target the currently active player.
- **Response**:
  ```json
  {
    "player_name": "player-name",
    "metadata": {
      "key1": "value1",
      "key2": "value2"
      // Various metadata key-value pairs
    }
  }
  ```
- **Error Response** (404 Not Found): String error message

#### Example
```bash
curl http://<device-ip>:1080/api/player/mpd/meta

# Get metadata for the currently active player
curl http://<device-ip>:1080/api/player/active/meta
```

### Get Specific Player Metadata Key

Retrieves a specific metadata key for a player.

- **Endpoint**: `/api/player/<player-name>/meta/<key>`
- **Method**: GET
- **Path Parameters**:
  - `player-name` (string): The name of the player. You can use "active" to target the currently active player.
  - `key` (string): The metadata key to retrieve
- **Response**:
  ```json
  {
    "player_name": "player-name",
    "key": "requested-key",
    "value": "metadata-value" // Can be null if key not found
  }
  ```
- **Error Response** (404 Not Found): String error message

#### Example
```bash
curl http://<device-ip>:1080/api/player/mpd/meta/volume

# Get specific metadata for the currently active player
curl http://<device-ip>:1080/api/player/active/meta/volume
```

## Volume Control API

The Volume Control API provides system-wide hardware volume control when supported by the device. This API manages physical audio hardware volume controls (e.g., ALSA controls) rather than software volume levels within individual players.

### Get Volume Information

Retrieves information about the available volume control and current state.

- **Endpoint**: `/api/volume/info`
- **Method**: GET
- **Response**:
  ```json
  {
    "available": true,
    "control_info": {
      "internal_name": "hw:0,0",
      "display_name": "Master Volume",
      "decibel_range": {
        "min_db": -96.0,
        "max_db": 0.0
      }
    },
    "current_state": {
      "percentage": 75.0,
      "decibels": -12.0,
      "raw_value": 120
    },
    "supports_change_monitoring": true
  }
  ```

#### Response Fields

- `available` (boolean): Whether volume control is available on this device
- `control_info` (object): Information about the volume control hardware
  - `internal_name` (string): Internal system name for the volume control
  - `display_name` (string): Human-readable name for the control
  - `decibel_range` (object): Supported decibel range (if available)
    - `min_db` (number): Minimum volume in decibels
    - `max_db` (number): Maximum volume in decibels
- `current_state` (object): Current volume state (if available)
  - `percentage` (number): Current volume as percentage (0-100)
  - `decibels` (number): Current volume in decibels (if supported)
  - `raw_value` (number): Raw hardware control value (implementation specific)
- `supports_change_monitoring` (boolean): Whether the system can monitor volume changes

#### Example
```bash
curl http://<device-ip>:1080/api/volume/info
```

### Get Current Volume State

Retrieves only the current volume state information.

- **Endpoint**: `/api/volume/state`
- **Method**: GET
- **Response**:
  ```json
  {
    "percentage": 75.0,
    "decibels": -12.0,
    "raw_value": 120
  }
  ```
- **Error Response** (503 Service Unavailable):
  ```json
  {
    "success": false,
    "message": "Volume control not available",
    "new_state": null
  }
  ```

#### Example
```bash
curl http://<device-ip>:1080/api/volume/state
```

### Set Volume Level

Sets the volume to a specific level using percentage, decibels, or raw value.

- **Endpoint**: `/api/volume/set`
- **Method**: POST
- **Content-Type**: `application/json`
- **Request Body** (at least one value required):
  ```json
  {
    "percentage": 75.0,
    "decibels": -12.0,
    "raw_value": 120
  }
  ```
- **Response**:
  ```json
  {
    "success": true,
    "message": "Volume set successfully",
    "new_state": {
      "percentage": 75.0,
      "decibels": -12.0,
      "raw_value": 120
    }
  }
  ```
- **Error Response** (400 Bad Request):
  ```json
  {
    "success": false,
    "message": "Volume percentage 150 is out of range (0-100)",
    "new_state": null
  }
  ```

#### Examples
```bash
# Set volume to 50%
curl -X POST http://<device-ip>:1080/api/volume/set \
  -H "Content-Type: application/json" \
  -d '{"percentage": 50.0}'

# Set volume to -20dB
curl -X POST http://<device-ip>:1080/api/volume/set \
  -H "Content-Type: application/json" \
  -d '{"decibels": -20.0}'

# Set volume using raw hardware value
curl -X POST http://<device-ip>:1080/api/volume/set \
  -H "Content-Type: application/json" \
  -d '{"raw_value": 100}'
```

### Increase Volume

Increases the volume by a specified percentage amount.

- **Endpoint**: `/api/volume/increase?<amount>`
- **Method**: POST
- **Query Parameters**:
  - `amount` (number, optional): Percentage to increase (default: 5.0)
- **Response**:
  ```json
  {
    "success": true,
    "message": "Volume increased to 80.0%",
    "new_state": {
      "percentage": 80.0,
      "decibels": -9.5,
      "raw_value": 128
    }
  }
  ```

#### Examples
```bash
# Increase volume by default amount (5%)
curl -X POST http://<device-ip>:1080/api/volume/increase

# Increase volume by 10%
curl -X POST "http://<device-ip>:1080/api/volume/increase?amount=10.0"
```

### Decrease Volume

Decreases the volume by a specified percentage amount.

- **Endpoint**: `/api/volume/decrease?<amount>`
- **Method**: POST
- **Query Parameters**:
  - `amount` (number, optional): Percentage to decrease (default: 5.0)
- **Response**:
  ```json
  {
    "success": true,
    "message": "Volume decreased to 70.0%",
    "new_state": {
      "percentage": 70.0,
      "decibels": -14.5,
      "raw_value": 112
    }
  }
  ```

#### Examples
```bash
# Decrease volume by default amount (5%)
curl -X POST http://<device-ip>:1080/api/volume/decrease

# Decrease volume by 15%
curl -X POST "http://<device-ip>:1080/api/volume/decrease?amount=15.0"
```

### Toggle Mute

Toggles between muted (0% volume) and unmuted (50% volume) states.

- **Endpoint**: `/api/volume/mute`
- **Method**: POST
- **Response**:
  ```json
  {
    "success": true,
    "message": "Volume muted at 0.0%",
    "new_state": {
      "percentage": 0.0,
      "decibels": -96.0,
      "raw_value": 0
    }
  }
  ```

#### Example
```bash
curl -X POST http://<device-ip>:1080/api/volume/mute
```

### Volume Control Notes

- **Hardware Dependency**: Volume control availability depends on the underlying hardware and ALSA configuration
- **System-Wide**: This controls the system's hardware volume, not individual player volumes
- **Range Limits**: Volume values are automatically clamped to valid ranges (0-100% for percentage)
- **Multiple Formats**: You can set volume using percentage (0-100), decibels (if supported), or raw hardware values
- **Priority**: When multiple values are provided in a set request, percentage takes priority, followed by decibels, then raw value
- **Monitoring**: Some systems support volume change monitoring to detect external volume changes (e.g., hardware volume buttons)

## Plugin API

### List Action Plugins

Retrieves a list of all active action plugins.

- **Endpoint**: `/api/plugins/actions`
- **Method**: GET
- **Response**:
  ```json
  {
    "plugins": [
      {
        "name": "plugin-name",
        "version": "x.y.z"
      }
    ]
  }
  ```

#### Example
```bash
curl http://<device-ip>:1080/api/plugins/actions
```

### List Event Filters

Retrieves a list of all active event filters.

- **Endpoint**: `/api/plugins/event-filters`
- **Method**: GET
- **Response**:
  ```json
  {
    "filters": [
      {
        "name": "filter-name",
        "version": "x.y.z"
      }
    ]
  }
  ```

#### Example
```bash
curl http://<device-ip>:1080/api/plugins/event-filters
```

## Library API

### List All Players with Library Information

Retrieves a list of all players and shows whether they offer library functionality.

- **Endpoint**: `/api/library`
- **Method**: GET
- **Response**:
  ```json
  {
    "players": [
      {
        "player_name": "player-name",
        "player_id": "player-id",
        "has_library": true,
        "is_loaded": true
      },
      {
        "player_name": "another-player",
        "player_id": "another-player-id",
        "has_library": false,
        "is_loaded": false
      }
    ]
  }
  ```

#### Example
```bash
curl http://<device-ip>:1080/api/library
```

### Get Library Information

Retrieves library information for a specific player.

- **Endpoint**: `/api/library/<player-name>`
- **Method**: GET
- **Path Parameters**:
  - `player-name` (string): The name of the player
- **Response**:
  ```json
  {
    "player_name": "player-name",
    "player_id": "player-id",
    "has_library": true,
    "is_loaded": true,
    "albums_count": 100,
    "artists_count": 50
  }
  ```
- **Error Response** (404 Not Found): Same structure as successful response but with `has_library: false`

#### Example
```bash
curl http://<device-ip>:1080/api/library/mpd
```

### Get Player Albums

Retrieves all albums for a specific player.

- **Endpoint**: `/api/library/<player-name>/albums`
- **Method**: GET
- **Path Parameters**:
  - `player-name` (string): The name of the player
- **Response**:
  ```json
  {
    "player_name": "player-name",
    "count": 100,
    "albums": [
      // Album objects
    ]
  }
  ```
- **Error Response** (404 Not Found): String error message

#### Examples
```bash
curl http://<device-ip>:1080/api/library/mpd/albums
```

### Get Player Artists

Retrieves all artists for a specific player.

- **Endpoint**: `/api/library/<player-name>/artists`
- **Method**: GET
- **Path Parameters**:
  - `player-name` (string): The name of the player
- **Response**:
  ```json
  {
    "player_name": "player-name",
    "count": 50,
    "artists": [
      // Artist objects with album counts and thumbnail URLs
      {
        "name": "artist-name",
        "id": "12345678",
        "is_multi": false,
        "album_count": 3,
        "thumb_url": ["/path/to/image1.jpg", "/path/to/image2.jpg"]
      }
    ]
  }
  ```
- **Error Response** (404 Not Found): String error message

#### Examples
```bash
curl http://<device-ip>:1080/api/library/mpd/artists
```

### Get Album by ID

Retrieves a specific album by its unique identifier.

- **Endpoint**: `/api/library/<player-name>/album/by-id/<album-id>`
- **Method**: GET
- **Path Parameters**:
  - `player-name` (string): The name of the player
  - `album-id` (string): The unique identifier of the album
- **Response**:
  ```json
  {
    "player_name": "player-name",
    "album": {
      // Album object with its metadata and tracks
      // Will be null if album not found
    }
  }
  ```
- **Error Response** (404 Not Found): String error message

#### Examples
```bash
curl "http://<device-ip>:1080/api/library/mpd/album/by-id/12345678"
```

### Get Artist by Name

Retrieves complete information for a specific artist by name.

- **Endpoint**: `/api/library/<player-name>/artist/by-name/<artist-name>`
- **Method**: GET
- **Path Parameters**:
  - `player-name` (string): The name of the player
  - `artist-name` (string): The name of the artist
- **Response**:
  ```json
  {
    "player_name": "player-name",
    "artist": {
      "id": "12345678",
      "name": "artist-name", 
      "is_multi": false,
      "metadata": {
        "mbid": ["musicbrainz-id-1", "musicbrainz-id-2"],
        "thumb_url": ["/path/to/image1.jpg", "/path/to/image2.jpg"],
        "banner_url": ["/path/to/banner.jpg"],
        "biography": "Artist biography text...",
        "genres": ["rock", "alternative"]
      }
    }
  }
  ```
- **Error Response** (404 Not Found): String error message

#### Example
```bash
curl "http://<device-ip>:1080/api/library/mpd/artist/by-name/Pink%20Floyd"
```

### Get Artist by ID

Retrieves complete information for a specific artist by ID.

- **Endpoint**: `/api/library/<player-name>/artist/by-id/<artist-id>`
- **Method**: GET
- **Path Parameters**:
  - `player-name` (string): The name of the player
  - `artist-id` (string): The unique identifier of the artist
- **Response**: Same structure as "Get Artist by Name"
- **Error Response** (404 Not Found): String error message

#### Example
```bash
curl "http://<device-ip>:1080/api/library/mpd/artist/by-id/12345678"
```

### Get Artist by MusicBrainz ID

Retrieves complete information for a specific artist by MusicBrainz ID.

- **Endpoint**: `/api/library/<player-name>/artist/by-mbid/<mbid>`
- **Method**: GET
- **Path Parameters**:
  - `player-name` (string): The name of the player
  - `mbid` (string): The MusicBrainz ID of the artist
- **Response**: Same structure as "Get Artist by Name"
- **Error Response** (404 Not Found): String error message

#### Example
```bash
curl "http://<device-ip>:1080/api/library/mpd/artist/by-mbid/83d91898-7763-47d7-b03b-b92132375c47"
```

### Get Albums by Artist Name

Retrieves all albums by a specific artist for a player.

- **Endpoint**: `/api/library/<player-name>/albums/by-artist/<artist-name>`
- **Method**: GET
- **Path Parameters**:
  - `player-name` (string): The name of the player
  - `artist-name` (string): The name of the artist
- **Response**:
  ```json
  {
    "player_name": "player-name",
    "artist_name": "artist-name",
    "count": 5,
    "albums": [
      // Album objects for this artist
    ]
  }
  ```
- **Error Response** (404 Not Found): String error message

#### Examples
```bash
curl "http://<device-ip>:1080/api/library/mpd/albums/by-artist/Pink%20Floyd"
```

### Get Albums by Artist ID

Retrieves all albums by a specific artist ID for a player.

- **Endpoint**: `/api/library/<player-name>/albums/by-artist-id/<artist-id>`
- **Method**: GET
- **Path Parameters**:
  - `player-name` (string): The name of the player
  - `artist-id` (string): The unique identifier of the artist
- **Response**: Same structure as "Get Albums by Artist Name"
- **Error Response** (404 Not Found): String error message

#### Examples
```bash
curl "http://<device-ip>:1080/api/library/mpd/albums/by-artist-id/12345678"
```

### Refresh Player Library

Triggers a refresh of the library for a specific player.

- **Endpoint**: `/api/library/<player-name>/refresh`
- **Method**: GET
- **Path Parameters**:
  - `player-name` (string): The name of the player
- **Response**: Same as "Get Library Information"
- **Error Response** (404 Not Found, 500 Internal Server Error): String error message

#### Example
```bash
curl http://<device-ip>:1080/api/library/mpd/refresh
```

### Update Player Library Media Database

Triggers a scan for new files in the underlying system. This is different from refresh in that it asks 
the backend system (e.g., MPD server) to look for new files on disk.

- **Endpoint**: `/api/library/<player-name>/update`
- **Method**: GET
- **Path Parameters**:
  - `player-name` (string): The name of the player
- **Response**:
  ```json
  {
    "player_name": "player-name",
    "update_started": true
  }
  ```
- **Error Response** (404 Not Found): String error message

#### Example
```bash
curl http://<device-ip>:1080/api/library/mpd/update
```

### Get Library Metadata

Retrieves all metadata for a player's library.

- **Endpoint**: `/api/library/<player-name>/meta`
- **Method**: GET
- **Path Parameters**:
  - `player-name` (string): The name of the player
- **Response**:
  ```json
  {
    "player_name": "player-name",
    "metadata": {
      "key1": "value1",
      "key2": "value2"
      // Various metadata key-value pairs
    }
  }
  ```
- **Error Response** (404 Not Found): String error message

#### Example
```bash
curl http://<device-ip>:1080/api/library/mpd/meta
```

### Get Specific Library Metadata Key

Retrieves a specific metadata key for a player's library.

- **Endpoint**: `/api/library/<player-name>/meta/<key>`
- **Method**: GET
- **Path Parameters**:
  - `player-name` (string): The name of the player
  - `key` (string): The metadata key to retrieve
- **Response**:
  ```json
  {
    "player_name": "player-name",
    "key": "requested-key",
    "value": "metadata-value" // Can be null if key not found
  }
  ```
- **Error Response** (404 Not Found): String error message

#### Example
```bash
curl http://<device-ip>:1080/api/library/mpd/meta/album_count
```

### Get Image from Library

Retrieves an image (such as album art) from a player's library.

- **Endpoint**: `/api/library/<player-name>/image/<identifier>`
- **Method**: GET
- **Path Parameters**:
  - `player-name` (string): The name of the player
  - `identifier` (string): The identifier for the image (e.g., "album:12345")
- **Response**: Binary image data with appropriate Content-Type header
- **Error Response** (404 Not Found): String error message

#### Example
```bash
curl http://<device-ip>:1080/api/library/mpd/image/album:12345 --output cover.jpg
```

## External Services API

### TheAudioDB Lookup

Retrieves artist information from TheAudioDB by MusicBrainz ID. This endpoint is primarily used for integration testing to verify that the TheAudioDB module is working correctly.

- **Endpoint**: `/api/audiodb/mbid/<mbid>`
- **Method**: GET
- **Path Parameters**:
  - `mbid` (string): The MusicBrainz ID of the artist to look up
- **Response** (200 OK):

  ```json
  {
    "mbid": "53b106e7-0cc6-42cc-ac95-ed8d30a3a98e",
    "success": true,
    "data": {
      "strArtist": "John Williams",
      "strBiographyEN": "John Towner Williams is an American composer...",
      "strGenre": "Classical",
      "strCountry": "United States",
      "strWebsite": "https://www.johnwilliams.org/"
    },
    "error": null
  }
  ```

- **Response** (404 Not Found):

  ```json
  {
    "mbid": "00000000-0000-0000-0000-000000000000",
    "success": false,
    "data": null,
    "error": "No artist found for MBID: 00000000-0000-0000-0000-000000000000"
  }
  ```

- **Response** (503 Service Unavailable):

  ```json
  {
    "mbid": "53b106e7-0cc6-42cc-ac95-ed8d30a3a98e",
    "success": false,
    "data": null,
    "error": "TheAudioDB lookups are disabled"
  }
  ```

- **Response** (500 Internal Server Error):

  ```json
  {
    "mbid": "53b106e7-0cc6-42cc-ac95-ed8d30a3a98e",
    "success": false,
    "data": null,
    "error": "Failed to send request to TheAudioDB: HTTP request error: status code 404"
  }
  ```

**Configuration Requirements**: This endpoint requires TheAudioDB to be enabled in the configuration with a valid API key:

```json
{
  "services": {
    "theaudiodb": {
      "enable": true,
      "api_key": "your_api_key_here",
      "rate_limit_ms": 500
    }
  }
}
```

#### TheAudioDB API Example

```bash
curl http://<device-ip>:1080/api/audiodb/mbid/53b106e7-0cc6-42cc-ac95-ed8d30a3a98e
```

#### John Williams Response Example

```json
{
  "mbid": "53b106e7-0cc6-42cc-ac95-ed8d30a3a98e",
  "success": true,
  "data": {
    "strArtist": "John Williams",
    "strBiographyEN": "John Towner Williams is an American composer, conductor and pianist...",
    "strGenre": "Classical",
    "strCountry": "United States",
    "strWebsite": "https://www.johnwilliams.org/",
    "strFacebook": "JohnWilliamsComposer",
    "strTwitter": null,
    "strLastFMChart": "https://www.last.fm/music/John+Williams"
  },
  "error": null
}
```

**Rate Limiting**: Requests to this endpoint are rate-limited according to the configured `rate_limit_ms` value (default: 500ms between requests).

**Use Cases**:

- Integration testing of TheAudioDB connectivity
- Validating artist MusicBrainz ID mappings
- Testing external service rate limiting
- Debugging TheAudioDB API configuration

### Favourites API

The Favourites API allows users to manage their favourite songs across multiple providers (LocalDB, Last.fm, etc.). The API supports adding, removing, and checking the favourite status of songs.

#### List Favourite Providers

Retrieves information about available and enabled favourite providers.

- **Endpoint**: `/api/favourites/providers`
- **Method**: GET
- **Response** (200 OK):

  ```json
  {
    "enabled_providers": ["settingsdb", "lastfm", "spotify"],
    "total_providers": 3,
    "enabled_count": 2,
    "providers": [
      {
        "name": "settingsdb",
        "display_name": "User settings",
        "enabled": true,
        "active": true,
        "favourite_count": 25
      },
      {
        "name": "lastfm",
        "display_name": "Last.fm",
        "enabled": true,
        "active": false,
        "favourite_count": null
      },
      {
        "name": "spotify",
        "display_name": "Spotify",
        "enabled": false,
        "active": false,
        "favourite_count": null
      }
    ]
  }
  ```

  - `enabled_providers`: List of provider names that are currently enabled
  - `total_providers`: Total number of providers (enabled and disabled)
  - `enabled_count`: Number of currently enabled providers  
  - `providers`: Detailed information for each provider
    - `name`: Provider identifier (e.g., "settingsdb", "lastfm", "spotify")
    - `display_name`: Human-readable name for the provider (e.g., "User settings", "Last.fm", "Spotify")
    - `enabled`: Whether the provider is currently enabled and available
    - `active`: Whether the provider is currently active (e.g., user logged in for remote providers)
    - `favourite_count`: Number of favorites stored by this provider (null if provider doesn't support counting)

**Example**:
```bash
curl http://<device-ip>:1080/api/favourites/providers
```

#### Check if Song is Favourite

Checks whether a song is marked as favourite by any enabled provider.

- **Endpoint**: `/api/favourites/is_favourite`
- **Method**: GET
- **Query Parameters**:
  - `artist` (string, required): Artist name
  - `title` (string, required): Song title
- **Response** (200 OK):

  ```json
  {
    "Ok": {
      "is_favourite": true,
      "providers": ["Last.fm", "Spotify"]
    }
  }
  ```

  - `is_favourite`: Boolean indicating if the song is marked as favourite by any enabled provider
  - `providers`: Array of provider display names where the song is actually marked as favourite

- **Response** (400 Bad Request):

  ```json
  {
    "Err": {
      "error": "Missing required parameters: artist and title"
    }
  }
  ```

**Example**:
```bash
curl "http://<device-ip>:1080/api/favourites/is_favourite?artist=The%20Beatles&title=Hey%20Jude"
```

#### Add Song to Favourites

Adds a song to favourites across all enabled providers.

- **Endpoint**: `/api/favourites/add`
- **Method**: POST
- **Content-Type**: `application/json`
- **Request Body**:

  ```json
  {
    "artist": "The Beatles",
    "title": "Hey Jude"
  }
  ```

- **Response** (200 OK):

  ```json
  {
    "Ok": {
      "success": true,
      "message": "Added 'Hey Jude' by 'The Beatles' to favourites",
      "providers": ["settingsdb", "lastfm"],
      "updated_providers": ["settingsdb", "lastfm"]
    }
  }
  ```

- **Response** (400 Bad Request):

  ```json
  {
    "Err": {
      "error": "Invalid song: Artist cannot be empty"
    }
  }
  ```

- **Response** (422 Unprocessable Entity):

  ```json
  {
    "Err": {
      "error": "Missing required fields: artist or title"
    }
  }
  ```

**Example**:
```bash
curl -X POST http://<device-ip>:1080/api/favourites/add \
  -H "Content-Type: application/json" \
  -d '{"artist": "The Beatles", "title": "Hey Jude"}'
```

#### Remove Song from Favourites

Removes a song from favourites across all enabled providers.

- **Endpoint**: `/api/favourites/remove`
- **Method**: DELETE
- **Content-Type**: `application/json`
- **Request Body**:

  ```json
  {
    "artist": "The Beatles",
    "title": "Hey Jude"
  }
  ```

- **Response** (200 OK):

  ```json
  {
    "Ok": {
      "success": true,
      "message": "Removed 'Hey Jude' by 'The Beatles' from favourites",
      "providers": ["settingsdb", "lastfm"],
      "updated_providers": ["settingsdb"]
    }
  }
  ```

- **Response** (400 Bad Request):

  ```json
  {
    "Err": {
      "error": "Invalid song: Title cannot be empty"
    }
  }
  ```

**Example**:
```bash
curl -X DELETE http://<device-ip>:1080/api/favourites/remove \
  -H "Content-Type: application/json" \
  -d '{"artist": "The Beatles", "title": "Hey Jude"}'
```

#### Configuration Requirements

The favourites API requires at least one provider to be configured. Available providers include:

**SettingsDB Provider** (Local Storage):
- Always available
- Stores favourites in the local database
- No additional configuration required
- `enabled`: Always true when database is accessible
- `active`: Always true when enabled (no authentication required)

**Last.fm Provider**:
- Requires Last.fm API credentials and user authentication
- `enabled`: True when API credentials are configured
- `active`: True when user is logged in/authenticated with Last.fm
- Configuration example:

```json
{
  "services": {
    "lastfm": {
      "enable": true,
      "api_key": "your_lastfm_api_key",
      "api_secret": "your_lastfm_api_secret",
      "now_playing_enabled": true,
      "scrobble": true
    }
  }
}
```

**Spotify Provider** (Read-Only):
- Requires Spotify authentication via OAuth
- Only supports checking if songs are favourites (read-only)
- Adding/removing favourites must be done through the Spotify app
- `enabled`: True when user has valid Spotify authentication tokens
- `active`: True when enabled (same as enabled for Spotify)
- Uses Spotify Web API to search for songs and check saved track status
- No additional configuration required beyond OAuth authentication

#### Response Format Notes

- All favourites API responses are wrapped in `Ok` for successful operations or `Err` for errors
- The `updated_providers` field shows which providers actually processed the operation successfully
- The `providers` field in favourite status checks returns human-readable display names (e.g., "Last.fm", "Spotify") for better user experience
- Case sensitivity depends on the provider implementation (SettingsDB is case-insensitive)
- Unicode and special characters in artist/title names are supported
- Spotify provider is read-only: it can check favourite status but cannot add/remove favourites

#### Error Handling

Common error scenarios:

- **Missing Parameters**: HTTP 400 with error message
- **Empty Strings**: HTTP 400 with validation error message  
- **Invalid JSON**: HTTP 422 Unprocessable Entity
- **Provider Errors**: Logged but don't prevent other providers from working
- **No Providers Available**: Operations will complete but may have empty `updated_providers`

## Lyrics API

The Lyrics API provides endpoints to retrieve song lyrics for supported players. Currently, only MPD-based players are supported. The API is designed with provider-specific endpoints to allow for future expansion to other music sources.

**Requirements for MPD:**
- Lyrics files must be in `.lrc` format (plain text or timed lyrics)
- Files must be placed alongside music files with the same name but `.lrc` extension
- Both plain text and LRC timed format are supported

For detailed information about the lyrics system, supported formats, file structure, and examples, see the [Lyrics API documentation](lyrics_api.md).

### Get Lyrics by Song ID

Retrieve lyrics for a specific song using its provider-specific song ID.

- **Endpoint**: `/api/lyrics/{provider}/{song_id}`
- **Method**: GET
- **Path Parameters**:
  - `provider` (string): The lyrics provider (currently only "mpd" is supported)
  - `song_id` (string): The provider-specific song ID. For MPD: base64-encoded file path of the song

**Example Request:**
```bash
curl -X GET "http://localhost:1080/api/lyrics/mpd/bXVzaWMvQXJ0aXN0L0FsYnVtL1NvbmcuZmxhYw"
```

**Note**: For MPD, the `song_id` is a URL-safe base64-encoded version of the song's file path. This ID is automatically provided in the song metadata when lyrics are available.

### Get Lyrics by Metadata

Retrieve lyrics by providing song metadata (artist, title, etc.) for a specific provider.

- **Endpoint**: `/api/lyrics/{provider}`
- **Method**: POST
- **Path Parameters**:
  - `provider` (string): The lyrics provider (currently only "mpd" is supported)
- **Request Body**:
  ```json
  {
    "artist": "Artist Name",
    "title": "Song Title",
    "duration": 180.5,
    "album": "Album Name"
  }
  ```

**Required Fields:**
- `artist`: Artist name (string)
- `title`: Song title (string)

**Optional Fields:**
- `duration`: Song duration in seconds (number)
- `album`: Album name (string)

**Example Request:**
```bash
curl -X POST "http://localhost:1080/api/lyrics/mpd" \
  -H "Content-Type: application/json" \
  -d '{
    "artist": "Example Artist",
    "title": "Example Song"
  }'
```

**Response Format (both endpoints):**

Success with timed lyrics:
```json
{
  "found": true,
  "lyrics": {
    "type": "timed",
    "lyrics": [
      {
        "timestamp": 0.0,
        "text": "Verse 1 starts here"
      },
      {
        "timestamp": 15.5,
        "text": "Chorus begins"
      }
    ]
  }
}
```

Success with plain text:
```json
{
  "found": true,
  "lyrics": {
    "type": "plain",
    "text": "Complete song lyrics as plain text"
  }
}
```

Not found:
```json
{
  "found": false,
  "error": "Lyrics not found for this song"
}
```

### MPD Integration

When lyrics are available for the current song, the player metadata includes additional fields:

- `lyrics_available`: Boolean indicating if lyrics exist for this song
- `lyrics_url`: Direct API endpoint for lyrics by song ID (e.g., `/api/lyrics/mpd/{base64_encoded_path}`)
- `lyrics_metadata`: Object containing the song metadata that can be used for POST requests to `/api/lyrics/mpd`

**Example song metadata with lyrics:**
```json
{
  "title": "Example Song",
  "artist": "Example Artist",
  "album": "Example Album",
  "metadata": {
    "lyrics_available": true,
    "lyrics_url": "/api/lyrics/mpd/bXVzaWMvRXhhbXBsZSBBcnRpc3QvRXhhbXBsZSBBbGJ1bS9FeGFtcGxlIFNvbmcuZmxhYw",
    "lyrics_metadata": {
      "artist": "Example Artist",
      "title": "Example Song",
      "album": "Example Album",
      "duration": 180.5
    }
  }
}
```

**Usage:**
- Use the `lyrics_url` for a direct GET request to retrieve lyrics for this specific song
- Use the `lyrics_metadata` object as the request body for a POST to `/api/lyrics/mpd` to find lyrics by metadata

## M3U Playlist API

The M3U Playlist API provides functionality to parse and extract URLs from M3U playlist files. The API can download playlists from remote URLs and parse both simple and extended M3U formats.

**Supported M3U Formats:**
- **Simple M3U**: Plain text format with one URL per line
- **Extended M3U**: Format with metadata including `#EXTM3U` header and `#EXTINF` directives

**Features:**
- HTTP download of remote M3U playlists with configurable timeout
- Parsing of both simple and extended M3U formats
- Extraction of track metadata (title, duration) from extended format
- URL validation and absolute URL resolution
- Support for live streams (duration -1 converted to null)

### Parse M3U Playlist

Parse an M3U playlist from a remote URL and return the contained URLs with metadata.

- **Endpoint**: `/api/m3u/parse`
- **Method**: POST
- **Request Body**:
  ```json
  {
    "url": "http://example.com/playlist.m3u",
    "timeout": 30
  }
  ```

**Required Fields:**
- `url`: URL of the M3U playlist to download and parse (string)

**Optional Fields:**
- `timeout`: Request timeout in seconds (number, default: 30)

**Example Request:**
```bash
curl -X POST "http://localhost:1080/api/m3u/parse" \
  -H "Content-Type: application/json" \
  -d '{
    "url": "http://example.com/playlist.m3u"
  }'
```

**Response Format:**

Success with simple M3U:
```json
{
  "success": true,
  "url": "http://example.com/playlist.m3u",
  "timestamp": "2024-01-01T12:00:00Z",
  "playlist": {
    "is_extended": false,
    "count": 3,
    "entries": [
      {
        "url": "http://example.com/song1.mp3",
        "title": null,
        "duration": null
      },
      {
        "url": "http://example.com/song2.mp3", 
        "title": null,
        "duration": null
      },
      {
        "url": "http://example.com/song3.mp3",
        "title": null,
        "duration": null
      }
    ]
  }
}
```

Success with extended M3U:
```json
{
  "success": true,
  "url": "http://example.com/extended.m3u",
  "timestamp": "2024-01-01T12:00:00Z",
  "playlist": {
    "is_extended": true,
    "count": 2,
    "entries": [
      {
        "url": "http://example.com/song1.mp3",
        "title": "Artist - Song Title",
        "duration": 180.5
      },
      {
        "url": "http://example.com/stream.m3u8",
        "title": "Live Radio Stream",
        "duration": null
      }
    ]
  }
}
```

Error response:
```json
{
  "success": false,
  "error": "Failed to download playlist: connection timeout",
  "url": "http://example.com/invalid.m3u",
  "timestamp": "2024-01-01T12:00:00Z"
}
```

**Common Error Cases:**
- Invalid or malformed URLs
- Network timeouts or connection failures
- Empty or malformed M3U content
- HTTP errors (404, 500, etc.)

**Usage Examples:**

Parse a simple internet radio station playlist:
```bash
curl -X POST "http://localhost:1080/api/m3u/parse" \
  -H "Content-Type: application/json" \
  -d '{
    "url": "http://www.byte.fm/stream/bytefmhq.m3u"
  }'
```

Parse with custom timeout:
```bash
curl -X POST "http://localhost:1080/api/m3u/parse" \
  -H "Content-Type: application/json" \
  -d '{
    "url": "http://example.com/large-playlist.m3u",
    "timeout": 60
  }'
```

## Data Structures

The following section describes the main data structures used in the API responses.

### Album

An Album represents a collection of tracks/songs by one or more artists.

```json
{
  "id": "12345678",
  "name": "Album Name",
  "artists": ["Artist 1", "Artist 2"],
  "release_date": "2023-01-01",
  "tracks_count": 12,
  "tracks": [
    // Track objects (if include_tracks=true)
  ],
  "cover_art": "/path/to/cover.jpg",
  "uri": "file:///music/album/"
}
```

| Field | Type | Description |
|-------|------|-------------|
| id | string | Unique identifier for the album (string representation of a 64-bit hash) |
| name | string | Album name |
| artists | array | List of artist names for this album |
| release_date | string | ISO 8601 formatted date of album release (YYYY-MM-DD), may be null |
| tracks_count | number | Number of tracks on the album |
| tracks | array | Array of Track objects (only included when requested) |
| cover_art | string | URL or path to album cover art image, may be null |
| uri | string | URI/filename of the first song in the album, may be null |

### Artist

An Artist represents a musician or band in the music library.

```json
{
  "id": "87654321",
  "name": "Artist Name",
  "is_multi": false,
  "metadata": {
    "mbid": ["musicbrainz-id-1", "musicbrainz-id-2"],
    "thumb_url": ["/path/to/image1.jpg", "/path/to/image2.jpg"],
    "banner_url": ["/path/to/banner.jpg"],
    "biography": "Artist biography text...",
    "genres": ["rock", "alternative"]
  }
}
```

| Field | Type | Description |
|-------|------|-------------|
| id | string | Unique identifier for the artist (string representation of a 64-bit hash) |
| name | string | Artist name |
| is_multi | boolean | Whether this is a multi-artist entry (e.g., "Artist1, Artist2") |
| metadata | object | Optional metadata information, may be null |
| metadata.mbid | array | List of MusicBrainz IDs for this artist |
| metadata.thumb_url | array | List of thumbnail image URLs |
| metadata.banner_url | array | List of banner image URLs |
| metadata.biography | string | Artist biography, may be null |
| metadata.genres | array | List of music genres associated with this artist |

### Track

A Track represents a single song on an album.

```json
{
  "id": "12345",
  "disc_number": "1",
  "track_number": 5,
  "name": "Track Name",
  "artist": "Track Artist",
  "uri": "file:///music/track.mp3"
}
```

| Field | Type | Description |
|-------|------|-------------|
| id | string | Unique identifier for the track, may be null |
| disc_number | string | Disc number as a string (to support formats like "1/2") |
| track_number | number | Track number on the disc |
| name | string | Track title |
| artist | string | Track-specific artist (only included if different from album artist), may be null |
| uri | string | URI/filename of the track, may be null |

## Generic Player Controller

The `GenericPlayerController` provides a configurable player that can be controlled entirely through the API events. It maintains internal state and can be used to represent external players or services that are controlled through the ACR API.

### Configuration

Multiple generic players can be configured in the JSON configuration file:

```json
{
  "generic_player_1": {
    "type": "generic",
    "name": "generic_player_1",
    "display_name": "Generic Player 1",
    "enable": true,
    "supports_api_events": true,
    "capabilities": ["play", "pause", "stop", "next", "previous", "seek", "shuffle", "loop"],
    "initial_state": "stopped",
    "shuffle": false,
    "loop_mode": "none"
  }
}
```

### Configuration Options

- `name`: Unique identifier for the player instance
- `display_name`: Human-readable name for the player
- `enable`: Whether the player is enabled (default: true)
- `supports_api_events`: Whether the player accepts API events (default: true)
- `capabilities`: Array of supported capabilities (default: ["play", "pause", "stop", "next", "previous"])
- `initial_state`: Initial playback state ("playing", "paused", "stopped")
- `shuffle`: Initial shuffle state (default: false)
- `loop_mode`: Initial loop mode ("none", "song", "playlist")

### Available Capabilities

- `play`: Can start playback
- `pause`: Can pause playback
- `stop`: Can stop playback
- `next`: Can skip to next track
- `previous`: Can skip to previous track
- `seek`: Can seek within track
- `shuffle`: Can toggle shuffle mode
- `loop`: Can set loop mode
- `queue`: Can manage queue
- `volume`: Can control volume

### API Events

The generic player responds to the standard player event API:

```bash
curl -X POST "http://localhost:3000/api/player/generic_player_1/update" \
  -H "Content-Type: application/json" \
  -d '{
    "type": "song_changed",
    "song": {
      "title": "Song Title",
      "artist": "Artist Name",
      "album": "Album Name",
      "duration": 240.5
    }
  }'
```

### Supported Event Types

- `state_changed`: Update playback state
- `song_changed`: Update current song
- `position_changed`: Update playback position
- `loop_mode_changed`: Update loop mode
- `shuffle_changed`: Update shuffle state

### Example API Events

#### State Change

```json
{
  "type": "state_changed",
  "state": "playing"
}
```

#### Song Change

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

#### Position Change

```json
{
  "type": "position_changed",
  "position": 120.5
}
```

### Multiple Instances

Multiple generic players can be configured with different names and used independently:

```json
{
  "player_a": {
    "type": "generic",
    "name": "player_a",
    "display_name": "Player A",
    "capabilities": ["play", "pause", "stop"]
  },
  "player_b": {
    "type": "generic",
    "name": "player_b", 
    "display_name": "Player B",
    "capabilities": ["play", "pause", "stop", "next", "previous", "seek"]
  }
}
```

Each instance has its own API endpoint:

- `POST /api/player/player_a/update`
- `POST /api/player/player_b/update`

## Cover Art API

The Cover Art API provides endpoints to retrieve cover art from registered providers. All text parameters must be encoded using URL-safe base64 encoding.

### URL-Safe Base64 Encoding

Text parameters (artist names, song titles, album titles, URLs) must be encoded using URL-safe base64 encoding without padding. This ensures proper handling of special characters and Unicode text.

**Example encoding:**
```bash
# Using command line tools
echo -n "The Beatles" | base64 -w 0 | tr '+/' '-_' | tr -d '='
# Result: VGhlIEJlYXRsZXM
```

### Get Cover Art for Artist

Retrieves cover art URLs for a specific artist from all registered providers.

- **Endpoint**: `/api/coverart/artist/<artist_b64>`
- **Method**: GET
- **Parameters**:
  - `artist_b64` (string, required): URL-safe base64 encoded artist name
- **Response**:
  ```json
  {
    "results": [
      {
        "provider": {
          "name": "local_files", 
          "display_name": "Local Files"
        },
        "urls": ["file:///music/covers/artist1.jpg", "file:///music/covers/artist2.png"]
      },
      {
        "provider": {
          "name": "theaudiodb",
          "display_name": "TheAudioDB"
        },
        "urls": ["https://www.theaudiodb.com/images/media/artist/thumb/the-beatles.jpg"]
      }
    ]
  }
  ```

#### Example
```bash
# Get cover art for "The Beatles"
curl http://<device-ip>:1080/api/coverart/artist/VGhlIEJlYXRsZXM
```

### Get Cover Art for Song

Retrieves cover art URLs for a specific song from all registered providers.

- **Endpoint**: `/api/coverart/song/<title_b64>/<artist_b64>`
- **Method**: GET
- **Parameters**:
  - `title_b64` (string, required): URL-safe base64 encoded song title
  - `artist_b64` (string, required): URL-safe base64 encoded artist name
- **Response**:
  ```json
  {
    "results": [
      {
        "provider": {
          "name": "local_files",
          "display_name": "Local Files"
        },
        "urls": ["file:///music/artist/album/cover.jpg"]
      },
      {
        "provider": {
          "name": "musicbrainz",
          "display_name": "MusicBrainz"
        },
        "urls": ["https://coverartarchive.org/release/12345/front-500.jpg"]
      }
    ]
  }
  ```

#### Example
```bash
# Get cover art for "Hey Jude" by "The Beatles"
curl http://<device-ip>:1080/api/coverart/song/SGV5IEp1ZGU/VGhlIEJlYXRsZXM
```

### Get Cover Art for Album

Retrieves cover art URLs for a specific album from all registered providers.

- **Endpoint**: `/api/coverart/album/<title_b64>/<artist_b64>`
- **Method**: GET
- **Parameters**:
  - `title_b64` (string, required): URL-safe base64 encoded album title
  - `artist_b64` (string, required): URL-safe base64 encoded artist name
- **Response**:
  ```json
  {
    "results": [
      {
        "provider": {
          "name": "local_files",
          "display_name": "Local Files"
        },
        "urls": ["file:///music/the-beatles/abbey-road/folder.jpg"]
      },
      {
        "provider": {
          "name": "theaudiodb",
          "display_name": "TheAudioDB"
        },
        "urls": ["https://www.theaudiodb.com/images/media/album/thumb/abbey-road.jpg"]
      },
      {
        "provider": {
          "name": "musicbrainz",
          "display_name": "MusicBrainz"
        },
        "urls": ["https://coverartarchive.org/release/67890/front.jpg"]
      }
    ]
  }
  ```

#### Example
```bash
# Get cover art for "Abbey Road" by "The Beatles"
curl http://<device-ip>:1080/api/coverart/album/QWJiZXkgUm9hZA/VGhlIEJlYXRsZXM
```

### Get Cover Art for Album with Year

Retrieves cover art URLs for a specific album with release year from all registered providers.

- **Endpoint**: `/api/coverart/album/<title_b64>/<artist_b64>/<year>`
- **Method**: GET
- **Parameters**:
  - `title_b64` (string, required): URL-safe base64 encoded album title
  - `artist_b64` (string, required): URL-safe base64 encoded artist name
  - `year` (integer, required): Release year
- **Response**:
  ```json
  {
    "results": [
      {
        "provider": {
          "name": "local_files",
          "display_name": "Local Files"  
        },
        "urls": ["file:///music/the-beatles/abbey-road-1969/cover.jpg"]
      },
      {
        "provider": {
          "name": "theaudiodb",
          "display_name": "TheAudioDB"
        },
        "urls": ["https://www.theaudiodb.com/images/media/album/thumb/abbey-road-1969.jpg"]
      }
    ]
  }
  ```

#### Example
```bash
# Get cover art for "Abbey Road" by "The Beatles" from 1969
curl http://<device-ip>:1080/api/coverart/album/QWJiZXkgUm9hZA/VGhlIEJlYXRsZXM/1969
```

### Get Cover Art from URL

Retrieves cover art URLs from a specific source URL from all registered providers.

- **Endpoint**: `/api/coverart/url/<url_b64>`
- **Method**: GET
- **Parameters**:
  - `url_b64` (string, required): URL-safe base64 encoded source URL
- **Response**:
  ```json
  {
    "results": [
      {
        "provider": {
          "name": "url_resolver",
          "display_name": "URL Resolver"
        },
        "urls": ["https://example.com/resolved-image.jpg", "https://example.com/alternative.png"]
      },
      {
        "provider": {
          "name": "metadata_extractor",
          "display_name": "Metadata Extractor"
        },
        "urls": ["data:image/jpeg;base64,/9j/4AAQSkZJRgABAQEAYABgAAD..."]
      }
    ]
  }
  ```

#### Example
```bash
# Get cover art from a specific URL
curl http://<device-ip>:1080/api/coverart/url/aHR0cHM6Ly9leGFtcGxlLmNvbS9hcnRpc3QvaW1hZ2U
```

### List Cover Art Methods and Providers

Retrieves information about available cover art methods and the providers that support each method.

- **Endpoint**: `/api/coverart/methods`
- **Method**: GET
- **Response**:
  ```json
  {
    "methods": [
      {
        "method": "Artist",
        "providers": [
          {
            "name": "local_files",
            "display_name": "Local Files"
          },
          {
            "name": "theaudiodb", 
            "display_name": "TheAudioDB"
          }
        ]
      },
      {
        "method": "Song", 
        "providers": [
          {
            "name": "local_files",
            "display_name": "Local Files"
          },
          {
            "name": "musicbrainz",
            "display_name": "MusicBrainz"
          }
        ]
      },
      {
        "method": "Album",
        "providers": [
          {
            "name": "local_files",
            "display_name": "Local Files"
          },
          {
            "name": "theaudiodb",
            "display_name": "TheAudioDB"
          },
          {
            "name": "musicbrainz",
            "display_name": "MusicBrainz"
          }
        ]
      },
      {
        "method": "Url",
        "providers": [
          {
            "name": "url_resolver",
            "display_name": "URL Resolver"
          },
          {
            "name": "metadata_extractor",
            "display_name": "Metadata Extractor"
          }
        ]
      }
    ]
  }
  ```

#### Example
```bash
# List all cover art methods and their providers
curl http://<device-ip>:1080/api/coverart/methods
```

### Cover Art Response Format

All cover art endpoints return results grouped by provider, with each provider containing:

- **Provider Information**:
  - `name`: Internal provider identifier (string)
  - `display_name`: Human-readable provider name (string)
- **URLs**: Array of cover art URLs from that provider

**URL Types**: Cover art URLs can be:

1. **HTTP/HTTPS URLs**: Direct links to online cover art images
2. **Local file paths**: Paths to locally cached or extracted cover art files (with `file://` prefix)
3. **Data URLs**: Base64-encoded image data (for small images, with `data:image/` prefix)

**Response Structure**:
```json
{
  "results": [
    {
      "provider": {
        "name": "provider_internal_name",
        "display_name": "Human Readable Provider Name"
      },
      "urls": ["url1", "url2", ...]
    }
  ]
}
```

The client application should handle all URL types appropriately and can choose to display provider information to users for transparency about cover art sources.

### Error Handling

- **Invalid base64 encoding**: Returns empty `results` array with warning logged
- **No providers registered**: Returns empty `results` array  
- **Provider errors**: Individual provider failures are handled gracefully; successful providers still return results
- **No results found**: Returns empty `results` array when no providers find cover art

**Error Response Example**:
```json
{
  "results": []
}
```

### Provider Registration

Cover art providers can be registered programmatically using the global cover art manager:

```rust
use crate::helpers::coverart::{get_coverart_manager, CoverartProvider};

// Register a new provider
let manager = get_coverart_manager();
let mut manager_lock = manager.lock().unwrap();
manager_lock.register_provider(Arc::new(my_provider));
```