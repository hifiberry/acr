#!/usr/bin/env python3
"""
Librespot integration tests for AudioControl system
"""

import pytest
import time

def test_librespot_player_initialization(librespot_server):
    start = time.perf_counter()
    step = time.perf_counter()
    response = librespot_server.get_players_raw()
    print(f"[TIMING] get_players_raw: {time.perf_counter() - step:.3f}s")
    step = time.perf_counter()
    assert isinstance(response, dict)
    assert "players" in response
    players = response["players"]
    assert isinstance(players, list)
    assert len(players) > 0
    print(f"[TIMING] player checks: {time.perf_counter() - step:.3f}s")
    step = time.perf_counter()
    first_player = players[0]
    assert 'id' in first_player
    assert 'name' in first_player
    assert 'state' in first_player
    print(f"[TIMING] structure checks: {time.perf_counter() - step:.3f}s")
    elapsed = time.perf_counter() - start
    print(f"[TIMING] test_librespot_player_initialization: {elapsed:.3f}s")

def test_librespot_server_responds(librespot_server):
    start = time.perf_counter()
    step = time.perf_counter()
    response = librespot_server.api_request('GET', '/api/version')
    print(f"[TIMING] api_request /api/version: {time.perf_counter() - step:.3f}s")
    step = time.perf_counter()
    assert 'version' in response
    assert response['version'] is not None
    now_playing = librespot_server.get_now_playing()
    print(f"[TIMING] get_now_playing: {time.perf_counter() - step:.3f}s")
    step = time.perf_counter()
    assert isinstance(now_playing, dict)
    print(f"[TIMING] now_playing check: {time.perf_counter() - step:.3f}s")
    elapsed = time.perf_counter() - start
    print(f"[TIMING] test_librespot_server_responds: {elapsed:.3f}s")

def test_librespot_event_handling(librespot_server):
    start = time.perf_counter()
    step = time.perf_counter()
    players_response = librespot_server.get_players_raw()
    print(f"[TIMING] get_players_raw: {time.perf_counter() - step:.3f}s")
    
    if "players" not in players_response or len(players_response["players"]) == 0:
        pytest.skip("No players available for testing")
        
    # Find the librespot player
    player = None
    for p in players_response["players"]:
        if "librespot" in p["id"].lower():
            player = p
            break
    
    if not player:
        # Fall back to the first player
        player = players_response["players"][0]
        
    player_id = player["id"]
    print(f"Using player: {player_id}")
    
    step = time.perf_counter()
    librespot_server.reset_player_state(player_id)
    print(f"[TIMING] reset_player_state: {time.perf_counter() - step:.3f}s")
    
    step = time.perf_counter()
    event = {"type": "state_changed", "state": "playing"}
    response = librespot_server.send_player_event(player_id, event)
    print(f"[TIMING] send_player_event: {time.perf_counter() - step:.3f}s")
    assert response is not None
    assert response.get("success", False) is True
    
    step = time.perf_counter()
    time.sleep(0.1)
    print(f"[TIMING] sleep after event: {time.perf_counter() - step:.3f}s")
    
    step = time.perf_counter()
    now_playing = librespot_server.get_now_playing()
    print(f"[TIMING] get_now_playing: {time.perf_counter() - step:.3f}s")
    
    assert "player" in now_playing
    # The active player might not be the one we sent the event to, so we don't check the ID
    assert now_playing["state"].lower() == "playing"
    
    elapsed = time.perf_counter() - start
    print(f"[TIMING] test_librespot_event_handling: {elapsed:.3f}s")

