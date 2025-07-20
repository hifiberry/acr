# Metadata Management in AudioControl

AudioControl implements a comprehensive metadata enrichment system that gathers information about artists, albums, and tracks from multiple external services. This document explains the metadata attributes, lookup mechanisms, and data sources used in the system.

## Overview

The metadata system is designed to enhance basic music information with rich metadata from various online services. It operates on a hierarchical approach, starting with basic identification and progressively enriching data through multiple service lookups.

## Metadata Structure

AudioControl handles metadata at multiple levels: core song/track metadata that comes directly from players and APIs, and enriched metadata that is gathered from external services.

### Core Song/Track Metadata

Core metadata is the basic information that comes directly from music players, streaming services, or API calls. This forms the foundation for all metadata operations:

| Attribute | Type | Description | Sources |
|-----------|------|-------------|---------|
| `title` | `String` | Track/song title | Player, MPRIS, API calls |
| `artist` | `String` | Primary artist name | Player, MPRIS, API calls |
| `album` | `Option<String>` | Album name | Player, MPRIS, API calls |
| `albumartist` | `Option<String>` | Album artist (may differ from track artist) | Player, MPRIS |
| `duration` | `Option<f64>` | Track duration in seconds | Player, MPRIS |
| `track_number` | `Option<u32>` | Track number within album | Player, MPRIS |
| `disc_number` | `Option<u32>` | Disc number for multi-disc albums | Player, MPRIS |
| `year` | `Option<u32>` | Release year | Player, MPRIS |
| `genre` | `Option<String>` | Track genre | Player, MPRIS |
| `coverart_url` | `Option<String>` | Album/track cover art URL | Player, MPRIS |
| `uri` | `Option<String>` | Unique resource identifier for the track | Player, MPRIS |

**Key Notes**:
- Core metadata is the **primary trigger** for metadata enrichment
- Artist names from core metadata are processed through name splitting logic
- Album information is used for album-level metadata lookup
- Cover art URLs may be enhanced with additional images from external services

### Enriched Artist Metadata (`ArtistMeta`)

The enriched artist metadata structure contains additional information gathered from external services:

| Attribute | Type | Description | Sources |
|-----------|------|-------------|---------|
| `mbid` | `Vec<String>` | MusicBrainz IDs for the artist | MusicBrainz |
| `genres` | `Vec<String>` | Musical genres associated with the artist | TheAudioDB, MusicBrainz, Last.fm |
| `biography` | `Option<String>` | Artist biography/description | TheAudioDB, Last.fm |
| `thumb_url` | `Vec<String>` | Thumbnail/avatar image URLs | TheAudioDB, FanArt.tv |
| `banner_url` | `Vec<String>` | Banner/header image URLs | FanArt.tv |
| `fanart_url` | `Vec<String>` | Fan art image URLs | FanArt.tv |

**Key Notes**:
- Enriched metadata is **triggered by** core artist names
- Multiple images may be collected from different services
- Genres from enriched metadata may supplement or override core genre information
- MBIDs serve as canonical identifiers for cross-service lookups

### Album Metadata

Album-level metadata can be enriched similarly to artist metadata:

| Attribute | Type | Description | Sources |
|-----------|------|-------------|---------|
| `album_name` | `String` | Album title | Core metadata |
| `album_artist` | `String` | Album artist (from core metadata) | Core metadata |
| `release_year` | `Option<u32>` | Album release year | Core metadata, external services |
| `album_art_url` | `Vec<String>` | Album cover art URLs | Core metadata, external services |
| `total_tracks` | `Option<u32>` | Total number of tracks in album | External services |
| `label` | `Option<String>` | Record label | External services |

## Data Sources and Services

### MusicBrainz

**Purpose**: Primary source for canonical music metadata and unique identifiers.

**Lookup Method**: Artist name matching
- Input: Artist name (string)
- Output: MusicBrainz ID (MBID), additional metadata
- Rate Limit: 1000ms (1 request per second)

**Key Attributes Provided**:
- Unique MusicBrainz IDs (MBIDs)
- Canonical artist names
- Genre information
- Relationships between artists

