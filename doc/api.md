# Audio Control REST API Documentation

This document describes the REST API endpoints available in the Audio Control REST (ACR) service.

## Base Information

- **Base URL**: `http://<device-ip>:1080`
- **Content Type**: All responses are in JSON format
- **Version**: As per current package version

## Core API

### Get API Version

Retrieves the current version of the API.

- **Endpoint**: `/version`
- **Method**: GET
- **Response**:
  ```json
  {
    "version": "x.y.z"
  }
  ```

#### Example
```bash
curl http://<device-ip>:1080/version
```

### Get System Memory Usage

Retrieves the memory usage statistics of the library components.

- **Endpoint**: `/system/memory`
- **Method**: GET
- **Response**:
  ```json
  {
    "artists_memory": 1024000,
    "albums_memory": 2048000,
    "tracks_memory": 4096000,
    "overhead_memory": 512000,
    "artist_count": 300,
    "album_count": 500,
    "track_count": 5000,
    "album_artists_count": 450,
    "total_memory": 7680000,
    "formatted": {
      "artists_memory": "1000.00 KB",
      "albums_memory": "2000.00 KB",
      "tracks_memory": "4000.00 KB", 
      "overhead_memory": "500.00 KB",
      "total_memory": "7500.00 KB"
    }
  }
  ```

#### Example
```bash
curl http://<device-ip>:1080/system/memory
```

## Player API

### Get Current Player

Retrieves information about the currently active player.

- **Endpoint**: `/player`
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
curl http://<device-ip>:1080/player
```

### List Available Players

Retrieves a list of all available audio players.

- **Endpoint**: `/players`
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
curl http://<device-ip>:1080/players
```

### Send Command to Active Player

Sends a playback command to the currently active player.

- **Endpoint**: `/player/active/send/<command>`
- **Method**: POST
- **Path Parameters**:
  - `command` (string): The command to send. Supported values:
    - Simple commands: `play`, `pause`, `playpause`, `next`, `previous`, `kill`
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
curl -X POST http://<device-ip>:1080/player/active/send/play

# Play/pause toggle
curl -X POST http://<device-ip>:1080/player/active/send/playpause

# Next track
curl -X POST http://<device-ip>:1080/player/active/send/next

# Set loop mode to playlist
curl -X POST http://<device-ip>:1080/player/active/send/set_loop:playlist

# Seek to 30 seconds
curl -X POST http://<device-ip>:1080/player/active/send/seek:30.0

# Enable shuffle
curl -X POST http://<device-ip>:1080/player/active/send/set_random:true
```

### Send Command to Specific Player

Sends a playback command to a specific player by name.

- **Endpoint**: `/player/<player-name>/command/<command>`
- **Method**: POST
- **Path Parameters**:
  - `player-name` (string): The name of the target player
  - `command` (string): The command to send (same options as above)
- **Response**: Same as "Send Command to Active Player"
- **Error Response** (400 Bad Request, 404 Not Found, 500 Internal Server Error): Same structure as above

#### Examples
```bash
# Play on a specific player
curl -X POST http://<device-ip>:1080/player/spotify/command/play

# Set volume on a specific player
curl -X POST http://<device-ip>:1080/player/mpd/command/set_volume:80

# Pause a specific player
curl -X POST http://<device-ip>:1080/player/raat/command/pause
```

### Get Now Playing Information

Retrieves information about the currently playing track and player status.

- **Endpoint**: `/now-playing`
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
curl http://<device-ip>:1080/now-playing
```

## Plugin API

### List Action Plugins

Retrieves a list of all active action plugins.

- **Endpoint**: `/plugins/actions`
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
curl http://<device-ip>:1080/plugins/actions
```

### List Event Filters

Retrieves a list of all active event filters.

- **Endpoint**: `/plugins/event-filters`
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
curl http://<device-ip>:1080/plugins/event-filters
```

## Library API

### List All Players with Library Information

Retrieves a list of all players and shows whether they offer library functionality.

- **Endpoint**: `/library`
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
curl http://<device-ip>:1080/library
```

### Get Library Information

Retrieves library information for a specific player.

- **Endpoint**: `/library/<player-name>`
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
curl http://<device-ip>:1080/library/mpd
```

### Get Player Albums

Retrieves all albums for a specific player.

- **Endpoint**: `/library/<player-name>/albums`
- **Method**: GET
- **Path Parameters**:
  - `player-name` (string): The name of the player
- **Query Parameters**:
  - `include_tracks` (boolean, optional): Whether to include track data for each album
- **Response**:
  ```json
  {
    "player_name": "player-name",
    "count": 100,
    "albums": [
      // Album objects
      // If include_tracks=true, each album will include its tracks
    ]
  }
  ```