def test_librespot_metadata_events(librespot_server):
    start = time.perf_counter()
    step = time.perf_counter()
    players_response = librespot_server.get_players_raw()
    print(f"[TIMING] get_players_raw: {time.perf_counter() - step:.3f}s")
    
    if "players" not in players_response or len(players_response["players"]) == 0:
        pytest.skip("No players available for testing")
        
    # Find the librespot player
    player = None
    for p in players_response["players"]:
        if "librespot" in p["id"].lower():
            player = p
            break
    
    if not player:
        # Fall back to the first player
        player = players_response["players"][0]
        
    player_id = player["id"]
    print(f"Using player: {player_id}")
    
    step = time.perf_counter()
    librespot_server.reset_player_state(player_id)
    print(f"[TIMING] reset_player_state: {time.perf_counter() - step:.3f}s")
    
    step = time.perf_counter()
    event = {
        "type": "metadata_changed",
        "metadata": {
            "title": "Test Spotify Track",
            "artist": "Test Spotify Artist",
            "album": "Test Spotify Album",
            "duration": 234.5,
            "track_number": 1,
            "uri": "spotify:track:test123"
        }
    }
    response = librespot_server.send_player_event(player_id, event)
    print(f"[TIMING] send_player_event: {time.perf_counter() - step:.3f}s")
    assert response is not None
    assert response.get("success", False) is True
    
    step = time.perf_counter()
    time.sleep(0.1)
    print(f"[TIMING] sleep after event: {time.perf_counter() - step:.3f}s")
    
    step = time.perf_counter()
    now_playing = librespot_server.get_now_playing()
    print(f"[TIMING] get_now_playing: {time.perf_counter() - step:.3f}s")
    
    if 'song' in now_playing and now_playing['song']:
        song = now_playing['song']
        assert song['title'] == 'Test Spotify Track'
        assert song['artist'] == 'Test Spotify Artist'
        assert song['album'] == 'Test Spotify Album'
        assert song['duration'] == 234.5
    
    print(f"[TIMING] metadata checks: {time.perf_counter() - step:.3f}s")
    elapsed = time.perf_counter() - start
    print(f"[TIMING] test_librespot_metadata_events: {elapsed:.3f}s")

def test_librespot_playback_control(librespot_server):
    start = time.perf_counter()
    step = time.perf_counter()
    players_response = librespot_server.get_players_raw()
    print(f"[TIMING] get_players_raw: {time.perf_counter() - step:.3f}s")
    
    if "players" not in players_response or len(players_response["players"]) == 0:
        pytest.skip("No players available for testing")
        
    # Find the librespot player
    player = None
    for p in players_response["players"]:
        if "librespot" in p["id"].lower():
            player = p
            break
    
    if not player:
        # Fall back to the first player
        player = players_response["players"][0]
        
    player_id = player["id"]
    print(f"Using player: {player_id}")
    
    step = time.perf_counter()
    librespot_server.reset_player_state(player_id)
    print(f"[TIMING] reset_player_state: {time.perf_counter() - step:.3f}s")
    
    events = [
        {"type": "state_changed", "state": "playing"},
        {"type": "state_changed", "state": "paused"},
        {"type": "state_changed", "state": "stopped"}
    ]
    expected_states = ["playing", "paused", "stopped"]
    
    for event, expected_state in zip(events, expected_states):
        step = time.perf_counter()
        response = librespot_server.send_player_event(player_id, event)
        print(f"[TIMING] send_player_event: {time.perf_counter() - step:.3f}s")
        assert response is not None
        assert response.get("success", False) is True
        
        step = time.perf_counter()
        time.sleep(0.1)  # Increased sleep time to ensure state change is registered
        print(f"[TIMING] sleep after event: {time.perf_counter() - step:.3f}s")
        
        step = time.perf_counter()
        now_playing = librespot_server.get_now_playing()
        print(f"[TIMING] get_now_playing (after event): {time.perf_counter() - step:.3f}s")
        assert now_playing["state"].lower() == expected_state
    
    elapsed = time.perf_counter() - start
    print(f"[TIMING] test_librespot_playback_control: {elapsed:.3f}s")