**Example Lookup**:
```rust
// Look up artist by name to get MBID
let mbid = musicbrainz::lookup_artist_by_name("The Beatles")?;
// mbid: "b10bbbfc-cf9e-42e0-be17-e2c3e1d2600d"
```

### TheAudioDB

**Purpose**: Rich multimedia metadata including images and biographies.

**Lookup Method**: MusicBrainz ID (MBID)
- Input: MusicBrainz ID
- Output: Artist thumbnails, biography, genre information
- Rate Limit: 500ms (2 requests per second)

**Key Attributes Provided**:
- Artist thumbnail images (`strArtistThumb`)
- English biography (`strBiographyEN`)
- Genre classification (`strGenre`)

**Example Lookup**:
```rust
// Look up by MBID obtained from MusicBrainz
let artist_data = theaudiodb::lookup_by_mbid("b10bbbfc-cf9e-42e0-be17-e2c3e1d2600d")?;
// Returns JSON with strArtistThumb, strBiographyEN, strGenre
```

### Last.fm

**Purpose**: Social metadata including tags, user-generated content, and additional images.

**Lookup Method**: Artist name
- Input: Artist name (string)
- Output: Tags, biography, images, listener statistics
- Rate Limit: 1000ms (1 request per second)

**Key Attributes Provided**:
- User-generated tags
- Artist biography
- Additional images (multiple sizes)
- Play counts and listener statistics

**Example Lookup**:
```rust
// Look up artist by name
let artist_info = lastfm::get_artist_info("The Beatles")?;
// Returns LastfmArtistDetails with tags, bio, images
```

### FanArt.tv

**Purpose**: High-quality fan art and promotional images.

**Lookup Method**: MusicBrainz ID (MBID)
- Input: MusicBrainz ID
- Output: Various image types (thumbnails, banners, fan art)
- Rate Limit: 500ms (2 requests per second)

**Key Attributes Provided**:
- Artist thumbnails
- HD banners
- Fan art images
- Logo images

**Example Lookup**:
```rust
// Look up images by MBID
let images = fanarttv::lookup_artist_images("b10bbbfc-cf9e-42e0-be17-e2c3e1d2600d")?;
// Returns various image types and URLs
```

## Artist Name Processing and Splitting

The metadata system includes sophisticated artist name processing to handle various formats and collaborative works.

### Artist Name Splitting Logic

The system recognizes multiple separator patterns to split artist names:

#### Supported Separators

| Separator | Description | Example |
|-----------|-------------|---------|
| ` feat. ` | Featuring | "Artist feat. Guest" |
| ` ft. ` | Featuring (abbreviated) | "Artist ft. Guest" |
| ` featuring ` | Featuring (full) | "Artist featuring Guest" |
| ` & ` | Collaboration | "Artist1 & Artist2" |
| ` and ` | Collaboration | "Artist1 and Artist2" |
| `, ` | List separator | "Artist1, Artist2, Artist3" |
| ` vs. ` | Versus/competition | "Artist1 vs. Artist2" |
| ` versus ` | Versus (full) | "Artist1 versus Artist2" |

#### Processing Rules

1. **Primary Artist Extraction**: The first part before any separator is considered the primary artist
2. **Featured Artist Handling**: Artists after "feat.", "ft.", or "featuring" are treated as featured artists
3. **Collaboration Detection**: Artists separated by "&", "and", or "vs." are treated as equal collaborators
4. **List Processing**: Comma-separated lists are processed to extract all participating artists

#### Examples

```rust
// Input: "Taylor Swift feat. Ed Sheeran"
// Primary: "Taylor Swift"
// Featured: ["Ed Sheeran"]

// Input: "Simon & Garfunkel"
// Collaborators: ["Simon", "Garfunkel"]

// Input: "The Beatles vs. The Rolling Stones"
// Versus: ["The Beatles", "The Rolling Stones"]

// Input: "Artist1, Artist2, Artist3"
// Multiple: ["Artist1", "Artist2", "Artist3"]
```

### Implementation

The artist splitting is implemented in the `helpers/sanitize.rs` module:

```rust
pub fn split_artist_name(artist_name: &str) -> Vec<String> {
    // Implementation handles various separator patterns
    // Returns vector of individual artist names
}
```

