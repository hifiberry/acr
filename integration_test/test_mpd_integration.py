#!/usr/bin/env python3
"""
MPD integration tests for AudioControl system
"""

import pytest
import time

def test_mpd_player_initialization(mpd_server):
    """Test that MPD player is initialized correctly"""
    players = mpd_server.get_players()
    assert isinstance(players, dict)
    
    # Check if we have any players (might be generic or mpd)
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

def test_mpd_server_responds(mpd_server):
    """Test that the server responds to basic requests"""
    response = mpd_server.api_request('GET', '/api/version')
    assert 'version' in response
    assert response['version'] is not None
    
    # Test now playing endpoint
    now_playing = mpd_server.get_now_playing()
    assert isinstance(now_playing, dict)

def test_mpd_player_events(mpd_server):
    """Test that MPD player events work"""
    # Get available players
    players = mpd_server.get_players()
    player_names = list(players.keys())
    
    if len(player_names) == 0:
        pytest.skip("No players available for testing")
    
    # Use the first available player
    player_name = player_names[0]
    
    # Reset player state
    mpd_server.reset_player_state(player_name)
    
    # Test basic state change event
    event = {"type": "state_changed", "state": "playing"}
    response = mpd_server.send_player_event(player_name, event)
    assert response is not None
    
    # Allow time for event processing
    time.sleep(0.1)
    
    # Check that the state changed
    updated_players = mpd_server.get_players()
    assert updated_players[player_name]['state'] == 'playing'

def test_mpd_metadata_events(mpd_server):
    """Test that MPD metadata events work"""
    # Get available players
    players = mpd_server.get_players()
    player_names = list(players.keys())
    
    if len(player_names) == 0:
        pytest.skip("No players available for testing")
    
    # Use the first available player
    player_name = player_names[0]
    
    # Reset player state
    mpd_server.reset_player_state(player_name)
    
    # Test metadata event typical for MPD
    event = {
        "type": "metadata_changed",
        "metadata": {
            "title": "Test MPD Track",
            "artist": "Test MPD Artist",
            "album": "Test MPD Album",
            "duration": 210.0,
            "track_number": 5,
            "genre": "Electronic",
            "file": "/music/test_track.flac"
        }
    }
    
    response = mpd_server.send_player_event(player_name, event)
    assert response is not None
    
    # Allow time for event processing
    time.sleep(0.1)
    
    # Check that metadata was processed
    now_playing = mpd_server.get_now_playing()
    if 'song' in now_playing and now_playing['song']:
        song = now_playing['song']
        assert song['title'] == 'Test MPD Track'
        assert song['artist'] == 'Test MPD Artist'
        assert song['album'] == 'Test MPD Album'
        assert song['duration'] == 210.0

def test_mpd_playback_control(mpd_server):
    """Test MPD playback control events"""
    # Get available players
    players = mpd_server.get_players()
    player_names = list(players.keys())
    
    if len(player_names) == 0:
        pytest.skip("No players available for testing")
    
    # Use the first available player
    player_name = player_names[0]
    
    # Reset player state
    mpd_server.reset_player_state(player_name)
    
    # Test play/pause/stop sequence
    states = ["playing", "paused", "stopped"]
    
    for state in states:
        event = {"type": "state_changed", "state": state}
        response = mpd_server.send_player_event(player_name, event)
        assert response is not None
        
        # Allow time for event processing
        time.sleep(0.05)
        
        # Check state
        updated_players = mpd_server.get_players()
        assert updated_players[player_name]['state'] == state

def test_mpd_queue_management(mpd_server):
    """Test MPD queue-related events"""
    # Get available players
    players = mpd_server.get_players()
    player_names = list(players.keys())
    
    if len(player_names) == 0:
        pytest.skip("No players available for testing")
    
    # Use the first available player
    player_name = player_names[0]
    
    # Reset player state
    mpd_server.reset_player_state(player_name)
    
    # Test queue position events (typical for MPD)
    event = {"type": "position_changed", "position": 45.0}
    response = mpd_server.send_player_event(player_name, event)
    assert response is not None
    
    # Allow time for event processing
    time.sleep(0.05)
    
    # Check position
    updated_players = mpd_server.get_players()
    assert updated_players[player_name]['position'] == 45.0

def test_mpd_repeat_and_shuffle(mpd_server):
    """Test MPD repeat and shuffle modes"""
    # Get available players
    players = mpd_server.get_players()
    player_names = list(players.keys())
    
    if len(player_names) == 0:
        pytest.skip("No players available for testing")
    
    # Use the first available player
    player_name = player_names[0]
    
    # Reset player state
    mpd_server.reset_player_state(player_name)
    
    # Test shuffle mode
    shuffle_event = {"type": "shuffle_changed", "enabled": True}
    response = mpd_server.send_player_event(player_name, shuffle_event)
    assert response is not None
    
    time.sleep(0.05)
    
    # Check shuffle state
    updated_players = mpd_server.get_players()
    assert updated_players[player_name]['shuffle'] is True
    
    # Test repeat mode
    repeat_event = {"type": "loop_mode_changed", "mode": "all"}
    response = mpd_server.send_player_event(player_name, repeat_event)
    assert response is not None
    
    time.sleep(0.05)
    
    # Check repeat state
    updated_players = mpd_server.get_players()
    assert updated_players[player_name]['loop_mode'] == 'all'

def test_mpd_file_metadata(mpd_server):
    """Test MPD file-based metadata"""
    # Get available players
    players = mpd_server.get_players()
    player_names = list(players.keys())
    
    if len(player_names) == 0:
        pytest.skip("No players available for testing")
    
    # Use the first available player
    player_name = player_names[0]
    
    # Reset player state
    mpd_server.reset_player_state(player_name)
    
    # Test file-based metadata with extended info
    event = {
        "type": "metadata_changed",
        "metadata": {
            "title": "Local File Test",
            "artist": "Local Artist",
            "album": "Local Album",
            "albumartist": "Local Album Artist",
            "duration": 187.5,
            "track_number": 2,
            "disc_number": 1,
            "genre": "Jazz",
            "date": "2023",
            "file": "/music/jazz/local_file.mp3",
            "format": "MP3"
        }
    }
    
    response = mpd_server.send_player_event(player_name, event)
    assert response is not None
    
    # Allow time for event processing
    time.sleep(0.1)
    
    # Check that file metadata was processed
    now_playing = mpd_server.get_now_playing()
    if 'song' in now_playing and now_playing['song']:
        song = now_playing['song']
        assert song['title'] == 'Local File Test'
        assert song['artist'] == 'Local Artist'
        assert song['album'] == 'Local Album'
