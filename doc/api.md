# Audio Control REST API Documentation

This document describes the REST API endpoints available in the Audio Control REST (Audiocontrol) service.

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
  - [Queue Management Commands](#queue-management-commands)
    - [Queue Track Metadata Structure](#queue-track-metadata-structure)
  - [Get Player Metadata](#get-player-metadata)
  - [Get Specific Player Metadata Key](#get-specific-player-metadata-key)
  - [Player Capabilities and Support Matrix](#player-capabilities-and-support-matrix)
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
- [Cover Art API](#cover-art-api)
  - [URL-Safe Base64 Encoding](#url-safe-base64-encoding)
  - [Get Cover Art for Artist](#get-cover-art-for-artist)
  - [Get Cover Art for Song](#get-cover-art-for-song)
  - [Get Cover Art for Album](#get-cover-art-for-album)
  - [Get Cover Art for Album with Year](#get-cover-art-for-album-with-year)
  - [Get Cover Art from URL](#get-cover-art-from-url)
  - [List Cover Art Methods and Providers](#list-cover-art-methods-and-providers)
  - [Update Artist Image](#update-artist-image)
  - [Cover Art Response Format](#cover-art-response-format)
  - [Image Grading System](imagegrading.md)
  - [Error Handling](#error-handling)
  - [Provider Registration](#provider-registration)
<!-- ========================================================================= -->
<!-- IMPORTANT: Settings API should be placed just before Generic Player Controller and Data Structures -->
<!-- Keep Generic Player Controller and Data Structures at the end of the documentation -->
<!-- ========================================================================= -->
- [Settings API](#settings-api)
  - [Get Setting Value](#get-setting-value)
  - [Set Setting Value](#set-setting-value)
- [Cache API](#cache-api)
  - [Get Cache Statistics](#get-cache-statistics)
- [Background Jobs API](#background-jobs-api)
  - [List Background Jobs](#list-background-jobs)
  - [Get Background Job by ID](#get-background-job-by-id)
- [Generic Player Controller](#generic-player-controller)
  - [Configuration](#configuration)
  - [Event Handling](#event-handling)
  - [Command Processing](#command-processing)
- [Data Structures](#data-structures)
  - [Album](#album)
  - [Track](#track)
  - [Artist](#artist)
  - [Playlist](#playlist)
  - [Genre](#genre)
  - [File](#file)

## Base Information

- **Base URL**: `http://<device-ip>:1080`
- **API Prefix**: All endpoints are prefixed with `/api`
- **Content Type**: All responses are in JSON format
- **Version**: As per current package version

## Events

The Audiocontrol system uses an event-based architecture to communicate state changes between components. Events can be monitored via WebSockets or server-sent events (SSE).

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

### Pause All Players

Pauses all available players. If a player does not support pause, it will be stopped instead.

- **Endpoint**: `/api/players/pause-all`
- **Method**: POST
- **Query Parameters**:
  - `except` (optional): Player name, ID, or alias to exclude from the pause operation. Supported aliases:
    - **mpd**: mpd
    - **spotify**: spotifyd, librespot, spotify
    - **raat**: roon, raat
    - **shairport**: airplay, shairport, shairport-sync
    - **lms**: lms, squeezelite
- **Response**:
  ```json
  {
    "success": true,
    "message": "Paused or stopped N players" // or "Paused or stopped N players (skipped 1 player 'player-name')" when using except
  }
  ```

#### Examples
```bash
# Pause all players
curl -X POST http://<device-ip>:1080/api/players/pause-all

# Pause all players except the one named "spotify"
curl -X POST "http://<device-ip>:1080/api/players/pause-all?except=spotify"

# Pause all players except Spotify (using alias)
curl -X POST "http://<device-ip>:1080/api/players/pause-all?except=librespot"
```

### Stop All Players

Stops all available players. If a player does not support stop, it will be paused instead.

- **Endpoint**: `/api/players/stop-all`
- **Method**: POST
- **Query Parameters**:
  - `except` (optional): Player name, ID, or alias to exclude from the stop operation. Supported aliases:
    - **mpd**: mpd
    - **spotify**: spotifyd, librespot, spotify
    - **raat**: roon, raat
    - **shairport**: airplay, shairport, shairport-sync
    - **lms**: lms, squeezelite
- **Response**:
  ```json
  {
    "success": true,
    "message": "Stopped or paused N players" // or "Stopped or paused N players (skipped 1 player 'player-name')" when using except
  }
  ```

#### Examples
```bash
# Stop all players
curl -X POST http://<device-ip>:1080/api/players/stop-all

# Stop all players except the one with ID "mpd:localhost:6600"
curl -X POST "http://<device-ip>:1080/api/players/stop-all?except=mpd:localhost:6600"

# Stop all players except Roon (using alias)
curl -X POST "http://<device-ip>:1080/api/players/stop-all?except=roon"
```

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
  - `command` (string): The command to send. Supported commands include:
    - **Basic playback**: `play`, `pause`, `playpause`, `stop`, `next`, `previous`, `kill`
    - **Playback control**: `seek:<position>`, `set_loop:none|track|playlist`, `set_random:true|false`
    - **Queue management**: `add_track`, `remove_track:<position>`, `clear_queue`, `play_queue_index:<index>`

**Note**: Queue management commands are only supported by certain players (MPD, LMS, Generic Players). See the [Queue Management Commands](#queue-management-commands) section for detailed information about player support and usage.

- **Request Body** (for `add_track` command only):
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

**Basic playback commands:**
```bash
# Play on a specific player
curl -X POST http://<device-ip>:1080/api/player/spotify/command/play

# Pause a specific player
curl -X POST http://<device-ip>:1080/api/player/raat/command/pause

# Send a command to the currently active player (alternative to /api/player/active/send/)
curl -X POST http://<device-ip>:1080/api/player/active/command/play

# Set loop mode to playlist
curl -X POST http://<device-ip>:1080/api/player/mpd/command/set_loop:playlist

# Seek to 2 minutes (120 seconds)
curl -X POST http://<device-ip>:1080/api/player/mpd/command/seek:120.0
```

**Queue management commands** (see [Queue Management Commands](#queue-management-commands) for full details):
```bash
# Add a track to the queue (requires JSON body)
curl -X POST http://<device-ip>:1080/api/player/mpd/command/add_track \
  -H "Content-Type: application/json" \
  -d '{"uri": "artist/album/song.mp3"}'

# Remove a track from the queue at position 2  
curl -X POST http://<device-ip>:1080/api/player/lms/command/remove_track:2

# Clear the entire queue
curl -X POST http://<device-ip>:1080/api/player/lms/command/clear_queue

# Play the track at index 3 in the queue
curl -X POST http://<device-ip>:1080/api/player/lms/command/play_queue_index:3
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
      {
        "id": "track-id-1",
        "name": "Track Title 1",
        "artist": "Artist Name",
        "album": "Album Name",
        "uri": "file:///path/to/track1.mp3",
        "disc_number": "1",
        "track_number": 1
      },
      {
        "id": "track-id-2", 
        "name": "Track Title 2",
        "artist": "Artist Name",
        "album": "Album Name",
        "uri": "https://example.com/stream/track2.mp3",
        "disc_number": "1",
        "track_number": 2
      }
    ]
  }
  ```
- **Error Response** (404 Not Found): 
  ```json
  {
    "success": false,
    "message": "Player 'player-name' not found"
  }
  ```

**Player Support**: Queue retrieval is supported by most players, but the level of detail varies:
- **MPD**: Full queue support with track metadata
- **LMS (Logitech Media Server)**: Full queue support with detailed track information
- **Generic Players**: Queue managed internally through API
- **MPRIS**: Limited queue support (many MPRIS players don't expose queue)
- **RAAT**: Returns empty queue (queue management handled externally)
- **Spotify/Librespot**: Returns empty queue (managed by Spotify service)

**Note**: While some players emit `QueueChanged` events when their queue is modified (such as when tracks are added, removed, or reordered), many player implementations might not actively inform about these updates. If you're building a UI that displays queue content, you may need to periodically poll this endpoint to ensure the display remains current.

#### Examples
```bash
# Get queue for MPD player
curl http://<device-ip>:1080/api/player/mpd/queue

# Get queue for LMS player
curl http://<device-ip>:1080/api/player/lms/queue

# Get queue for the currently active player
curl http://<device-ip>:1080/api/player/active/queue
```

### Queue Management Commands

The following queue management commands can be sent to players using the command endpoints. Note that not all players support all queue operations.

#### Add Track to Queue

Adds a single track to the player's queue.

- **Command**: `add_track`
- **Method**: POST to `/api/player/<player-name>/command/add_track`
- **Request Body** (JSON required):
  ```json
  {
    "uri": "string (required)",
    "metadata": {
      "title": "string (optional)",
      "artist": "string (optional)",
      "album": "string (optional)",
      "coverart_url": "string (optional)",
      "duration": 180.5,
      "genre": "string (optional)",
      "year": 2024,
      "custom_field": "any JSON value (optional)"
    }
  }
  ```
  
  **Note**: The `metadata` field is a flexible object that can contain any key-value pairs. Common metadata fields include:
  - `title`: Track title
  - `artist`: Artist name
  - `album`: Album name
  - `coverart_url`: URL to cover art image
  - `duration`: Track duration in seconds (number)
  - `genre`: Music genre
  - `year`: Release year (number)
  - Any custom fields can be added as needed
- **Supported URI Formats**:
  - **Local files**: `file:///path/to/music/song.mp3`
  - **HTTP streams**: `http://example.com/stream.mp3`
  - **HTTPS streams**: `https://example.com/stream.mp3`
  - **Relative paths**: `artist/album/song.mp3` (for MPD with music directory)

**Player Support**:
- **MPD**: ✅ Full support for all URI types within music directory
- **LMS**: ✅ Full support for local files and streams
- **Generic Players**: ✅ Stores track information for API-driven playback
- **MPRIS**: ❌ Not supported (queue managed by external application)
- **RAAT**: ❌ Not supported (queue managed by RAAT controller)
- **Spotify**: ❌ Not supported (queue managed by Spotify service)

#### Remove Track from Queue

Removes a track at a specific position from the queue.

- **Command**: `remove_track:<position>`
- **Method**: POST to `/api/player/<player-name>/command/remove_track:<position>`
- **Parameters**:
  - `position` (integer): Zero-based index of the track to remove

**Player Support**:
- **MPD**: ✅ Removes track at specified position
- **LMS**: ✅ Removes track at specified position  
- **Generic Players**: ✅ Removes track from internal queue
- **Others**: ❌ Not supported

#### Clear Entire Queue

Removes all tracks from the player's queue.

- **Command**: `clear_queue`
- **Method**: POST to `/api/player/<player-name>/command/clear_queue`

**Player Support**:
- **MPD**: ✅ Clears entire playlist/queue
- **LMS**: ✅ Clears entire queue
- **Generic Players**: ✅ Clears internal queue
- **Others**: ❌ Not supported

#### Play Track by Queue Position

Starts playback of a track at a specific position in the queue.

- **Command**: `play_queue_index:<index>`
- **Method**: POST to `/api/player/<player-name>/command/play_queue_index:<index>`
- **Parameters**:
  - `index` (integer): Zero-based index of the track to play

**Player Support**:
- **MPD**: ✅ Switches to track at specified position
- **LMS**: ✅ Plays track at specified position
- **Generic Players**: ✅ Sets current track in internal queue
- **Others**: ❌ Not supported

#### Queue Management Examples

```bash
# Add a local file to MPD queue
curl -X POST http://<device-ip>:1080/api/player/mpd/command/add_track \
  -H "Content-Type: application/json" \
  -d '{"uri": "artist/album/song.mp3"}'

# Add an HTTP stream to LMS queue
curl -X POST http://<device-ip>:1080/api/player/lms/command/add_track \
  -H "Content-Type: application/json" \
  -d '{"uri": "https://stream.example.com/radio.mp3"}'

# Add a track with metadata for future use
curl -X POST http://<device-ip>:1080/api/player/generic_player_1/command/add_track \
  -H "Content-Type: application/json" \
  -d '{
    "uri": "file:///music/beatles/yellow_submarine.mp3",
    "metadata": {
      "title": "Yellow Submarine",
      "artist": "The Beatles",
      "album": "Yellow Submarine",
      "coverart_url": "https://example.com/covers/yellow_submarine.jpg",
      "duration": 180.5,
      "genre": "Rock",
      "year": 1969
    }
  }'

# Remove track at position 2 from the queue
curl -X POST http://<device-ip>:1080/api/player/mpd/command/remove_track:2

# Clear the entire queue
curl -X POST http://<device-ip>:1080/api/player/lms/command/clear_queue

# Play the track at index 3 in the queue (4th track)
curl -X POST http://<device-ip>:1080/api/player/mpd/command/play_queue_index:3

# Error example: Missing required 'uri' field
curl -X POST http://<device-ip>:1080/api/player/mpd/command/add_track \
  -H "Content-Type: application/json" \
  -d '{"title": "Some Song"}'
# Response: {"success": false, "message": "Invalid command: add_track - add_track command requires JSON body with 'uri' field"}

# Error example: Invalid position (negative index)
curl -X POST http://<device-ip>:1080/api/player/mpd/command/remove_track:-1
# Response: {"success": false, "message": "Invalid command format"}

# Error example: Trying queue operation on unsupported player
curl -X POST http://<device-ip>:1080/api/player/spotify/command/add_track \
  -H "Content-Type: application/json" \
  -d '{"uri": "spotify:track:4uLU6hMCjMI75M1A2tKUQC"}'
# Response: {"success": false, "message": "Queue operations not supported by this player type"}
```

#### Queue Track Metadata Structure

When adding tracks to the queue, you can provide optional metadata that will be cached by certain players (especially MPD). This metadata can enhance the song information when the track is played:

```json
{
  "uri": "string (required)",
  "metadata": {
    "title": "string (optional) - Track title",
    "artist": "string (optional) - Artist name", 
    "album": "string (optional) - Album name",
    "coverart_url": "string (optional) - URL to cover art image",
    "duration": "number (optional) - Track duration in seconds",
    "genre": "string (optional) - Music genre",
    "year": "number (optional) - Release year",
    "custom_field": "any (optional) - Any custom metadata field"
  }
}
```

**Metadata Usage**:
- **MPD**: Stores metadata in an LRU cache (max 1000 entries) and automatically enhances songs when they match the cached URL
- **LMS**: Stores metadata for API-driven playback enhancement
- **Generic Players**: Uses metadata for display and tracking purposes
- **Other Players**: Metadata may be ignored if not supported

**Important Notes**:
- **No Fixed Semantics**: The metadata has no enforced semantics or validation. Field names and values are suggestions only.
- **Player-Specific Handling**: Each player implementation can choose to ignore metadata entirely, handle only specific fields, or process all fields according to their own logic.
- **No Guarantees**: There is no guarantee that provided metadata will be used, stored, or displayed by any player.
- **Best Effort**: Metadata should be considered "best effort" hints to improve the user experience when supported.

**Flexible Schema**: The metadata object accepts any key-value pairs, allowing for custom fields beyond the common ones listed above. All values are stored as JSON values and can be strings, numbers, booleans, or complex objects.

### Queue Events

When queue operations are performed, players may emit events to notify about changes:

- **`QueueChanged`**: Emitted when tracks are added, removed, or reordered
- **`PlaylistChanged`**: Emitted when the current playlist/queue is replaced

**Event Monitoring**: You can listen for these events through the WebSocket API or by polling the queue endpoint periodically.

### Queue Position Indexing

**Important**: Queue positions and indices are **zero-based** across all operations:
- Position `0` = First track in queue
- Position `1` = Second track in queue  
- Position `n-1` = Last track in queue (where n = queue length)

When removing tracks or playing by index, ensure you account for zero-based indexing to avoid off-by-one errors.

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

### Player Capabilities and Support Matrix

Different player implementations support different sets of capabilities. Understanding these differences is important when building applications that work with multiple player types.

#### Capability Overview

The following table shows which capabilities are supported by each player type:

| Capability | MPD | LMS | Generic | MPRIS | RAAT | Spotify | Description |
|------------|-----|-----|---------|-------|------|---------|-------------|
| **Basic Playback** | | | | | | | |
| Play | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | Start playback |
| Pause | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | Pause playback |
| Stop | ✅ | ✅ | ✅ | ✅ | ❌ | ❌ | Stop playback |
| Next | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | Skip to next track |
| Previous | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | Skip to previous track |
| **Advanced Playback** | | | | | | | |
| Seek | ✅ | ✅ | ✅ | ✅ | ❌ | ✅ | Seek within track |
| Position | ✅ | ✅ | ✅ | ✅ | ❌ | ✅ | Report current position |
| Length | ✅ | ✅ | ✅ | ✅ | ❌ | ✅ | Report track duration |
| Shuffle | ✅ | ✅ | ✅ | ✅ | ❌ | ✅ | Toggle shuffle mode |
| Loop | ✅ | ✅ | ✅ | ✅ | ❌ | ✅ | Set loop mode |
| **Queue Management** | | | | | | | |
| Queue | ✅ | ✅ | ✅ | ⚠️ | ❌ | ❌ | Manage playback queue |
| Add Track | ✅ | ✅ | ✅ | ❌ | ❌ | ❌ | Add tracks to queue |
| Remove Track | ✅ | ✅ | ✅ | ❌ | ❌ | ❌ | Remove tracks from queue |
| Clear Queue | ✅ | ✅ | ✅ | ❌ | ❌ | ❌ | Clear entire queue |
| **Audio Control** | | | | | | | |
| Volume | ✅ | ✅ | ✅ | ✅ | ✅ | ❌ | Control playback volume |
| Mute | ✅ | ✅ | ✅ | ✅ | ✅ | ❌ | Mute/unmute audio |
| **Content & Metadata** | | | | | | | |
| Metadata | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | Provide track metadata |
| Album Art | ✅ | ✅ | ✅ | ✅ | ❌ | ✅ | Provide album artwork |
| Browse | ✅ | ✅ | ❌ | ❌ | ❌ | ❌ | Browse media library |
| Search | ✅ | ✅ | ❌ | ❌ | ❌ | ❌ | Search media library |
| Playlists | ✅ | ✅ | ❌ | ❌ | ❌ | ❌ | Manage playlists |

**Legend**:
- ✅ = Full support
- ⚠️ = Limited support (MPRIS queue support varies by application)
- ❌ = Not supported

#### Player-Specific Notes

**MPD (Music Player Daemon)**:
- Most comprehensive feature support
- Full library browsing and search capabilities
- Robust queue management with playlist support
- Local file playback with network stream support

**LMS (Logitech Media Server)**:
- Full-featured server with comprehensive API
- Excellent queue management and playlist support
- Strong network streaming capabilities
- Multi-room audio support

**Generic Players**:
- API-driven players controlled entirely through Audiocontrol
- Configurable capabilities in player configuration
- Internal state management for queue and playback
- No built-in library or content browsing

**MPRIS (Media Player Remote Interfacing Specification)**:
- Interface to external MPRIS-compliant applications
- Queue support depends on the underlying application
- Limited control over queue management
- Good for integrating with desktop media players

**RAAT (Roon Advanced Audio Transport)**:
- Focused on high-quality audio transport
- Queue management handled by Roon core
- Limited local control capabilities
- Optimized for audiophile use cases

**Spotify/Librespot**:
- Spotify Connect integration
- Queue management handled by Spotify service
- No local queue manipulation possible
- Content controlled through Spotify applications

#### Checking Player Capabilities

You can query a player's capabilities programmatically:

```bash
# Get capabilities for a specific player (through metadata)
curl http://<device-ip>:1080/api/player/mpd/meta

# Check if a player supports queue operations before attempting them
curl http://<device-ip>:1080/api/player/mpd/queue
```

When building applications, always check player capabilities before attempting operations to provide appropriate fallbacks or UI elements.

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

## Cover Art API

The Cover Art API provides endpoints to retrieve cover art from registered providers with comprehensive image metadata. All text parameters must be encoded using URL-safe base64 encoding.

**Enhanced Response Format**: The API returns image metadata including dimensions, file size, format information, and quality grading for each cover art image, enabling clients to select the most appropriate image based on their requirements. Images are automatically sorted by quality grade (highest quality first).

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
        "images": [
          {
            "url": "file:///music/covers/artist1.jpg",
            "width": 600,
            "height": 600,
            "size_bytes": 85432,
            "format": "JPEG",
            "grade": 3
          },
          {
            "url": "file:///music/covers/artist2.png",
            "width": 1000,
            "height": 1000,
            "size_bytes": 234567,
            "format": "PNG",
            "grade": 3
          }
        ]
      },
      {
        "provider": {
          "name": "spotify",
          "display_name": "Spotify"
        },
        "images": [
          {
            "url": "https://i.scdn.co/image/ab6761610000e5ebeb8b0e6ccea3b130a69c8d9c",
            "width": 640,
            "height": 640,
            "size_bytes": 123456,
            "format": "JPEG",
            "grade": 2
          }
        ]
      },
      {
        "provider": {
          "name": "theaudiodb",
          "display_name": "TheAudioDB"
        },
        "images": [
          {
            "url": "https://www.theaudiodb.com/images/media/artist/thumb/the-beatles.jpg",
            "width": 700,
            "height": 700,
            "size_bytes": 141677,
            "format": "JPEG",
            "grade": 4
          }
        ]
      }
    ]
  }
  ```

#### Examples

**Get cover art for "The Beatles":**
```bash
# First encode the artist name
echo -n "The Beatles" | base64 -w 0 | tr '+/' '-_' | tr -d '='
# Result: VGhlIEJlYXRsZXM

# Then make the API request
curl http://<device-ip>:1080/api/coverart/artist/VGhlIEJlYXRsZXM
```

### Get Artist Image File

Directly serves the cached artist image file if available. This endpoint returns the actual image data with proper content-type headers, making it suitable for direct use in `<img>` tags or as image sources.

- **Endpoint**: `/api/coverart/artist/<artist_b64>/image`
- **Method**: GET
- **Parameters**:
  - `artist_b64` (string, required): URL-safe base64 encoded artist name
- **Response**: 
  - **Success (200)**: Binary image data with appropriate `Content-Type` header (`image/jpeg`, `image/png`, `image/gif`, or `image/webp`)
  - **Not Found (404)**: JSON error message if no cached image is available
  - **Bad Request (400)**: JSON error message for invalid artist name encoding
  - **Internal Server Error (500)**: JSON error message if image file cannot be read

#### Examples

**Get image file for "The Beatles":**
```bash
# First encode the artist name
echo -n "The Beatles" | base64 -w 0 | tr '+/' '-_' | tr -d '='
# Result: VGhlIEJlYXRsZXM

# Get the image file directly (returns binary image data)
curl http://<device-ip>:1080/api/coverart/artist/VGhlIEJlYXRsZXM/image

# Save image to file
curl http://<device-ip>:1080/api/coverart/artist/VGhlIEJlYXRsZXM/image -o beatles.jpg

# Use in HTML
# <img src="http://<device-ip>:1080/api/coverart/artist/VGhlIEJlYXRsZXM/image" alt="The Beatles">
```

**Error responses:**
```bash
# Artist not found or no cached image
curl http://<device-ip>:1080/api/coverart/artist/Tm9uZXhpc3RlbnQ/image
# Returns: 404 with {"error": "No image found for artist 'Nonexistent'"}

# Invalid encoding
curl http://<device-ip>:1080/api/coverart/artist/invalid!/image  
# Returns: 400 with {"error": "Invalid artist name encoding"}
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
        "images": [
          {
            "url": "file:///music/artist/album/cover.jpg",
            "width": 500,
            "height": 500,
            "size_bytes": 67890,
            "format": "JPEG"
          }
        ]
      },
      {
        "provider": {
          "name": "musicbrainz",
          "display_name": "MusicBrainz"
        },
        "images": [
          {
            "url": "https://coverartarchive.org/release/12345/front-500.jpg",
            "width": 500,
            "height": 500,
            "size_bytes": 98765,
            "format": "JPEG"
          }
        ]
      }
    ]
  }
  ```

#### Examples

**Get cover art for "Yellow Submarine" by "The Beatles":**
```bash
# First encode the song title and artist
echo -n "Yellow Submarine" | base64 -w 0 | tr '+/' '-_' | tr -d '='
# Result: WWVsbG93IFN1Ym1hcmluZQ

echo -n "The Beatles" | base64 -w 0 | tr '+/' '-_' | tr -d '='
# Result: VGhlIEJlYXRsZXM

# Then make the API request
curl http://<device-ip>:1080/api/coverart/song/WWVsbG93IFN1Ym1hcmluZQ/VGhlIEJlYXRsZXM
```

**Get cover art for "Hey Jude" by "The Beatles":**
```bash
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
        "images": [
          {
            "url": "file:///music/the-beatles/abbey-road/folder.jpg",
            "width": 1200,
            "height": 1200,
            "size_bytes": 345678,
            "format": "JPEG",
            "grade": 5
          }
        ]
      },
      {
        "provider": {
          "name": "theaudiodb",
          "display_name": "TheAudioDB"
        },
        "images": [
          {
            "url": "https://www.theaudiodb.com/images/media/album/thumb/abbey-road.jpg",
            "width": 800,
            "height": 800,
            "size_bytes": 156789,
            "format": "JPEG",
            "grade": 4
          }
        ]
      },
      {
        "provider": {
          "name": "musicbrainz",
          "display_name": "MusicBrainz"
        },
        "images": [
          {
            "url": "https://coverartarchive.org/release/67890/front.jpg",
            "width": 1000,
            "height": 1000,
            "size_bytes": 234567,
            "format": "JPEG"
          }
        ]
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
        "images": [
          {
            "url": "file:///music/the-beatles/abbey-road-1969/cover.jpg",
            "width": 1000,
            "height": 1000,
            "size_bytes": 278901,
            "format": "JPEG"
          }
        ]
      },
      {
        "provider": {
          "name": "theaudiodb",
          "display_name": "TheAudioDB"
        },
        "images": [
          {
            "url": "https://www.theaudiodb.com/images/media/album/thumb/abbey-road-1969.jpg",
            "width": 700,
            "height": 700,
            "size_bytes": 145234,
            "format": "JPEG"
          }
        ]
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
        "images": [
          {
            "url": "https://example.com/resolved-image.jpg",
            "width": 1920,
            "height": 1080,
            "size_bytes": 456789,
            "format": "JPEG"
          },
          {
            "url": "https://example.com/alternative.png",
            "width": 800,
            "height": 600,
            "size_bytes": 123456,
            "format": "PNG"
          }
        ]
      },
      {
        "provider": {
          "name": "metadata_extractor",
          "display_name": "Metadata Extractor"
        },
        "images": [
          {
            "url": "data:image/jpeg;base64,/9j/4AAQSkZJRgABAQEAYABgAAD...",
            "width": 300,
            "height": 300,
            "size_bytes": 8192,
            "format": "JPEG"
          }
        ]
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

### Update Artist Image

Updates the custom image URL for a specific artist. The custom image will take priority over images from external providers when retrieving artist cover art.

- **Endpoint**: `/api/coverart/artist/<artist_b64>/update`
- **Method**: POST
- **Content-Type**: `application/json`
- **Parameters**:
  - `artist_b64` (string, required): URL-safe base64 encoded artist name
- **Request Body**:
  ```json
  {
    "url": "string (required) - URL of the custom image to set for the artist"
  }
  ```
- **Response** (Success):
  ```json
  {
    "success": true,
    "message": "Artist image URL updated successfully"
  }
  ```
- **Response** (Error):
  ```json
  {
    "success": false,
    "message": "Error description (e.g., 'Invalid artist name encoding', 'Failed to update artist image: ...')"
  }
  ```

**Important Notes**:
- The custom image URL is stored persistently in the settings database with the key format: `artist.image.{artist_name}`
- Custom images take priority over external provider images when retrieving artist cover art
- Setting an empty URL (`""`) will clear the custom image for the artist
- Cached images are automatically invalidated when a custom URL is updated
- The system will attempt to download and cache the custom image on the next artist metadata update

#### Examples

```bash
# Set a custom image for an artist
# First, encode the artist name: "The Beatles" -> "VGhlIEJlYXRsZXM"
curl -X POST http://<device-ip>:1080/api/coverart/artist/VGhlIEJlYXRsZXM/update \
  -H "Content-Type: application/json" \
  -d '{"url": "https://example.com/custom-beatles-image.jpg"}'

# Response:
# {
#   "success": true,
#   "message": "Artist image URL updated successfully"
# }

# Clear a custom image (set empty URL)
curl -X POST http://<device-ip>:1080/api/coverart/artist/VGhlIEJlYXRsZXM/update \
  -H "Content-Type: application/json" \
  -d '{"url": ""}'

# Invalid artist name encoding
curl -X POST http://<device-ip>:1080/api/coverart/artist/invalid_encoding!/update \
  -H "Content-Type: application/json" \
  -d '{"url": "https://example.com/image.jpg"}'

# Response:
# {
#   "success": false,
#   "message": "Invalid artist name encoding"
# }
```

### Cover Art Response Format

All cover art endpoints return results grouped by provider, with each provider containing:

- **Provider Information**:
  - `name`: Internal provider identifier (string)
  - `display_name`: Human-readable provider name (string)
- **Images**: Array of cover art image objects, each containing:
  - `url`: Direct URL or file path to the cover art image (string)
  - `width`: Image width in pixels (integer, optional)
  - `height`: Image height in pixels (integer, optional) 
  - `size_bytes`: File size in bytes (integer, optional)
  - `format`: Image format (string, optional) - Common formats: "JPEG", "PNG", "GIF", "WebP", "BMP"
  - `grade`: Image quality score (integer, optional) - Quality score based on provider reputation, file size, and resolution

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
      "images": [
        {
          "url": "https://example.com/image.jpg",
          "width": 1000,
          "height": 1000,
          "size_bytes": 234567,
          "format": "JPEG",
          "grade": 4
        }
      ]
    }
  ]
}
```

**Metadata Fields**: The optional metadata fields provide additional information to help clients select the most appropriate image:
- **Dimensions** (`width`, `height`): Enable selection based on resolution requirements
- **File Size** (`size_bytes`): Useful for bandwidth-conscious applications
- **Format** (`format`): Allows format-specific handling (e.g., preferring PNG for transparency)
- **Grade** (`grade`): Quality score calculated from multiple factors to help select the best images

**Image Grading**: The `grade` field contains an integer score (typically 0-6) that evaluates image quality based on provider reputation, file size, and image resolution. Higher scores indicate better quality. Images are automatically sorted by grade in descending order (best quality first).

For detailed information about the grading system, scoring criteria, and implementation guidelines, see the [Image Grading System documentation](imagegrading.md).

The client application should handle all URL types appropriately and can use the metadata to select optimal images for their use case.

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

<!-- ========================================================================= -->
<!-- IMPORTANT: Settings API should be placed just before Generic Player Controller and Data Structures -->
<!-- Keep Generic Player Controller and Data Structures at the end of the documentation -->
<!-- ========================================================================= -->

## Settings API

The Settings API provides access to the system's settings database, allowing you to get and set configuration values.

### Get Setting Value

Retrieves the value of a specific setting from the settings database.

- **Endpoint**: `/api/settings/get`
- **Method**: POST
- **Content-Type**: `application/json`
- **Request Body**:
  ```json
  {
    "key": "string (required)"
  }
  ```
- **Response** (Success):
  ```json
  {
    "success": true,
    "key": "setting_key",
    "value": "setting_value",
    "exists": true
  }
  ```
- **Response** (Key not found):
  ```json
  {
    "success": true,
    "key": "setting_key",
    "value": null,
    "exists": false
  }
  ```
- **Response** (Error):
  ```json
  {
    "success": false,
    "message": "Error description"
  }
  ```

#### Examples
```bash
# Get a simple setting
curl -X POST http://<device-ip>:1080/api/settings/get \
  -H "Content-Type: application/json" \
  -d '{"key": "audio.volume.default"}'

# Get a setting with non-ASCII characters
curl -X POST http://<device-ip>:1080/api/settings/get \
  -H "Content-Type: application/json" \
  -d '{"key": "user.display_name.默认用户"}'

# Get a setting that doesn't exist
curl -X POST http://<device-ip>:1080/api/settings/get \
  -H "Content-Type: application/json" \
  -d '{"key": "nonexistent.setting"}'
```

### Set Setting Value

Sets the value of a specific setting in the settings database.

- **Endpoint**: `/api/settings/set`
- **Method**: POST
- **Content-Type**: `application/json`
- **Request Body**:
  ```json
  {
    "key": "string (required)",
    "value": "any (required) - The value to set (string, number, boolean, object, array)"
  }
  ```
- **Response** (Success):
  ```json
  {
    "success": true,
    "key": "setting_key",
    "value": "setting_value",
    "previous_value": "previous_value_or_null"
  }
  ```
- **Response** (Error):
  ```json
  {
    "success": false,
    "message": "Error description"
  }
  ```

#### Examples
```bash
# Set a string value
curl -X POST http://<device-ip>:1080/api/settings/set \
  -H "Content-Type: application/json" \
  -d '{"key": "audio.output.device", "value": "hw:0,0"}'

# Set a numeric value
curl -X POST http://<device-ip>:1080/api/settings/set \
  -H "Content-Type: application/json" \
  -d '{"key": "audio.volume.default", "value": 75}'

# Set a boolean value
curl -X POST http://<device-ip>:1080/api/settings/set \
  -H "Content-Type: application/json" \
  -d '{"key": "player.autostart", "value": true}'

# Set an object value
curl -X POST http://<device-ip>:1080/api/settings/set \
  -H "Content-Type: application/json" \
  -d '{"key": "ui.theme", "value": {"background": "#000000", "foreground": "#ffffff"}}'

# Set a setting with non-ASCII characters
curl -X POST http://<device-ip>:1080/api/settings/set \
  -H "Content-Type: application/json" \
  -d '{"key": "user.preferences.语言", "value": "中文"}'

# Update an existing setting
curl -X POST http://<device-ip>:1080/api/settings/set \
  -H "Content-Type: application/json" \
  -d '{"key": "audio.volume.default", "value": 85}'
```

### Settings API Notes

**Key Format**: 
- Settings keys can contain any UTF-8 characters including non-ASCII characters
- Common convention is to use dot-separated hierarchical keys (e.g., `audio.volume.default`)
- Keys are case-sensitive

**Value Types**: 
- The settings database supports any JSON-serializable value types:
  - Strings: `"hello world"`
  - Numbers: `42`, `3.14`
  - Booleans: `true`, `false`
  - Objects: `{"key": "value"}`
  - Arrays: `[1, 2, 3]`
  - Null: `null`

**Persistence**: 
- Settings are automatically persisted to the database
- Changes take effect immediately
- Some settings may require application restart to be fully applied

**Security**: 
- No authentication or authorization is currently implemented
- All settings are accessible via the API
- Consider network security when exposing the API

## Cache API

The Cache API provides endpoints to retrieve information about the internal caching system used by the audio control service. This includes statistics about memory and disk cache usage, as well as image cache statistics.

### Get Cache Statistics

Retrieves comprehensive statistics about the current cache state, including memory usage, disk entries, cache limits, and image cache information.

**Endpoint**: `GET /api/cache/stats`

**Response Format**:
```json
{
  "success": true,
  "stats": {
    "disk_entries": 245,
    "memory_entries": 128,
    "memory_bytes": 2048576,
    "memory_limit_bytes": 10485760
  },
  "image_cache_stats": {
    "total_images": 150,
    "total_size": 25165824,
    "last_updated": 1722254400
  },
  "message": null
}
```

**Response Fields**:
- `success` (boolean): Indicates if the request was successful
- `stats` (object): Attribute cache statistics object containing:
  - `disk_entries` (number): Number of entries stored on disk
  - `memory_entries` (number): Number of entries currently in memory
  - `memory_bytes` (number): Current memory usage in bytes
  - `memory_limit_bytes` (number): Maximum memory limit in bytes (null if no limit)
- `image_cache_stats` (object|null): Image cache statistics object containing:
  - `total_images` (number): Total number of cached images
  - `total_size` (number): Total size of all cached images in bytes
  - `last_updated` (number): Timestamp when statistics were last updated (Unix epoch seconds)
- `message` (string|null): Error message if success is false, null otherwise

**Example Request**:
```bash
curl -X GET "http://localhost:8080/api/cache/stats"
```

**Example Response**:
```json
{
  "success": true,
  "stats": {
    "disk_entries": 1250,
    "memory_entries": 450,
    "memory_bytes": 5242880,
    "memory_limit_bytes": 20971520
  },
  "image_cache_stats": {
    "total_images": 342,
    "total_size": 67108864,
    "last_updated": 1722254400
  },
  "message": null
}
```

**Use Cases**:
- Monitoring cache performance and memory usage
- Debugging cache-related issues
- Optimizing cache configuration based on usage patterns
- System health monitoring and alerting
- Tracking image cache storage usage and performance

**Notes**:
- Cache statistics are updated in real-time
- Memory limits can be configured in the application settings
- Image cache statistics include metadata stored in the attribute cache
- The `image_cache_stats` field may be null if image cache statistics are unavailable
- Disk cache location is configurable via the application configuration

## Background Jobs API

The Background Jobs API provides endpoints to monitor long-running background operations within the audio control service. This includes metadata updates, library scans, and other asynchronous tasks.

Jobs remain in the system after completion and are marked with `finished: true`. This allows clients to track both active and completed jobs. When a new job is created with the same ID as an existing job, it will overwrite the previous job data.

### List Background Jobs

Retrieves a list of all background jobs (both running and finished) with their progress and timing information.

**Endpoint**: `GET /api/background/jobs`

**Response Format**:
```json
{
  "success": true,
  "jobs": [
    {
      "id": "artist_metadata_update_1234567890",
      "name": "Artist Metadata Update",
      "start_time": 1640995200,
      "last_update": 1640995245,
      "progress": "Processing artist 150/500",
      "total_items": 500,
      "completed_items": 150,
      "duration_seconds": 45,
      "time_since_last_update": 2,
      "completion_percentage": 30.0,
      "finished": false,
      "finish_time": null
    }
  ],
  "message": null
}
```

**Response Fields**:
- `success` (boolean): Indicates if the request was successful
- `jobs` (array): List of background job objects, each containing:
  - `id` (string): Unique identifier for the job
  - `name` (string): Human-readable name of the job
  - `start_time` (number): Unix timestamp when the job started
  - `last_update` (number): Unix timestamp of the last progress update
  - `progress` (string|null): Current progress description
  - `total_items` (number|null): Total number of items to process
  - `completed_items` (number|null): Number of items completed
  - `duration_seconds` (number): Total time the job has been running
  - `time_since_last_update` (number): Seconds since the last update
  - `completion_percentage` (number|null): Percentage completion (0-100)
  - `finished` (boolean): Whether the job has completed
  - `finish_time` (number|null): Unix timestamp when the job finished, null if not finished
- `message` (string|null): Error message if success is false, null otherwise

**Example Request**:
```bash
curl -X GET "http://localhost:8080/api/background/jobs"
```

**Example Response (No Jobs Running)**:
```json
{
  "success": true,
  "jobs": [],
  "message": null
}
```

**Example Response (With Running Jobs)**:
```json
{
  "success": true,
  "jobs": [
    {
      "id": "artist_metadata_update_1640995200",
      "name": "Artist Metadata Update",
      "start_time": 1640995200,
      "last_update": 1640995320,
      "progress": "Processing artist metadata: 75/120 completed",
      "total_items": 120,
      "completed_items": 75,
      "duration_seconds": 120,
      "time_since_last_update": 5,
      "completion_percentage": 62.5,
      "finished": false,
      "finish_time": null
    }
  ],
  "message": null
}
```

**Example Response (With Finished Jobs)**:
```json
{
  "success": true,
  "jobs": [
    {
      "id": "library_scan_1640995100",
      "name": "Library Scan",
      "start_time": 1640995100,
      "last_update": 1640995300,
      "progress": "Scan completed successfully",
      "total_items": 1500,
      "completed_items": 1500,
      "duration_seconds": 200,
      "time_since_last_update": 120,
      "completion_percentage": 100.0,
      "finished": true,
      "finish_time": 1640995300
    }
  ],
  "message": null
}
```

### Get Background Job by ID

Retrieves detailed information about a specific background job by its unique identifier.

**Endpoint**: `GET /api/background/jobs/{job_id}`

**Path Parameters**:
- `job_id` (string): Unique identifier of the background job

**Response Format**:
```json
{
  "success": true,
  "jobs": [
    {
      "id": "artist_metadata_update_1234567890",
      "name": "Artist Metadata Update",
      "start_time": 1640995200,
      "last_update": 1640995245,
      "progress": "Processing artist 150/500",
      "total_items": 500,
      "completed_items": 150,
      "duration_seconds": 45,
      "time_since_last_update": 2,
      "completion_percentage": 30.0,
      "finished": false,
      "finish_time": null
    }
  ],
  "message": null
}
```

**Example Request**:
```bash
curl -X GET "http://localhost:8080/api/background/jobs/artist_metadata_update_1640995200"
```

**Example Response (Job Found)**:
```json
{
  "success": true,
  "jobs": [
    {
      "id": "artist_metadata_update_1640995200",
      "name": "Artist Metadata Update",
      "start_time": 1640995200,
      "last_update": 1640995280,
      "progress": "Updating artist images: 45/120",
      "total_items": 120,
      "completed_items": 45,
      "duration_seconds": 80,
      "time_since_last_update": 3,
      "completion_percentage": 37.5,
      "finished": false,
      "finish_time": null
    }
  ],
  "message": null
}
```

**Example Response (Finished Job)**:
```json
{
  "success": true,
  "jobs": [
    {
      "id": "cover_art_download_1640995150",
      "name": "Cover Art Download",
      "start_time": 1640995150,
      "last_update": 1640995250,
      "progress": "Downloaded cover art for all albums",
      "total_items": 85,
      "completed_items": 85,
      "duration_seconds": 100,
      "time_since_last_update": 60,
      "completion_percentage": 100.0,
      "finished": true,
      "finish_time": 1640995250
    }
  ],
  "message": null
}
```

**Example Response (Job Not Found)**:
```json
{
  "success": false,
  "jobs": null,
  "message": "Background job 'invalid_job_id' not found"
}
```

**Use Cases**:
- Monitoring progress of long-running operations
- Building progress indicators in user interfaces
- Debugging background task performance
- Tracking job completion and error states
- System administration and maintenance
- Reviewing completed job history

**Job Lifecycle**:
- Jobs are created with `finished: false` and `finish_time: null`
- During execution, jobs are updated with progress information
- When completed, jobs are marked with `finished: true` and `finish_time` is set
- Finished jobs remain in the system for tracking purposes
- New jobs with the same ID will overwrite existing job data

**Background Job Types**:
Common background jobs include:
- `Artist Metadata Update`: Updates metadata for library artists
- `Library Scan`: Scans and indexes music library files
- `Cover Art Download`: Downloads cover art for albums/artists
- `Database Maintenance`: Performs database cleanup and optimization

## Generic Player Controller

The `GenericPlayerController` provides a configurable player that can be controlled entirely through the API events. It maintains internal state and can be used to represent external players or services that are controlled through the Audiocontrol API.

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