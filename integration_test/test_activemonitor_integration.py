#!/usr/bin/env python3
"""
Active Monitor integration tests for AudioControl system
"""

import pytest
import time

def test_activemonitor_plugin_initialization(activemonitor_server):
    """Test that Active Monitor plugin is initialized correctly"""
    response = activemonitor_server.api_request('GET', '/api/version')
    assert 'version' in response
    assert response['version'] is not None
    
    # The server should be running with the active monitor plugin
    # We can't directly test the plugin without more specific endpoints
    # but we can verify the server is responding

def test_activemonitor_server_responds(activemonitor_server):
    """Test that the server with active monitor responds to basic requests"""
    # Test basic endpoints
    response = activemonitor_server.api_request('GET', '/api/version')
    assert 'version' in response
    
    # Test players endpoint
    players = activemonitor_server.get_players()
    assert isinstance(players, dict)
    
    # Test now playing endpoint
    now_playing = activemonitor_server.get_now_playing()
    assert isinstance(now_playing, dict)

def test_activemonitor_multiple_players(activemonitor_server):
    """Test that multiple players (generic and librespot) are available"""
    players = activemonitor_server.get_players()
    
    # We should have both generic and librespot players
    assert len(players) >= 2
    
    # Check that we have the expected players
    player_names = set(players.keys())
    assert 'test_player' in player_names  # Generic player
    assert 'librespot' in player_names  # Librespot player
    
    # Verify player properties
    generic_player = players['test_player']
    assert generic_player['name'] == 'test_player'
    assert generic_player['supports_api_events'] is True
    
    librespot_player = players['librespot']
    assert librespot_player['name'] == 'spotify'  # Display name for librespot
    assert librespot_player['id'] == 'librespot'

def test_activemonitor_player_events(activemonitor_server):
    """Test that player events work with active monitor enabled"""
    # Get available players
    players = activemonitor_server.get_players()
    player_names = list(players.keys())
    
    if len(player_names) == 0:
        pytest.skip("No players available for testing")
    
    # Use the first available player
    player_name = player_names[0]
    
    # Reset player state
    activemonitor_server.reset_player_state(player_name)
    
    # Test that we can send events and they work normally
    # The active monitor should be observing these events
    event = {"type": "state_changed", "state": "playing"}
    response = activemonitor_server.send_player_event(player_name, event)
    assert response is not None
    
    # Allow time for event processing
    time.sleep(0.1)
    
    # Check that the state changed
    updated_players = activemonitor_server.get_players()
    assert updated_players[player_name]['state'] == 'playing'

def test_activemonitor_state_transitions(activemonitor_server):
    """Test state transitions with active monitor"""
    # Get available players
    players = activemonitor_server.get_players()
    player_names = list(players.keys())
    
    if len(player_names) == 0:
        pytest.skip("No players available for testing")
    
    # Use the first available player
    player_name = player_names[0]
    
    # Reset player state
    activemonitor_server.reset_player_state(player_name)
    
    # Test sequence of state changes that might trigger active monitor
    states = ["playing", "paused", "playing", "stopped"]
    
    for state in states:
        event = {"type": "state_changed", "state": state}
        response = activemonitor_server.send_player_event(player_name, event)
        assert response is not None
        
        # Allow time for event processing and active monitor logic
        time.sleep(0.1)
        
        # Check that the state changed
        updated_players = activemonitor_server.get_players()
        assert updated_players[player_name]['state'] == state

def test_activemonitor_metadata_events(activemonitor_server):
    """Test metadata events with active monitor"""
    # Get available players
    players = activemonitor_server.get_players()
    player_names = list(players.keys())
    
    if len(player_names) == 0:
        pytest.skip("No players available for testing")
    
    # Use the first available player
    player_name = player_names[0]
    
    # Reset player state
    activemonitor_server.reset_player_state(player_name)
    
    # Test metadata event - active monitor might track this
    event = {
        "type": "metadata_changed",
        "metadata": {
            "title": "Active Monitor Test",
            "artist": "Test Artist",
            "album": "Test Album",
            "duration": 180.0
        }
    }
    
    response = activemonitor_server.send_player_event(player_name, event)
    assert response is not None
    
    # Allow time for event processing
    time.sleep(0.1)
    
    # Check that metadata was processed
    now_playing = activemonitor_server.get_now_playing()
    if 'song' in now_playing and now_playing['song']:
        song = now_playing['song']
        assert song['title'] == 'Active Monitor Test'