## Metadata Lookup Flow

The metadata enrichment process is triggered whenever new song information arrives from players or API calls. The flow progresses from core metadata processing to external service enrichment.

### 1. Core Metadata Processing

When new song information arrives (from MPRIS, player events, or API calls):

1. **Core Data Extraction**: Extract basic song information:
   - Artist name (primary trigger for enrichment)
   - Album name (for album-level enrichment)
   - Track title, duration, track numbers, etc.
   - Cover art URL (if provided by player)

2. **Artist Name Sanitization**: Clean and normalize the artist name from core metadata
3. **Artist Splitting**: Split compound artist names into individual artists using separator logic
4. **Deduplication**: Remove duplicate artist names from the split results

**Example Core Metadata**:
```json
{
  "title": "Hey Jude",
  "artist": "The Beatles feat. John Lennon",
  "album": "Hey Jude",
  "duration": 431.0,
  "year": 1968,
  "coverart_url": "https://player.example.com/covers/hey_jude.jpg"
}
```

### 2. Primary Lookup (MusicBrainz)

For each individual artist extracted from core metadata:

1. **Name-based Lookup**: Search MusicBrainz using the cleaned artist name
2. **MBID Assignment**: Obtain unique MusicBrainz ID for canonical identification
3. **Canonical Name Mapping**: Map player-provided name to canonical MusicBrainz name
4. **Basic Metadata**: Extract genre and relationship information
5. **Cache Storage**: Store both positive and negative results

**Flow Example**:
```rust
// From core metadata: "The Beatles feat. John Lennon"
// After splitting: ["The Beatles", "John Lennon"]
// 
// For "The Beatles":
let mbid = musicbrainz::lookup_artist_by_name("The Beatles")?;
// mbid: "b10bbbfc-cf9e-42e0-be17-e2c3e1d2600d"
// canonical_name: "The Beatles"
```

### 3. Secondary Lookups (External Services)

Using the MBIDs obtained from MusicBrainz:

1. **TheAudioDB Lookup**: 
   - Input: MBID from step 2
   - Output: Artist thumbnails, biography, genre information
   - Enhancement: Adds rich media and descriptive content

2. **FanArt.tv Lookup**: 
   - Input: MBID from step 2
   - Output: High-quality images (thumbnails, banners, fan art)
   - Enhancement: Adds professional artwork and promotional images

3. **Last.fm Lookup**: 
   - Input: Original artist name from core metadata
   - Output: Social tags, user-generated content, additional images
   - Enhancement: Adds community-driven metadata and statistics

**Parallel Processing**: Secondary lookups can be performed in parallel since they use different input methods (MBID vs. name)

### 4. Data Consolidation and Enhancement

1. **Image Collection**: 
   - Combine cover art from core metadata with images from external services
   - Prioritize high-quality sources (FanArt.tv > TheAudioDB > core metadata)
   - Store multiple image URLs for different use cases

2. **Genre Enhancement**: 
   - Start with genre from core metadata (if available)
   - Supplement with genres from external services
   - Use priority order: MusicBrainz > Last.fm > TheAudioDB > core metadata

3. **Biography and Descriptive Content**: 
   - Select best available biography (Last.fm > TheAudioDB)
   - Add social tags and listener statistics from Last.fm

4. **Metadata Merging**:
   - Preserve all core metadata
   - Add enriched metadata as additional fields
   - Maintain source attribution for debugging

**Final Enhanced Metadata Example**:
```json
{
  // Core metadata (preserved)
  "title": "Hey Jude",
  "artist": "The Beatles feat. John Lennon", 
  "album": "Hey Jude",
  "duration": 431.0,
  "year": 1968,
  "coverart_url": "https://player.example.com/covers/hey_jude.jpg",
  
  // Enhanced metadata (added)
  "artists": [
    {
      "name": "The Beatles",
      "mbid": ["b10bbbfc-cf9e-42e0-be17-e2c3e1d2600d"],
      "genres": ["Rock", "Pop Rock", "Psychedelic Rock"],
      "biography": "The Beatles were an English rock band...",
      "thumb_url": ["https://theaudiodb.com/images/media/artist/thumb/rvvnvv1347913617.jpg"],
      "banner_url": ["https://fanart.tv/fanart/music/b10bbbfc-cf9e-42e0-be17-e2c3e1d2600d/artistbackground/the-beatles-5018a594a0a4c.jpg"]
    }
  ]
}

## Caching Strategy

### Positive Caching

Successful lookups are cached with service-specific keys:

```rust
// MusicBrainz cache
"musicbrainz::artist::Artist Name" -> ArtistMeta

