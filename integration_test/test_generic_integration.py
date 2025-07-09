#!/usr/bin/env python3
"""
Generic integration tests for AudioControl system
"""

import pytest
import json
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
    # Check for the new supports_api_events field
    assert 'supports_api_events' in player
    assert isinstance(player['supports_api_events'], bool)
    print(f"Player supports API events: {player['supports_api_events']}")
    # API response may not include capabilities directly

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
    response = generic_server.send_generic_player_event("test_player", event)
    assert response is not None
    
    # Check if the tool call was successful
    assert response.get("success", False), f"Tool call failed: {response.get('message', 'Unknown error')}"
    
    # Small delay to allow state to propagate
    time.sleep(1.0)  # Increase delay to ensure event propagation
    
    # Get the current player state from now-playing endpoint
    now_playing = generic_server.get_now_playing()
    
    # Print debug info
    print(f"Now playing response: {json.dumps(now_playing, indent=2)}")
    
    # Check if the state was updated correctly
    state_updated = False
    
    if 'player' in now_playing and now_playing['player'].get('id') == 'test_player':
        if now_playing['player']['state'].lower() == 'playing':
            state_updated = True
    
    # If not updated via now-playing, try direct player lookup
    if not state_updated:
        players = generic_server.get_players()
        if 'test_player' in players and players['test_player'].get('state', '').lower() == 'playing':
            state_updated = True
        
    assert state_updated, f"Player state was not updated to 'playing'. Current state: {now_playing}"

def test_player_shuffle_events(generic_server):
    """Test sending player shuffle events"""
    # Reset player state first
    generic_server.reset_player_state()
    
    # Test shuffle enable event
    event = {"type": "shuffle_changed", "enabled": True}
    response = generic_server.send_generic_player_event("test_player", event)
    assert response is not None
    
    # Small delay to allow state to propagate
    time.sleep(1.0)
    
    # Verify the shuffle state changed if available
    players = generic_server.get_players()
    assert 'shuffle' in players['test_player'], "Player does not expose 'shuffle' property in API response"
    assert players['test_player']['shuffle'] is True, f"Expected shuffle to be True, got {players['test_player']['shuffle']}"

def test_player_loop_mode_events(generic_server):
    """Test sending player loop mode events"""
    # Reset player state first
    generic_server.reset_player_state()
    
    # Test loop mode change event
    event = {"type": "loop_mode_changed", "mode": "playlist"}
    response = generic_server.send_generic_player_event("test_player", event)
    assert response is not None
    
    # Small delay to allow state to propagate
    time.sleep(1.0)
    
    # Verify the loop mode changed if available
    players = generic_server.get_players()
    assert 'loop_mode' in players['test_player'], "Player does not expose 'loop_mode' property in API response"
    assert players['test_player']['loop_mode'] == 'playlist', f"Expected loop_mode to be 'playlist', got {players['test_player']['loop_mode']}"

def test_player_position_events(generic_server):
    """Test sending player position events"""
    # Reset player state first
    generic_server.reset_player_state()
    
    # Test position change event
    event = {"type": "position_changed", "position": 42.5}
    response = generic_server.send_generic_player_event("test_player", event)
    assert response is not None
    
    # Small delay to allow state to propagate
    time.sleep(1.0)
    
    # Check the current position
    # Position is often not exposed directly via the player endpoint
    # but may be available in the now_playing response
    now_playing = generic_server.get_now_playing()
    
    position_checked = False
    
    # Check now_playing response
    if 'song' in now_playing and 'position' in now_playing['song']:
        position = now_playing['song']['position']
        assert position == 42.5, f"Expected position 42.5, got {position}"
        position_checked = True
    
    # Also check the player object
    players = generic_server.get_players()
    if 'position' in players['test_player']:
        position = players['test_player']['position']
        assert position == 42.5, f"Expected position 42.5, got {position}"
        position_checked = True
        
    assert position_checked, "Position is not exposed in API responses"

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
    response = generic_server.send_generic_player_event("test_player", event)
    assert response is not None
    
    # Small delay to allow state to propagate
    time.sleep(1.0)
    
    # Verify the metadata was set
    now_playing = generic_server.get_now_playing()
    assert 'song' in now_playing and now_playing['song'], "Song data not available in now_playing response"
    
    song = now_playing['song']
    assert song.get('title') == 'Test Song', f"Expected title 'Test Song', got {song.get('title', 'N/A')}"
    assert song.get('artist') == 'Test Artist', f"Expected artist 'Test Artist', got {song.get('artist', 'N/A')}"
    assert song.get('album') == 'Test Album', f"Expected album 'Test Album', got {song.get('album', 'N/A')}"

