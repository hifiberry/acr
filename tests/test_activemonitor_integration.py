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
    assert player['loop_mode'] == 'one'
    assert player['position'] == 30.0
    
    # Check metadata
    now_playing = activemonitor_server.get_now_playing()
    if 'song' in now_playing and now_playing['song']:
        song = now_playing['song']
        assert song['title'] == 'Resilience Test'
