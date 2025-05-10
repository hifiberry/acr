# Audio Control REST API Documentation

This document describes the REST API endpoints available in the Audio Control REST (ACR) service.

## Base Information

- **Base URL**: `http://<device-ip>:1080`
- **API Prefix**: All endpoints are prefixed with `/api`
- **Content Type**: All responses are in JSON format
- **Version**: As per current package version

## Events

The ACR system uses an event-based architecture to communicate state changes between components. Events can be monitored via WebSockets or server-sent events (SSE).

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
  - `command` (string): The command to send (same options as above)
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
  "disc_number": "1",
  "track_number": 5,
  "name": "Track Name",
  "artist": "Track Artist",
  "uri": "file:///music/track.mp3"
}
```

| Field | Type | Description |
|-------|------|-------------|
| disc_number | string | Disc number as a string (to support formats like "1/2") |
| track_number | number | Track number on the disc |
| name | string | Track title |
| artist | string | Track-specific artist (only included if different from album artist), may be null |
| uri | string | URI/filename of the track, may be null |