# AudioControl Integration Tests (Python)

This directory contains Python-based integration tests for the AudioControl system. These tests are **synchronous** and run **sequentially** for simplicity and maintainability, replacing the original Rust integration tests.

## Overview

The tests start AudioControl server instances and make HTTP API requests to test functionality. Each test suite uses a separate server instance on a different port to avoid conflicts. All tests run synchronously using standard Python `requests` library and `time.sleep()` for delays.

## Test Files

- `test_generic_integration.py` - Tests basic player functionality and API events
- `test_librespot_integration.py` - Tests Librespot/Spotify player integration  
- `test_activemonitor_integration.py` - Tests the active monitor plugin
- `test_raat_integration.py` - Tests RAAT player integration
- `test_mpd_integration.py` - Tests MPD player integration
- `test_websocket.py` - Tests WebSocket event notifications

### Known Issues

- **WebSocket Tests**: The WebSocket tests currently skip with "Event processing is disabled on the generic player". Even though the player is configured with `"supports_api_events": True` in `conftest.py`, API events are not processed. This issue is documented in `test_websocket.py`. See also `test_player_api_event_support` in `test_generic_integration.py` for diagnosis.

- **Generic Integration Tests**: Some tests in `test_generic_integration.py` need to be updated to match the current API response structure. The API now returns player information in an array under the `players` key, rather than as direct keys.

## Running Tests

### Option 1: Using the test runner (recommended)

```bash
python tests/run_tests.py
```

This will:
1. Install Python dependencies
2. Build the AudioControl binary
3. Run all integration tests **sequentially**

### Option 2: Manual setup

1. Install Python dependencies:
```bash
pip install -r tests/requirements.txt
```

2. Build AudioControl:
```bash
cargo build
```

3. Run specific test files:
```bash
pytest tests/test_generic_integration.py -v
pytest tests/test_librespot_integration.py -v
# etc.
```

4. Run all tests:
```bash
pytest tests/ -v
```

## Test Structure

Each test file follows this pattern:

1. **Setup**: Uses pytest fixtures to start a dedicated AudioControl server instance
2. **Test**: Makes synchronous HTTP API calls to test functionality
3. **Cleanup**: Automatically stops the server and cleans up artifacts

**All tests run synchronously** - no async/await, just regular functions with `time.sleep()` for timing.

## Dependencies

- `pytest` - Test framework
- `requests` - Synchronous HTTP client for API calls
- `psutil` - Process management for cleanup

## Benefits over Rust Tests

1. **Simpler**: No complex process management or unsafe blocks
2. **More reliable**: Better process cleanup and error handling  
3. **Easier debugging**: Clear error messages and better logging
4. **More maintainable**: Familiar Python syntax and tools
5. **Cross-platform**: Works on Windows, macOS, and Linux
6. **Sequential execution**: Tests run one after another, no concurrency issues

## Configuration

Tests create temporary configuration files and use separate ports for each test suite:

- Generic tests: Port 3001
- Librespot tests: Port 3002
- Active monitor tests: Port 3003
- RAAT tests: Port 3004
- MPD tests: Port 3005

## Troubleshooting

If tests fail:

1. Check that the AudioControl binary builds successfully: `cargo build`
2. Verify no processes are using the test ports
3. Check that dependencies are installed: `pip install -r tests/requirements.txt`
4. Run tests individually to isolate issues: `pytest tests/test_generic_integration.py -v`

## Notes

- Tests that require external dependencies (like MPD server) will be skipped if the dependency is not available
- Process cleanup is handled automatically by pytest fixtures
- Each test suite uses a separate server instance to avoid interference
- **All tests are synchronous and sequential** - no async complexity

## Cleaning Up Test Artifacts

Tests create temporary files that are automatically cleaned up when tests complete normally. However, if tests are interrupted or crash, you may need to clean up these files manually.

### Automatic Cleanup

The `setup_and_cleanup` fixture automatically cleans up:

- Temporary config files (`test_config_*.json`)
- Cache directories (`test_cache_*`)
- Pipe files used by players (`test_librespot_event_*`, `test_raat_*`)
- Python cache files (`__pycache__`)

### Manual Cleanup

If tests are interrupted or fail to clean up properly, you can use one of these utilities:

#### Python Script (Windows, macOS, Linux)

```bash
python tests/cleanup_tests.py
```

#### PowerShell Script (Windows)

```powershell
./tests/cleanup_tests.ps1
```

#### Shell Script (macOS, Linux)

```bash
chmod +x tests/cleanup_tests.sh
./tests/cleanup_tests.sh
```

These scripts will remove all temporary files created during testing.
