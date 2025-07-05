# Integration Test Scripts

This directory contains scripts to run the AudioControl integration test suite with proper cleanup.

## Usage

### Run all tests

**Windows:**

```cmd
run-test.bat
```

**Linux/Unix/macOS:**

```bash
./run-test.sh
```

### Run specific tests

**Windows:**

```cmd
run-test.bat test_librespot_api_events
run-test.bat test_librespot_api_events test_generic_player_becomes_active_on_playing
```

**Linux/Unix/macOS:**

```bash
./run-test.sh test_librespot_api_events
./run-test.sh test_librespot_api_events test_generic_player_becomes_active_on_playing
```

### Available tests

- `test_full_integration_state_change`
- `test_full_integration_song_change`
- `test_full_integration_multiple_events`
- `test_full_integration_custom_event`
- `test_players_initialization`
- `test_raat_player_initialization`
- `test_mpd_player_initialization`
- `test_librespot_player_initialization`
- `test_librespot_api_events`
- `test_librespot_pipe_events`
- `test_librespot_legacy_format_api`
- `test_librespot_mixed_events`
- `test_librespot_error_handling`
- `test_generic_player_becomes_active_on_playing`
- `test_librespot_player_becomes_active_on_playing`

## What the scripts do

1. **Pre-test cleanup**: Kill any existing audiocontrol processes
2. **Run tests**: Execute the full integration test suite with verbose output (`--nocapture`)
3. **Post-test cleanup**: Kill any remaining processes and clean up test artifacts
4. **Report results**: Show final test results and exit with appropriate code

## Features

- **Robust cleanup**: Ensures server processes are always killed, even if tests panic
- **Verbose output**: Shows detailed test output with `--nocapture` flag
- **Cross-platform**: Works on Windows (`.bat`) and Unix-like systems (`.sh`)
- **Exit codes**: Returns proper exit codes for CI/CD integration
- **Artifact cleanup**: Removes test config files and cache directories

## Test behavior

The integration tests are designed to:

- Start a single AudioControl server shared across all tests
- Run tests serially to avoid conflicts
- Use robust error handling (no panics except on server startup failure)
- **Fail explicitly** when expected conditions aren't met (no soft failures)
- Automatically clean up the server when tests complete or fail

The cleanup mechanism ensures that the server is always killed regardless of how the tests finish.

## Test expectations

All tests are expected to pass in a proper test environment:

- **Player initialization tests** expect all configured players to be present
- **Event processing tests** expect events to be processed when sent
- **State transition tests** expect players to become active when playing
- **Pipe/API tests** expect communication mechanisms to work properly

Tests will fail if dependencies are missing or if the system doesn't behave as expected.
