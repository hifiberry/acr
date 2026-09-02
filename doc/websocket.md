# WebSocket API

The Audiocontrol provides a WebSocket interface for real-time updates about player state, playback, and other events. This allows clients to maintain synchronized state without constant polling.

## Connection

Connect to the WebSocket endpoint at:

```
ws://<host>:<port>/api/events
```

Where:
- `<host>` is the address of the Audiocontrol server
- `<port>` is the port number (default is 1080)

> **On a HiFiBerryOS device, clients do not connect to port 1080.** nginx fronts
> the device on port 80 and proxies `/api/audiocontrol/` to `127.0.0.1:1080/api/`,
> so the WebSocket URL a client uses is `ws://<device>/api/audiocontrol/events`.
> The port 1080 form above is for direct access when testing against audiocontrol
> on its own.

## Message Format

All messages are JSON-formatted and follow these conventions:

### From Client to Server

#### Subscription Message

When a client connects to the WebSocket, it should send a subscription message to specify which events it wants to receive:

```json
{
  "players": ["mpd", "spotify"],  // Array of player IDs to subscribe to, or null for all players
  "event_types": ["state_changed", "song_changed"]  // Array of event types to subscribe to
}
```

Parameters:
- `players`: (Optional) Array of player IDs to subscribe to. Use `null` to subscribe to all players including the active player.
- `event_types`: (Optional) Array of event types to subscribe to. If omitted, subscribes to all events.

### From Server to Client

#### Welcome Message

When a client first connects:

```json
{
  "type": "welcome",
  "message": "Connected to AudioControl WebSocket API"
}
```

#### Subscription Confirmation

After a subscription request is processed:

```json
{
  "type": "subscription_updated",
  "message": "Subscription updated"
}
```

#### Event Messages

Event messages follow this general format:

```json
{
  "type": "event_type",
  "player_name": "mpd",
  "player_id": "mpd:6600",
  "source": {
    "player_name": "mpd",
    "player_id": "mpd:6600"
  }
  // Additional fields specific to the event type
}
```

`player_name` and `player_id` appear both at the top level and inside `source`.

> **Known issue.** The `player_id` inside `source` is currently built by appending
> the hardcoded MPD port `6600` to the player name, so it is wrong for every player
> other than MPD. The top-level `player_id`, which comes from the event's own
> source, is correct. **Clients should read the top-level `player_id` and ignore
> `source.player_id`.** For `volume_changed`, which has no player source at all,
> `source` degrades to `{"player_name": null, "player_id": "system:6600"}`.

`source` contains only `player_name` and `player_id`. It has no `is_active` field.

## Event Types

The authoritative list is `convert_to_websocket_message` in `src/api/events.rs`.
Every event below carries the common `player_name`, `player_id` and `source`
fields documented above; only the event-specific fields are shown.

### `state_changed`

Sent when player state changes.

```json
{
  "type": "state_changed",
  "state": "playing"
}
```

`state` is the string form of `PlaybackState`.

### `song_changed`

Sent when the current song changes. `song` may be `null` when playback stops with
no track loaded.

```json
{
  "type": "song_changed",
  "song": {
    "title": "Song Title",
    "artist": "Artist Name",
    "album": "Album Name",
    "duration": 180,
    "uri": "spotify:track:1234567890",
    "cover_art_url": "/api/library/mpd/image/..."
  }
}
```

### `song_information_update`

Sent when information about the *current* song is enriched after the fact — cover
art resolved, metadata looked up.

```json
{
  "type": "song_information_update",
  "song": { "title": "Song Title", "artist": "Artist Name", "cover_art_url": "..." }
}
```

**This is a partial update.** `title` and `artist` are present only so a client can
confirm the update still applies to the song it is showing; they are not themselves
updated. Every other field is optional, and an absent field means **unchanged** — it
does not mean "cleared". A client that overwrites its current song wholesale with
this payload will blank out fields that were previously populated.

From 0.16.0 `cover_art_url` for a radio stream can be superseded shortly after
the `song_changed` that introduced it. A stream carries no artwork for the track
itself, so the MPD backend fills the field with the station's logo straight away
— a client always has something to show — and marks it by setting
`song.metadata.cover_art_source` to `"station_logo"`. When a lookup then finds
the track's real album art, a `song_information_update` carries the better image
and resets `cover_art_source` to the provider that supplied it. Before 0.16.0 the
logo was the final answer and no marker was sent.

A client that treats the first `cover_art_url` it sees as final will keep showing
the station logo; one that applies partial updates as documented above needs no
change. Artwork that arrives with the song is never marked, and is never
replaced.

The better image lives only in this event. `GET /api/now-playing` is built from
the player's stored song, which the lookup does not write back into, so it keeps
returning the station logo -- a client that reconnects and re-fetches
now-playing will fall back to the logo until the next
`song_information_update`.