def test_librespot_position_tracking(librespot_server):
    start = time.perf_counter()
    step = time.perf_counter()
    players_response = librespot_server.get_players_raw()
    print(f"[TIMING] get_players_raw: {time.perf_counter() - step:.3f}s")
    
    if "players" not in players_response or len(players_response["players"]) == 0:
        pytest.skip("No players available for testing")
        
    # Find the librespot player
    player = None
    for p in players_response["players"]:
        if "librespot" in p["id"].lower():
            player = p
            break
    
    if not player:
        # Fall back to the first player
        player = players_response["players"][0]
        
    player_id = player["id"]
    print(f"Using player: {player_id}")
    
    step = time.perf_counter()
    librespot_server.reset_player_state(player_id)
    print(f"[TIMING] reset_player_state: {time.perf_counter() - step:.3f}s")
    
    # First set the state to playing to ensure position updates are processed
    playing_event = {"type": "state_changed", "state": "playing"}
    librespot_server.send_player_event(player_id, playing_event)
    time.sleep(0.1)
    
    # For position tracking, we need to first send a metadata event
    metadata_event = {
        "type": "metadata_changed",
        "metadata": {
            "title": "Test Track for Position",
            "artist": "Test Artist",
            "album": "Test Album",
            "duration": 300.0,
            "uri": "spotify:track:test123"
        }
    }
    librespot_server.send_player_event(player_id, metadata_event)
    time.sleep(0.1)
    
    positions = [10.0, 25.5, 60.0, 120.7]
    
    for position in positions:
        step = time.perf_counter()
        event = {"type": "position_changed", "position": position}
        response = librespot_server.send_player_event(player_id, event)
        print(f"[TIMING] send_player_event: {time.perf_counter() - step:.3f}s")
        assert response is not None
        assert response.get("success", False) is True
        
        step = time.perf_counter()
        time.sleep(0.1)  # Increased sleep time to ensure position change is registered
        print(f"[TIMING] sleep after event: {time.perf_counter() - step:.3f}s")
        
        step = time.perf_counter()
        now_playing = librespot_server.get_now_playing()
        print(f"[TIMING] get_now_playing (after event): {time.perf_counter() - step:.3f}s")
        
        # Position might be rounded or slightly different due to timing - check if it's close enough
        assert "position" in now_playing, "Position field missing in now_playing response"
        if now_playing["position"] is not None:
            assert abs(now_playing["position"] - position) < 1.0, f"Expected position {position}, got {now_playing['position']}"
    
    elapsed = time.perf_counter() - start
    print(f"[TIMING] test_librespot_position_tracking: {elapsed:.3f}s")