// TheAudioDB cache  
"theaudiodb::mbid::MBID" -> JSON response

// Last.fm cache
"lastfm::artist::Artist Name" -> LastfmArtistDetails
```

### Negative Caching

Failed lookups are cached to avoid repeated requests:

```rust
// Not found markers
"theaudiodb::not_found::MBID" -> true
"theaudiodb::no_thumbnail::MBID" -> true
"lastfm::not_found::Artist Name" -> true
```

### Cache Duration

- **Positive Results**: Long-term caching (days to weeks)
- **Negative Results**: Medium-term caching (hours to days)
- **Error Results**: Short-term caching (minutes to hours)

## Rate Limiting

Each service has configured rate limits to respect API constraints:

| Service | Default Rate Limit | Reasoning |
|---------|-------------------|-----------|
| MusicBrainz | 1000ms | Strictly enforced 1 req/sec limit |
| TheAudioDB | 500ms | Allows 2 req/sec for API users |
| Last.fm | 1000ms | Conservative limit within 5 req/sec allowance |
| FanArt.tv | 500ms | 2 req/sec for personal API keys |

## Configuration

### Core Metadata Sources

Configure how core metadata is handled from different player sources:

```json
{
  "players": {
    "mpris": {
      "enable": true,
      "metadata_mapping": {
        "prefer_albumartist": true,
        "fallback_to_artist": true,
        "normalize_genres": true
      }
    },
    "generic": {
      "enable": true,
      "supports_api_events": true,
      "metadata_validation": {
        "require_title": true,
        "require_artist": true,
        "validate_duration": true
      }
    }
  }
}
```

### External Service Configuration

Each metadata enrichment service can be configured independently:

```json
{
  "services": {
    "musicbrainz": {
      "enable": true,
      "rate_limit_ms": 1000,
      "user_agent": "AudioControl/1.0",
      "search_accuracy": "high"
    },
    "theaudiodb": {
      "enable": true,
      "api_key": "your_api_key",
      "rate_limit_ms": 500,
      "preferred_language": "en"
    },
    "lastfm": {
      "enable": true,
      "api_key": "your_api_key",
      "api_secret": "your_api_secret",
      "rate_limit_ms": 1000,
      "include_similar_artists": true
    },
    "fanarttv": {
      "enable": true,
      "api_key": "your_api_key",
      "rate_limit_ms": 500,
      "image_quality": "hd"
    }
  }
}
```

### Metadata Enhancement Preferences

Configure priority and behavior for metadata enhancement:

```json
{
  "metadata": {
    "enrichment": {
      "enable": true,
      "trigger_on_song_change": true,
      "trigger_on_artist_change": true,
      "background_processing": true
    },
    "image_priority": ["fanarttv", "theaudiodb", "core_metadata"],
    "biography_priority": ["lastfm", "theaudiodb"],
    "genre_priority": ["musicbrainz", "lastfm", "theaudiodb", "core_metadata"],
    "artist_splitting": {
      "enable": true,
      "separators": ["feat.", "ft.", "featuring", "&", "and", "vs.", "versus", ","],
      "preserve_original": true
    }
  }
}
```

## Error Handling and Fallbacks

### Lookup Failures

1. **Service Unavailable**: Graceful degradation, continue with other services
2. **Rate Limit Exceeded**: Automatic retry with exponential backoff
3. **Invalid Response**: Log error, mark as failed, continue processing
4. **Network Issues**: Temporary failure, retry after delay

### Data Quality

1. **Missing MBIDs**: Continue with name-based lookups where possible
2. **Conflicting Data**: Use configured priority order for resolution
3. **Invalid Images**: Validate image URLs before caching
4. **Empty Responses**: Mark as "not found" to avoid repeated requests

## Best Practices

### For Developers

1. **Always Check Cache First**: Implement proper cache checking before API calls
2. **Respect Rate Limits**: Use the centralized rate limiting system
3. **Handle Errors Gracefully**: Don't fail entire operations due to metadata failures
4. **Log Important Events**: Log successful updates and failures for monitoring

### For Administrators

1. **Configure API Keys**: Ensure all services have valid API credentials
2. **Monitor Rate Limits**: Watch for rate limit violations in logs
3. **Cache Management**: Ensure adequate storage for metadata caching
4. **Service Health**: Monitor external service availability

## Troubleshooting

### Common Issues

1. **Missing Core Metadata**
   - Check player MPRIS implementation
   - Verify API event format for generic players
   - Ensure required fields (title, artist) are provided
   - Check metadata validation settings

2. **Incorrect Artist Name Splitting**
   - Review separator configuration
   - Check for unusual artist name formats
   - Verify artist splitting is enabled
   - Test with `helpers::sanitize::split_artist_name()`

3. **Missing Artist Images**
   - Verify core metadata provides artist name
   - Check if artist has MusicBrainz ID
   - Verify TheAudioDB/FanArt.tv API keys
   - Check negative cache entries
   - Ensure image priority includes core metadata sources

4. **Incorrect Genre Information**
   - Check genre priority configuration
   - Verify core metadata genre field
   - Review individual service responses
   - Verify artist name matching accuracy

5. **Album Metadata Not Enhanced**
   - Ensure album name is provided in core metadata
   - Check albumartist vs. artist fields
   - Verify album-level lookup services are configured

6. **Slow Metadata Loading**
   - Review rate limit settings
   - Check network connectivity to services
   - Monitor cache hit rates
   - Consider disabling background processing for testing

### Debug Configuration

Enable debug logging for both core and enriched metadata operations:

```json
{
  "logging": {
    "level": "debug",
    "modules": {
      "helpers::artistupdater": "debug",
      "helpers::theaudiodb": "debug", 
      "helpers::lastfm": "debug",
      "helpers::musicbrainz": "debug",
      "helpers::sanitize": "debug",
      "data::metadata": "debug",
      "players::mpris": "debug",
      "players::generic": "debug"
    }
  }
}
```

## Future Enhancements

### Planned Features

1. **Spotify Integration**: Add Spotify as metadata source

### Data Extensions

1. **Album Metadata**: Extend system to handle album-level metadata
2. **Track-Level Data**: Enhanced track metadata from multiple sources
3. **Lyric Integration**: Support for lyric providers
4. **Release Information**: Detailed release and label information

## Command Line Tools for Metadata Inspection

AudioControl provides command line tools to inspect cached metadata, which is useful for debugging and understanding the current state of the metadata system.

### Dump Cache Tool (`audiocontrol_dump_cache`)

The primary tool for inspecting the metadata cache is `audiocontrol_dump_cache`, which displays all cached key-value pairs in a readable format.

#### Basic Usage

```bash
# Dump all cache contents (uses default path)
audiocontrol_dump_cache

