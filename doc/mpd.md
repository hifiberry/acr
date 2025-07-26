# MPD Backend

## Configuration

### Basic Configuration

The MPD backend can be configured in the Audiocontrol configuration file. Here is a minimal configuration example:

```json
{
  "players": [
    {
      "type": "mpd",
      "name": "mpd",
      "host": "localhost",
      "port": 6600,
      "enable_library": true
    }
  ]
}
```

### Advanced Configuration Options

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| `type` | string | (required) | Must be set to `"mpd"` |
| `name` | string | (required) | User-defined name for the player |
| `host` | string | `"localhost"` | MPD server hostname or IP address |
| `port` | number | `6600` | MPD server port |
| `enable_library` | boolean | `true` | Whether to load and maintain the MPD library |
| `password` | string | `null` | Optional password for MPD authentication |
| `update_interval` | number | `3` | Polling interval in seconds for status updates |
| `metadata_sources` | array | `["musicbrainz", "theartistdb"]` | Sources for metadata enrichment |

## Features

### Playback Control

Audiocontrol supports the following MPD playback controls:

- Play, pause, stop
- Next/previous track
- Volume control
- Shuffle and repeat modes
- Playlist management

### Real-Time Updates

The MPD backend monitors the MPD server for changes and provides real-time updates:

- Current playback status (playing, paused, stopped)
- Position in the current track
- Library updates when MPD's database changes
- Playlist modifications

## Library Management

### Library Features

The MPD backend provides full access to the MPD library, allowing you to:

- Browse artists, albums, and tracks
- Filter by artist, genre, or other metadata
- Automatically enrich with external metadata like artist images and biographies
- Handle multi-artist entries correctly

### Loading Process

#### Startup
When Audiocontrol starts, no library is loaded initially. The system will show that the MPD player exists but its library is not loaded yet.

**Example API Response** (http://127.0.0.1:1080/library/):
```json
{
  "players": [
    {
      "player_name": "mpd",
      "player_id": "localhost:6600",
      "has_library": true,
      "is_loaded": false
    }
  ]
}
```

#### Initial Loading

Audiocontrol then retrieves all artists, albums, and tracks from MPD. A key challenge is that MPD lists multiple artists as a single comma-separated string. Simple string splitting would fail with artist names like "Crosby, Stills & Nash". Therefore, when available, Audiocontrol uses the MusicBrainz database to properly identify artists.

> **Note:** The initial loading process can be slow. However, as results are cached locally, subsequent startups will be significantly faster.

During this phase, the MPD backend sends database update notifications with progress information.

When the entire database has been loaded and processed, it sends a final database update notification with 100% progress, indicating that the database has been successfully loaded:

```json
{
  "players": [
    {
      "player_name": "mpd",
      "player_id": "localhost:6600",
      "has_library": true,
      "is_loaded": true
    }
  ]
}
```

#### Metadata Enhancement

Audiocontrol uses various services to retrieve additional metadata, including artist images. Without this metadata enhancement, artists will appear in the API without thumbnail images:

```json
[
  {
    "name": "16 Horsepower",
    "id": "4800476484544871526",
    "is_multi": false,
    "album_count": 7,
    "thumb_url": []
  },
  {
    "name": "2 Chainz",
    "id": "15793527172476567953",
    "is_multi": false,
    "album_count": 1,
    "thumb_url": []
  }
]
```

After the metadata update completes (which may also be slow initially but uses caching for future lookups), artists will have thumbnail images where available:

```json
[
  {
    "name": "16 Horsepower",
    "id": "4800476484544871526",
    "is_multi": false,
    "album_count": 7,
    "thumb_url": [
      "https://r2.theaudiodb.com/images/media/artist/thumb/vtxsxr1358638421.jpg"
    ]
  },
  {
    "name": "2 Chainz",
    "id": "15793527172476567953",
    "is_multi": false,
    "album_count": 1,
    "thumb_url": [
      "https://r2.theaudiodb.com/images/media/artist/thumb/2-chainz-4ff3c2f2aba7b.jpg"
    ]
  }
]
```

## Troubleshooting

### Common Issues

#### Library Not Loading

If the library does not load properly:

1. Ensure MPD is running and accessible at the configured host/port
2. Check if the MPD database is properly built
3. Verify that `enable_library` is set to `true` in the configuration

#### Missing Metadata

If artists or albums are missing images or metadata:

1. Check your internet connection (required for metadata lookup)
2. The metadata services might be temporarily unavailable

#### Slow Initial Loading

The initial loading can be slow due to:

1. Large music library size
2. MusicBrainz lookups for artist identification
3. Metadata enrichment from external services

Subsequent loads will be faster as Audiocontrol caches the results.

### Logging

To get more information about MPD backend issues, increase the log level in your Audiocontrol configuration:

```json
{
  "logging": {
    "level": "debug",
    "modules": {
      "players::mpd": "debug"
    }
  }
}
```