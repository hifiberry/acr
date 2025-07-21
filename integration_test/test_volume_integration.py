#!/usr/bin/env python3
"""
Volume Control API integration tests for AudioControl system
These tests use the dummy volume control to test all volume endpoints and event handling
"""

import pytest
import json
import time
import requests
import websocket
import threading
from typing import Dict, List, Any, Optional


class VolumeTestHelper:
    """Helper class for volume control testing"""
    
    def __init__(self, server):
        self.server = server
        self.websocket_events = []
        self.websocket_connection = None
        self.websocket_thread = None
        
    def connect_websocket(self):
        """Connect to WebSocket for receiving events"""
        def on_message(ws, message):
            try:
                event = json.loads(message)
                self.websocket_events.append(event)
                print(f"WebSocket received: {event}")
            except json.JSONDecodeError:
                print(f"Failed to parse WebSocket message: {message}")
        
        def on_error(ws, error):
            print(f"WebSocket error: {error}")
        
        def on_close(ws, close_status_code, close_msg):
            print("WebSocket connection closed")
        
        def on_open(ws):
            print("WebSocket connection opened")
            # Subscribe to volume events
            subscribe_message = {
                "type": "subscribe",
                "events": ["volume_changed"]
            }
            ws.send(json.dumps(subscribe_message))
        
        base_url = f"http://localhost:{self.server.port}"
        ws_url = base_url.replace("http://", "ws://") + "/api/events/ws"
        
        self.websocket_connection = websocket.WebSocketApp(
            ws_url,
            on_message=on_message,
            on_error=on_error,
            on_close=on_close,
            on_open=on_open
        )
        
        self.websocket_thread = threading.Thread(
            target=self.websocket_connection.run_forever
        )
        self.websocket_thread.daemon = True
        self.websocket_thread.start()
        
        # Wait for connection to establish
        time.sleep(1.0)
    
    def disconnect_websocket(self):
        """Disconnect from WebSocket"""
        if self.websocket_connection:
            self.websocket_connection.close()
        if self.websocket_thread:
            self.websocket_thread.join(timeout=2.0)
    
    def clear_events(self):
        """Clear accumulated WebSocket events"""
        self.websocket_events.clear()
    
    def wait_for_volume_event(self, timeout=5.0) -> Optional[Dict]:
        """Wait for a volume change event"""
        start_time = time.time()
        while time.time() - start_time < timeout:
            for event in self.websocket_events:
                # Check if it's a volume change event directly
                if event.get("type") == "volume_changed":
                    return event
                # Also check if it's wrapped in event_data
                if event.get("event_data", {}).get("type") == "volume_changed":
                    return event
            time.sleep(0.1)
        return None


@pytest.fixture
def volume_server(request):
    """Fixture to start the server with volume control configuration"""
    from conftest import AudioControlTestServer
    server = AudioControlTestServer("volume", 18080)
    
    try:
        success = server.start_server()
        if not success:
            raise RuntimeError("Failed to start volume test server")
        yield server
    finally:
        server.stop_server()


@pytest.fixture
def volume_helper(volume_server):
    """Fixture to provide volume test helper"""
    helper = VolumeTestHelper(volume_server)
    helper.connect_websocket()
    yield helper
    helper.disconnect_websocket()


def test_volume_info_endpoint(volume_server):
    """Test that the volume info endpoint returns expected data"""
    response = volume_server.api_request('GET', '/api/volume/info')
    
    assert isinstance(response, dict), "Volume info should return a dict for single global control"
    assert "available" in response
    assert "control_info" in response
    assert "current_state" in response
    assert "supports_change_monitoring" in response
    
    # Check the dummy volume control info
    control_info = response["control_info"]
    assert control_info["internal_name"] == "test_dummy"
    assert control_info["display_name"] == "Test Dummy Volume Control"
    assert "decibel_range" in control_info
    assert control_info["decibel_range"]["min_db"] == -120.0
    assert control_info["decibel_range"]["max_db"] == 0.0
    
    # Check current state
    current_state = response["current_state"]
    assert "percentage" in current_state
    assert "decibels" in current_state
    assert "raw_value" in current_state
    
    # Should start at 50% as configured
    assert current_state["percentage"] == 50.0
    assert response["available"] is True


