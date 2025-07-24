# Lyrics API

The Lyrics API provides endpoints to retrieve song lyrics for supported music players. Currently, only MPD-based players are supported. The API is designed with provider-specific endpoints to allow for future expansion to other music sources.

## Overview

The lyrics system looks for `.lrc` files alongside music files in the MPD music directory. These files should have the same name as the music file but with a `.lrc` extension.

## Supported Providers

- **MPD**: Music Player Daemon with local file system access

## Supported Formats

- **Plain Text**: Simple text lyrics without timing information
- **Timed Lyrics (LRC)**: Synchronized lyrics with timestamps in `[mm:ss.cc]` format

## Endpoints

### Get Lyrics by Song ID

Retrieve lyrics for a specific song using its provider-specific song ID.

**Endpoint:** `GET /api/lyrics/{provider}/{song_id}`

**Parameters:**
- `provider`: The lyrics provider (currently only "mpd" is supported)
- `song_id`: The provider-specific song ID. For MPD: base64-encoded file path of the song

**Example Request:**
```bash
curl -X GET "http://localhost:1080/api/lyrics/mpd/bXVzaWMvRXhhbXBsZSBBcnRpc3QvRXhhbXBsZSBBbGJ1bS9FeGFtcGxlIFNvbmcuZmxhYw"
```

**Note**: The song ID is automatically provided in the song metadata's `lyrics_url` field when lyrics are available.

**Example Response (Timed Lyrics):**
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

**Example Response (Plain Text):**
```json
{
  "found": true,
  "lyrics": {
    "type": "plain",
    "text": "Complete song lyrics as plain text"
  }
}
```

**Example Response (Not Found):**
```json
{
  "found": false,
  "error": "Lyrics not found for this song"
}
```

### Get Lyrics by Metadata

Retrieve lyrics by providing song metadata (artist, title, etc.) for a specific provider.

**Endpoint:** `POST /api/lyrics/{provider}`

**Parameters:**
- `provider`: The lyrics provider (currently only "mpd" is supported)

**Request Body:**
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
    "title": "Example Song",
    "duration": 210.0,
    "album": "Example Album"
  }'
```

**Response Format:**
Same as the GET endpoint above.

## MPD Integration

When playing a song through MPD, the player metadata will include lyrics information if lyrics are available:

### Additional Metadata Fields

When lyrics are available for the current song, the following fields are added to the song metadata:

- `lyrics_available`: Boolean indicating if lyrics exist for this song
- `lyrics_url`: Direct API endpoint for lyrics by song ID (using base64-encoded file path)
- `lyrics_metadata`: Object containing the song metadata that can be used for POST requests to `/api/lyrics/mpd`

**Example Song Metadata with Lyrics:**
```json
{
  "title": "Example Song",
  "artist": "Example Artist", 
  "album": "Example Album",
  "duration": 210.5,
  "metadata": {
    "lyrics_available": true,
    "lyrics_url": "/api/lyrics/mpd/bXVzaWMvRXhhbXBsZSBBcnRpc3QvRXhhbXBsZSBBbGJ1bS9FeGFtcGxlIFNvbmcuZmxhYw",
    "lyrics_metadata": {
      "artist": "Example Artist",
      "title": "Example Song",
      "duration": 210.5,
      "album": "Example Album"
    }
  }
}
```

**Usage:**
- Use the `lyrics_url` for a direct GET request to retrieve lyrics for this specific song
- Use the `lyrics_metadata` object as the request body for a POST to `/api/lyrics/mpd` to find lyrics by metadata

**Note**: The song ID in the `lyrics_url` is a URL-safe base64-encoded version of the song's file path in the MPD music directory.

## File Structure

Lyrics files should be placed alongside music files with the `.lrc` extension:

```
/var/lib/mpd/music/
├── Artist/
│   └── Album/
│       ├── 01 - Song Title.mp3
│       ├── 01 - Song Title.lrc  # Lyrics file
│       ├── 02 - Another Song.flac
│       └── 02 - Another Song.lrc
```

## LRC Format

The LRC (Lyric) format uses timestamps in the format `[mm:ss.cc]` where:
- `mm`: Minutes (00-99)
- `ss`: Seconds (00-59)  
- `cc`: Centiseconds (00-99)

**Example LRC File:**
```
[00:12.50]Line 1 of lyrics
[00:17.20]Line 2 of lyrics
[00:21.10]Line 3 of lyrics
[01:06.00]Chorus starts here
```

## Error Responses

All endpoints return appropriate HTTP status codes:

- `200 OK`: Request successful (lyrics may or may not be found)
- `404 Not Found`: No MPD player with library support found
- `500 Internal Server Error`: Server error occurred

**Error Response Format:**
```json
{
  "found": false,
  "error": "Error message describing what went wrong"
}
```

## Notes

- The lyrics API currently only works with MPD players that have library support enabled
- Song ID lookup is simplified and may return "not found" for songs not in the current queue
- Metadata-based lookup is not yet fully implemented and may return "not found"
- The system is designed to be extensible for future lyrics providers (online services, etc.)
