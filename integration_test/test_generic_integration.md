# Generic Integration Tests Documentation

This document explains the integration tests in `test_generic_integration.py` for the AudioControl system.

## Overview

These tests verify the functionality of the AudioControl REST API using a generic player. The tests focus on API endpoints, event handling, and state management across various player operations.

**Important:** All tests use strict assertions - they will fail if expected functionality doesn't work correctly. No soft fails, warnings, or pytest.skip() calls are used.

## Test Configuration

The tests use a static configuration file (`test_config_generic.json`) that configures:

- A generic player with API event support
- Web server on port 3001
- Cache directories for attributes and images
- Support for various player capabilities

## Tests Explained

### `test_server_startup`

**Purpose:** Verifies that the AudioControl server starts properly and responds to basic API requests.

**What it tests:**

- Server successfully starts and binds to configured port
- API version endpoint is accessible and returns valid response

**Assertions:**

- Version field is present in response
- Version value is not null

### `test_players_endpoint`

**Purpose:** Verifies that the `/api/players` endpoint returns expected data structure.

**What it tests:**

- The API returns player information in the expected format
- The test player is present in the response
- The player has expected basic fields (id, state, supports_api_events)

**Assertions:**

- Response is a dictionary
- 'test_player' key exists
- Player has 'id', 'state', and 'supports_api_events' fields
- supports_api_events is a boolean value

### `test_now_playing_endpoint`

**Purpose:** Verifies that the `/api/now-playing` endpoint returns data in the expected format.

**What it tests:**

- The endpoint returns a valid JSON response
- The response contains at least one of the expected top-level fields

**Assertions:**

- Response is a dictionary
- Contains at least one of: 'player', 'song', or 'state'

### `test_player_state_events`

**Purpose:** Verifies that the player can receive and process state change events via the API.

**What it tests:**

- Sending a "playing" state event
- Verification that the player state is updated correctly

**Assertions:**

- Player state is updated to "playing" after sending the event
- State change is reflected in either now-playing or players endpoint

### `test_player_shuffle_events`

**Purpose:** Verifies that the player can receive and process shuffle events via the API.

**What it tests:**

- Sending a shuffle enable event
- Verification that the shuffle state is updated correctly

**Assertions:**

- 'shuffle' property is present in player API response
- Shuffle state is updated to True after sending the event

### `test_player_loop_mode_events`

**Purpose:** Verifies that the player can receive and process loop mode events via the API.

**What it tests:**

- Sending a loop mode change event
- Verification that the loop mode is updated correctly

**Assertions:**

- 'loop_mode' property is present in player API response
- Loop mode is updated to 'all' after sending the event

### `test_player_position_events`

**Purpose:** Verifies that the player can receive and process position change events via the API.

**What it tests:**

- Sending a position change event
- Verification that the position is updated correctly

**Assertions:**

- Position is exposed in API responses (either in now-playing or players endpoint)
- Position is updated to the exact value sent (42.5)

### `test_song_metadata_events`

**Purpose:** Verifies that the player can receive and process song metadata events via the API.

**What it tests:**

- Sending a metadata change event with song details
- Verification that metadata is updated correctly

**Assertions:**

- Song data is available in now-playing response
- Title, artist, and album are updated to exact values sent

### `test_multiple_events_sequence`

**Purpose:** Verifies that the player can handle multiple events sent in sequence.

**What it tests:**

- Sending multiple events in sequence: state, shuffle, loop mode, position, metadata
- Verification that all properties are updated correctly

**Assertions:**

- All properties (state, shuffle, loop_mode, position) are present in API responses
- All properties are updated to exact values sent
- Song metadata is updated correctly

### `test_player_api_event_support`

**Purpose:** Verifies that the player reports API event support correctly.

**What it tests:**

- Checks if the player reports 'supports_api_events' flag
- Attempts to send a test event and verify processing

**Assertions:**

- 'supports_api_events' field is present in API response
- Player reports that API events are supported (True)
- Test event is processed successfully

## Running the Tests

From the integration_test directory:

```bash
# Run all generic integration tests
python -m pytest test_generic_integration.py -v

# Run a specific test
python -m pytest test_generic_integration.py::test_player_state_events -v

# Run with detailed output
python -m pytest test_generic_integration.py -v -s
```

## Test Expectations

All tests are expected to pass if the generic player controller is properly implemented. Test failures indicate:

1. **API endpoint issues** - Server not responding or returning incorrect data structure
2. **Event processing issues** - Player not processing API events correctly
3. **State management issues** - Player not updating internal state based on events
4. **Configuration issues** - Player not configured with proper capabilities or API event support

## Troubleshooting Failed Tests

### Common Issues:

1. **Server startup failures** - Check if port 3001 is available and server configuration is correct
2. **Event processing failures** - Verify that 'supports_api_events' is set to true in the player configuration
3. **State update failures** - Check that the generic player controller properly implements state management
4. **Missing API fields** - Verify that the player exposes all required fields in API responses

### Debug Information:

The tests include detailed debug output showing:

- API response structures
- Current player state
- Event processing results
- Timing information for slow operations

Use this information to diagnose issues when tests fail.
