# Audiocontrol Caching

Audiocontrol uses caching extensively to improve performance when accessing external services and databases. As these can be time-consuming to query, it caches lookups internally to improve performance.

## Cache Types

Audiocontrol implements two types of caches:

1. **Attribute Cache**: Stores key-value pairs like metadata and IDs from external services
2. **Image Cache**: Stores image files like album covers and artist images

By default, entries in the cache have no expiry date, though the attribute cache can be configured with a maximum age. Critical negative cache entries (like MusicBrainz lookup failures) use extended timeouts to prevent excessive API requests.

## Cache Locations

By default, the cache directories are:
- Attribute cache: `/var/lib/audiocontrol/cache/attributes`
- Image cache: `/var/lib/audiocontrol/cache/images`

These paths can be customized in the configuration file.

## Cache Management Tools

### audiocontrol_dump_cache Tool

Audiocontrol includes a dedicated cache management tool for inspecting and managing cache contents:

```bash
# List all cache entries with details
audiocontrol_dump_cache list --detailed

# List entries with a specific prefix
audiocontrol_dump_cache list --prefix "artist::mbid"

# Use shortcuts for common cache types
audiocontrol_dump_cache list --artistmbid          # MusicBrainz artist data
audiocontrol_dump_cache list --artistnotfound      # MusicBrainz negative cache
audiocontrol_dump_cache list --artistsplit         # Artist name splitting cache
audiocontrol_dump_cache list --imagemeta           # Image metadata cache

# Show cache statistics
audiocontrol_dump_cache stats --by-prefix

# Clean specific cache entries (dry run first)
audiocontrol_dump_cache clean --prefix "artist::mbid" --dry-run
audiocontrol_dump_cache clean --prefix "artist::mbid"

# Clean old entries
audiocontrol_dump_cache clean --older-than-days 7

# Clean all cache entries (use with caution!)
audiocontrol_dump_cache clean --all
```

### SQLite Direct Access

You can also use standard SQLite tools to inspect the cache:

```bash
# View all cached entries
sqlite3 /var/lib/audiocontrol/cache/attributes/attributes.db "SELECT key, value FROM cache;"

# View cache schema
sqlite3 /var/lib/audiocontrol/cache/attributes/attributes.db ".schema"

# Count total entries
sqlite3 /var/lib/audiocontrol/cache/attributes/attributes.db "SELECT COUNT(*) FROM cache;"

# Search for specific entries
sqlite3 /var/lib/audiocontrol/cache/attributes/attributes.db "SELECT * FROM cache WHERE key LIKE '%artist::mbid%';"
```

## Managing the Cache

### Using the audiocontrol_dump_cache Tool

The recommended way to manage the cache is using the built-in tool:

```bash
# Inspect cache contents before cleaning
audiocontrol_dump_cache list --artistnotfound --detailed

# Clean only expired negative cache entries (safe)
audiocontrol_dump_cache clean --artistnotfound --dry-run
audiocontrol_dump_cache clean --artistnotfound

# Clean old entries across all cache types
audiocontrol_dump_cache clean --older-than-days 30
```

### Manual Cache Deletion

You can also manually delete the cache directory to clear all cached data:

```bash
# Stop audiocontrol service first
sudo systemctl stop audiocontrol

# Remove cache directory
sudo rm -rf /var/lib/audiocontrol/cache

# Restart audiocontrol (cache will be recreated)
sudo systemctl start audiocontrol
```

**Warning**: Deleting the cache can significantly slow down operation, particularly during startup, as Audiocontrol will need to rebuild the cache by querying external services again. The audiocontrol_dump_cache tool provides more granular control and is the preferred method.

## Cache Key Prefixes

The attribute cache uses specific key formats for various types of data. All cache key prefixes are defined as constants in the code for maintainability:

| Key Pattern | Description | Timeout | Module |
|-------------|-------------|---------|---------|
| `artist::mbid::<artist>` | MusicBrainz ID(s) for artist or artist list | Permanent | musicbrainz |
| `artist::mbid_partial::<artistlist>` | Partial MusicBrainz matches (not all artists found) | Permanent | musicbrainz |
| `artist::mbid_not_found::<artist>` | MusicBrainz negative cache (artist not found) | 48 hours | musicbrainz |
| `artist::split::<artist>` | Artist name splitting results | Permanent | artistsplitter |
| `artist::simple_split::<artist>` | Simple artist splitting results | Permanent | artistsplitter |
| `image_meta::<url>` | Image metadata (dimensions, format, size) | Permanent | image_meta |
| `artist::fanart::<mbid>` | URLs to artist images from FanartTV | Permanent | fanarttv |
| `artist::metadata::<artist>` | Full artist metadata from multiple sources | Permanent | metadata |
| `album::mbid::<album>::<artist>` | MusicBrainz ID for album | Permanent | musicbrainz |
| `theaudiodb::mbid::<mbid>` | Artist data from TheAudioDB API | Permanent | theaudiodb |
| `theaudiodb::not_found::<mbid>` | TheAudioDB negative cache | Permanent | theaudiodb |
| `theaudiodb::no_thumbnail::<mbid>` | No thumbnail available in TheAudioDB | Permanent | theaudiodb |

