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
- `test_metadata_seams.py` - Tests the player/metadata HTTP seams from the
  player/metadata split: song-information reaching `now-playing`, a stale
  song-information update being refused, the `/api/metadata/capabilities`
  mount, artist-split resolution with MusicBrainz disabled, and `GET
  /api/player` reporting playback state. Uses `test_config_metadata.json`,
  which points `services.core` back at the daemon's own port and deliberately
  has **no** `services.metadata` -- one process serving both halves, so the
  seams are loopback calls to itself.
- `test_two_daemons.py` - The same split as **two processes**: a player daemon
  and a metadata daemon, each on its own port. See *The two-daemon suite*
  below; it needs more than a line, and reading a green run as more than it is
  would be easy.

## The two-daemon suite

`test_two_daemons.py` is the only suite here that runs **two** daemons: the
player daemon (`audiocontrol`) on 18080 and the metadata daemon
(`audiocontrol-metadata`) on 18084, mirroring the shipped 1080/1084. The
`two_daemons` fixture in `conftest.py` starts both, each with its own
configuration file, its own caches, its own settings database and its own
credential store. It asserts the three properties
`doc/specs/2026-09-08-two-processes.md` names for this phase: playback working
with the metadata daemon stopped, a credential-store write on one side leaving
the other's file untouched, and a `metadata.json` with no `core.url` refusing
to start with a message naming the key.

Four things about it are worth knowing before changing it.

**Build with `--workspace`.** The metadata daemon is a `[[bin]]` of
`crates/audiocontrol-metadata`, not of the root package, so a plain `cargo
build` does not produce it and the fixture fails with a missing-binary error
naming the flag.

**The fixture runs the default build, and the shipped player daemon is not
that build.** The package builds the player daemon `--no-default-features
--features alsa`, which serves no metadata routes at all; a default build
carries an in-process metadata shadow that answers `/api/coverart/methods`,
`/api/favourites/providers`, `/api/lastfm/status`,
`/api/metadata/capabilities` and `/api/resolve/title-order` out of its own
state. The default build is what the fixture uses, because it is what
`run_tests.py`, every other suite here and any ordinary checkout produce, and
because both feature sets write the same `target/debug/audiocontrol` and
cannot coexist. The cost is real: **this suite does not test the shipped
feature set of the player daemon.** What keeps that from making the tests
misleading is that none of them asks the player daemon for a metadata route.
The metadata-daemon-stopped case asserts only player-owned routes -- the play
command, now-playing, volume, the library, `/api/events` -- and it first
requires the metadata daemon's *own* port to refuse a connection, which
nothing in the other process can fake. A test that started asking port 18080
for `/api/metadata/...` would be testing the shadow, and would pass on a
daemon that had lost the seam entirely.

**Readiness is per-daemon.** The metadata daemon is ready when *its* port
answers `/api/metadata/capabilities`, not when the player daemon answers
anything. The seam is one-way: the metadata daemon opens every connection, and
it does not wait for the player daemon to be up -- it binds its own port,
probes `GET /api/version` there for up to 30 s and then starts its subscriber
and library puller regardless. So a fixture that waited on the player daemon's
port would be waiting on the wrong thing, and every wait in the suite is
bounded and says what it was waiting for when it expires.

**What it does not cover.** No credentials are involved anywhere in it. The
credential-store case seeds both stores with placeholder values that are not
credentials and are never decrypted, and asserts on key names and file
digests; the writes it triggers (`POST /api/spotify/tokens` and `POST
/api/lastfm/disconnect`) reach no provider. It therefore covers the file
boundary that makes the two-writer hazard impossible, and not a real OAuth
refresh racing a real Last.fm authentication. The spec's fourth test -- a
store written by 0.22.0 opening in both daemons after an upgrade -- is a device
test and is not here.

### Known Issues

- **WebSocket Tests**: The WebSocket tests currently skip with "Event processing is disabled on the generic player". Even though the player is configured with `"supports_api_events": True` in `conftest.py`, API events are not processed. This issue is documented in `test_websocket.py`. See also `test_player_api_event_support` in `test_generic_integration.py` for diagnosis. Note that `test_websocket.py` is no longer in this directory, and that the WebSocket case in `test_two_daemons.py` does receive a `state_changed` frame on `/api/events` after posting to `/api/player/test/update` on a generic player configured this way -- so whatever this item described, it is not "the generic player never reaches the event stream".

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
cargo build --workspace
```

`--workspace` is not optional. The metadata daemon `test_two_daemons.py` starts
is a `[[bin]]` of `crates/audiocontrol-metadata`; a plain `cargo build` builds
the root package only and does not produce it.

Do not run this straight after `scripts/check-crate-deps.sh`: its last step
builds `--no-default-features` into the same target directory, leaving a
`target/debug/audiocontrol` with no metadata routes. Around forty routes then
answer 404, which looks exactly like a regression in route mounting and is not.
Rebuild with default features first.

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
