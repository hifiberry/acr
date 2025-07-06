# Generic Player Controller API Documentation

The Generic Player Controller is a versatile player implementation that accepts API updates to control playback state, metadata, and other player properties. It's designed for integration with external media players or as a testing interface.

## Overview

The Generic Player Controller supports the following features:
- State management (playing, paused, stopped)
- Song metadata updates
- Position tracking
- Shuffle and loop mode control
- API event processing
- Command execution

## Configuration

Add a generic player to your AudioControl configuration:

```json
{
  "players": [
    {
      "generic": {
        "type": "generic",
        "enable": true,
        "name": "my_player",
        "display_name": "My Media Player",
        "supports_api_events": true,
        "capabilities": ["play", "pause", "stop", "next", "previous", "seek", "shuffle", "loop"],
        "initial_state": "stopped",
        "shuffle": false,
        "loop_mode": "none"
      }
    }
  ]
}
```

### Configuration Options

- `name`: Unique identifier for the player
- `display_name`: Human-readable name (optional)
- `supports_api_events`: Set to `true` to enable API event processing
- `capabilities`: Array of supported player capabilities
- `initial_state`: Initial playback state (`"playing"`, `"paused"`, `"stopped"`)
- `shuffle`: Initial shuffle state (`true` or `false`)
- `loop_mode`: Initial loop mode (`"none"`, `"track"`, `"playlist"`)

## API Endpoints

### Player Update Endpoint

Send updates to control the generic player state.

**Endpoint:** `POST /api/player/{player_name}/update`

**Content-Type:** `application/json`

### Supported Event Types

#### 1. State Change Events

Update the player's playback state.

**Event Structure:**
```json
{
  "type": "state_changed",
  "state": "playing|paused|stopped"
}
```

#### 2. Song Change Events

Update the currently playing song metadata.

**Event Structure:**
```json
{
  "type": "song_changed",
  "song": {
    "title": "Song Title",
    "artist": "Artist Name",
    "album": "Album Name",
    "duration": 240.5,
    "uri": "file:///path/to/song.mp3"
  }
}
```

#### 3. Position Change Events

Update the current playback position.

**Event Structure:**
```json
{
  "type": "position_changed",
  "position": 45.2
}
```

#### 4. Loop Mode Change Events

Update the loop/repeat mode.

**Event Structure:**
```json
{
  "type": "loop_mode_changed",
  "loop_mode": "none|track|playlist"
}
```

#### 5. Shuffle Change Events

Update the shuffle state.

**Event Structure:**
```json
{
  "type": "shuffle_changed",
  "shuffle": true
}
```

## Example Usage

### Linux (using curl)

#### Start Playing
```bash
curl -X POST http://localhost:1080/api/player/my_player/update \
  -H "Content-Type: application/json" \
  -d '{
    "type": "state_changed",
    "state": "playing"
  }'
```

#### Update Song Metadata
```bash
curl -X POST http://localhost:1080/api/player/my_player/update \
  -H "Content-Type: application/json" \
  -d '{
    "type": "song_changed",
    "song": {
      "title": "Bohemian Rhapsody",
      "artist": "Queen",
      "album": "A Night at the Opera",
      "duration": 355.0,
      "uri": "spotify:track:4uLU6hMCjMI75M1A2tKUQC"
    }
  }'
```

#### Update Position
```bash
curl -X POST http://localhost:1080/api/player/my_player/update \
  -H "Content-Type: application/json" \
  -d '{
    "type": "position_changed",
    "position": 120.5
  }'
```

#### Enable Shuffle
```bash
curl -X POST http://localhost:1080/api/player/my_player/update \
  -H "Content-Type: application/json" \
  -d '{
    "type": "shuffle_changed",
    "shuffle": true
  }'
```

#### Set Loop Mode
```bash
curl -X POST http://localhost:1080/api/player/my_player/update \
  -H "Content-Type: application/json" \
  -d '{
    "type": "loop_mode_changed",
    "loop_mode": "track"
  }'
```

### PowerShell (using Invoke-RestMethod)

#### Start Playing
```powershell
$body = @{
    type = "state_changed"
    state = "playing"
} | ConvertTo-Json

Invoke-RestMethod -Uri "http://localhost:1080/api/player/my_player/update" `
                  -Method POST `
                  -ContentType "application/json" `
                  -Body $body
```

#### Update Song Metadata
```powershell
$body = @{
    type = "song_changed"
    song = @{
        title = "Bohemian Rhapsody"
        artist = "Queen"
        album = "A Night at the Opera"
        duration = 355.0
        uri = "spotify:track:4uLU6hMCjMI75M1A2tKUQC"
    }
} | ConvertTo-Json -Depth 3