def test_activemonitor_rapid_events(activemonitor_server):
    """Test rapid event sequence with active monitor"""
    # Get available players
    players = activemonitor_server.get_players()
    player_names = list(players.keys())
    
    if len(player_names) == 0:
        pytest.skip("No players available for testing")
    
    # Use the first available player
    player_name = player_names[0]
    
    # Reset player state
    activemonitor_server.reset_player_state(player_name)
    
    # Send rapid sequence of events to test active monitor handling
    events = [
        {"type": "state_changed", "state": "playing"},
        {"type": "position_changed", "position": 10.0},
        {"type": "position_changed", "position": 11.0},
        {"type": "position_changed", "position": 12.0},
        {"type": "state_changed", "state": "paused"},
        {"type": "state_changed", "state": "playing"},
        {"type": "position_changed", "position": 15.0},
    ]
    
    for event in events:
        response = activemonitor_server.send_player_event(player_name, event)
        assert response is not None
        time.sleep(0.02)  # Very small delay between events
    
    # Allow time for all events to be processed
    time.sleep(0.2)
    
    # Check final state
    updated_players = activemonitor_server.get_players()
    assert updated_players[player_name]['state'] == 'playing'
    # Position might not be present in all cases
    if 'position' in updated_players[player_name]:
        assert updated_players[player_name]['position'] == 15.0

def test_activemonitor_plugin_resilience(activemonitor_server):
    """Test that active monitor plugin doesn't break normal operation"""
    # Get available players
    players = activemonitor_server.get_players()
    player_names = list(players.keys())
    
    if len(player_names) == 0:
        pytest.skip("No players available for testing")
    
    # Use the first available player
    player_name = player_names[0]
    
    # Reset player state
    activemonitor_server.reset_player_state(player_name)
    
    # Test that normal operations still work with active monitor running
    # This is a comprehensive test to ensure the plugin doesn't interfere
    
    # Set up initial state
    setup_events = [
        {"type": "state_changed", "state": "playing"},
        {"type": "shuffle_changed", "enabled": True},
        {"type": "loop_mode_changed", "mode": "one"},
        {"type": "metadata_changed", "metadata": {
            "title": "Resilience Test",
            "artist": "Test Artist",
            "album": "Test Album",
            "duration": 240.0
        }},
        {"type": "position_changed", "position": 30.0},
    ]
    
    for event in setup_events:
        response = activemonitor_server.send_player_event(player_name, event)
        assert response is not None
        time.sleep(0.05)
    
    # Allow time for all events to be processed
    time.sleep(0.2)
    
    # Verify final state
    updated_players = activemonitor_server.get_players()
    player = updated_players[player_name]
    assert player['state'] == 'playing'
    assert player['shuffle'] is True
    assert player['loop_mode'] == 'song'
    # Position might not be present in all cases
    if 'position' in player:
        assert player['position'] == 30.0
    
    # Check metadata
    now_playing = activemonitor_server.get_now_playing()
    if 'song' in now_playing and now_playing['song']:
        song = now_playing['song']
        assert song['title'] == 'Resilience Test'