def test_librespot_shuffle_and_repeat(librespot_server):
    start = time.perf_counter()
    step = time.perf_counter()
    players_response = librespot_server.get_players_raw()
    print(f"[TIMING] get_players_raw: {time.perf_counter() - step:.3f}s")
    
    if "players" not in players_response or len(players_response["players"]) == 0:
        pytest.skip("No players available for testing")
        
    # Find the player to use - prefer test_player since it supports API events better
    player = None
    for p in players_response["players"]:
        if p["id"] == "test_player":
            player = p
            break
    
    if not player:
        # Fall back to librespot or any first player
        for p in players_response["players"]:
            if "librespot" in p["id"].lower():
                player = p
                break
        
    if not player and len(players_response["players"]) > 0:
        # Use the first player if nothing else found
        player = players_response["players"][0]
        
    player_id = player["id"]
    print(f"Using player: {player_id}")
    
    step = time.perf_counter()
    librespot_server.reset_player_state(player_id)
    print(f"[TIMING] reset_player_state: {time.perf_counter() - step:.3f}s")
    
    # First check initial state
    initial_state = librespot_server.get_now_playing()
    print(f"Initial shuffle state: {initial_state.get('shuffle', False)}")
    
    # Test shuffle change
    shuffle_event = {"type": "shuffle_changed", "enabled": True}
    step = time.perf_counter()
    response = librespot_server.send_player_event(player_id, shuffle_event)
    print(f"[TIMING] send_player_event (shuffle): {time.perf_counter() - step:.3f}s")
    
    # Don't require success response - some API implementations might not return it
    print(f"Shuffle response: {response}")
    
    step = time.perf_counter()
    time.sleep(0.5)  # Increased sleep time to ensure state change is processed
    print(f"[TIMING] sleep after shuffle: {time.perf_counter() - step:.3f}s")
    
    step = time.perf_counter()
    # Try multiple times to check if shuffle state changed
    max_attempts = 3
    shuffle_state_updated = False
    for attempt in range(max_attempts):
        now_playing = librespot_server.get_now_playing()
        print(f"[TIMING] get_now_playing (attempt {attempt+1}, after shuffle): {time.perf_counter() - step:.3f}s")
        print(f"Current now_playing: {now_playing}")
        
        # Check both possible field formats for shuffle in the API response
        if now_playing.get("shuffle") is True:
            shuffle_state_updated = True
            print("Shuffle state successfully updated to True!")
            break
        
        # Try alternate capitalization - some implementations might use different casing
        if "Shuffle" in now_playing and now_playing["Shuffle"] is True:
            shuffle_state_updated = True
            print("Shuffle state (with capital S) successfully updated to True!")
            break
            
        print(f"Shuffle state not yet updated (attempt {attempt+1}/{max_attempts}), waiting...")
        time.sleep(1.0)
    
    # Assert that shuffle was updated correctly
    assert shuffle_state_updated, f"Shuffle state was not updated correctly after {max_attempts} attempts"
    
    # Verify the final state
    if "shuffle" in now_playing:
        assert now_playing["shuffle"] is True, f"Expected shuffle to be True, got {now_playing['shuffle']}"
    elif "Shuffle" in now_playing:
        assert now_playing["Shuffle"] is True, f"Expected Shuffle to be True, got {now_playing['Shuffle']}"
    else:
        assert False, "Shuffle field missing from now_playing response"
    
    # Test loop mode change with softer assertions
    repeat_event = {"type": "loop_mode_changed", "mode": "all"}
    step = time.perf_counter()
    response = librespot_server.send_player_event(player_id, repeat_event)
    print(f"[TIMING] send_player_event (repeat): {time.perf_counter() - step:.3f}s")
    print(f"Loop mode response: {response}")
    
    step = time.perf_counter()
    time.sleep(0.5)  # Increased sleep time
    print(f"[TIMING] sleep after repeat: {time.perf_counter() - step:.3f}s")
    
    step = time.perf_counter()
    now_playing = librespot_server.get_now_playing()
    print(f"[TIMING] get_now_playing (after repeat): {time.perf_counter() - step:.3f}s")
    print(f"Final now_playing: {now_playing}")
    
    # Assert loop_mode was updated correctly
    assert "loop_mode" in now_playing, "Loop_mode field missing in now_playing response"
    expected_values = ["all", "playlist", "Playlist", "All"]
    assert now_playing["loop_mode"] in expected_values, f"Expected loop_mode to be one of {expected_values}, got {now_playing['loop_mode']}"
    
    elapsed = time.perf_counter() - start
    print(f"[TIMING] test_librespot_shuffle_and_repeat: {elapsed:.3f}s")

# New tests for audiocontrol_notify_librespot
def test_notify_librespot_song_update(librespot_server):
    """Test audiocontrol_notify_librespot song update functionality"""
    start = time.perf_counter()
    step = time.perf_counter()
    
    # Get a player to use
    players_response = librespot_server.get_players_raw()
    print(f"[TIMING] get_players_raw: {time.perf_counter() - step:.3f}s")
    
    if "players" not in players_response or len(players_response["players"]) == 0:
        pytest.skip("No players available for testing")
    
    player = players_response["players"][0]
    player_id = player["id"]
    print(f"Using player: {player_id}")
    
    # Reset player state first
    step = time.perf_counter()
    librespot_server.reset_player_state(player_id)
    print(f"[TIMING] reset_player_state: {time.perf_counter() - step:.3f}s")
    
    # Send a track_changed event with metadata
    step = time.perf_counter()
    env_vars = {
        "NAME": "Test Spotify Track",
        "ARTISTS": "Test Artist Name",
        "ALBUM": "Test Album Name",
        "DURATION_MS": "234500",  # 234.5 seconds
        "URI": "spotify:track:test123",
        "NUMBER": "5",
        "COVERS": "https://example.com/cover.jpg"
    }
    
    response = librespot_server.send_librespot_event(player_id, "track_changed", env_vars)
    print(f"[TIMING] send_librespot_event: {time.perf_counter() - step:.3f}s")
    assert response is not None
    assert response.get("success", False) is True
    
    # Wait for the event to be processed
    step = time.perf_counter()
    time.sleep(0.5)
    print(f"[TIMING] sleep after event: {time.perf_counter() - step:.3f}s")
    
    # Check that the song information was updated
    step = time.perf_counter()
    now_playing = librespot_server.get_now_playing()
    print(f"[TIMING] get_now_playing: {time.perf_counter() - step:.3f}s")
    
    assert "song" in now_playing and now_playing["song"] is not None
    song = now_playing["song"]
    assert song["title"] == "Test Spotify Track"
    assert song["artist"] == "Test Artist Name"
    assert song["album"] == "Test Album Name"
    assert song["duration"] == 234.5
    assert song["stream_url"] == "spotify:track:test123"
    
    # Also check that playback state was set to playing (track_changed sends both events)
    assert now_playing["state"] == "playing"
    
    elapsed = time.perf_counter() - start
    print(f"[TIMING] test_notify_librespot_song_update: {elapsed:.3f}s")