# Dump cache from specific path
audiocontrol_dump_cache /custom/path/to/cache/attributes

# Show help
audiocontrol_dump_cache --help
```

#### Output Format

The tool outputs data in `key|value` format, where values are displayed as JSON when possible:

```
artist::metadata::The Beatles|{"name":"The Beatles","mbid":["b10bbbfc-cf9e-42e0-be17-e2c3e1d2600d"],"genres":["Rock","Pop Rock"],"biography":"The Beatles were an English rock band...","thumb_url":["https://example.com/image.jpg"]}
artist::mbid::Pink Floyd|["83d91898-7763-47d7-b03b-b92132375c47"]
theaudiodb::mbid::b10bbbfc-cf9e-42e0-be17-e2c3e1d2600d|{"artists":[{"strArtist":"The Beatles","strBiographyEN":"...","strArtistThumb":"..."}]}
theaudiodb::not_found::invalid-mbid-123|true
Total entries: 125
```

### Cache Key Patterns and Examples

Understanding the cache key patterns helps identify specific types of cached metadata:

#### Artist Metadata Keys

| Key Pattern | Description | Example |
|-------------|-------------|---------|
| `artist::metadata::<artist_name>` | Complete artist metadata | `artist::metadata::The Beatles` |
| `artist::mbid::<artist_name>` | MusicBrainz IDs for artist | `artist::mbid::Pink Floyd` |
| `artist::mbid_partial::<artist_list>` | Partial MBID results for artist list | `artist::mbid_partial::Artist1 feat. Artist2` |

#### Service-Specific Cache Keys

| Key Pattern | Description | Example |
|-------------|-------------|---------|
| `theaudiodb::mbid::<mbid>` | TheAudioDB data by MBID | `theaudiodb::mbid::b10bbbfc-cf9e-42e0-be17-e2c3e1d2600d` |
| `theaudiodb::not_found::<mbid>` | Artist not found in TheAudioDB | `theaudiodb::not_found::invalid-mbid-123` |
| `theaudiodb::no_thumbnail::<mbid>` | Artist has no thumbnail | `theaudiodb::no_thumbnail::b10bbbfc-cf9e-42e0-be17-e2c3e1d2600d` |
| `musicbrainz::<lookup_type>::<query>` | MusicBrainz lookup results | `musicbrainz::artist::The Beatles` |
| `lastfm::artist::<artist_name>` | Last.fm artist information | `lastfm::artist::The Beatles` |
| `fanarttv::artist::<mbid>` | FanArt.tv images by MBID | `fanarttv::artist::b10bbbfc-cf9e-42e0-be17-e2c3e1d2600d` |

#### Album Metadata Keys

| Key Pattern | Description | Example |
|-------------|-------------|---------|
| `album::mbid::<album>::<artist>` | MusicBrainz ID for album | `album::mbid::Abbey Road::The Beatles` |
| `album::metadata::<album>::<artist>` | Complete album metadata | `album::metadata::Dark Side of the Moon::Pink Floyd` |

### Practical Examples

#### 1. Check if Artist Metadata Exists

```bash
# Look for specific artist metadata
audiocontrol_dump_cache | grep "artist::metadata::The Beatles"