def test_activemonitor_player_switching(activemonitor_server):
    """Test switching between generic and librespot players with active monitor"""
    players = activemonitor_server.get_players()
    
    # Ensure we have both players
    assert 'test_player' in players
    assert 'librespot' in players
    
    generic_player = 'test_player'
    librespot_player = 'librespot'
    
    # Reset both players
    activemonitor_server.reset_player_state(generic_player)
    activemonitor_server.reset_player_state(librespot_player)
    
    # Test 1: Start playing on generic player
    print("Testing generic player events...")
    generic_event = {
        "type": "metadata_changed",
        "metadata": {
            "title": "Generic Song",
            "artist": "Generic Artist",
            "album": "Generic Album",
            "duration": 180.0
        },
        "state": "playing"
    }
    
    response = activemonitor_server.send_player_event(generic_player, generic_event)
    assert response is not None
    time.sleep(0.2)
    
    # Check state
    updated_players = activemonitor_server.get_players()
    assert updated_players[generic_player]['state'] == 'playing'
    
    # Check now playing shows the generic player's song
    now_playing = activemonitor_server.get_now_playing()
    if 'song' in now_playing and now_playing['song']:
        song = now_playing['song']
        assert song['title'] == 'Generic Song'
        assert song['artist'] == 'Generic Artist'
    
    # Test 2: Switch to librespot player using environment variables
    print("Testing librespot player events...")
    librespot_env = {
        "TRACK_ID": "spotify:track:test123",
        "ARTIST_NAME": "Librespot Artist",
        "ALBUM_NAME": "Librespot Album",
        "TRACK_NAME": "Librespot Song",
        "DURATION_MS": "200000"
    }
    
    response = activemonitor_server.send_librespot_event(librespot_player, "changed", librespot_env)
    assert response is not None
    time.sleep(0.2)
    
    # Check that librespot player is now active
    updated_players = activemonitor_server.get_players()
    # Note: Librespot events might not directly set state to 'playing' without a separate state event
    
    # Test 3: Switch back to generic player
    print("Testing switch back to generic player...")
    generic_event2 = {
        "type": "metadata_changed",
        "metadata": {
            "title": "Generic Song 2",
            "artist": "Generic Artist 2",
            "album": "Generic Album 2",
            "duration": 220.0
        },
        "state": "playing"
    }
    
    response = activemonitor_server.send_player_event(generic_player, generic_event2)
    assert response is not None
    time.sleep(0.2)
    
    # Check state
    updated_players = activemonitor_server.get_players()
    assert updated_players[generic_player]['state'] == 'playing'
    
    # Check now playing shows the new generic player's song
    now_playing = activemonitor_server.get_now_playing()
    if 'song' in now_playing and now_playing['song']:
        song = now_playing['song']
        assert song['title'] == 'Generic Song 2'
        assert song['artist'] == 'Generic Artist 2'
    
    print("Player switching test completed successfully")

def test_activemonitor_concurrent_player_events(activemonitor_server):
    """Test concurrent events from both players with active monitor"""
    players = activemonitor_server.get_players()
    
    # Ensure we have both players
    assert 'test_player' in players
    assert 'librespot' in players
    
    generic_player = 'test_player'
    librespot_player = 'librespot'
    
    # Reset both players
    activemonitor_server.reset_player_state(generic_player)
    activemonitor_server.reset_player_state(librespot_player)
    
    # Send events to both players rapidly
    print("Testing concurrent player events...")
    
    # Generic player events
    generic_events = [
        {"type": "state_changed", "state": "playing"},
        {"type": "position_changed", "position": 10.0},
        {"type": "position_changed", "position": 15.0},
    ]
    
    # Librespot player events
    librespot_envs = [
        {"PLAYER_EVENT": "changed", "TRACK_NAME": "Librespot Track 1"},
        {"PLAYER_EVENT": "changed", "TRACK_NAME": "Librespot Track 2"},
    ]
    
    # Send events alternately
    for i in range(max(len(generic_events), len(librespot_envs))):
        if i < len(generic_events):
            response = activemonitor_server.send_player_event(generic_player, generic_events[i])
            assert response is not None
            time.sleep(0.05)
        
        if i < len(librespot_envs):
            response = activemonitor_server.send_librespot_event(librespot_player, "changed", librespot_envs[i])
            assert response is not None
            time.sleep(0.05)
    
    # Allow time for all events to be processed
    time.sleep(0.3)
    
    # Check that both players processed their events
    updated_players = activemonitor_server.get_players()
    assert updated_players[generic_player]['state'] == 'playing'
    # Position might not be present in all cases
    if 'position' in updated_players[generic_player]:
        assert updated_players[generic_player]['position'] == 15.0
    
    print("Concurrent player events test completed successfully")