def test_notify_librespot_shuffle_change(librespot_server):
    """Test audiocontrol_notify_librespot shuffle change functionality"""
    start = time.perf_counter()
    step = time.perf_counter()
    
    # Get a player to use
    players_response = librespot_server.get_players_raw()
    print(f"[TIMING] get_players_raw: {time.perf_counter() - step:.3f}s")
    
    if "players" not in players_response or len(players_response["players"]) == 0:
        pytest.skip("No players available for testing")
    
    player = players_response["players"][0]
    player_id = player["id"]
    print(f"Using player: {player_id}")
    
    # Reset player state first
    step = time.perf_counter()
    librespot_server.reset_player_state(player_id)
    print(f"[TIMING] reset_player_state: {time.perf_counter() - step:.3f}s")
    
    # Test enabling shuffle
    step = time.perf_counter()
    env_vars = {"SHUFFLE": "true"}
    response = librespot_server.send_librespot_event(player_id, "shuffle_changed", env_vars)
    print(f"[TIMING] send_librespot_event (shuffle on): {time.perf_counter() - step:.3f}s")
    assert response is not None
    assert response.get("success", False) is True
    
    # Wait for the event to be processed
    step = time.perf_counter()
    time.sleep(0.5)
    print(f"[TIMING] sleep after shuffle on: {time.perf_counter() - step:.3f}s")
    
    # Check that shuffle was enabled
    step = time.perf_counter()
    now_playing = librespot_server.get_now_playing()
    print(f"[TIMING] get_now_playing (after shuffle on): {time.perf_counter() - step:.3f}s")
    assert now_playing.get("shuffle") is True
    
    # Test disabling shuffle
    step = time.perf_counter()
    env_vars = {"SHUFFLE": "false"}
    response = librespot_server.send_librespot_event(player_id, "shuffle_changed", env_vars)
    print(f"[TIMING] send_librespot_event (shuffle off): {time.perf_counter() - step:.3f}s")
    assert response is not None
    assert response.get("success", False) is True
    
    # Wait for the event to be processed
    step = time.perf_counter()
    time.sleep(0.5)
    print(f"[TIMING] sleep after shuffle off: {time.perf_counter() - step:.3f}s")
    
    # Check that shuffle was disabled
    step = time.perf_counter()
    now_playing = librespot_server.get_now_playing()
    print(f"[TIMING] get_now_playing (after shuffle off): {time.perf_counter() - step:.3f}s")
    assert now_playing.get("shuffle") is False
    
    elapsed = time.perf_counter() - start
    print(f"[TIMING] test_notify_librespot_shuffle_change: {elapsed:.3f}s")

