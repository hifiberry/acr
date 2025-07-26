#!/usr/bin/env python3
"""
Integration tests for Settings API
These tests verify the settings endpoints for get and set operations
"""

import pytest
import json
import time
import uuid


def test_settings_get_nonexistent_key(generic_server):
    """Test getting a key that doesn't exist"""
    test_key = f"test_nonexistent_{uuid.uuid4().hex}"
    
    response = generic_server.api_request('POST', '/api/settings/get', {
        'key': test_key
    })
    
    assert isinstance(response, dict)
    assert response['success'] is True
    assert response['key'] == test_key
    assert response['value'] is None
    assert response['exists'] is False


def test_settings_set_and_get_string_value(generic_server):
    """Test setting and getting a string value"""
    test_key = f"test_string_{uuid.uuid4().hex}"
    test_value = "Hello, World!"
    
    # Set the value
    set_response = generic_server.api_request('POST', '/api/settings/set', {
        'key': test_key,
        'value': test_value
    })
    
    assert isinstance(set_response, dict)
    assert set_response['success'] is True
    assert set_response['key'] == test_key
    assert set_response['value'] == test_value
    assert set_response['previous_value'] is None
    
    # Get the value
    get_response = generic_server.api_request('POST', '/api/settings/get', {
        'key': test_key
    })
    
    assert isinstance(get_response, dict)
    assert get_response['success'] is True
    assert get_response['key'] == test_key
    assert get_response['value'] == test_value
    assert get_response['exists'] is True


def test_settings_set_and_get_number_value(generic_server):
    """Test setting and getting a numeric value"""
    test_key = f"test_number_{uuid.uuid4().hex}"
    test_value = 42
    
    # Set the value
    set_response = generic_server.api_request('POST', '/api/settings/set', {
        'key': test_key,
        'value': test_value
    })
    
    assert isinstance(set_response, dict)
    assert set_response['success'] is True
    assert set_response['key'] == test_key
    assert set_response['value'] == test_value
    assert set_response['previous_value'] is None
    
    # Get the value
    get_response = generic_server.api_request('POST', '/api/settings/get', {
        'key': test_key
    })
    
    assert isinstance(get_response, dict)
    assert get_response['success'] is True
    assert get_response['key'] == test_key
    assert get_response['value'] == test_value
    assert get_response['exists'] is True


def test_settings_set_and_get_boolean_value(generic_server):
    """Test setting and getting a boolean value"""
    test_key = f"test_boolean_{uuid.uuid4().hex}"
    test_value = True
    
    # Set the value
    set_response = generic_server.api_request('POST', '/api/settings/set', {
        'key': test_key,
        'value': test_value
    })
    
    assert isinstance(set_response, dict)
    assert set_response['success'] is True
    assert set_response['key'] == test_key
    assert set_response['value'] == test_value
    assert set_response['previous_value'] is None
    
    # Get the value
    get_response = generic_server.api_request('POST', '/api/settings/get', {
        'key': test_key
    })
    
    assert isinstance(get_response, dict)
    assert get_response['success'] is True
    assert get_response['key'] == test_key
    assert get_response['value'] == test_value
    assert get_response['exists'] is True


def test_settings_set_and_get_object_value(generic_server):
    """Test setting and getting a complex object value"""
    test_key = f"test_object_{uuid.uuid4().hex}"
    test_value = {
        "name": "Test Object",
        "settings": {
            "enabled": True,
            "count": 123,
            "items": ["a", "b", "c"]
        }
    }
    
    # Set the value
    set_response = generic_server.api_request('POST', '/api/settings/set', {
        'key': test_key,
        'value': test_value
    })
    
    assert isinstance(set_response, dict)
    assert set_response['success'] is True
    assert set_response['key'] == test_key
    assert set_response['value'] == test_value
    assert set_response['previous_value'] is None
    
    # Get the value
    get_response = generic_server.api_request('POST', '/api/settings/get', {
        'key': test_key
    })
    
    assert isinstance(get_response, dict)
    assert get_response['success'] is True
    assert get_response['key'] == test_key
    assert get_response['value'] == test_value
    assert get_response['exists'] is True