Invoke-RestMethod -Uri "http://localhost:1080/api/player/my_player/update" `
                  -Method POST `
                  -ContentType "application/json" `
                  -Body $body
```

#### Update Position
```powershell
$body = @{
    type = "position_changed"
    position = 120.5
} | ConvertTo-Json

Invoke-RestMethod -Uri "http://localhost:1080/api/player/my_player/update" `
                  -Method POST `
                  -ContentType "application/json" `
                  -Body $body
```

#### Enable Shuffle
```powershell
$body = @{
    type = "shuffle_changed"
    shuffle = $true
} | ConvertTo-Json

Invoke-RestMethod -Uri "http://localhost:1080/api/player/my_player/update" `
                  -Method POST `
                  -ContentType "application/json" `
                  -Body $body
```

#### Set Loop Mode
```powershell
$body = @{
    type = "loop_mode_changed"
    loop_mode = "track"
} | ConvertTo-Json

Invoke-RestMethod -Uri "http://localhost:1080/api/player/my_player/update" `
                  -Method POST `
                  -ContentType "application/json" `
                  -Body $body
```

## Response Format

All API calls return a JSON response indicating success or failure:

### Success Response
```json
{
  "success": true,
  "message": "Update sent successfully to player: my_player"
}
```

### Error Response
```json
{
  "success": false,
  "message": "Player 'my_player' not found"
}
```

## Complete Example Script

### Linux Bash Script
```bash
#!/bin/bash

PLAYER_NAME="my_player"
BASE_URL="http://localhost:1080/api/player/${PLAYER_NAME}/update"

# Function to send API request
send_update() {
    local data="$1"
    echo "Sending: $data"
    curl -X POST "$BASE_URL" \
         -H "Content-Type: application/json" \
         -d "$data"
    echo -e "\n"
}

# Start playback with song
send_update '{
  "type": "song_changed",
  "song": {
    "title": "Example Song",
    "artist": "Example Artist",
    "album": "Example Album",
    "duration": 180.0
  }
}'

send_update '{
  "type": "state_changed",
  "state": "playing"
}'

# Wait and update position
sleep 2
send_update '{
  "type": "position_changed",
  "position": 2.0
}'

# Enable shuffle
send_update '{
  "type": "shuffle_changed",
  "shuffle": true
}'

echo "Demo completed!"
```

### PowerShell Script
```powershell
$PlayerName = "my_player"
$BaseUrl = "http://localhost:1080/api/player/$PlayerName/update"

# Function to send API request
function Send-Update {
    param([hashtable]$Data)
    
    $json = $Data | ConvertTo-Json -Depth 3
    Write-Host "Sending: $json"
    
    try {
        $response = Invoke-RestMethod -Uri $BaseUrl `
                                    -Method POST `
                                    -ContentType "application/json" `
                                    -Body $json
        Write-Host "Response: $($response | ConvertTo-Json)"
    }
    catch {
        Write-Error "Failed to send update: $_"
    }
    Write-Host ""
}

# Start playback with song
Send-Update @{
    type = "song_changed"
    song = @{
        title = "Example Song"
        artist = "Example Artist"
        album = "Example Album"
        duration = 180.0
    }
}

Send-Update @{
    type = "state_changed"
    state = "playing"
}

# Wait and update position
Start-Sleep -Seconds 2
Send-Update @{
    type = "position_changed"
    position = 2.0
}

# Enable shuffle
Send-Update @{
    type = "shuffle_changed"
    shuffle = $true
}

Write-Host "Demo completed!"
```

## Integration Notes

1. **Player Name**: Use the exact player name configured in your AudioControl configuration
2. **Port**: Default port is 1080, adjust if your configuration uses a different port
3. **Error Handling**: Always check the response for success/failure status
4. **Event Order**: Events are processed immediately, but some may depend on previous state
5. **Validation**: The API validates event structure and will return errors for malformed requests

## Troubleshooting

### Common Issues

1. **Player Not Found**: Ensure the player name in the URL matches the configuration
2. **Events Not Processing**: Verify that `supports_api_events` is set to `true` in the configuration
3. **Invalid JSON**: Ensure proper JSON formatting in request bodies
4. **Connection Failed**: Check that AudioControl server is running and accessible

### Debug Information

Check the AudioControl logs for detailed information about event processing:
```bash
# View logs (adjust path as needed)
tail -f /var/log/audiocontrol.log
```

The generic player logs debug information for all received events and state changes.
