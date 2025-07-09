#!/usr/bin/env python3
"""
MPD integration tests for AudioControl system
"""

import pytest

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
    
    # Check for the actual fields that exist
    assert 'id' in first_player
    assert 'name' in first_player
    assert 'state' in first_player
    assert 'is_active' in first_player
    assert 'has_library' in first_player
    assert 'supports_api_events' in first_player
    assert 'last_seen' in first_player
    assert 'shuffle' in first_player
    assert 'loop_mode' in first_player

def test_mpd_server_responds(mpd_server):
    """Test that the server responds to basic requests"""
    response = mpd_server.api_request('GET', '/api/version')
    assert 'version' in response
    assert response['version'] is not None
    
    # Test now playing endpoint
    now_playing = mpd_server.get_now_playing()
    assert isinstance(now_playing, dict)

def test_mpd_players_listed(mpd_server):
    """Test that MPD players are properly listed in the API"""
    # Get available players using raw format
    players_response = mpd_server.get_players_raw()
    assert isinstance(players_response, dict)
    assert "players" in players_response
    
    players = players_response["players"]
    assert isinstance(players, list)
    assert len(players) > 0
    
    # Check each player has the expected structure
    for player in players:
        assert 'id' in player
        assert 'name' in player
        assert 'state' in player
        assert 'is_active' in player
        assert 'has_library' in player
        assert 'supports_api_events' in player
        assert 'last_seen' in player
        assert 'shuffle' in player
        assert 'loop_mode' in player
        
        # Log player info for debugging
        print(f"Found player: {player['id']} - {player['name']}")

def test_mpd_player_capabilities(mpd_server):
    """Test that MPD player capabilities are correctly exposed"""
    players = mpd_server.get_players()
    player_names = list(players.keys())
    
    if len(player_names) == 0:
        pytest.skip("No players available for testing")
    
    # Check the first player's basic properties
    first_player = players[player_names[0]]
    
    # Check basic capabilities
    assert isinstance(first_player.get('has_library'), bool)
    assert isinstance(first_player.get('supports_api_events'), bool)
    assert isinstance(first_player.get('is_active'), bool)
    
    # Log capabilities for debugging
    print(f"Player has_library: {first_player.get('has_library')}")
    print(f"Player supports_api_events: {first_player.get('supports_api_events')}")
    print(f"Player is_active: {first_player.get('is_active')}")

def test_mpd_player_state_structure(mpd_server):
    """Test that MPD player state has the expected structure"""
    players = mpd_server.get_players()
    player_names = list(players.keys())
    
    if len(player_names) == 0:
        pytest.skip("No players available for testing")
    
    # Check the first player's state structure
    first_player = players[player_names[0]]
    
    # Verify basic state fields exist
    assert 'state' in first_player
    assert 'name' in first_player
    
    # State should be a valid value
    valid_states = ['playing', 'paused', 'stopped', 'unknown']
    assert first_player['state'] in valid_states
    
    # Check shuffle and loop_mode values
    assert isinstance(first_player.get('shuffle'), bool)
    valid_loop_modes = ['no', 'all', 'one', 'track', 'playlist']
    assert first_player.get('loop_mode') in valid_loop_modes
    
    # Log player state for debugging
    print(f"Player state: {first_player['state']}")
    print(f"Player name: {first_player['name']}")
    print(f"Player shuffle: {first_player.get('shuffle')}")
    print(f"Player loop_mode: {first_player.get('loop_mode')}")

def test_mpd_now_playing_structure(mpd_server):
    """Test that the now playing endpoint returns proper structure"""
    now_playing = mpd_server.get_now_playing()
    assert isinstance(now_playing, dict)
    
    # Check for expected fields in now playing response
    # Note: These might be None or empty depending on player state
    expected_fields = ['state', 'player', 'song', 'position', 'shuffle', 'loop_mode']
    
    for field in expected_fields:
        # Field should exist in the response
        assert field in now_playing
    
    # Log now playing structure for debugging
    print(f"Now playing structure: {list(now_playing.keys())}")
    if now_playing.get('player'):
        print(f"Active player: {now_playing['player']}")
    if now_playing.get('song'):
        print(f"Current song: {now_playing['song']}")
    if now_playing.get('state'):
        print(f"Playback state: {now_playing['state']}")