- **Error Response** (404 Not Found): String error message

#### Examples
```bash
# Get albums without tracks
curl http://<device-ip>:1080/library/mpd/albums

# Get albums with tracks included
curl http://<device-ip>:1080/library/mpd/albums?include_tracks=true
```

### Get Player Artists

Retrieves all artists for a specific player.

- **Endpoint**: `/library/<player-name>/artists`
- **Method**: GET
- **Path Parameters**:
  - `player-name` (string): The name of the player
- **Response**:
  ```json
  {
    "player_name": "player-name",
    "count": 50,
    "artists": [
      // Artist objects
    ]
  }
  ```
- **Error Response** (404 Not Found): String error message

#### Examples
```bash
# Get artists
curl http://<device-ip>:1080/library/mpd/artists
```

### Get Album by Name

Retrieves a specific album by name for a player.

- **Endpoint**: `/library/<player-name>/album/<album-name>`
- **Method**: GET
- **Path Parameters**:
  - `player-name` (string): The name of the player
  - `album-name` (string): The name of the album
- **Query Parameters**:
  - `include_tracks` (boolean, optional): Whether to include track data for the album
- **Response**:
  ```json
  {
    "player_name": "player-name",
    "include_tracks": true,
    "album": {
      // Album object with its metadata
      // If include_tracks=true, includes tracks data
      // Will be null if album not found
    }
  }
  ```
- **Error Response** (404 Not Found): String error message

#### Examples
```bash
# Get an album without tracks
curl "http://<device-ip>:1080/library/mpd/album/Dark%20Side%20of%20the%20Moon"

# Get an album with tracks included
curl "http://<device-ip>:1080/library/mpd/album/Dark%20Side%20of%20the%20Moon?include_tracks=true"
```

### Get Albums by Artist

Retrieves all albums by a specific artist for a player.

- **Endpoint**: `/library/<player-name>/artist/<artist-name>/albums`
- **Method**: GET
- **Path Parameters**:
  - `player-name` (string): The name of the player
  - `artist-name` (string): The name of the artist
- **Query Parameters**:
  - `include_tracks` (boolean, optional): Whether to include track data for each album
- **Response**:
  ```json
  {
    "player_name": "player-name",
    "artist_name": "artist-name",
    "count": 5,
    "include_tracks": true,
    "albums": [
      // Album objects for this artist
      // If include_tracks=true, each album will include its tracks
    ]
  }
  ```
- **Error Response** (404 Not Found): String error message

#### Examples
```bash
# Get albums for an artist without tracks
curl "http://<device-ip>:1080/library/mpd/artist/Pink%20Floyd/albums"

# Get albums for an artist with tracks included
curl "http://<device-ip>:1080/library/mpd/artist/Pink%20Floyd/albums?include_tracks=true"
```

### Refresh Player Library

Triggers a refresh of the library for a specific player.

- **Endpoint**: `/library/<player-name>/refresh`
- **Method**: GET
- **Path Parameters**:
  - `player-name` (string): The name of the player
- **Response**: Same as "Get Library Information"
- **Error Response** (404 Not Found, 500 Internal Server Error): String error message

#### Example
```bash
curl http://<device-ip>:1080/library/mpd/refresh
```

### Get Artist by Name or MusicBrainz ID

Retrieves complete information for a specific artist, including metadata and image URLs.
If the artist-name parameter is formatted like a MusicBrainz ID (UUID format: xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx),
it will search for artists with that MBID instead of by name.

- **Endpoint**: `/library/<player-name>/artist/<artist-name-or-mbid>`
- **Method**: GET
- **Path Parameters**:
  - `player-name` (string): The name of the player
  - `artist-name-or-mbid` (string): The name of the artist or a MusicBrainz ID
- **Response**:
  ```json
  {
    "player_name": "player-name",
    "artist": {
      "id": 12345678,
      "name": "artist-name", 
      "is_multi": false,
      "albums": ["album1", "album2"],
      "track_count": 25,
      "metadata": {
        "mbid": ["musicbrainz-id-1", "musicbrainz-id-2"],
        "thumb_url": ["/path/to/image1.jpg", "/path/to/image2.jpg"],
        "banner_url": ["/path/to/banner.jpg"],
        "biography": "Artist biography text...",
        "genres": ["rock", "alternative"],
        "is_partial_match": false
      }
    }
  }
  ```
- **Error Response** (404 Not Found): String error message

#### Example
```bash
# Get artist information by name
curl "http://<device-ip>:1080/library/mpd/artist/Pink%20Floyd"

# Get artist information by MusicBrainz ID
curl "http://<device-ip>:1080/library/mpd/artist/83d91898-7763-47d7-b03b-b92132375c47"
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