# Output example:
# artist::metadata::The Beatles|{"name":"The Beatles","mbid":["b10bbbfc-cf9e-42e0-be17-e2c3e1d2600d"],"genres":["Rock","Pop Rock","Psychedelic Rock"],"biography":"The Beatles were an English rock band formed in Liverpool in 1960...","thumb_url":["https://theaudiodb.com/images/media/artist/thumb/rvvnvv1347913617.jpg"],"banner_url":["https://fanart.tv/fanart/music/b10bbbfc-cf9e-42e0-be17-e2c3e1d2600d/artistbackground/the-beatles-5018a594a0a4c.jpg"],"fanart_url":["https://fanart.tv/fanart/music/b10bbbfc-cf9e-42e0-be17-e2c3e1d2600d/artistbackground/the-beatles-5018a594a0a4c.jpg"]}
```

#### 2. Find MusicBrainz IDs for Artists

```bash
# Search for MBID cache entries
audiocontrol_dump_cache | grep "artist::mbid::"

# Output example:
# artist::mbid::The Beatles|["b10bbbfc-cf9e-42e0-be17-e2c3e1d2600d"]
# artist::mbid::Pink Floyd|["83d91898-7763-47d7-b03b-b92132375c47"]
# artist::mbid::Led Zeppelin|["678d88b2-87b0-403b-b63d-5da7465aecc3"]
```

#### 3. Check Service-Specific Data

```bash
# Check TheAudioDB cache entries
audiocontrol_dump_cache | grep "theaudiodb::"

# Output example:
# theaudiodb::mbid::b10bbbfc-cf9e-42e0-be17-e2c3e1d2600d|{"artists":[{"strArtist":"The Beatles","strBiographyEN":"The Beatles were an English rock band...","strArtistThumb":"https://theaudiodb.com/images/media/artist/thumb/rvvnvv1347913617.jpg"}]}
# theaudiodb::not_found::00000000-0000-0000-0000-000000000000|true
# theaudiodb::no_thumbnail::invalid-mbid|true
```

#### 4. Find Negative Cache Entries (Failed Lookups)

```bash
# Find failed lookups that are cached to avoid retries
audiocontrol_dump_cache | grep "not_found\|no_thumbnail"

