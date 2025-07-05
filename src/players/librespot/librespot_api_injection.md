# Injecting Data to Librespot via the API

This guide explains how to send events to the librespot player using the audiocontrol API and `curl`.

## Prerequisites
- Audiocontrol server running with librespot support
- API accessible (default: `http://localhost:1350`)
- Librespot player registered (usually as `librespot`)

## Sending a "playing" State Event
To set the librespot player to the "playing" state, use:

```
curl -X POST \
  -H "Content-Type: application/json" \
  -d '{"type":"state_changed","state":"playing"}' \
  http://localhost:1350/api/players/librespot/event
```

## Sending a Song Change Event
To simulate a song change:

```
curl -X POST \
  -H "Content-Type: application/json" \
  -d '{
    "type": "song_changed",
    "song": {
      "title": "Test Song",
      "artist": "Test Artist",
      "album": "Test Album",
      "duration": 180.0
    }
  }' \
  http://localhost:1350/api/players/librespot/event
```

## Checking the Player State
To verify the current state:

```
curl http://localhost:1350/api/players/librespot
```

Look for the `"state":"playing"` field in the output.

## Debugging
- Run audiocontrol with debug logging enabled to see detailed logs:
  - **PowerShell:**
    ```
    $env:RUST_LOG="debug"; cargo run
    ```
- Check the terminal output for `[API DEBUG]` lines to trace event handling.

## Notes
- Replace `localhost:1350` with your actual API host/port if different.
- You can inject other event types by changing the JSON payload accordingly.

---
For more details, see the librespot player source code and API documentation.