def test_settings_update_existing_value(generic_server):
    """Test updating an existing value and getting the previous value"""
    test_key = f"test_update_{uuid.uuid4().hex}"
    initial_value = "initial_value"
    updated_value = "updated_value"
    
    # Set initial value
    set_response1 = generic_server.api_request('POST', '/api/settings/set', {
        'key': test_key,
        'value': initial_value
    })
    
    assert set_response1['success'] is True
    assert set_response1['value'] == initial_value
    assert set_response1['previous_value'] is None
    
    # Update the value
    set_response2 = generic_server.api_request('POST', '/api/settings/set', {
        'key': test_key,
        'value': updated_value
    })
    
    assert isinstance(set_response2, dict)
    assert set_response2['success'] is True
    assert set_response2['key'] == test_key
    assert set_response2['value'] == updated_value
    assert set_response2['previous_value'] == initial_value
    
    # Verify the new value
    get_response = generic_server.api_request('POST', '/api/settings/get', {
        'key': test_key
    })
    
    assert get_response['success'] is True
    assert get_response['value'] == updated_value


def test_settings_unicode_keys(generic_server):
    """Test setting and getting values with Unicode keys"""
    test_key = f"test_unicode_🔑_{uuid.uuid4().hex}"
    test_value = "Unicode value 🌟"
    
    # Set the value
    set_response = generic_server.api_request('POST', '/api/settings/set', {
        'key': test_key,
        'value': test_value
    })
    
    assert isinstance(set_response, dict)
    assert set_response['success'] is True
    assert set_response['key'] == test_key
    assert set_response['value'] == test_value
    
    # Get the value
    get_response = generic_server.api_request('POST', '/api/settings/get', {
        'key': test_key
    })
    
    assert isinstance(get_response, dict)
    assert get_response['success'] is True
    assert get_response['key'] == test_key
    assert get_response['value'] == test_value
    assert get_response['exists'] is True


def test_settings_empty_key(generic_server):
    """Test handling of empty key"""
    test_key = ""
    test_value = "value for empty key"
    
    # Set the value
    set_response = generic_server.api_request('POST', '/api/settings/set', {
        'key': test_key,
        'value': test_value
    })
    
    assert isinstance(set_response, dict)
    assert set_response['success'] is True
    assert set_response['key'] == test_key
    assert set_response['value'] == test_value
    
    # Get the value
    get_response = generic_server.api_request('POST', '/api/settings/get', {
        'key': test_key
    })
    
    assert isinstance(get_response, dict)
    assert get_response['success'] is True
    assert get_response['key'] == test_key
    assert get_response['value'] == test_value
    assert get_response['exists'] is True


def test_settings_null_value(generic_server):
    """Test setting a null value"""
    test_key = f"test_null_{uuid.uuid4().hex}"
    test_value = None
    
    # Set the value
    set_response = generic_server.api_request('POST', '/api/settings/set', {
        'key': test_key,
        'value': test_value
    })
    
    assert isinstance(set_response, dict)
    assert set_response['success'] is True
    assert set_response['key'] == test_key
    assert set_response['value'] is None
    
    # Get the value
    get_response = generic_server.api_request('POST', '/api/settings/get', {
        'key': test_key
    })
    
    assert isinstance(get_response, dict)
    assert get_response['success'] is True
    assert get_response['key'] == test_key
    assert get_response['value'] is None
    assert get_response['exists'] is True


def test_settings_large_value(generic_server):
    """Test setting and getting a large value"""
    test_key = f"test_large_{uuid.uuid4().hex}"
    # Create a large string (10KB)
    test_value = "x" * 10240
    
    # Set the value
    set_response = generic_server.api_request('POST', '/api/settings/set', {
        'key': test_key,
        'value': test_value
    })
    
    assert isinstance(set_response, dict)
    assert set_response['success'] is True
    assert set_response['key'] == test_key
    assert set_response['value'] == test_value
    
    # Get the value
    get_response = generic_server.api_request('POST', '/api/settings/get', {
        'key': test_key
    })
    
    assert isinstance(get_response, dict)
    assert get_response['success'] is True
    assert get_response['key'] == test_key
    assert get_response['value'] == test_value
    assert get_response['exists'] is True