### Extended Timeout Strategy

Critical services like MusicBrainz use extended negative caching (48 hours) to prevent excessive API requests for non-existent data. This significantly reduces load on external services while maintaining good user experience.

## Implementation Details

### Unified Attribute Cache Architecture

The attribute cache is implemented using a unified architecture with the following features:

- **Single SQLite Database**: All cache types use the same SQLite database for consistency
- **Two-tier Caching**: Uses both an in-memory cache for fast access and a persistent SQLite database for durability
- **JSON Serialization**: All values are serialized to JSON before storage
- **Thread Safety**: The global cache instance is protected by a mutex for thread-safe access
- **Configurable Expiry**: Supports per-entry expiry times with automatic cleanup
- **Cache Key Constants**: All cache key prefixes are defined as constants for maintainability and consistency

### Performance Optimizations

Recent performance improvements include:

- **MPD Library Optimization**: Removed redundant metadata updates during API access
- **Extended Negative Caching**: 48-hour timeout for MusicBrainz failures reduces unnecessary API calls
- **Unified Cache System**: All services use the same attributecache infrastructure
- **Constant-based Keys**: Cache key prefixes defined as constants prevent typos and facilitate maintenance

### MusicBrainz Caching Strategy

The MusicBrainz integration implements sophisticated caching:

- **Positive Result Caching**: Successful MBID lookups cached permanently
- **Extended Negative Caching**: Failed lookups cached for 48 hours to prevent excessive API requests
- **Partial Match Support**: Handles cases where only some artists in a multi-artist name are found
- **Name Matching**: Uses fuzzy matching and alias support for better accuracy

### Image Cache

The image cache is a simple file-based cache that:

- Stores images as files in the configured directory
- Creates subdirectories as needed based on the path structure
- Uses the filesystem's native caching to optimize read performance

### Image Metadata Cache

Image metadata (dimensions, format, file size) is cached using the unified attribute cache:

- **Cache Key Format**: `image_meta::<url>` where URL can be local file path or remote URL
- **Cached Data**: Width, height, size in bytes, and image format (JPEG, PNG, GIF, WebP)
- **Performance Benefit**: Avoids re-analyzing image files for metadata
- **Local and Remote Support**: Works with both local files and remote URLs

### Legacy Service Caching

#### TheArtistDB Caching

The caching for TheArtistDB API implements these specific strategies:

- **Positive Result Caching**: Artist data retrieved from TheArtistDB is stored with the key `theaudiodb::mbid::<mbid>` to avoid redundant API calls
- **Negative Result Caching**: When an artist is not found in TheArtistDB, this fact is cached with the key `theaudiodb::not_found::<mbid>` to avoid attempting to look up the same non-existent artist repeatedly
- **No Thumbnail Caching**: When an artist exists in TheArtistDB but has no thumbnail, this is cached with the key `theaudiodb::no_thumbnail::<mbid>` to avoid redundant processing
- **Cache-First Approach**: Each API function first checks the cache before making any network requests

## Configuration

In the main configuration file, you can customize the cache behavior:

```json
{
  "cache": {
    "attribute_cache_path": "custom/path/to/attributes",
    "image_cache_path": "custom/path/to/images", 
    "max_age_days": 30,
    "enabled": true
  },
  "musicbrainz": {
    "enable": true,
    "rate_limit_ms": 500
  }
}
```

Available configuration options:

| Option | Default | Description |
|--------|---------|-------------|
| `attribute_cache_path` | `"/var/lib/audiocontrol/cache/attributes"` | Path to the attribute cache directory |
| `image_cache_path` | `"/var/lib/audiocontrol/cache/images"` | Path to the image cache directory |
| `max_age_days` | `30` | Maximum age of cached items in days (0 = no expiration) |
| `enabled` | `true` | Whether caching is enabled |

## Recent Improvements

### Cache Architecture Unification (2025)

Recent updates have significantly improved the caching system:

- **Unified Constants**: All cache key prefixes are now defined as constants in their respective modules
- **Extended Negative Caching**: MusicBrainz failures are cached for 48 hours instead of indefinitely
- **Performance Optimization**: MPD library no longer performs redundant metadata updates during API access
- **Consistent Key Format**: All cache keys now use "::" as the separator for consistency
- **Enhanced Tooling**: The audiocontrol_dump_cache tool provides comprehensive cache management capabilities

### Performance Benefits

These improvements provide:

- **Reduced API Calls**: Extended negative caching prevents excessive requests to external services
- **Faster API Response**: MPD endpoints no longer trigger expensive metadata updates
- **Better Maintainability**: Cache key constants prevent typos and facilitate code maintenance
- **Improved Debugging**: Enhanced tools make cache inspection and management easier

