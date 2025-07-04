# Spotify/Librespot Event Pipe Format

The `EventPipeReader` expects JSON events from a named pipe or network connection. Each event is a complete JSON object on separate lines, with opening and closing braces on their own lines.

## Event Structure

Each event follows this basic structure:

```json
{
  "event": "event_type",
  "FIELD_NAME": "field_value",
  ...
}
```

## Supported Event Types

### 1. `playing` Event

Indicates that playback has started or resumed.

**Format:**

```json
{
  "event": "playing",
  "POSITION_MS": "12345",
  "TRACK_ID": "spotify:track:4uLU6hMCjMI75M1A2tKUQC"
}
```

**Fields:**

- `POSITION_MS` (optional): Current playback position in milliseconds
- `TRACK_ID` (optional): Spotify track identifier

### 2. `paused` Event

Indicates that playback has been paused.

**Format:**

```json
{
  "event": "paused",
  "POSITION_MS": "67890",
  "TRACK_ID": "spotify:track:4uLU6hMCjMI75M1A2tKUQC"
}
```

**Fields:**

- `POSITION_MS` (optional): Current playback position in milliseconds when paused
- `TRACK_ID` (optional): Spotify track identifier

### 3. `stopped` Event

Indicates that playback has been stopped.

**Format:**

```json
{
  "event": "stopped",
  "TRACK_ID": "spotify:track:4uLU6hMCjMI75M1A2tKUQC"
}
```

**Fields:**

- `TRACK_ID` (optional): Spotify track identifier

### 4. `track_changed` Event

Indicates that a new track has started playing. Contains full track metadata.

**Format:**

```json
{
  "event": "track_changed",
  "NAME": "Bohemian Rhapsody",
  "ARTISTS": "Queen",
  "ALBUM": "A Night at the Opera",
  "ALBUM_ARTISTS": "Queen",
  "NUMBER": "11",
  "DURATION_MS": "354000",
  "COVERS": "https://i.scdn.co/image/ab67616d0000b273ce4f1737bc8a646c8c4bd25a",
  "TRACK_ID": "spotify:track:4uLU6hMCjMI75M1A2tKUQC",
  "URI": "spotify:track:4uLU6hMCjMI75M1A2tKUQC",
  "POPULARITY": "85",
  "IS_EXPLICIT": "false"
}
```

**Fields:**

- `NAME`: Track title
- `ARTISTS`: Artist name(s)
- `ALBUM`: Album name
- `ALBUM_ARTISTS`: Album artist name(s)
- `NUMBER`: Track number on the album
- `DURATION_MS`: Track duration in milliseconds
- `COVERS`: Cover art URL
- `TRACK_ID`: Spotify track identifier
- `URI`: Spotify URI for the track
- `POPULARITY`: Track popularity score (0-100)
- `IS_EXPLICIT`: Whether the track contains explicit content ("true" or "false")

### 5. `volume_changed` Event

Indicates that the volume has been changed.

**Format:**

```json
{
  "event": "volume_changed",
  "VOLUME": "32768"
}
```

**Fields:**

- `VOLUME`: Volume level (0-65536, where 65536 is maximum volume)

### 6. `repeat_changed` Event

Indicates that the repeat/loop mode has been changed.

**Format:**

```json
{
  "event": "repeat_changed",
  "REPEAT": "true",
  "REPEAT_TRACK": "false"
}
```

**Fields:**

- `REPEAT`: Whether playlist repeat is enabled ("true" or "false")
- `REPEAT_TRACK`: Whether single track repeat is enabled ("true" or "false")

### 7. `shuffle_changed` Event

Indicates that the shuffle mode has been changed.

**Format:**

```json
{
  "event": "shuffle_changed",
  "SHUFFLE": "true"
}
```

**Fields:**

- `SHUFFLE`: Whether shuffle is enabled ("true" or "false")

### 8. `seeked` Event

Indicates that the playback position has been changed (seek operation).

**Format:**

```json
{
  "event": "seeked",
  "POSITION_MS": "123456",
  "TRACK_ID": "spotify:track:4uLU6hMCjMI75M1A2tKUQC"
}
```

**Fields:**

- `POSITION_MS`: New playback position in milliseconds
- `TRACK_ID` (optional): Spotify track identifier

### 9. Ignored Events

The following event types are recognized but ignored:

- `loading`
- `play_request_id_changed`
- `preloading`

## Stream Format

Events are sent as line-delimited JSON, with each JSON object formatted across multiple lines:

```json
{
  "event": "track_changed",
  "NAME": "Example Song",
  "ARTISTS": "Example Artist"
}
{
  "event": "playing",
  "POSITION_MS": "0"
}
{
  "event": "paused",
  "POSITION_MS": "30000"
}
```

## Example Event Sequence

Here's a typical sequence of events when a song starts playing:

```json
{
  "event": "track_changed",
  "NAME": "Bohemian Rhapsody",
  "ARTISTS": "Queen",
  "ALBUM": "A Night at the Opera",
  "ALBUM_ARTISTS": "Queen",
  "NUMBER": "11",
  "DURATION_MS": "354000",
  "COVERS": "https://i.scdn.co/image/ab67616d0000b273ce4f1737bc8a646c8c4bd25a",
  "TRACK_ID": "spotify:track:4uLU6hMCjMI75M1A2tKUQC",
  "URI": "spotify:track:4uLU6hMCjMI75M1A2tKUQC",
  "POPULARITY": "85",
  "IS_EXPLICIT": "false"
}
{
  "event": "playing",
  "POSITION_MS": "0",
  "TRACK_ID": "spotify:track:4uLU6hMCjMI75M1A2tKUQC"
}
{
  "event": "seeked",
  "POSITION_MS": "60000",
  "TRACK_ID": "spotify:track:4uLU6hMCjMI75M1A2tKUQC"
}
{
  "event": "paused",
  "POSITION_MS": "60000",
  "TRACK_ID": "spotify:track:4uLU6hMCjMI75M1A2tKUQC"
}
```

## Implementation Notes

1. **JSON Parsing**: Each event must be a valid JSON object
2. **Line Format**: Opening `{` and closing `}` braces should be on separate lines
3. **String Values**: All field values are strings, even numeric values
4. **Optional Fields**: Most fields are optional and may not be present in all events
5. **Error Handling**: Invalid JSON or unknown event types are logged but ignored
6. **Reconnection**: The reader supports automatic reconnection with exponential backoff

## Usage

The `EventPipeReader` can be configured to:

- Read from named pipes or network connections
- Call a callback function when events are parsed
- Automatically reconnect when the connection is lost
- Log events for debugging purposes

```rust
// Create a reader
let mut reader = EventPipeReader::new("/path/to/event/pipe");

// Set up a callback
reader.set_callback(Box::new(|song, player_state, capabilities, stream_details| {
    // Handle the parsed event data
    println!("Received event: {:?}", player_state.state);
}));

// Start reading with automatic reconnection
reader.reopen_with_backoff().unwrap();
```