def test_multiple_events_sequence(generic_server):
    """Test sending multiple events in sequence"""
    # Reset player state first
    generic_server.reset_player_state()
    
    # Send a sequence of events
    events = [
        {"type": "state_changed", "state": "playing"},
        {"type": "shuffle_changed", "enabled": True},
        {"type": "loop_mode_changed", "mode": "song"},
        {"type": "position_changed", "position": 30.0},
        {"type": "metadata_changed", "metadata": {
            "title": "Sequence Test",
            "artist": "Test Artist",
            "album": "Test Album",
            "duration": 200.0
        }}
    ]
    
    for event in events:
        response = generic_server.send_generic_player_event("test_player", event)
        assert response is not None
        time.sleep(0.1)  # Small delay between events
    
    # Wait for all events to be processed
    time.sleep(1.0)
    
    # Verify final state - all properties must be updated correctly
    players = generic_server.get_players()
    player = players['test_player']
    
    # Check state
    assert player.get('state') == 'playing', f"Expected state 'playing', got {player.get('state', 'N/A')}"
    
    # Check shuffle
    assert 'shuffle' in player, "Shuffle property not exposed in player API"
    assert player['shuffle'] is True, f"Expected shuffle True, got {player['shuffle']}"
    
    # Check loop mode
    assert 'loop_mode' in player, "Loop mode property not exposed in player API"
    assert player['loop_mode'] == 'song', f"Expected loop_mode 'song', got {player['loop_mode']}"
        
    # Check position
    assert 'position' in player, "Position property not exposed in player API"
    assert player['position'] == 30.0, f"Expected position 30.0, got {player['position']}"
    
    # Check metadata
    now_playing = generic_server.get_now_playing()
    assert 'song' in now_playing and now_playing['song'], "Song data not available in now_playing response"
    song = now_playing['song']
    assert song['title'] == 'Sequence Test', f"Expected title 'Sequence Test', got {song.get('title', 'N/A')}"

def test_player_api_event_support(generic_server):
    """Check if the generic player supports API events
    
    This test doesn't fail if API events aren't supported, it just reports the status.
    This helps diagnose why the websocket tests might be skipped.
    """
    # Get player configuration
    players = generic_server.get_players()
    assert 'test_player' in players, "Test player not found in response"
    
    # Get the test player
    test_player = players['test_player']
    assert test_player is not None, "Test player not found in players list"
    print(f"Player configuration: {test_player}")
    
    # Check if the player reports supports_api_events
    assert 'supports_api_events' in test_player, "supports_api_events field missing from API response"
    assert test_player.get('supports_api_events', False), "Player reports API events are NOT supported"
        
    # Check capabilities
    capabilities = test_player.get('capabilities', [])
    print(f"Player capabilities: {capabilities}")
    
    # Let's try a simple event and see if it works
    print("\nTrying a simple state change event...")
    event = {"type": "state_changed", "state": "playing"}
    response = generic_server.send_generic_player_event(test_player['id'], event)
    print(f"API Response: {response}")
    
    assert response.get('success') != False, f"API event was not processed: {response.get('message', 'Unknown error')}"
    print("SUCCESS: API event was processed successfully")
