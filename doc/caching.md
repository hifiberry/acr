# Audiocontrol Caching

Audiocontrol uses caching extensively to improve performance when accessing external services and databases. As these can be time-consuming to query, it caches lookups internally to improve performance.

## Cache Types

Audiocontrol implements two types of caches:

1. **Attribute Cache**: Stores key-value pairs like metadata and IDs from external services
2. **Image Cache**: Stores image files like album covers and artist images

By default, entries in the cache have no expiry date, though the attribute cache can be configured with a maximum age.

## Cache Locations

By default, the cache directories are:
- Attribute cache: `/var/lib/audiocontrol/cache/attributes`
- Image cache: `/var/lib/audiocontrol/cache/images`

These paths can be customized in the configuration file.

## Display cache contents

Audiocontrol uses SQLite database engine to implement its attribute caching. To view the contents of the attribute cache, you can use standard SQLite tools:

```bash
# View all cached entries
sqlite3 /var/lib/audiocontrol/cache/attributes/attributes.db "SELECT key, value FROM cache;"

# View cache schema
sqlite3 /var/lib/audiocontrol/cache/attributes/attributes.db ".schema"

# Count total entries
sqlite3 /var/lib/audiocontrol/cache/attributes/attributes.db "SELECT COUNT(*) FROM cache;"
```

Alternatively, you can use any SQLite browser or viewer tool to inspect the database.

## Cache Management Tools

Audiocontrol provides tools to manage and inspect the cache:

```
acr_dumpcache [PATH]
```

The argument is the full path to the cache directory. If no path is specified, it defaults to `/var/lib/audiocontrol/cache/attributes`.

Example:
```
audiocontrol_dump_cache /var/lib/audiocontrol/cache/attributes
```

This will output all key-value pairs in the cache in a `key|value` format.

## Managing the cache

### Deleting the cache

You can simply delete the cache directory to clear all cached data. The directory will be recreated automatically when needed. 

Note that deleting the cache can significantly slow down operation, particularly during startup, as Audiocontrol will need to rebuild the cache by querying external services again. You should only delete the cache if there are incorrect or outdated entries.

## Internal cache IDs

The attribute cache uses specific key formats for various types of data:

| Key | Value |
|-----|-------|
| `artist::mbid::<artist>` | Musicbrainz ID(s) for this artist or this list of artists |
| `artist::mbid_partial::<artistlist>` | Musicbrainz IDs could not be found for all artists in the list |
| `artist::fanart::<mbid>` | URLs to artist images from FanartTV |
| `artist::metadata::<artist>` | Full artist metadata collected from multiple 3rd party sources |
| `album::mbid::<album>::<artist>` | Musicbrainz ID for this album |
| `theartistdb::mbid::<mbid>` | Artist data retrieved from TheArtistDB API |
| `theartistdb::not_found::<mbid>` | Records that an artist was not found in TheArtistDB (negative cache) |
| `theartistdb::no_thumbnail::<mbid>` | Records that an artist has no thumbnail in TheArtistDB (negative cache) |

## Implementation Details

### Attribute Cache

The attribute cache is implemented using the [SQLite](https://www.sqlite.org/) database with the following features:

- **Two-tier Caching**: Uses both an in-memory cache for fast access and a persistent SQLite database for durability
- **JSON Serialization**: All values are serialized to JSON before storage
- **Thread Safety**: The global cache instance is protected by a mutex for thread-safe access
- **Configurable Max Age**: Can be configured to automatically expire entries after a specified number of days
- **SQL Interface**: Standard SQL database allows for easy inspection and debugging with SQLite tools

### TheArtistDB Caching Implementation

The TheArtistDB integration uses the attribute cache to improve performance:

- **`lookup_artistdb_by_mbid`**: Checks for cached artist data before making API calls and stores both successful and failed results
- **`download_artist_thumbnail`**: Checks whether an artist has been previously identified as having no thumbnail before attempting a download
- **`update_artist`**: Uses cached data to avoid redundant processing when a previous attempt found no thumbnail

This multi-level caching approach significantly reduces API calls, particularly for artists that don't have thumbnails available in TheArtistDB.

### Image Cache

The image cache is a simple file-based cache that:

- Stores images as files in the configured directory
- Creates subdirectories as needed based on the path structure
- Uses the filesystem's native caching to optimize read performance

### TheArtistDB Caching

The caching for TheArtistDB API implements these specific strategies:

- **Positive Result Caching**: Artist data retrieved from TheArtistDB is stored with the key `theartistdb::mbid::<mbid>` to avoid redundant API calls
- **Negative Result Caching**: When an artist is not found in TheArtistDB, this fact is cached with the key `theartistdb::not_found::<mbid>` to avoid attempting to look up the same non-existent artist repeatedly
- **No Thumbnail Caching**: When an artist exists in TheArtistDB but has no thumbnail, this is cached with the key `theartistdb::no_thumbnail::<mbid>` to avoid redundant processing
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