`song.cover_art_url` and `song.metadata.lyrics_url` carry the externally
visible prefix when the connection was upgraded through a proxy that reports
`X-Forwarded-Prefix`. Before 0.15.0 these two fields carried the internal path
on the WebSocket while the REST `now-playing` response carried the external
one; a client that compensated for that difference by adding the prefix itself
should check for it first, as the shipped clients already do.

### `position_changed`

Sent periodically as playback position advances.

```json
{
  "type": "position_changed",
  "position": 45.5
}
```

`position` is a number — seconds, as a float. It is never an object. Duration is
not included; take it from the current song.

### `loop_mode_changed`

```json
{
  "type": "loop_mode_changed",
  "mode": "song"
}
```

`mode` is one of `"no"`, `"song"`, `"playlist"` — the `Display` form of `LoopMode`.
Note these are not the enum's Rust variant names (`None`, `Track`, `Playlist`), and
the field is `mode`, not `loop_mode`.

### `shuffle_changed`

Sent when shuffle mode changes.

```json
{
  "type": "shuffle_changed",
  "shuffle": true,
  "enabled": true
}
```

`shuffle` is the canonical field. `enabled` carries the same value and is emitted
only for compatibility with clients written before the rename described below; new
clients should read `shuffle` and treat `enabled` as deprecated.

> **Renamed.** This event was previously emitted as `random_changed`. The name was
> inconsistent with the rest of the API, where the capability is `shuffle` and the
> inbound update event is `shuffle_changed` — and because the server filters
> subscriptions by event name, a client subscribing to `shuffle_changed` (as the
> WebUI did) never received it. The subscription filter still accepts
> `random_changed`, so clients that subscribed to the old name keep working.

### `capabilities_changed`

```json
{
  "type": "capabilities_changed",
  "capabilities": ["play", "pause", "stop", "next", "previous", "seek", "shuffle", "loop", "queue"]
}
```

### `queue_changed`

Sent when the queue contents change. Carries no payload beyond the common fields —
clients must re-fetch `/api/player/<player-name>/queue`.

```json
{
  "type": "queue_changed"
}
```

### `active_player_changed`

Sent when the active player changes. `new_player_id` is the player becoming active;
the common `player_id` still refers to the event's source.

```json
{
  "type": "active_player_changed",
  "new_player_id": "spotify:librespot"
}
```

### `database_updating`

Sent while a player's library is being scanned. All four payload fields are
optional.

```json
{
  "type": "database_updating",
  "artist": "Artist Name",
  "album": "Album Name",
  "song": "Song Title",
  "percentage": 75
}
```

### `volume_changed`

A system-wide event with no player source. `decibels` and `raw_value` are optional.

```json
{
  "type": "volume_changed",
  "control_name": "Digital",
  "display_name": "Digital",
  "percentage": 65.0,
  "decibels": -18.5,
  "raw_value": 168
}
```

### Events that do not exist

Earlier revisions of this document listed `metadata_changed`. It has never been
emitted and there is no such event type; `song_information_update` covers that
case. It is noted here because clients were written against it.

## Example Client Implementation

Here's a basic JavaScript example for connecting to the WebSocket API:

```javascript
// Connect to the WebSocket server
const socket = new WebSocket('ws://localhost:1080/api/events');

// Connection opened
socket.addEventListener('open', (event) => {
    // Subscribe to all events for the active player
    const subscription = {
        players: null,  // null for active player
        event_types: [
            "state_changed",
            "song_changed",
            "song_information_update",
            "position_changed",
            "loop_mode_changed",
            "shuffle_changed",
            "capabilities_changed",
            "queue_changed"
        ]
    };
    socket.send(JSON.stringify(subscription));
});

// Listen for messages
socket.addEventListener('message', (event) => {
    try {
        const data = JSON.parse(event.data);
        console.log('Message from server:', data);
        
        // Handle different event types
        if (data.type === 'state_changed') {
            console.log(`Player ${data.player_name} state: ${data.state}`);
        }
        else if (data.type === 'song_changed') {
            console.log(`Now playing: ${data.song.title} by ${data.song.artist}`);
        }
    } catch (e) {
        console.error('Error parsing message:', e);
    }
});

// Connection closed
socket.addEventListener('close', (event) => {
    console.log('Connection closed:', event.code, event.reason);
});

// Connection error
socket.addEventListener('error', (error) => {
    console.error('WebSocket error:', error);
});
```

## Error Handling

If the server encounters an error processing a subscription request, it will send an error message:

```json
{
  "type": "error",
  "message": "Error message details",
  "code": 1001
}
```

Common error codes:
- 1001: Invalid subscription format
- 1002: Unknown player specified
- 1003: Unknown event type specified

## Best Practices

1. **Handle reconnections**: Implement automatic reconnection if the connection drops
2. **Validate messages**: Always check the message format before processing
3. **Subscription management**: Only subscribe to events you need to minimize traffic
4. **Backoff strategy**: Use exponential backoff for reconnection attempts