def test_volume_state_endpoint(volume_server):
    """Test that the volume state endpoint returns current state"""
    response = volume_server.api_request('GET', '/api/volume/state')
    
    assert isinstance(response, dict), "Volume state should return a dict for single global control"
    assert "percentage" in response
    assert "decibels" in response
    assert "raw_value" in response
    
    # Should start at 50% as configured
    assert response["percentage"] == 50.0
    assert response["decibels"] == -60.0
    assert response["raw_value"] == 50


def test_volume_set_percentage(volume_server, volume_helper):
    """Test setting volume by percentage"""
    volume_helper.clear_events()
    
    # Set volume to 75%
    request_data = {"percentage": 75.0}
    response = volume_server.api_request('POST', '/api/volume/set', json=request_data)
    
    assert "success" in response
    assert response["success"] is True
    
    # Verify the change
    state_response = volume_server.api_request('GET', '/api/volume/state')
    assert state_response["percentage"] == 75.0
    
    # Wait for volume change event
    volume_event = volume_helper.wait_for_volume_event()
    assert volume_event is not None, "Should receive volume change event"
    
    # Check if event is direct or wrapped
    event_data = volume_event.get("event_data", volume_event)
    assert event_data["percentage"] == 75.0


def test_volume_set_decibels(volume_server, volume_helper):
    """Test setting volume by decibels"""
    volume_helper.clear_events()
    
    # Set volume to -20dB
    request_data = {"decibels": -20.0}
    response = volume_server.api_request('POST', '/api/volume/set', json=request_data)
    
    assert "success" in response
    assert response["success"] is True
    
    # Verify the change
    state_response = volume_server.api_request('GET', '/api/volume/state')
    assert abs(state_response["decibels"] - (-20.0)) < 0.1  # Allow small floating point differences
    
    # Wait for volume change event
    volume_event = volume_helper.wait_for_volume_event()
    assert volume_event is not None, "Should receive volume change event"
    
    event_data = volume_event.get("event_data", volume_event)
    assert abs(event_data["decibels"] - (-20.0)) < 0.1


def test_volume_set_raw_value(volume_server, volume_helper):
    """Test setting volume by raw value"""
    volume_helper.clear_events()
    
    # Set volume to raw value 25
    request_data = {"raw_value": 25}
    response = volume_server.api_request('POST', '/api/volume/set', json=request_data)
    
    assert "success" in response
    assert response["success"] is True
    
    # Verify the change
    state_response = volume_server.api_request('GET', '/api/volume/state')
    assert state_response["raw_value"] == 25
    assert state_response["percentage"] == 25.0  # For dummy control, raw value = percentage
    
    # Wait for volume change event
    volume_event = volume_helper.wait_for_volume_event()
    assert volume_event is not None, "Should receive volume change event"
    
    event_data = volume_event.get("event_data", volume_event)
    assert event_data["raw_value"] == 25


def test_volume_increase(volume_server, volume_helper):
    """Test increasing volume"""
    # First set a known state
    volume_server.api_request('POST', '/api/volume/set', json={"percentage": 50.0})
    time.sleep(0.5)
    volume_helper.clear_events()
    
    # Increase by 10%
    response = volume_server.api_request('POST', '/api/volume/increase?amount=10.0')
    
    assert "success" in response
    assert response["success"] is True
    
    # Verify the change
    state_response = volume_server.api_request('GET', '/api/volume/state')
    assert state_response["percentage"] == 60.0
    
    # Wait for volume change event
    volume_event = volume_helper.wait_for_volume_event()
    assert volume_event is not None, "Should receive volume change event"
    
    event_data = volume_event.get("event_data", volume_event)
    assert event_data["percentage"] == 60.0