def test_settings_special_characters_in_key(generic_server):
    """Test keys with special characters"""
    special_chars = "!@#$%^&*()_+-=[]{}|;':\",./<>?"
    test_key = f"test_special_{special_chars}_{uuid.uuid4().hex}"
    test_value = "value with special key"
    
    # Set the value
    set_response = generic_server.api_request('POST', '/api/settings/set', {
        'key': test_key,
        'value': test_value
    })
    
    assert isinstance(set_response, dict)
    assert set_response['success'] is True
    assert set_response['key'] == test_key
    assert set_response['value'] == test_value
    
    # Get the value
    get_response = generic_server.api_request('POST', '/api/settings/get', {
        'key': test_key
    })
    
    assert isinstance(get_response, dict)
    assert get_response['success'] is True
    assert get_response['key'] == test_key
    assert get_response['value'] == test_value
    assert get_response['exists'] is True


def test_settings_array_value(generic_server):
    """Test setting and getting an array value"""
    test_key = f"test_array_{uuid.uuid4().hex}"
    test_value = [1, "two", {"three": 3}, [4, 5], True, None]
    
    # Set the value
    set_response = generic_server.api_request('POST', '/api/settings/set', {
        'key': test_key,
        'value': test_value
    })
    
    assert isinstance(set_response, dict)
    assert set_response['success'] is True
    assert set_response['key'] == test_key
    assert set_response['value'] == test_value
    
    # Get the value
    get_response = generic_server.api_request('POST', '/api/settings/get', {
        'key': test_key
    })
    
    assert isinstance(get_response, dict)
    assert get_response['success'] is True
    assert get_response['key'] == test_key
    assert get_response['value'] == test_value
    assert get_response['exists'] is True


def test_settings_concurrent_operations(generic_server):
    """Test that concurrent set/get operations work correctly"""
    import threading
    import queue
    
    test_key = f"test_concurrent_{uuid.uuid4().hex}"
    results = queue.Queue()
    
    def set_value(value_suffix):
        try:
            value = f"concurrent_value_{value_suffix}"
            response = generic_server.api_request('POST', '/api/settings/set', {
                'key': test_key,
                'value': value
            })
            results.put(('set', value_suffix, response))
        except Exception as e:
            results.put(('set', value_suffix, {'error': str(e)}))
    
    def get_value(get_id):
        try:
            response = generic_server.api_request('POST', '/api/settings/get', {
                'key': test_key
            })
            results.put(('get', get_id, response))
        except Exception as e:
            results.put(('get', get_id, {'error': str(e)}))
    
    # Start multiple threads
    threads = []
    for i in range(5):
        t = threading.Thread(target=set_value, args=(i,))
        threads.append(t)
        t.start()
    
    for i in range(3):
        t = threading.Thread(target=get_value, args=(i,))
        threads.append(t)
        t.start()
    
    # Wait for all threads
    for t in threads:
        t.join()
    
    # Collect results
    set_results = []
    get_results = []
    
    while not results.empty():
        op_type, op_id, response = results.get()
        if op_type == 'set':
            set_results.append((op_id, response))
        else:
            get_results.append((op_id, response))
    
    # All set operations should succeed
    assert len(set_results) == 5
    for op_id, response in set_results:
        assert 'error' not in response
        assert response['success'] is True
    
    # All get operations should succeed
    assert len(get_results) == 3
    for op_id, response in get_results:
        assert 'error' not in response
        # The get might return None if it happened before any set, or a value if after
        assert response['success'] is True


def test_settings_invalid_json_handling(generic_server):
    """Test that the API handles malformed requests gracefully"""
    # This test sends raw data that's not valid JSON to test error handling
    import requests
    
    base_url = f"http://localhost:{generic_server.port}"
    
    # Test invalid JSON
    try:
        response = requests.post(
            f"{base_url}/api/settings/get",
            data="invalid json",
            headers={"Content-Type": "application/json"},
            timeout=5
        )
        # Should return an error response, not crash
        assert response.status_code in [400, 422]  # Bad request or unprocessable entity
    except requests.exceptions.RequestException:
        # Connection errors are also acceptable (server might reject the request)
        pass


if __name__ == '__main__':
    pytest.main([__file__])
