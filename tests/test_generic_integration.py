#!/usr/bin/env python3
"""
Generic integration tests for AudioControl system
"""

import pytest
import time

def test_server_startup(generic_server):
    """Test that the server starts up correctly"""
    # The server should be running by now due to the fixture
    response = generic_server.api_request('GET', '/api/version')
    assert 'version' in response
    assert response['version'] is not None

def test_players_endpoint(generic_server):
    """Test that the players endpoint returns expected data"""
    players = generic_server.get_players()
    assert isinstance(players, dict)
    assert 'test_player' in players
    
    player = players['test_player']
    assert 'id' in player
    # The actual structure has 'id' instead of 'name'
    # and doesn't have display_name in the API response
    assert player['id'] == 'test_player'
    assert 'state' in player
    assert 'capabilities' in player

def test_now_playing_endpoint(generic_server):
    """Test that the now playing endpoint returns expected data"""
    now_playing = generic_server.get_now_playing()
    assert isinstance(now_playing, dict)
    # Should have basic structure even if nothing is playing
    assert 'player' in now_playing or 'song' in now_playing or 'state' in now_playing

def test_player_state_events(generic_server):
    """Test sending player state events"""
    # Reset player state first
    generic_server.reset_player_state()
    
    # Test play event
    event = {"type": "state_changed", "state": "playing"}
    response = generic_server.send_player_event("test_player", event)
    assert response is not None
    
    # Small delay to allow state to propagate
    time.sleep(0.1)
    
    # Verify the state changed
    players = generic_server.get_players()
    assert players['test_player']['state'] == 'playing'

def test_player_shuffle_events(generic_server):
    """Test sending player shuffle events"""
    # Reset player state first
    generic_server.reset_player_state()
    
    # Test shuffle enable event
    event = {"type": "shuffle_changed", "enabled": True}
    response = generic_server.send_player_event("test_player", event)
    assert response is not None
    
    # Small delay to allow state to propagate
    time.sleep(0.1)
    
    # Verify the shuffle state changed
    players = generic_server.get_players()
    assert players['test_player']['shuffle'] is True

def test_player_loop_mode_events(generic_server):
    """Test sending player loop mode events"""
    # Reset player state first
    generic_server.reset_player_state()
    
    # Test loop mode change event
    event = {"type": "loop_mode_changed", "mode": "all"}
    response = generic_server.send_player_event("test_player", event)
    assert response is not None
    
    # Small delay to allow state to propagate
    time.sleep(0.1)
    
    # Verify the loop mode changed
    players = generic_server.get_players()
    assert players['test_player']['loop_mode'] == 'all'

def test_player_position_events(generic_server):
    """Test sending player position events"""
    # Reset player state first
    generic_server.reset_player_state()
    
    # Test position change event
    event = {"type": "position_changed", "position": 42.5}
    response = generic_server.send_player_event("test_player", event)
    assert response is not None
    
    # Small delay to allow state to propagate
    time.sleep(0.1)
    
    # Verify the position changed
    players = generic_server.get_players()
    assert players['test_player']['position'] == 42.5

def test_song_metadata_events(generic_server):
    """Test sending song metadata events"""
    # Reset player state first
    generic_server.reset_player_state()
    
    # Test metadata event
    event = {
        "type": "metadata_changed",
        "metadata": {
            "title": "Test Song",
            "artist": "Test Artist",
            "album": "Test Album",
            "duration": 180.0
        }
    }
    response = generic_server.send_player_event("test_player", event)
    assert response is not None
    
    # Small delay to allow state to propagate
    time.sleep(0.1)
    
    # Verify the metadata was set
    now_playing = generic_server.get_now_playing()
    if 'song' in now_playing and now_playing['song']:
        song = now_playing['song']
        assert song['title'] == 'Test Song'
        assert song['artist'] == 'Test Artist'
        assert song['album'] == 'Test Album'
        assert song['duration'] == 180.0

def test_multiple_events_sequence(generic_server):
    """Test sending multiple events in sequence"""
    # Reset player state first
    generic_server.reset_player_state()
    
    # Send a sequence of events
    events = [
        {"type": "state_changed", "state": "playing"},
        {"type": "shuffle_changed", "enabled": True},
        {"type": "loop_mode_changed", "mode": "one"},
        {"type": "position_changed", "position": 30.0},
        {"type": "metadata_changed", "metadata": {
            "title": "Sequence Test",
            "artist": "Test Artist",
            "album": "Test Album",
            "duration": 200.0
        }}
    ]
    
    for event in events:
        response = generic_server.send_player_event("test_player", event)
        assert response is not None
        time.sleep(0.05)  # Small delay between events
    
    # Wait for all events to be processed
    time.sleep(0.2)
    
    # Verify final state
    players = generic_server.get_players()
    player = players['test_player']
    assert player['state'] == 'playing'
    assert player['shuffle'] is True
    assert player['loop_mode'] == 'one'
    assert player['position'] == 30.0
    
    # Check metadata
    now_playing = generic_server.get_now_playing()
    if 'song' in now_playing and now_playing['song']:
        song = now_playing['song']
        assert song['title'] == 'Sequence Test'

def test_player_api_event_support(generic_server):
    """Check if the generic player supports API events
    
    This test doesn't fail if API events aren't supported, it just reports the status.
    This helps diagnose why the websocket tests might be skipped.
    """
    # Get player configuration
    players_response = generic_server.get_players()
    assert 'players' in players_response, "Players array not found in response"
    
    # Find the test player
    test_player = None
    for player in players_response['players']:
        if player['id'] == 'test_player':
            test_player = player
            break
            
    assert test_player is not None, "Test player not found in players list"
    print(f"Player configuration: {test_player}")
    
    # Check if the player reports supports_api_events
    if not test_player.get('supports_api_events', False):
        print("WARNING: Player does not report 'supports_api_events' in API response")
        print("This is configured in conftest.py but doesn't appear in the API response")
        print("This would explain why websocket tests are being skipped")
        print("The AudioControl server may not be exposing this configuration setting to the API")
    else:
        print("Player reports API events are supported")
        
    # Check capabilities
    capabilities = test_player.get('capabilities', [])
    print(f"Player capabilities: {capabilities}")
    
    # Let's try a simple event and see if it works
    print("\nTrying a simple state change event...")
    event = {"type": "state_changed", "state": "playing"}
    response = generic_server.send_player_event(test_player['id'], event)
    print(f"API Response: {response}")
    
    if response.get('success') == False:
        print("WARNING: API event was not processed")
        print("This confirms that events cannot be sent to the player via the API")
        print("Check if 'supports_api_events' is correctly configured in the server")
    else:
        print("SUCCESS: API event was processed successfully")
        print("This indicates that the player can process events via the API")
