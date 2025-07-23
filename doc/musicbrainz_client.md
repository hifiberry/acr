# MusicBrainz API Client

The `audiocontrol_musicbrainz_client` tool is a command-line client for making direct API calls to MusicBrainz without using any local caching. This tool is designed for debugging and testing MusicBrainz integration.

## Overview

This tool provides direct API access capabilities:

- **Artist name lookup**: Search for artists by name and retrieve their MusicBrainz IDs (MBIDs)
- **Artist MBID lookup**: Look up and validate artists by their MusicBrainz IDs
- **Album MBID lookup**: Look up and validate album MusicBrainz IDs
- **Artist name splitting**: Check if artist names contain multiple artists separated by common delimiters

All operations make direct HTTP requests to the MusicBrainz API without using any local caching.

## Usage

```bash
audiocontrol_musicbrainz_client [OPTIONS]
```

### Basic Options

- `-c, --config FILE`: Configuration file path (default: audiocontrol.json)
- `-v, --verbose`: Enable verbose output with additional debugging information
- `-h, --help`: Display help information

### Lookup Operations

#### Artist Name Lookup
```bash
# Basic artist lookup
audiocontrol_musicbrainz_client --artist-name "The Beatles"

# Verbose output
audiocontrol_musicbrainz_client --artist-name "Pink Floyd" --verbose
```

#### Artist MBID Lookup
```bash
# Validate and look up artist by MusicBrainz ID
audiocontrol_musicbrainz_client --artist-mbid "b10bbbfc-cf9e-42e0-be17-e2c3e1d2600d"
```

#### Album MBID Lookup
```bash
# Validate album MusicBrainz ID
audiocontrol_musicbrainz_client --album-mbid "5b11f4ce-a62d-471e-81fc-a69a8278c7da"
```

#### Artist Name Splitting
```bash
# Check if artist name contains multiple artists
audiocontrol_musicbrainz_client --split "John Williams & London Symphony Orchestra"
audiocontrol_musicbrainz_client --split "Eminem feat. Dr. Dre"
audiocontrol_musicbrainz_client --split "Various Artists"
```

## Output Format

The tool provides clear feedback using symbols:

- **✓** Success: Operation completed successfully
- **✗** Error: Operation failed or item not found  
- **ℹ** Information: Additional context or suggestions

### Example Output

```
=== Artist Name Lookup ===
Artist: The Beatles
Making direct API call to MusicBrainz...
✓ API call successful
✓ Found 1 artist(s):
  1: The Beatles (MBID: b10bbbfc-cf9e-42e0-be17-e2c3e1d2600d, Score: 100)
```

## Artist Separators

The tool checks for these common separators when splitting artist names:

- `,` (comma)
- `&` (ampersand)
- ` feat ` (featuring)
- ` feat.` (featuring abbreviated)
- ` featuring ` (featuring full word)
- ` with ` (with)

## Configuration

The tool reads MusicBrainz settings from the audiocontrol configuration file:

```json
{
  "services": {
    "musicbrainz": {
      "enabled": true
    }
  }
}
```

MusicBrainz lookups must be enabled in the configuration for the tool to work.

## API Calls

All operations make direct HTTP requests to the MusicBrainz API:

- **Artist Search**: `https://musicbrainz.org/ws/2/artist?query=artist:{name}&fmt=json&limit=3`
- **Artist Lookup**: `https://musicbrainz.org/ws/2/artist/{mbid}?fmt=json`
- **Album Lookup**: `https://musicbrainz.org/ws/2/release/{mbid}?fmt=json`

The tool uses proper User-Agent headers and follows MusicBrainz API guidelines.

## Troubleshooting

### Common Issues

1. **MusicBrainz disabled**: If MusicBrainz lookups are disabled in configuration, the tool will exit with an error message

2. **Invalid MBID format**: MusicBrainz IDs must be in UUID format (xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx)

3. **Network issues**: The tool requires internet connectivity to reach the MusicBrainz API

4. **Rate limiting**: The tool makes direct API calls which may be rate-limited by MusicBrainz

### Debug Tips

1. **Use verbose mode** (`-v`) to see detailed information including raw API responses
2. **Check API URLs** in verbose mode to verify correct request formatting
3. **Test with known good artists** like "The Beatles" to verify basic functionality
4. **Verify network connectivity** if you see HTTP request failures

## Examples by Use Case

### Testing Artist Search
```bash
# Test basic search
audiocontrol_musicbrainz_client --artist-name "The Beatles" --verbose

# Test artist with disambiguation
audiocontrol_musicbrainz_client --artist-name "John Williams" --verbose

# Test obscure artist
audiocontrol_musicbrainz_client --artist-name "Very Obscure Artist Name"
```

### Validating MBIDs
```bash
# Valid MBID
audiocontrol_musicbrainz_client --artist-mbid "b10bbbfc-cf9e-42e0-be17-e2c3e1d2600d" --verbose

# Invalid MBID format
audiocontrol_musicbrainz_client --artist-mbid "invalid-format"

# Valid album MBID
audiocontrol_musicbrainz_client --album-mbid "5b11f4ce-a62d-471e-81fc-a69a8278c7da"
```

### Testing Multi-Artist Names
```bash
# Test featuring
audiocontrol_musicbrainz_client --split "Artist A feat. Artist B" --verbose

# Test ampersand
audiocontrol_musicbrainz_client --split "Artist A & Artist B" --verbose

# Test comma separation
audiocontrol_musicbrainz_client --split "Artist A, Artist B, Artist C"
```

## API Rate Limiting

This tool makes direct API calls to MusicBrainz, which may be subject to rate limiting. The MusicBrainz API allows:

- 1 request per second for anonymous requests
- More requests for authenticated clients (not implemented in this tool)

If you encounter rate limiting, wait between requests or reduce the frequency of API calls.

## Differences from audiocontrol

This client tool differs from the main audiocontrol application in several ways:

1. **No caching**: All requests go directly to the API
2. **No rate limiting**: The tool doesn't implement the same rate limiting as the main application
3. **Simplified responses**: Responses are parsed and displayed in a human-readable format
4. **Debugging focus**: Output is designed for debugging rather than integration

Use this tool for testing and debugging, but be aware that results may differ slightly from what the main audiocontrol application experiences due to caching and rate limiting differences.