def test_volume_decrease(volume_server, volume_helper):
    """Test decreasing volume"""
    # First set a known state
    volume_server.api_request('POST', '/api/volume/set', json={"percentage": 50.0})
    time.sleep(0.5)
    volume_helper.clear_events()
    
    # Decrease by 15%
    response = volume_server.api_request('POST', '/api/volume/decrease?amount=15.0')
    
    assert "success" in response
    assert response["success"] is True
    
    # Verify the change
    state_response = volume_server.api_request('GET', '/api/volume/state')
    assert state_response["percentage"] == 35.0
    
    # Wait for volume change event
    volume_event = volume_helper.wait_for_volume_event()
    assert volume_event is not None, "Should receive volume change event"
    
    event_data = volume_event.get("event_data", volume_event)
    assert event_data["percentage"] == 35.0


def test_volume_mute_unmute(volume_server, volume_helper):
    """Test muting and unmuting volume"""
    # First ensure we're at 50%
    volume_server.api_request('POST', '/api/volume/set', json={"percentage": 50.0})
    time.sleep(0.5)
    volume_helper.clear_events()
    
    # Toggle mute (should go to 0%)
    response = volume_server.api_request('POST', '/api/volume/mute')
    
    assert "success" in response
    assert response["success"] is True
    
    # Verify muted state (volume should be 0)
    state_response = volume_server.api_request('GET', '/api/volume/state')
    assert state_response["percentage"] == 0.0
    
    # Wait for volume change event
    volume_event = volume_helper.wait_for_volume_event()
    assert volume_event is not None, "Should receive volume change event for mute"
    
    # Clear events and toggle mute again (should go to 50%)
    volume_helper.clear_events()
    response = volume_server.api_request('POST', '/api/volume/mute')
    
    assert "success" in response
    assert response["success"] is True
    
    # Verify unmuted state (volume should be 50%)
    state_response = volume_server.api_request('GET', '/api/volume/state')
    assert state_response["percentage"] == 50.0
    
    # Wait for unmute event
    volume_event = volume_helper.wait_for_volume_event()
    assert volume_event is not None, "Should receive volume change event for unmute"


def test_volume_bounds_checking(volume_server):
    """Test that volume bounds are properly enforced"""
    # Test setting volume above 100%
    request_data = {"percentage": 150.0}
    response = volume_server.api_request('POST', '/api/volume/set', json=request_data)
    
    # Should either succeed with clamping or return an error
    if "success" in response and response["success"]:
        # If successful, should be clamped to 100%
        state_response = volume_server.api_request('GET', '/api/volume/state')
        assert state_response["percentage"] <= 100.0
    else:
        # Should return an error for out of range
        assert "error" in response or not response["success"]
    
    # Test setting volume below 0%
    request_data = {"percentage": -10.0}
    response = volume_server.api_request('POST', '/api/volume/set', json=request_data)
    
    if "success" in response and response["success"]:
        # If successful, should be clamped to 0%
        state_response = volume_server.api_request('GET', '/api/volume/state')
        assert state_response["percentage"] >= 0.0
    else:
        # Should return an error for out of range
        assert "error" in response or not response["success"]


def test_volume_decibel_range_validation(volume_server):
    """Test that decibel values are properly validated"""
    # Test setting decibels above max (0dB for dummy control)
    request_data = {"decibels": 10.0}
    response = volume_server.api_request('POST', '/api/volume/set', json=request_data)
    
    if "success" in response and response["success"]:
        # Should be clamped to valid range
        state_response = volume_server.api_request('GET', '/api/volume/state')
        assert state_response["decibels"] <= 0.0
    else:
        assert "error" in response or not response["success"]
    
    # Test setting decibels below min (-120dB for dummy control)
    request_data = {"decibels": -200.0}
    response = volume_server.api_request('POST', '/api/volume/set', json=request_data)
    
    if "success" in response and response["success"]:
        # Should be clamped to valid range
        state_response = volume_server.api_request('GET', '/api/volume/state')
        assert state_response["decibels"] >= -120.0
    else:
        assert "error" in response or not response["success"]