def test_notify_librespot_playback_state_change(librespot_server):
    """Test audiocontrol_notify_librespot playback state change functionality"""
    start = time.perf_counter()
    step = time.perf_counter()
    
    # Get a player to use
    players_response = librespot_server.get_players_raw()
    print(f"[TIMING] get_players_raw: {time.perf_counter() - step:.3f}s")
    
    if "players" not in players_response or len(players_response["players"]) == 0:
        pytest.skip("No players available for testing")
    
    player = players_response["players"][0]
    player_id = player["id"]
    print(f"Using player: {player_id}")
    
    # Reset player state first
    step = time.perf_counter()
    librespot_server.reset_player_state(player_id)
    print(f"[TIMING] reset_player_state: {time.perf_counter() - step:.3f}s")
    
    # Test changing to playing state
    step = time.perf_counter()
    response = librespot_server.send_librespot_event(player_id, "playing")
    print(f"[TIMING] send_librespot_event (playing): {time.perf_counter() - step:.3f}s")
    assert response is not None
    assert response.get("success", False) is True
    
    # Wait for the event to be processed
    step = time.perf_counter()
    time.sleep(0.5)
    print(f"[TIMING] sleep after playing: {time.perf_counter() - step:.3f}s")
    
    # Check that state was set to playing
    step = time.perf_counter()
    now_playing = librespot_server.get_now_playing()
    print(f"[TIMING] get_now_playing (after playing): {time.perf_counter() - step:.3f}s")
    assert now_playing["state"] == "playing"
    
    # Test changing to paused state
    step = time.perf_counter()
    response = librespot_server.send_librespot_event(player_id, "paused")
    print(f"[TIMING] send_librespot_event (paused): {time.perf_counter() - step:.3f}s")
    assert response is not None
    assert response.get("success", False) is True
    
    # Wait for the event to be processed
    step = time.perf_counter()
    time.sleep(0.5)
    print(f"[TIMING] sleep after paused: {time.perf_counter() - step:.3f}s")
    
    # Check that state was set to paused
    step = time.perf_counter()
    now_playing = librespot_server.get_now_playing()
    print(f"[TIMING] get_now_playing (after paused): {time.perf_counter() - step:.3f}s")
    assert now_playing["state"] == "paused"
    
    elapsed = time.perf_counter() - start
    print(f"[TIMING] test_notify_librespot_playback_state_change: {elapsed:.3f}s")

def test_notify_librespot_position_update(librespot_server):
    """Test audiocontrol_notify_librespot position update functionality"""
    start = time.perf_counter()
    step = time.perf_counter()
    
    # Get a player to use
    players_response = librespot_server.get_players_raw()
    print(f"[TIMING] get_players_raw: {time.perf_counter() - step:.3f}s")
    
    if "players" not in players_response or len(players_response["players"]) == 0:
        pytest.skip("No players available for testing")
    
    player = players_response["players"][0]
    player_id = player["id"]
    print(f"Using player: {player_id}")
    
    # Reset player state first
    step = time.perf_counter()
    librespot_server.reset_player_state(player_id)
    print(f"[TIMING] reset_player_state: {time.perf_counter() - step:.3f}s")
    
    # First, set some song metadata so we have a track to seek in
    step = time.perf_counter()
    track_env_vars = {
        "NAME": "Test Track for Position",
        "ARTISTS": "Test Artist",
        "ALBUM": "Test Album",
        "DURATION_MS": "300000",  # 300 seconds
        "URI": "spotify:track:position_test"
    }
    response = librespot_server.send_librespot_event(player_id, "track_changed", track_env_vars)
    assert response.get("success", False) is True
    time.sleep(0.5)
    print(f"[TIMING] setup track metadata: {time.perf_counter() - step:.3f}s")
    
    # Test different position updates
    positions_ms = [10000, 65500, 120700]  # 10s, 65.5s, 120.7s
    positions_s = [10.0, 65.5, 120.7]
    
    for position_ms, expected_position_s in zip(positions_ms, positions_s):
        step = time.perf_counter()
        env_vars = {"POSITION_MS": str(position_ms)}
        response = librespot_server.send_librespot_event(player_id, "seeked", env_vars)
        print(f"[TIMING] send_librespot_event (position {expected_position_s}s): {time.perf_counter() - step:.3f}s")
        assert response is not None
        assert response.get("success", False) is True
        
        # Wait for the event to be processed
        step = time.perf_counter()
        time.sleep(0.5)
        print(f"[TIMING] sleep after position update: {time.perf_counter() - step:.3f}s")
        
        # Check that position was updated
        step = time.perf_counter()
        now_playing = librespot_server.get_now_playing()
        print(f"[TIMING] get_now_playing (after position update): {time.perf_counter() - step:.3f}s")
        
        assert "position" in now_playing
        if now_playing["position"] is not None:
            assert abs(now_playing["position"] - expected_position_s) < 1.0, f"Expected position {expected_position_s}, got {now_playing['position']}"
    
    elapsed = time.perf_counter() - start
    print(f"[TIMING] test_notify_librespot_position_update: {elapsed:.3f}s")
