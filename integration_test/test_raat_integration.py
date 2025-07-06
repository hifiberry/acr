#!/usr/bin/env python3
"""
RAAT integration tests for AudioControl system
"""

import pytest
import time

def test_raat_player_initialization(raat_server):
    """Test that RAAT player is initialized correctly"""
    players = raat_server.get_players()
    assert isinstance(players, dict)
    
    # Check if we have any players (might be generic or raat)
    assert len(players) > 0
    
    # The actual player name depends on configuration
    player_names = list(players.keys())
    assert len(player_names) > 0
    
    # Check the first player has expected structure
    first_player = players[player_names[0]]
    assert 'name' in first_player
    assert 'display_name' in first_player
    assert 'state' in first_player
    assert 'capabilities' in first_player

def test_raat_server_responds(raat_server):
    """Test that the server responds to basic requests"""
    response = raat_server.api_request('GET', '/api/version')
    assert 'version' in response
    assert response['version'] is not None
    
    # Test now playing endpoint
    now_playing = raat_server.get_now_playing()
    assert isinstance(now_playing, dict)

def test_raat_player_events(raat_server):
    """Test that RAAT player events work"""
    # Get available players
    players = raat_server.get_players()
    player_names = list(players.keys())
    
    if len(player_names) == 0:
        pytest.skip("No players available for testing")
    
    # Use the first available player
    player_name = player_names[0]
    
    # Reset player state
    raat_server.reset_player_state(player_name)
    
    # Test basic state change event
    event = {"type": "state_changed", "state": "playing"}
    response = raat_server.send_player_event(player_name, event)
    assert response is not None
    
    # Allow time for event processing
    time.sleep(0.1)
    
    # Check that the state changed
    updated_players = raat_server.get_players()
    assert updated_players[player_name]['state'] == 'playing'

def test_raat_metadata_events(raat_server):
    """Test that RAAT metadata events work"""
    # Get available players
    players = raat_server.get_players()
    player_names = list(players.keys())
    
    if len(player_names) == 0:
        pytest.skip("No players available for testing")
    
    # Use the first available player
    player_name = player_names[0]
    
    # Reset player state
    raat_server.reset_player_state(player_name)
    
    # Test metadata event typical for RAAT
    event = {
        "type": "metadata_changed",
        "metadata": {
            "title": "Test RAAT Track",
            "artist": "Test RAAT Artist",
            "album": "Test RAAT Album",
            "duration": 195.0,
            "track_number": 3,
            "sample_rate": 44100,
            "bit_depth": 16
        }
    }
    
    response = raat_server.send_player_event(player_name, event)
    assert response is not None
    
    # Allow time for event processing
    time.sleep(0.1)
    
    # Check that metadata was processed
    now_playing = raat_server.get_now_playing()
    if 'song' in now_playing and now_playing['song']:
        song = now_playing['song']
        assert song['title'] == 'Test RAAT Track'
        assert song['artist'] == 'Test RAAT Artist'
        assert song['album'] == 'Test RAAT Album'
        assert song['duration'] == 195.0

def test_raat_playback_control(raat_server):
    """Test RAAT playback control events"""
    # Get available players
    players = raat_server.get_players()
    player_names = list(players.keys())
    
    if len(player_names) == 0:
        pytest.skip("No players available for testing")
    
    # Use the first available player
    player_name = player_names[0]
    
    # Reset player state
    raat_server.reset_player_state(player_name)
    
    # Test play/pause/stop sequence
    states = ["playing", "paused", "stopped"]
    
    for state in states:
        event = {"type": "state_changed", "state": state}
        response = raat_server.send_player_event(player_name, event)
        assert response is not None
        
        # Allow time for event processing
        time.sleep(0.05)
        
        # Check state
        updated_players = raat_server.get_players()
        assert updated_players[player_name]['state'] == state

def test_raat_audio_format_events(raat_server):
    """Test RAAT audio format specific events"""
    # Get available players
    players = raat_server.get_players()
    player_names = list(players.keys())
    
    if len(player_names) == 0:
        pytest.skip("No players available for testing")
    
    # Use the first available player
    player_name = player_names[0]
    
    # Reset player state
    raat_server.reset_player_state(player_name)
    
    # Test high-resolution audio metadata (typical for RAAT)
    event = {
        "type": "metadata_changed",
        "metadata": {
            "title": "High-Res Test",
            "artist": "Audiophile Artist",
            "album": "Reference Album",
            "duration": 300.0,
            "sample_rate": 192000,
            "bit_depth": 24,
            "channels": 2
        }
    }
    
    response = raat_server.send_player_event(player_name, event)
    assert response is not None
    
    # Allow time for event processing
    time.sleep(0.1)
    
    # Check that high-res metadata was processed
    now_playing = raat_server.get_now_playing()
    if 'song' in now_playing and now_playing['song']:
        song = now_playing['song']
        assert song['title'] == 'High-Res Test'
        assert song['artist'] == 'Audiophile Artist'