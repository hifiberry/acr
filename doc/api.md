# Audio Control REST API Documentation

This document describes the REST API endpoints available in the Audio Control REST (Audiocontrol) service.

## Table of Contents

- [Base Information](#base-information)
- [Image and Lyrics Paths](#image-and-lyrics-paths)
- [Events](#events)
  - [Player Events](#player-events)
- [Core API](#core-api)
  - [Get API Version](#get-api-version)
  - [Get Input Status](#get-input-status)
- [Player API](#player-api)
  - [Get Current Player](#get-current-player)
  - [List Available Players](#list-available-players)
  - [Send Command to Active Player](#send-command-to-active-player)
  - [Send Command to Specific Player](#send-command-to-specific-player)
  - [Player Event Update](#player-event-update)
  - [Song Information Update](#song-information-update)
  - [Get Now Playing Information](#get-now-playing-information)
  - [Get Player Queue](#get-player-queue)
  - [Queue Management Commands](#queue-management-commands)
    - [Queue Track Metadata Structure](#queue-track-metadata-structure)
  - [Get Player Metadata](#get-player-metadata)
  - [Get Specific Player Metadata Key](#get-specific-player-metadata-key)
  - [Player Capabilities and Support Matrix](#player-capabilities-and-support-matrix)
  - [Stream Title Splitting](#stream-title-splitting)
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
  - [Apply Enrichment](#apply-enrichment)
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
  - [Metadata Service Routes](#metadata-service-routes)
    - [The `/api/metadata/` mount](#the-apimetadata-mount)
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
  - [Purge Image Variants](#purge-image-variants)
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

> **On a HiFiBerryOS device, clients do not talk to port 1080.** nginx fronts the
> device on port 80 and proxies `/api/audiocontrol/` to `127.0.0.1:1080/api/`,
> setting `X-Forwarded-Prefix: /api/audiocontrol`. An endpoint documented below as
> `/api/now-playing` is therefore reached at
> `http://<device>/api/audiocontrol/now-playing`. The port 1080 base URL is for
> direct access when testing audiocontrol on its own.
>
> Authentication is also applied at the proxy, not here: when the `hifiberry-auth`
> package is installed, nginx gates every `/api/<service>/` location with
> `auth_request`, so any endpoint below can return `401` with a
> `WWW-Authenticate-Hint` header even though audiocontrol itself never produces
> one.

## Image and Lyrics Paths

Paths in responses are ready to use as they are. A client joins them to the
host it is talking to and fetches them unmodified — it must not add a prefix
of its own.

Where audiocontrol sits behind a reverse proxy that reports
`X-Forwarded-Prefix`, every path in a response already carries that prefix.
Reached directly, the same paths come back in their internal form, which is
correct for that route. This holds for album `cover_art`, artist `thumb_url`,
`song.cover_art_url` and `song.metadata.lyrics_url`, over REST and over the
WebSocket alike.

Before 0.15.0 only the `now-playing` response carried the external form; the
album, artist and album-detail responses carried the internal path even behind
a proxy. A client that compensated for that by prepending the prefix itself
should check whether the path already carries it, as the shipped clients do.

External URLs — artist images from last.fm or theaudiodb, for instance — are
absolute and are never rewritten.

A song pushed in through `POST /api/player/<name>/update` is stored with
whatever `cover_art_url` the sender supplied, and is served back as it was
stored. A sender that supplies an already-external path — one that carries the
proxy's prefix — therefore produces a value that is wrong for a client reached
directly, whatever the rewrite does on the way out. Senders should supply
either an absolute URL or a path in audiocontrol's own internal form.

Because their bodies depend on the request header, the two library list
responses — `/api/library/<player>/albums` and `/api/library/<player>/artists` —
carry `Vary: X-Forwarded-Prefix`, and their `ETag`s, along with the
`library_version` those endpoints and `/api/library/<player>` report, vary with
the prefix as well as with the library's contents. The remaining
prefix-dependent responses (`now-playing`, `library/<player>` and the 200 and
409 of `library/<player>/enrichment` - whose `library_version` varies with the
prefix even though neither carries an image path, so that a caller can compare
the version one route hands it against the version the other reports, while the
`library_generation` in those bodies deliberately does not: it is the token an
enrichment batch names, not a validator, and a prefixed one would match nothing
the library holds -
`album/by-id`, `artist/by-id`,
`artist/by-name`, `artist/by-mbid`, `albums/by-artist`, `albums/by-artist-id`,
`albums/by-genre`, `albums/by-category`) return a plain JSON body with neither
header; a shared cache in front of
audiocontrol must not be keyed on URL alone for those.

The forwarded prefix must not shadow an API route segment such as `library`,
`coverart` or `lyrics`. A path is recognized as already carrying the prefix by
a plain string match, so a prefix of `/api/library`, say, would make every
unrewritten path under `/api/library/...` look like one that had already been
rewritten — and those paths would then go out without the prefix, leaving the
client with a path it cannot fetch.

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

### GET /capabilities

What this daemon supports, as opposed to which release it is.

```json
{
  "version": "0.9.3",
  "images": {
    "sizes": [100, 140, 200, 280, 400, 800]
  }
}
```

`images.sizes` lists the sizes accepted by the `size` parameter on the image
endpoints; requests are rounded up to the next listed size. **The list is
configured per installation** via `services.images.sizes` in
`audiocontrol.json` and defaults to `[100, 140, 200, 280, 400, 800]` — so a
client must read it rather than assuming a fixed ladder.

**This is a daemon-level capability, not a per-player guarantee.** It says which
sizes this release understands, not that every image can be served at them.
Resizing works from acr's own image cache, so on `/api/library/<player>/image/<id>`
it applies to `album:` identifiers on players that populate that cache -- MPD
does, LMS does not. `artist:` identifiers, bare track URLs, base64 identifiers,
and GIF or BMP sources are served at full size. See
[Get Image from Library](#get-image-from-library) for the full list; a request
that cannot be resized still returns `200` with the original.

**A release that answers this endpoint with 404 does not resize images.** That
is a complete answer — ask for originals rather than probing.

### Get Input Status

Reports the configured input sources (USB HID remotes and keyboards), the devices currently bound to them, and the last mapped keypress seen. This is the "is my remote detected?" endpoint.

- **Endpoint**: `/api/inputs`
- **Method**: GET
- **Response**:
  ```json
  {
    "inputs": [
      {
        "name": "keyboard",
        "status": {
          "enabled": true,
          "volume_step": 5.0,
          "grab": false,
          "device_filter": "",
          "mapped_keys": 14,
          "devices": [
            { "path": "/dev/input/event0", "name": "HiFiBerry USBRemote", "matched_keys": ["KEY_VOLUMEUP", "KEY_VOLUMEDOWN"] }
          ],
          "last_key": {
            "code": 115,
            "name": "KEY_VOLUMEUP",
            "action": "volume_up",
            "device": "HiFiBerry USBRemote"
          }
        }
      }
    ]
  }
  ```

`devices` only lists devices that matched the configured keymap, and `last_key` is `null` until a mapped key has been pressed. A device that is unmatched, filtered out, or hidden by a permission problem does not appear here at all -- use `audiocontrol_input_devices` (see [CLI Tools](cli_tools.md#audiocontrol_input_devices)) for that, and see [Input Sources](inputs.md) for the full configuration reference.

#### Example
```bash
curl http://<device-ip>:1080/api/inputs
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
    - **Queue management**: `add_track`, `add_tracks`, `remove_track:<position>`, `clear_queue`, `play_queue_index:<index>`

**Note**: Queue management commands are only supported by certain players (MPD, LMS, Generic Players). See the [Queue Management Commands](#queue-management-commands) section for detailed information about player support and usage.

- **Request Body** (for `add_track` and `add_tracks` commands only):
  ```json
  {
    "uri": "string (required)",
    "title": "string (optional, future use)",
    "coverart_url": "string (optional, future use)"
  }
  ```
  `add_tracks` takes a `uris` array instead; see
  [Add Several Tracks to Queue](#add-several-tracks-to-queue).
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

# Add a whole album in one request
curl -X POST http://<device-ip>:1080/api/player/mpd/command/add_tracks \
  -H "Content-Type: application/json" \
  -d '{"uris": ["artist/album/01.flac", "artist/album/02.flac"]}'

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

### Song Information Update

Accepts a better version of the currently playing song from an outside lookup (for example, a metadata enrichment process) and merges it into the player's current song.

- **Endpoint**: `/api/player/<player-name>/song-information`
- **Method**: POST
- **Content-Type**: `application/json`
- **Request Body**: A partial `Song` object. Any field the body omits is not asserted about and is left unchanged; only `title` and `artist` are used to confirm the partial still describes the song being played.
- **Responses**:

  | Condition | Status | Body |
  |---|---|---|
  | `title`/`artist` in the body match the current song | 200 OK | `{"success": true, "applied": true}` |
  | `title`/`artist` in the body no longer match the current song | 200 OK | `{"success": true, "applied": false}` |
  | Body has neither `title` nor `artist` | 400 Bad Request | `{"success": false, "message": "..."}` |
  | `<player-name>` does not name a known player | 404 Not Found | `{"success": false, "message": "..."}` |

The merge follows the rule documented under `song_information_update` in the WebSocket contract: a title or artist the partial carries must match the current song, only `cover_art_url`, `liked` and `metadata` are merged, and artwork that belongs to the song is never replaced. `applied: false` for a song that has moved on is the expected answer, not an error.

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

Where `song.cover_art_url` is only a placeholder rather than artwork for the
track, `song.metadata.cover_art_source` says so: a radio stream carries its
station's logo under `"station_logo"`. The key is absent for artwork that
belongs to the song, which is never replaced.

This response is built from the player's own stored song, and a lookup that
finds the track's real artwork writes it there: `cover_art_url` is updated
shortly after the song change, and `song.metadata.cover_art_source` names
whatever provider supplied it — `"lastfm"`, or `"enrichment"` for a lookup
that named no source, which is new in 0.18.0. The full set of values, and the
rule for reading one a client does not recognise, is in
[the event contract](websocket.md#songmetadatacover_art_source). A client that
only polls `/api/now-playing` sees the same progression a WebSocket subscriber
sees on `song_information_update`.

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

#### Add Several Tracks to Queue

Adds any number of tracks to the player's queue in a single request. Queueing an
album with `add_track` costs one request per track; `add_tracks` takes the whole
list, and the tracks are added over a single connection to the backend.

- **Command**: `add_tracks`
- **Method**: POST to `/api/player/<player-name>/command/add_tracks`
- **Request Body** (JSON required):
  ```json
  {
    "uris": ["artist/album/01.flac", "artist/album/02.flac"],
    "insert_at_beginning": false,
    "metadata": [
      { "title": "First track" },
      null
    ]
  }
  ```
  - `uris` (required): the tracks to add, in the order they should appear in the
    queue. An empty array is accepted and does nothing.
  - `insert_at_beginning` (optional, default `false`): insert the batch at the
    front of the queue instead of appending it. The batch keeps its own order
    either way.
  - `metadata` (optional): positional — entry `i` describes `uris[i]`, and
    `null` leaves a track without metadata. A shorter list is padded; more
    entries than URIs is rejected with 400. Each entry takes the same shape as
    the `metadata` object of [`add_track`](#add-track-to-queue).
- **Supported URI Formats**: same as [`add_track`](#add-track-to-queue).
- **Response**: `success` is true only if every track was added.

**Player Support**: identical to `add_track` — the command maps onto the same
queueing path, so any player that supports `add_track` supports `add_tracks`.

**Compatibility**: `add_track` is unchanged and remains supported. Clients that
cannot rely on the server version should fall back to it when `add_tracks`
returns 400.

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

### Stream Title Splitting

Radio streams announce a single combined title such as `Nightwish - Nemo`. The
server splits it into artist and song. The order is not fixed — some stations
announce `Title - Artist` — so a station this daemon has neither been told
about nor learned falls back to reading it as `Artist - Title`, which is what
streams overwhelmingly announce. That fallback is a fixed guess, not a lookup:
this daemon does not ask anyone before deciding.

The guess can be wrong. Two things correct it over time: these endpoints let
the order and separator be **set** outright, and a companion metadata process
can report what it has separately worked out for a station — typically with a
MusicBrainz lookup this daemon no longer makes itself — as an **observation**.
A set value always wins, over both the guess and any observation; an
observation only ever moves what the station has **learned**, which is what a
client reads back as `learned_order`/`learned_separator`.

Splitting is MPD-only; other players return 400.

The `<station>` path segment is the stream URL, URL-safe base64 encoded — the
same encoding used elsewhere in this API.

#### List Splitters

- **Endpoint**: `/api/player/<player-name>/splitters`
- **Method**: GET
- **Response**:
  ```json
  {
    "player_name": "mpd",
    "count": 1,
    "splitters": [
      {
        "station": "http://stream.example/radio",
        "order": "song_artist",
        "separator": "-",
        "learned_order": "artist_song",
        "learned_separator": null,
        "artist_song_count": 12,
        "song_artist_count": 3,
        "unknown_count": 1,
        "undecided_count": 0
      }
    ]
  }
  ```
  `order` and `separator` are what was set explicitly; `learned_order` and
  `learned_separator` are what the station taught the server, through
  observations (below). Either may be `null`. The counts are observation
  outcomes, not play counts, and do not include the fallback guess: a guess
  is not fed back as an observation of itself.

Only stations played since the last restart are listed.

#### Get a Splitter

- **Endpoint**: `/api/player/<player-name>/splitter/<station>`
- **Method**: GET
- **Response**: a single splitter object, as above.
- **Errors**: 404 if the station has no splitter, 400 if `<station>` is not
  URL-safe base64.

Unlike the list, this also finds stations persisted by an earlier run.

#### Set a Splitter

- **Endpoint**: `/api/player/<player-name>/splitter/<station>`
- **Method**: POST
- **Request Body**:
  ```json
  {
    "order": "artist_song",
    "separator": "-"
  }
  ```
  - `order`: `artist_song` or `song_artist`. Omit or send `null` to clear the
    setting and return the station to guessing.
  - `separator`: one of `-`, `/`, `:`. Omit or send `null` to clear it. A set
    separator is tried before any other, which is how `AC/DC - Highway to Hell`
    is made to split on the dash rather than inside the band name.

  Both fields are replaced together, so a request always states the whole
  setting: sending only `order` clears any separator that was set.
- **Response**: the resulting splitter object.
- **Errors**: 400 for an unrecognised order or separator (`unknown` and
  `undecided` are detection outcomes and cannot be set), 500 if the setting was
  applied but could not be persisted.

The setting is saved, so it survives a restart.

#### Report an Observation

- **Endpoint**: `/api/player/<player-name>/splitter/<station>/observation`
- **Method**: POST
- **Request Body**:
  ```json
  { "order": "song_artist" }
  ```
  - `order`: `artist_song` or `song_artist`, required. `unknown` and
    `undecided` are outcomes of a lookup, not readings of a title, so they
    are never a valid observation.
- **Response**: the resulting splitter object, as above.
- **Errors**: 400 for an unrecognised order or `<station>`, 404 if the
  player has no splitter for that station and the manager is not otherwise
  able to create one, 500 if the observation was recorded but could not be
  persisted.

This is how a companion metadata process feeds what it has separately
determined about a station — typically a MusicBrainz-backed correction of
this daemon's own guess — back into `learned_order`. **It never touches
`order`**: a value set through *Set a Splitter* keeps winning regardless of
how many observations disagree with it, and the intended caller only ever
reports an observation when its own answer disagrees with what this daemon
assumed, so a station whose guess already happens to be right accumulates no
observations and needs none. Repeated agreeing observations are what
establish `learned_order`, the same threshold `learned_order` has always
used.

This is also the only route on this daemon that a companion metadata
process calls — every other exchange between the two travels the other way,
initiated by the metadata side. A title-order correction cannot be relayed
through [Song Information Update](#song-information-update): that route
identifies a song by its current title and artist and refuses a partial
that disagrees with either, and a swapped order disagrees with both by
construction. The **currently playing** song therefore keeps whatever split
it was first given, right or wrong; only the *next* title from that station
benefits from an observation recorded against it.

Recorded observations are saved along with everything else this splitter
holds, so they survive a restart.

#### Delete a Splitter

- **Endpoint**: `/api/player/<player-name>/splitter/<station>`
- **Method**: DELETE
- **Response**: 204 No Content.
- **Errors**: 404 if the station has no splitter.

Discards both what was set and what was learned; the station is guessed from
scratch next time it plays.

#### Examples

```bash
# The station announces "Title - Artist"; correct it once
STATION=$(printf 'http://stream.example/radio' | basenc --base64url | tr -d '=')
curl -X POST http://<device-ip>:1080/api/player/mpd/splitter/$STATION \
  -H "Content-Type: application/json" \
  -d '{"order": "song_artist", "separator": "-"}'

# See what a station has learned
curl http://<device-ip>:1080/api/player/mpd/splitter/$STATION

# Back to guessing
curl -X POST http://<device-ip>:1080/api/player/mpd/splitter/$STATION \
  -H "Content-Type: application/json" -d '{}'

# A companion metadata process reports what it separately determined --
# this feeds learned_order, and does nothing if the station's order was
# set explicitly (as just above)
curl -X POST http://<device-ip>:1080/api/player/mpd/splitter/$STATION/observation \
  -H "Content-Type: application/json" -d '{"order": "song_artist"}'
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
      },
      "volume_scale": "perceptual"
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
  - `decibel_range` (object): Supported decibel range, or `null` when the
    control exposes none. These are the levels the hardware actually reports,
    read from its ALSA TLV data.
    - `min_db` (number): Quietest audible level in decibels. A control whose
      bottom step is a hard mute reports the quietest step above it, not the
      mute.
    - `max_db` (number): Maximum volume in decibels
  - `volume_scale` (string): The domain every `percentage` in this API lives
    in. See [Volume scales](#volume-scales).
- `current_state` (object): Current volume state (if available)
  - `percentage` (number): Current volume as percentage (0-100), in the domain
    named by `control_info.volume_scale`
  - `decibels` (number): Current volume in decibels as reported by the hardware,
    or `null` when it reports no usable level. Independent of `decibel_range`: a
    control can report where it is now without describing the span it covers, so
    `decibels` may be present when `decibel_range` is `null`.
  - `raw_value` (number): Raw hardware control value (implementation specific)
- `supports_change_monitoring` (boolean): Whether the system can monitor volume changes

#### Example
```bash
curl http://<device-ip>:1080/api/volume/info
```

### Volume scales

`percentage` is a user-facing control position, not a position within the
hardware's raw range. `control_info.volume_scale` says which mapping is in use:

- **`perceptual`** (default) — the percentage follows a cube-root loudness
  curve across `decibel_range`, the same normalisation `alsamixer` and
  PulseAudio apply. This is what makes a slider behave: on a control spanning
  -103.5 dB to 0 dB, 50% lands near -17.6 dB.
- **`raw`** — the percentage is a linear position within the hardware's raw
  mixer range. Used automatically for controls that expose no usable decibel
  information, and selectable with `volume_scale: "raw"` under
  `services.volume` in the configuration.

Selecting `raw` restores the earlier *percentage* mapping only. `decibels` and
`decibel_range` are corrected either way: they are read from the hardware
rather than interpolated, and a control whose bottom step is a mute no longer
has a placeholder minimum invented for it. There is no setting that brings
those back, because what they reported before was not a property of the
hardware.

A hardware mixer's raw range is normally linear in decibels, so the `raw` scale
spreads a ~100 dB span evenly across the slider and pushes nearly all of the
audible change into its top quarter. The two scales therefore report very
different numbers for the same hardware state:

| Raw value | Hardware level | `perceptual` | `raw` |
| --: | --: | --: | --: |
| 137 | -35.0 dB | 24.7% | 66.2% |
| 163 | -22.0 dB | 41.9% | 78.7% |
| 193 | -7.0 dB | 76.0% | 93.2% |

**Clients that persist a percentage must persist the scale with it.** A stored
`78` means -22.8 dB under `raw` and -6.3 dB under `perceptual` — replaying the
wrong one is roughly a 16 dB error. Releases predating this field behaved as
`raw` throughout, so treat a missing `volume_scale` as `raw`.

`decibels` and `raw_value` are unaffected by the scale, and `raw_value` remains
the way to address the hardware range directly.

`/api/volume/increase` and `/api/volume/decrease` step by percentage points in
the active scale. Under `perceptual` a 5-point step is a gentle change near the
top of the range and a larger one down in the quiet tail, which is what a volume
button is expected to do; under `raw` it was a fixed dB step at every position.

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

Every entry in the configuration's `action_plugins` array that loads is listed
here, in configuration order. `Lastfm` is listed for the `lastfm` entry as it
always has been, although the scrobbling that entry configures now runs as a
worker rather than as an action plugin.

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
    "artists_count": 50,
    "tracks_count": 1000,
    "supports_delete": false,
    "library_version": "5e2b91c0-a3f9c1d2-42"
  }
  ```
- **Error Response** (404 Not Found): Same structure as successful response but with `has_library: false`

**Response Fields**

| Field | Type | Description |
|-------|------|-------------|
| `player_name` | string | The name of the player |
| `player_id` | string | The unique identifier of the player |
| `has_library` | boolean | Whether the player has a library |
| `is_loaded` | boolean | Whether the library is loaded |
| `albums_count` | integer | Number of albums in the library |
| `artists_count` | integer | Number of artists in the library |
| `tracks_count` | integer | Total number of tracks in the library |
| `supports_delete` | boolean | Whether the player supports deleting tracks |
| `library_version` | string (opaque) | Changes whenever the library's contents change. Poll this one small response to learn whether any list needs re-fetching, rather than issuing a conditional request per list. **Compare it for equality only** - it is opaque, not ordered, and carries no arithmetic meaning. **Absent** when the backend does not track changes. It also changes when the daemon restarts, which costs one refetch and is what makes it safe to trust. |
| `library_generation` | string (opaque) | Changes when the library is *reloaded*, and not when its contents change. This is the token an enrichment batch names (see [Apply Enrichment](#apply-enrichment)); it is not a cache validator and, unlike `library_version`, does not vary with a forwarded prefix. **Compare it for equality only.** **Absent** when the backend cannot tell whether it has reloaded, which is a caller's signal to name no generation in its batches. It also changes when the daemon restarts. |

The two tokens are not interchangeable. `library_version` is the validator on
the list routes and the "have I seen this yet" token; `library_generation` says
which loaded library the current contents belong to. An enrichment write moves
the version and not the generation; a reload moves both.

#### Example
```bash
curl http://<device-ip>:1080/api/library/mpd
```

### Apply Enrichment

Merges what an outside lookup learned about a library's artists and albums into
that library. This is how the metadata side hands back genres, MusicBrainz IDs
and artist thumbnails after it has looked them up.

- **Endpoint**: `/api/library/<player-name>/enrichment`
- **Method**: POST
- **Content-Type**: `application/json`
- **Path Parameters**:
  - `player-name` (string): The name of the player
- **Request Body**:
  ```json
  {
    "library_generation": "5e2b91c0-a3f9c1d2-g3",
    "artists": [
      {
        "name": "The Beatles",
        "mbid": ["b10bbbfc-cf9e-42e0-be17-e2c3e1d2600d"],
        "is_multi": false,
        "genres": ["rock"],
        "thumb_url": ["/api/coverart/artist/YWJj/image"]
      }
    ],
    "albums": [
      { "id": "1", "genres": ["rock", "pop"] }
    ]
  }
  ```

  Every field is optional. `library_generation` is the `library_generation` from
  `GET /api/library/<player-name>`, naming the loaded library this batch was
  computed against; omitting it makes no claim and the batch is applied as it
  arrives. An artist is matched by `name` and an album by `id`; an entry naming
  something the library does not have is skipped, never inserted.

  A field this list does not name is an error: the body is refused with 422
  rather than parsed with the unknown field dropped. That is deliberate, and it
  is about the one name above that matters. Because every field is optional, a
  caller that wrote `library_version` here instead of `library_generation` would
  otherwise be understood as making *no* staleness claim, and would have every
  batch applied unchecked — silently, and precisely where the check is what
  keeps a rebuilt library from being written with results computed against the
  library it replaced.

- **Responses**:

  | Condition | Status | Body |
  |---|---|---|
  | Merged | 200 OK | `{"artists": 1, "albums": 1, "library_version": "..."}` — how much was applied, and the library's version after the merge, folded with this request's prefix |
  | The library was reloaded since `library_generation` | 409 Conflict | `{"library_generation": "...", "library_version": "..."}` — the current values of both; the version folded with this request's prefix, the generation not |
  | `<player-name>` does not name a known player, or that player has no library | 404 Not Found | `{"error": "..."}` |

**Merge rules**

- Album genres: an empty `genres` list never clears what the library read from
  the file's own tags — the tags are better data than a lookup that found
  nothing. A list holding the same genres in a different order is not a change;
  the stored order is left as it was.
- Artist `mbid`, `is_multi`, `genres` and `thumb_url` replace what is stored.
  An artist marked `is_multi` whose other fields are all empty is one whose
  lookup found nothing describing a single artist: it keeps no metadata at all,
  which the artist routes serve as `"metadata": null`.
- `thumb_url` is stored verbatim, a provider's own URL included. An empty list
  means no image was found, which is what a client reads to tell "no picture"
  from "not looked up yet".
- `split_into` names the artists an album-artist *string* really splits into.
  It rewrites the `artists` list of every album whose `album_artist` equals the
  summary's `name`, creating and removing artists as that requires, and
  reindexing them against their albums. Absent means "no claim", and the
  library's own split stands. See **Artist splits arrive late** below.
- One batch bumps `library_version` at most once, and not at all when nothing
  changed — a bump invalidates every client's cached list.

The two counts in the 200 say **how much was applied, not how many entries were
understood**, and they are not bounded by the number of entries sent. An artist
summary carrying a `split_into` claim can create several artists and remove
several more, each of which counts in `artists`, and can rewrite an album's
artist list, which counts in `albums`. Three artist entries and no album entries
can therefore answer `{"artists": 8, "albums": 2}`. Read them as a measure of
work done; the only value to compare against anything is `library_version`.

The `library_version` in a 200 is the value the caller should record as seen: it
already accounts for this batch, so polling `GET /api/library/<player-name>` will
not report the caller's own write back to it as a change. A 409 carries both
tokens for the same reason — the caller needs the new generation to recompute
against and the version for its own bookkeeping.

Both bodies fold that version with **this request's** `X-Forwarded-Prefix`,
exactly as `GET /api/library/<player-name>` folds the one it reports. That is
what makes the two comparable: a caller records the version it was handed here
and compares it against what the library route tells *it*, on its own route.
Note that a request with no prefix at all still gets a folded token rather than
the library's bare counter, so a client must compare the two tokens for equality
and never assume either is the raw value.

The `library_generation` is deliberately **not** folded, in either body. It is
not a validator for anything a proxy rewrites, and a prefixed one would match
nothing the library holds.

A backend that reports no `library_generation` (LMS) refuses any batch that
names one, because it cannot honour the claim: it has no way to tell whether it
has reloaded. Such a caller names no generation.

**Artist splits arrive late**

An album-artist tag may name one artist or several, and the daemon cannot tell
which from the text alone. It splits on separators — the built-in `,`, `&`,
` feat `, ` feat.`, ` featuring `, ` with `, or the player's `artist_separator`
list where it has one, which **replaces** the built-in list rather than adding to
it. That is right for "Simon & Garfunkel" and wrong for "Emerson, Lake &
Palmer". The correction arrives here, in `split_into`.

**So the first load that meets a new album artist shows the plain separator
split, and the enrichment sweep corrects it.** An album may briefly list three
artists where there is one, or one where there are two. This is the same
eventual consistency genres, images and biographies already have on that
screen: the load itself makes no network call for it any more, which is what
took a per-album MusicBrainz round trip out of a load that can cover 200,000
songs. A client that renders an artist list should expect it to change under it
when the library version moves, exactly as it already does for cover art.

**The correction is not unconditional.** The metadata side only claims a split
it can back, so two installs see less than the paragraph above promises:

- **MusicBrainz lookups disabled.** No claim is made at all, because the only
  thing available on that side is a separator split and the loader already did
  one. Such an install sees *no change* from earlier releases: the removed route
  answered with a plain split when MusicBrainz was off, so the answer was
  already the plain split.
- **A name holding none of the built-in separators**, which is what a configured
  `artist_separator` list produces. No claim is made there either, because "no
  separator here, therefore one artist" would rejoin a split the operator's own
  configuration asked for. See the note under `album_artist` in
  [Album](#album) for the case this does not cover.

The shipped configuration enables MusicBrainz, so a default install does get the
correction.

`split_into` has three states and the middle one is easy to miss:

| Value | Meaning |
|---|---|
| absent | No claim. Whatever the library split stays. |
| one element | **The name is a single artist.** This is what undoes a wrong split — it is not the same as absent. |
| several elements | The name is those artists. |

The name a claim is made about is the *unsplit* album-artist string, which is
why `GET /api/library/<player-name>/albums` serves it as `album_artist`: once a
name has been split, the parts cannot be turned back into it, so a caller has no
other way to name it. Such a string appears in
`GET /api/library/<player-name>/artists` only where the library kept it whole.

#### Example
```bash
curl -X POST http://<device-ip>:1080/api/library/mpd/enrichment \
  -H 'Content-Type: application/json' \
  -d '{"library_generation":"5e2b91c0-a3f9c1d2-g3","albums":[{"id":"1","genres":["rock"]}]}'
```

Correcting a wrongly split album artist, and nothing else:
```bash
curl -X POST http://<device-ip>:1080/api/library/mpd/enrichment \
  -H 'Content-Type: application/json' \
  -d '{"artists":[{"name":"Emerson, Lake & Palmer","split_into":["Emerson, Lake & Palmer"]}]}'
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

**Caching**

The response carries a weak `ETag` derived from the library version, for example
`W/"albums-a3f9c1d2-42"`. Send it back as `If-None-Match` and an unchanged library answers
`304 Not Modified` with no body, instead of re-sending the list.

The version moves whenever the library's contents change, including the
background genre and metadata updates that continue for a while after a scan -
so during that window a conditional request will usually still return a full
response. That is the library genuinely changing, not the validator failing.

**A player whose backend does not track changes sends no `ETag`.** MPD does;
LMS does not. A client must treat the header's absence as "cannot revalidate"
rather than assuming the list is stable.

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

**Caching**

The response carries a weak `ETag` derived from the library version, for example
`W/"artists-a3f9c1d2-42"`. Send it back as `If-None-Match` and an unchanged library answers
`304 Not Modified` with no body, instead of re-sending the list.

The version moves whenever the library's contents change, including the
background genre and metadata updates that continue for a while after a scan -
so during that window a conditional request will usually still return a full
response. That is the library genuinely changing, not the validator failing.

**A player whose backend does not track changes sends no `ETag`.** MPD does;
LMS does not. A client must treat the header's absence as "cannot revalidate"
rather than assuming the list is stable.

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

> **An `artist:` identifier answers `302`, not bytes** (from 0.22.0). The
> `Location` is `/api/coverart/artist/<b64>/image`, carrying the request's
> forwarded prefix and any `size`. Artist art belongs to the metadata half —
> it is fetched from providers and kept in its artist store — and this daemon
> no longer calls that half to answer its own routes, so it names the route
> instead. That path is the one the artist lists have always put in
> `thumb_url`, so a client following the redirect lands where it would have
> gone from the list. **Follow redirects**; a client that does not now
> receives a `302` where it received an image before. Earlier daemons answered
> `200` with the bytes.
>
> `?size=` works on the redirect target, which is a change in its favour: it
> was accepted and silently ignored here.

**Query parameters**

| Name | Type | Meaning |
|---|---|---|
| `size` | integer | Longest edge in pixels. Rounded up to the next configured size. The defaults are 100, 140, 200, 280, 400, 800; consult `GET /capabilities` for the authoritative list on this installation. Omit it to get the original. It takes effect for `album:` identifiers on players that keep cover art in acr's image cache; every other combination is accepted and validated but then ignored, and the response is the full-size original with no signal that resizing did not happen. |

**When `size` does nothing.** Resizing works from acr's own image cache, so it
applies only where two things are both true:

- the identifier is an `album:` identifier. Bare track URLs and
  URL-safe-base64 identifiers that do not decode to `artist:` are served at
  full size. An `artist:` identifier is redirected, and `size` is honoured
  there.
- the player keeps its cover art in acr's image cache. **MPD does; LMS does
  not** — an LMS library fetches album art over HTTP from the LMS server on
  every request and never populates the cache, so `?size=` on an LMS player is
  a silent no-op today.

In every one of those cases the response is the full-size original, with a
normal `200` and no error. A client that must know whether it received a
thumbnail should look at the image it got, not at the request it sent.

GIF and BMP sources are never resized either: only the `jpeg`, `png` and
`webp` decoders are compiled in, so anything else is served at full size.

A size larger than the top rung, or larger than the image itself, returns the
original: acr never upscales. A size that is not a positive integer is a
`400`, not a silent fallback.

Responses carry an `ETag` and honour `If-None-Match` with a `304`. The
`Cache-Control` header depends on the identifier: `album:` art does not
change under a given album id, so those responses get
`public, max-age=31536000, immutable` and clients can hold them
indefinitely. Bare track URLs get `public, max-age=86400` instead, so clients
revalidate daily rather than being stuck with a stale image for a year.
`artist:` identifiers are redirected and carry no cache headers of their own;
the route they point at sets the same daily revalidation, because a user can
replace artist art with a new upload.

- **Response**: Binary image data with appropriate Content-Type header
- **Error Response** (404 Not Found): String error message
- **Error Response** (400 Bad Request): String error message when `size` is not a positive integer

#### Example
```bash
curl http://<device-ip>:1080/api/library/mpd/image/album:12345 --output cover.jpg
curl http://<device-ip>:1080/api/library/mpd/image/album:12345?size=400 --output cover-thumb.jpg
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

### Metadata Service Routes

These routes are served by the metadata side of the daemon (the code that will
become a separate `audiocontrol-metadata` process in a later phase) but answer
at `/api` alongside everything else in this document, because both halves
currently share one Rocket. See [architecture](architecture.md) for what that
means and does not mean.

**They exist for clients, and no longer for the player daemon.** They used to
be how it asked over HTTP for what it once computed in-process: artist detail,
artist images and the two resolvers. Every one of those calls is gone, and
nothing in the player daemon calls anything here — it holds no address for this
side at all. What replaced each is in
[the one-way seam spec](specs/2026-09-07-one-way-seam.md) and in
[communications](communications.md); in short, the biography and banner travel
in the enrichment batch, and `/api/library/<p>/image/artist:<name>` redirects to
`/api/coverart/artist/<b64>/image` rather than fetching it.

Data crosses the other way instead: the metadata side subscribes to the
daemon's events, reads its library, posts results back, and asks it for a
Spotify token at `GET /api/spotify/access_token` — see
[Spotify Routes](#spotify-routes) below.

Calling these routes from outside the daemon is entirely supported, and for
artist detail it is now the only way to get a *fresh* answer: the player-facing
routes (`Get Artist by Name`, `Get Artist by ID`, `Get Artist by MusicBrainz
ID`) serve the biography their library was last given by an enrichment sweep,
which is what a client should normally want, while `GET /api/artist/<b64>` with
`?lookup=true` will run a lookup on demand.

*`GET /capabilities` is not one of the four.* The player daemon already
serves `GET /api/capabilities` (see above), and two identical routes at the
same path and rank make Rocket refuse to start rather than pick one. The
metadata side's own copy answers under the second mount below instead.

#### The `/api/metadata/` mount

Every metadata route is mounted a second time under `/api/metadata/`, and the
two mounts mean different things.

- **`/api/...`** — the historical paths. Both shipped clients reach them
  through nginx's `/api/audiocontrol/` prefix, and `/api/coverart/artist/` in
  particular is where every artist thumbnail and every redirected artist image
  request goes. They are not going to move, and nothing in this process calls
  them: `services.metadata` no longer exists.
- **`/api/metadata/...`** — the same routes under the prefix a client will use
  once the metadata side answers on a port of its own. In a later phase nginx
  routes `/api/metadata/` to that process; today it reaches the same code in
  the same process, so a client can be written against it now and keep
  working across the split.

What is mounted under `/api/metadata/`:

| Path | Same as |
| --- | --- |
| `/api/metadata/artist/<artist_b64>` | [Get Artist Detail](#get-artist-detail) |
| `/api/metadata/resolve/title-order` | [Resolve Title Order](#resolve-title-order) |
| `/api/metadata/resolve/artist-split` | [Resolve Artist Split](#resolve-artist-split) |
| `/api/metadata/audiodb/mbid/<mbid>` | [TheAudioDB Integration](#theaudiodb-integration) |
| `/api/metadata/coverart/...` | [Cover Art API](#cover-art-api) |
| `/api/metadata/imagecache/...` | the image cache paths |
| `/api/metadata/lastfm/...` | [Last.fm Integration](#lastfm-integration) |
| `/api/metadata/favourites/...` | [Favourites API](#favourites-api) |
| `/api/metadata/capabilities` | the metadata side's own capabilities report |

`GET /api/metadata/capabilities` is the one path that exists *only* under this
mount, for the reason given above. It reports the metadata side's image size
ladder in the same shape as the player daemon's own capabilities response.
Today both halves read one `images.sizes` list, because they are one process;
once they are two, the list is configured in each and the two must agree.

Everything else in this table answers identically under either prefix, and a
response's own image paths are written with whatever prefix the request
carried, exactly as described in [Image and Lyrics Paths](#image-and-lyrics-paths).

The player daemon's own routes — players, library, volume, lyrics, settings,
cache, background jobs, genres, **Spotify** and the WebSocket — are *not* under
`/api/metadata/`. They stay where they are and will stay on this process after
the split.

`/api/metadata/spotify/...` is gone. It never reached a release: it was added
by the mount that put every metadata route under a second prefix, in this same
unreleased version, so no shipped client can have used it. The Spotify account moved to the player daemon, so `/api/spotify/...` —
the historical path every shipped client already uses — is the only one. No
client-facing URL changed; a client that had adopted the `/api/metadata/`
prefix for Spotify specifically must use `/api/spotify/` instead.

### Spotify Routes

Served by the player daemon at `/api/spotify/`, which through nginx is
`/api/audiocontrol/spotify/`. These paths have not changed and are not going
to.

| Path | Method | Purpose |
| --- | --- | --- |
| `/api/spotify/tokens` | POST | store the tokens an OAuth flow produced |
| `/api/spotify/status` | GET | whether an account is linked, and when its token expires |
| `/api/spotify/logout` | POST | forget the account |
| `/api/spotify/oauth_config` | GET | the OAuth proxy URL and redirect URI |
| `/api/spotify/create_session` | GET | begin a browser-mediated login |
| `/api/spotify/login/<session_id>` | GET | the Spotify authorize URL for that session |
| `/api/spotify/poll/<session_id>` | GET | poll for the tokens that login produced |
| `/api/spotify/check_server` | GET | whether the OAuth proxy is reachable |
| `/api/spotify/access_token` | GET | the current bearer token as `text/plain`; 404 when no account is linked |
| `/api/spotify/playback` | GET | the Spotify playback state |
| `/api/spotify/command/<command>` | POST | play, pause, next, previous, seek, repeat, shuffle |
| `/api/spotify/currently_playing` | GET | the currently playing track |
| `/api/spotify/search` | POST | search Spotify for artists, albums or tracks |

The last four are served only when `services.spotify.api_enabled` is `true` in
`audiocontrol.json`; the rest are served whether it is set or not. That
includes `access_token`, which the metadata side reads for its cover-art and
favourites providers — gating it on a flag meant for client-facing playback
routes would turn off Spotify cover art on every device that has not set it.

#### Get Artist Detail

Returns what the metadata side knows about one artist, by name.

- **Endpoint**: `/api/artist/<artist_b64>`
- **Method**: GET
- **Path Parameters**:
  - `artist_b64` (string): The artist name, URL-safe base64 encoded (see
    [URL-Safe Base64 Encoding](#url-safe-base64-encoding))
- **Query Parameters**:
  - `lookup` (boolean, optional, default `false`): when `true` and nothing is
    cached yet, run a synchronous lookup through the provider chain (the same
    one a library load runs) before answering. The player daemon never sets
    this; it accepts a miss and waits for the next enrichment batch instead of
    paying for a lookup on every request.
- **Response** (200 OK): the cached `ArtistMeta` --- MusicBrainz IDs, thumbnail
  and banner URLs (in the daemon's own internal form, the same as elsewhere in
  this API), biography, biography source, genres, and whether the name is a
  partial match on a multi-artist string.
- **Response** (404 Not Found): nothing is cached for this artist, and either
  `lookup` was not set or the lookup found nothing.

```bash
curl "http://<device-ip>:1080/api/artist/UGluayBGbG95ZA"
```

#### Resolve Title Order

Guesses which half of a two-part radio stream title is the artist, the same
MusicBrainz-backed guess the stream title splitter makes for MPD stations.

- **Endpoint**: `/api/resolve/title-order`
- **Method**: GET
- **Query Parameters**:
  - `part1` (string, required): the first half of the split title
  - `part2` (string, required): the second half
- **Response** (200 OK):

  ```json
  { "order": "artist_song" }
  ```

  `order` is one of `artist_song`, `song_artist`, `unknown` (neither reading
  matched anything) or `undecided` (both readings did). With MusicBrainz
  lookups disabled the answer is always `unknown`, which callers already treat
  as "keep the fallback order and don't learn from this."

#### Resolve Artist Split

Decides whether a combined artist string names more than one artist.

**The player daemon no longer calls this.** It was the last route on the
metadata daemon that it did call — once per album, blocking, on a load that can
cover 200,000 songs. Both library loaders now split on separators alone and are
corrected afterwards, through `split_into` in
[Apply Enrichment](#apply-enrichment); see **Artist splits arrive late** there
for what a user sees. The route stays for clients that ask the question
directly, and answers exactly as it did.

- **Endpoint**: `/api/resolve/artist-split`
- **Method**: GET
- **Query Parameters**:
  - `name` (string, required): the combined artist string, e.g. `Simon &
    Garfunkel`
  - `separator` (string, repeatable, optional): one separator per occurrence —
    `?name=X&separator=,&separator=%26` — to try instead of the built-in
    defaults (`,`, `&`, ` feat `, ` feat.`, ` featuring `, ` with `). Omit it
    entirely for the defaults; sending none is the same as omitting it.

    Repeated rather than one comma-separated value, because `,` is itself the
    first default separator: a comma-joined list cannot carry it, and a
    separator of `", "` would arrive as `" "` and split every two-word artist
    name in two.
- **Response** (200 OK):

  ```json
  { "artists": ["Simon", "Garfunkel"] }
  ```

  or, when the name is a single artist:

  ```json
  { "artists": null }
  ```

  The answer is cached without expiry once computed, keyed on the exact input
  string.

#### Enrichment is not requested over a route

There was a `POST /api/enrich/nudge?player=` here, an advisory hint that the
metadata side should look at one player's library sooner than its next periodic
poll. **It is gone, and nothing replaced it as a route.** No route on the
metadata side is called by the player daemon any more, so the announcement
travels the other way instead: a library load emits
[`library_changed`](websocket.md#library_changed) on `/api/events`, and the
metadata side reacts to that.

It is recorded here rather than dropped silently because the route did exist in
this repository, though never in a released package — it was added and removed
within 0.22.0, so no shipped client can have called it. A caller that somehow
does gets a 404. Nothing else about enrichment changed: the metadata side still
reads `GET /api/library/<p>`, `/artists` and `/albums`, and still posts results
to [Apply Enrichment](#apply-enrichment).

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

All five cover art lookup endpoints accept an optional `include_slow` query
parameter:

```
GET /api/coverart/song/<title_b64>/<artist_b64>?include_slow=true
```

By default a lookup queries only providers that answer quickly, plus any
answer a slow provider has already cached, and returns within about five
seconds. `include_slow=true` also waits for providers configured under
`services.external_coverart`, which may take tens of seconds — see
[external cover art providers](external-coverart.md). Use it only where
someone asked for the lookup and can be shown that it is running; it is not
suitable for a page load.

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

**Query parameters**

| Name | Type | Meaning |
|---|---|---|
| `size` | integer | Longest edge in pixels. Rounded up to the next configured size. The defaults are 100, 140, 200, 280, 400, 800; consult `GET /capabilities` for the authoritative list on this installation. Omit it to get the original. |

A size larger than the top rung, or larger than the image itself, returns the
original: acr never upscales. A size that is not a positive integer is a
`400`, not a silent fallback. GIF and BMP sources are served at full size
whatever `size` says, because only the `jpeg`, `png` and `webp` decoders are
compiled in.

Responses carry `ETag` and `Cache-Control: public, max-age=86400`. The shorter
lifetime is deliberate: an artist image can be replaced by uploading a custom
one, so it is not immutable the way album art is.

- **Response**: 
  - **Success (200)**: Binary image data with appropriate `Content-Type` header (`image/jpeg`, `image/png`, `image/gif`, or `image/webp`)
  - **Not Found (404)**: String error message if no cached image is available
  - **Bad Request (400)**: String error message for invalid artist name encoding or an invalid `size`
  - **Internal Server Error (500)**: String error message if image file cannot be read

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

# Get a thumbnail-sized variant
curl http://<device-ip>:1080/api/coverart/artist/VGhlIEJlYXRsZXM/image?size=400 -o beatles-thumb.jpg

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
          { "name": "spotify", "display_name": "Spotify" },
          { "name": "lastfm", "display_name": "Last.fm" },
          { "name": "theaudiodb", "display_name": "TheAudioDB" },
          { "name": "fanarttv", "display_name": "FanArt.tv" }
        ]
      },
      {
        "method": "Song",
        "providers": [
          { "name": "spotify", "display_name": "Spotify" },
          { "name": "lastfm", "display_name": "Last.fm" },
          { "name": "theaudiodb", "display_name": "TheAudioDB" }
        ]
      },
      {
        "method": "Album",
        "providers": [
          { "name": "spotify", "display_name": "Spotify" },
          { "name": "theaudiodb", "display_name": "TheAudioDB" }
        ]
      },
      {
        "method": "Url",
        "providers": []
      }
    ]
  }
  ```

Every provider is registered unconditionally, so this listing is the same on
every device; only the results differ. Spotify answers nothing until an account
has been linked, so before 0.16.0 the `Song` method returned an empty result for
every track on a device without one. Last.fm covers it from 0.16.0 on, using the
album art it reports for the track, and needs only an API key rather than a
linked account. TheAudioDB joins it in 0.17.0, answering with the track's own
picture where it has one and with its album's cover otherwise.

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

### List Artist Images

An artist holds a *set* of images — the downloaded `cover.jpg`/`custom.jpg` plus whatever has been uploaded — rather than a single picture. This lists every member of that set and marks which one is selected.

- **Endpoint**: `/api/coverart/artist/<artist_b64>/images`
- **Method**: GET
- **Parameters**:
  - `artist_b64` (string, required): URL-safe base64 encoded artist name
- **Response** (Success):
  ```json
  {
    "images": [
      {
        "id": "3f2504e04f8964..." ,
        "url": "/api/coverart/artist/VGhlIEJlYXRsZXM/image/3f2504e04f8964...",
        "source": "upload",
        "selected": true,
        "width": 800,
        "height": 800,
        "size_bytes": 123456
      }
    ]
  }
  ```
- **Response** (Error): `400` with a plain-text body, `Invalid artist name encoding`

**Important Notes**:
- An artist nobody has uploaded anything for, or whose name is unknown, returns `{"images": []}` with status 200, not a 404 — absence is an answer, not an error.
- `source` is `"upload"` for a file the user uploaded, `"download"` for the daemon's own `cover.jpg`/`custom.jpg`.
- `id` is the value the other three routes below take as `<id>`.
- A member whose file cannot be read or measured is dropped from the listing rather than failing the whole request.

#### Examples

```bash
# List every image stored for an artist
curl http://<device-ip>:1080/api/coverart/artist/VGhlIEJlYXRsZXM/images
```

### Get Artist Image by ID

Serve one specific member of an artist's image set, by the id reported by the listing above.

- **Endpoint**: `/api/coverart/artist/<artist_b64>/image/<id>`
- **Method**: GET
- **Parameters**:
  - `artist_b64` (string, required): URL-safe base64 encoded artist name
  - `id` (string, required): the member's id, as reported by `GET /artist/<artist_b64>/images`
  - `size` (integer, optional): requested size in pixels on the longest edge; the image is resized and cached at that size
- **Response** (Success): the image bytes, with a matching `Content-Type` and `ETag`/`If-None-Match` support
- **Response** (Error): `404` when `id` is not a member of the set, `400` for an undecodable artist name or an invalid `size`

**Important Notes**:
- Unlike `GET /artist/<artist_b64>/image`, this never falls back to downloading anything: an unknown id is a 404, because the client asked for one specific picture, not "an" image.

#### Examples

```bash
# Fetch one member of the set at its stored size
curl http://<device-ip>:1080/api/coverart/artist/VGhlIEJlYXRsZXM/image/3f2504e04f8964... -o image.jpg

# Fetch it resized to 200px on the longest edge
curl "http://<device-ip>:1080/api/coverart/artist/VGhlIEJlYXRsZXM/image/3f2504e04f8964...?size=200" -o image.jpg
```

### Upload Artist Image

Add one image to an artist's set, and select it.

- **Endpoint**: `/api/coverart/artist/<artist_b64>/upload`
- **Method**: POST
- **Content-Type**: `application/json`
- **Parameters**:
  - `artist_b64` (string, required): URL-safe base64 encoded artist name
- **Request Body**:
  ```json
  {
    "image_base64": "string (required) - the image bytes, base64 encoded"
  }
  ```
- **Response** (Success):
  ```json
  {
    "success": true,
    "id": "3f2504e04f8964...",
    "message": "Stored image for 'The Beatles'"
  }
  ```
- **Response** (Error):
  ```json
  {
    "success": false,
    "id": null,
    "message": "Error description (e.g., 'Invalid base64 data: ...', 'Invalid image data: ...', 'This artist already has the maximum of 10 uploaded images; delete one first')"
  }
  ```

**Important Notes**:
- The bytes are decoded and validated as an image before anything is written; garbage is refused without touching the artist's set.
- The upload is stored as a member of the set and then selected — uploading a picture is a request to use it. The image that was selected before stays in the set, so the choice is reversible.
- An artist can hold at most 10 uploaded images at once; re-uploading bytes that are already stored is not a new member and does not count against the cap.
- `id` is stable across uploads: uploading the same bytes twice returns the same id rather than storing a duplicate.

#### Examples

```bash
# Upload and select a new image for an artist
curl -X POST http://<device-ip>:1080/api/coverart/artist/VGhlIEJlYXRsZXM/upload \
  -H "Content-Type: application/json" \
  -d '{"image_base64": "iVBORw0KGgoAAAANSUhEUgAA..."}'

# Response:
# {
#   "success": true,
#   "id": "3f2504e04f8964...",
#   "message": "Stored image for 'The Beatles'"
# }
```

### Delete Artist Image

Remove one member from an artist's set.

- **Endpoint**: `/api/coverart/artist/<artist_b64>/image/<id>`
- **Method**: DELETE
- **Parameters**:
  - `artist_b64` (string, required): URL-safe base64 encoded artist name
  - `id` (string, required): the member's id, as reported by `GET /artist/<artist_b64>/images`
- **Response** (Success):
  ```json
  {
    "success": true,
    "message": "Deleted image '3f2504e04f8964...' for artist 'The Beatles'"
  }
  ```
- **Response** (Error):
  ```json
  {
    "success": false,
    "message": "Error description (e.g., 'No image \'...\' for artist \'...\'', 'Invalid artist name encoding')"
  }
  ```

**Important Notes**:
- An `id` that is not a member of the set is `success: false`, not `true` — deleting something already gone is not a success.
- Deleting the currently selected member clears the selection, and the lookup falls back to the chain, so removing an upload reveals the downloaded image again rather than leaving the artist with nothing.

#### Examples

```bash
# Remove one image from an artist's set
curl -X DELETE http://<device-ip>:1080/api/coverart/artist/VGhlIEJlYXRsZXM/image/3f2504e04f8964...

# Response:
# {
#   "success": true,
#   "message": "Deleted image '3f2504e04f8964...' for artist 'The Beatles'"
# }
```

**Selecting a listed image via Update Artist Image**: posting one of an artist's own image URLs — as reported by the `url` field in the listing above — to `POST /artist/<artist_b64>/update` selects that member instead of being treated as a remote URL to fetch; the daemon does not make HTTP requests to itself, and the bytes are already where they need to be. The response message names the id that was selected (`"Selected image '<id>' for artist '<name>'"`) rather than the generic "Artist image URL updated successfully" used for a genuine remote URL.

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
use audiocontrol_metadata::coverart::{get_coverart_manager, CoverartProvider};

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

The Cache API provides endpoints to retrieve information about the internal caching system used by the audio control service. This includes statistics about memory and disk cache usage, as well as image cache statistics. It also carries the one maintenance operation the cache exposes: purging generated image variants.

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
    "variant_images": 48,
    "variant_size": 921600,
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
  - `total_images` (number): Total number of cached images, generated variants included
  - `total_size` (number): Total size of all cached images in bytes, generated variants included
  - `variant_images` (number): How many of `total_images` are generated thumbnails (see the `size` parameter on the image endpoints)
  - `variant_size` (number): How many of `total_size` bytes are generated thumbnails
  - `last_updated` (number): Timestamp when statistics were last updated (Unix epoch seconds)

`variant_images` and `variant_size` are a subset of the totals, not an
addition to them. They are the figures to watch before and after
`POST /api/imagecache/variants/purge`: the image cache has no eviction policy,
and thumbnails are the only content in it that grows without bound.
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
    "variant_images": 120,
    "variant_size": 2359296,
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

### Purge Image Variants

Deletes every generated thumbnail from the image cache, keeping all originals.

**Endpoint**: `POST /api/imagecache/variants/purge`

**Request Body**: none

**Response Format**:
```json
{
  "removed": 4271
}
```

**Response Fields**:
- `removed` (number): How many variant files were deleted

**Example Request**:
```bash
curl -X POST "http://<device-ip>:1080/api/imagecache/variants/purge"
```

**Why it exists**: the image cache has no eviction policy and no size cap, and
variants -- the thumbnails generated for the `size` parameter on the image
endpoints -- are the only content in it that grows without bound. On a library
of 11,000 albums the four sizes come to roughly 213 MB. This is the way to
reclaim that space on a small SD card.

**It is always safe.** Variants are derived data: every one of them is
regenerated from its original the next time a client asks for that size, so a
purge costs CPU on the next request, never an image. Originals are never
touched.

Watch `image_cache_stats.variant_size` from `GET /api/cache/stats` to see how
much a purge would reclaim, and to confirm what it reclaimed.

- **Response**:
  - **Success (200)**: JSON object with the number of files removed
  - **Internal Server Error (500)**: String error message if the cache could not be walked

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
  "album_artist": "Artist 1 & Artist 2",
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
| album_artist | string | The album-artist tag as the backend reported it, before `artists` was split out of it. **Omitted** where the library recorded none. It is here because the split is lossy — "Emerson" plus "Lake" plus "Palmer" cannot be turned back into the name they came from — and it is the name an enrichment batch makes a `split_into` claim about; see [Apply Enrichment](#apply-enrichment). Note that a player's configured `artist_separator` list does not reach the metadata side, so a claim about a name that *also* holds a built-in separator can override a split made with that list. |
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