def test_volume_invalid_control_name(volume_server):
    """Test handling of requests to non-existent volume controls - not applicable for single global control"""
    # For single global volume control, this test is not applicable
    # But we can test malformed endpoint paths
    request_data = {"percentage": 50.0}
    
    try:
        response = volume_server.api_request('POST', '/api/volume/nonexistent/set', json=request_data)
        # Should return an error or 404
        assert "error" in response or "success" not in response or not response["success"]
    except requests.exceptions.HTTPError as e:
        # 404 Not Found is expected for invalid paths
        assert e.response.status_code == 404


def test_volume_malformed_requests(volume_server):
    """Test handling of malformed volume control requests"""
    # Request with no data
    try:
        response = volume_server.api_request('POST', '/api/volume/set', json={})
        assert "error" in response or not response.get("success", True)
    except requests.exceptions.HTTPError:
        pass  # 400 Bad Request is acceptable
    
    # Request with invalid data type
    try:
        response = volume_server.api_request('POST', '/api/volume/set', json={"percentage": "not_a_number"})
        assert "error" in response or not response.get("success", True)
    except requests.exceptions.HTTPError:
        pass  # 400 Bad Request is acceptable
    
    # Request with multiple conflicting parameters
    try:
        response = volume_server.api_request('POST', '/api/volume/set', json={
            "percentage": 50.0,
            "decibels": -20.0,
            "raw_value": 75
        })
        # Should either succeed with one parameter taking precedence or return an error
        if "success" in response:
            assert response["success"] is True or "error" in response
    except requests.exceptions.HTTPError:
        pass  # 400 Bad Request is acceptable


def test_volume_event_subscription(volume_server, volume_helper):
    """Test that volume events are properly sent to WebSocket subscribers"""
    volume_helper.clear_events()
    
    # Make several volume changes
    changes = [
        {"percentage": 25.0},
        {"percentage": 75.0},
        {"decibels": -30.0},
        {"raw_value": 90}
    ]
    
    for change in changes:
        volume_server.api_request('POST', '/api/volume/set', json=change)
        time.sleep(0.5)  # Allow time for event propagation
    
    # Should have received multiple volume change events
    volume_events = []
    for event in volume_helper.websocket_events:
        # Check both direct and wrapped event formats
        if event.get("type") == "volume_changed":
            volume_events.append(event)
        elif event.get("event_data", {}).get("type") == "volume_changed":
            volume_events.append(event)
    
    assert len(volume_events) >= len(changes), f"Expected at least {len(changes)} events, got {len(volume_events)}"
    
    # Verify event structure
    for event in volume_events:
        event_data = event.get("event_data", event)
        assert "percentage" in event_data
        assert "decibels" in event_data
        assert "raw_value" in event_data


def test_single_global_volume_control(volume_server):
    """Test that the API works with single global volume control"""
    info_response = volume_server.api_request('GET', '/api/volume/info')
    state_response = volume_server.api_request('GET', '/api/volume/state')
    
    # Should return single objects, not lists
    assert isinstance(info_response, dict), "Info should return single object"
    assert isinstance(state_response, dict), "State should return single object"
    
    # Should have consistent structure
    assert "control_info" in info_response
    assert "current_state" in info_response
    assert "available" in info_response
    
    # State should match current_state in info
    current_state = info_response["current_state"]
    assert current_state["percentage"] == state_response["percentage"]
    assert current_state["decibels"] == state_response["decibels"]
    assert current_state["raw_value"] == state_response["raw_value"]