# Output example:
# theaudiodb::not_found::invalid-mbid-123|true
# theaudiodb::no_thumbnail::some-mbid-without-image|true
```

#### 5. Search for Specific Artist Across All Services

```bash
# Find all cache entries for a specific artist
audiocontrol_dump_cache | grep -i "beatles"

# Output example:
# artist::metadata::The Beatles|{...}
# artist::mbid::The Beatles|["b10bbbfc-cf9e-42e0-be17-e2c3e1d2600d"]
# theaudiodb::mbid::b10bbbfc-cf9e-42e0-be17-e2c3e1d2600d|{...}
# lastfm::artist::The Beatles|{...}
```

#### 6. Count Cache Entries by Type

```bash
# Count different types of cache entries
echo "Artist metadata entries:"
audiocontrol_dump_cache | grep -c "artist::metadata::"

echo "MusicBrainz ID entries:"
audiocontrol_dump_cache | grep -c "artist::mbid::"

echo "TheAudioDB entries:"
audiocontrol_dump_cache | grep -c "theaudiodb::"

echo "Failed lookup entries:"
audiocontrol_dump_cache | grep -c "not_found"
```

### Filtering and Analysis

#### Extract JSON Values for Analysis

```bash
# Extract and pretty-print JSON for specific artist
audiocontrol_dump_cache | grep "artist::metadata::The Beatles" | cut -d'|' -f2 | jq '.'

# Extract all artist names with metadata
audiocontrol_dump_cache | grep "artist::metadata::" | cut -d'|' -f1 | sed 's/artist::metadata:://'

# Extract all cached MBIDs
audiocontrol_dump_cache | grep "artist::mbid::" | cut -d'|' -f2 | jq -r '.[]'
```

#### Monitor Cache for Debugging

```bash
# Watch cache changes (if running during active metadata updates)
while true; do
    echo "=== $(date) ==="
    audiocontrol_dump_cache | wc -l
    echo "Total cache entries"
    sleep 5
done
```

### Cache Location and Permissions

The default cache location is `/var/lib/audiocontrol/cache/attributes`. Common issues:

```bash
# Check if cache directory exists and is accessible
ls -la /var/lib/audiocontrol/cache/

# Check cache directory permissions
stat /var/lib/audiocontrol/cache/attributes

# Check if running as correct user
whoami

# If permission issues, run with sudo (if appropriate)
sudo audiocontrol_dump_cache
```

### Performance Considerations

For large caches, consider filtering output:

```bash
# Count total entries without displaying all content
audiocontrol_dump_cache | wc -l

# Search for specific patterns only
audiocontrol_dump_cache | grep "artist::metadata::" | head -10

# Export cache for analysis
audiocontrol_dump_cache > /tmp/audiocontrol_cache_dump.txt
```

### Integration with Other Tools

The cache dump output can be integrated with other analysis tools:

```bash
# Convert to CSV for spreadsheet analysis
audiocontrol_dump_cache | sed 's/|/,/' > cache_data.csv

# Extract specific data for reporting
audiocontrol_dump_cache | grep "artist::metadata::" | while IFS='|' read -r key value; do
    artist=$(echo "$key" | sed 's/artist::metadata:://')
    genre=$(echo "$value" | jq -r '.genres[0] // "Unknown"')
    echo "$artist: $genre"
done
```

## Conclusion

The AudioControl metadata system provides comprehensive artist information by intelligently combining data from multiple authoritative sources. The system handles artist name complexity, respects service limitations through rate limiting, and provides robust caching for optimal performance.

The command line tools, particularly `audiocontrol_dump_cache`, provide powerful ways to inspect and debug the metadata cache, helping administrators and developers understand the current state of the metadata system and troubleshoot issues.

For questions or issues related to metadata, consult the service-specific documentation and ensure proper configuration according to this guide.
