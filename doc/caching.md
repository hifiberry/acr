# ACR caching

ACR uses external databases for many of its functionalities. As these can be time-consuming to query, it tries to cache lookups internally to improve performance.

## Cache Types

ACR implements two types of caches:

1. **Attribute Cache**: Stores key-value pairs like metadata and IDs from external services
2. **Image Cache**: Stores image files like album covers and artist images

By default, entries in the cache have no expiry date, though the attribute cache can be configured with a maximum age.

## Cache Locations

By default, the cache directories are:
- Attribute cache: `cache/attributes`
- Image cache: `cache/images`

These paths can be customized in the configuration file.

## Display cache contents

ACR uses the Sled database engine to implement its attribute caching. To view the contents of the attribute cache, a tool is provided:

```
dumpcache [PATH]
```

The argument is the full path to the cache directory. If no path is specified, it defaults to `cache/attributes`.

Example:
```
dumpcache cache/attributes
```

This will output all key-value pairs in the cache in a `key|value` format.

## Managing the cache

### Deleting the cache

You can simply delete the cache directory to clear all cached data. The directory will be recreated automatically when needed. 

Note that deleting the cache can significantly slow down operation, particularly during startup, as ACR will need to rebuild the cache by querying external services again. You should only delete the cache if there are incorrect or outdated entries.

## Internal cache IDs

The attribute cache uses specific key formats for various types of data:

| Key | Value |
|-----|-------|
| `artist::mbid::<artist>` | Musicbrainz ID(s) for this artist or this list of artists |
| `artist::mbid_partial::<artistlist>` | Musicbrainz IDs could not be found for all artists in the list |
| `artist::fanart::<mbid>` | URLs to artist images from FanartTV |
| `artist::tadb::<mbid>` | Artist data from TheArtistDB |
| `album::mbid::<album>::<artist>` | Musicbrainz ID for this album |

## Implementation Details

### Attribute Cache

The attribute cache is implemented using the [Sled](https://github.com/spacejam/sled) embedded database with the following features:

- **Two-tier Caching**: Uses both an in-memory cache for fast access and a persistent database for durability
- **JSON Serialization**: All values are serialized to JSON before storage
- **Thread Safety**: The global cache instance is protected by a mutex for thread-safe access
- **Configurable Max Age**: Can be configured to automatically expire entries after a specified number of days

### Image Cache

The image cache is a simple file-based cache that:

- Stores images as files in the configured directory
- Creates subdirectories as needed based on the path structure
- Uses the filesystem's native caching to optimize read performance

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
| `attribute_cache_path` | `"cache/attributes"` | Path to the attribute cache directory |
| `image_cache_path` | `"cache/images"` | Path to the image cache directory |
| `max_age_days` | `30` | Maximum age of cached items in days (0 = no expiration) |
| `enabled` | `true` | Whether caching is enabled |

