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
  "player_name": "player_id",
  "source": {
    "player_id": "player_id",
    "player_name": "player_name",
    "is_active": true|false
  },
  // Additional fields specific to the event type
}
```

## Event Types

### `state_changed`

Sent when player state changes (playing, paused, stopped):

```json
{
  "type": "state_changed",
  "state": "playing|paused|stopped",
  "player_name": "mpd",
  "source": {
    "player_id": "mpd:6600",
    "player_name": "mpd",
    "is_active": true
  }
}
```

### `song_changed`

Sent when the current song changes:

```json
{
  "type": "song_changed",
  "song": {
    "title": "Song Title",
    "artist": "Artist Name",
    "album": "Album Name",
    "duration": 180,
    "uri": "spotify:track:1234567890",
    "artwork_url": "http://example.com/image.jpg"
  },
  "player_name": "spotify",
  "source": {
    "player_id": "spotify",
    "player_name": "spotify",
    "is_active": true
  }
}
```

### `position_changed`

Sent periodically when the playback position changes:

```json
{
  "type": "position_changed",
  "position": {
    "position": 45.5,
    "duration": 180.0
  },
  "player_name": "mpd",
  "source": {
    "player_id": "mpd:6600",
    "player_name": "mpd"
  }
}
```

Note: Some players might send a simplified version with just the position as a number:

```json
{
  "type": "position_changed",
  "position": 45.5,
  "player_name": "raat",
  "source": {
    "player_id": "raat",
    "player_name": "raat"
  }
}
```

### `loop_mode_changed`

Sent when the loop mode changes:

```json
{
  "type": "loop_mode_changed",
  "mode": "none|track|playlist",
  "player_name": "mpd",
  "source": {
    "player_id": "mpd:6600",
    "player_name": "mpd"
  }
}
```

### `shuffle_changed`

Sent when shuffle mode changes:

```json
{
  "type": "shuffle_changed",
  "shuffle": true|false,
  "player_name": "spotify",
  "source": {
    "player_id": "spotify",
    "player_name": "spotify"
  }
}
```

### `capabilities_changed`

Sent when the player's capabilities change:

```json
{
  "type": "capabilities_changed",
  "capabilities": ["play", "pause", "stop", "next", "previous", "seek", "shuffle", "loop", "queue"],
  "player_name": "spotify",
  "source": {
    "player_id": "spotify",
    "player_name": "spotify"
  }
}
```

### `metadata_changed`

Sent when metadata for the current track is updated:

```json
{
  "type": "metadata_changed",
  "metadata": {
    "title": "Updated Song Title",
    "artist": "Updated Artist",
    "album": "Updated Album",
    "artwork_url": "http://example.com/updated_image.jpg"
  },
  "player_name": "mpd",
  "source": {
    "player_id": "mpd:6600",
    "player_name": "mpd"
  }
}
```

### `database_updating`

Sent when a player's database/library is being updated:

```json
{
  "type": "database_updating",
  "percentage": 75,
  "player_name": "mpd",
  "source": {
    "player_id": "mpd:6600",
    "player_name": "mpd"
  }
}
```

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
            "position_changed",
            "loop_mode_changed",
            "shuffle_changed",
            "capabilities_changed",
            "metadata_changed"
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