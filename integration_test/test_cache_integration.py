#!/usr/bin/env python3
"""
Integration tests for Cache Statistics API
These tests verify the cache statistics endpoint functionality
"""

import pytest
import json
import time
import uuid


def test_cache_stats_basic_response(generic_server):
    """Test that the cache stats endpoint returns a valid response structure"""
    response = generic_server.api_request('GET', '/api/cache/stats')
    
    assert isinstance(response, dict)
    assert response['success'] is True
    assert response['stats'] is not None
    assert response['message'] is None
    
    # Verify the stats structure
    stats = response['stats']
    assert isinstance(stats, dict)
    assert 'disk_entries' in stats
    assert 'memory_entries' in stats  
    assert 'memory_bytes' in stats
    assert 'memory_limit_bytes' in stats
    
    # Verify data types
    assert isinstance(stats['disk_entries'], int)
    assert isinstance(stats['memory_entries'], int)
    assert isinstance(stats['memory_bytes'], int) 
    assert isinstance(stats['memory_limit_bytes'], int)


def test_cache_stats_memory_limit_configuration(generic_server):
    """Test that the memory limit from configuration is correctly reflected"""
    response = generic_server.api_request('GET', '/api/cache/stats')
    
    assert response['success'] is True
    stats = response['stats']
    
    # The generic test config uses a default memory limit
    # We're testing that some reasonable memory limit is configured (should be > 1MB)
    assert stats['memory_limit_bytes'] > 1024 * 1024  # At least 1MB
    assert stats['memory_limit_bytes'] <= 100 * 1024 * 1024  # Not more than 100MB


def test_cache_stats_initial_state(generic_server):
    """Test that cache starts in a clean state"""
    response = generic_server.api_request('GET', '/api/cache/stats')
    
    assert response['success'] is True
    stats = response['stats']
    
    # Initially, cache should be empty
    assert stats['disk_entries'] >= 0  # Could be 0 or have some entries
    assert stats['memory_entries'] == 0  # Memory cache should start empty
    assert stats['memory_bytes'] == 0  # No memory usage initially
    assert stats['memory_limit_bytes'] > 0  # Limit should be configured


def test_cache_stats_multiple_requests(generic_server):
    """Test that multiple requests to cache stats work consistently"""
    responses = []
    
    # Make multiple requests
    for i in range(3):
        response = generic_server.api_request('GET', '/api/cache/stats')
        responses.append(response)
        time.sleep(0.1)  # Small delay between requests
    
    # All requests should succeed
    for response in responses:
        assert response['success'] is True
        assert response['stats'] is not None
    
    # Memory limit should be consistent across all requests
    memory_limits = [r['stats']['memory_limit_bytes'] for r in responses]
    assert all(limit == memory_limits[0] for limit in memory_limits)


def test_cache_stats_non_negative_values(generic_server):
    """Test that all cache statistics are non-negative"""
    response = generic_server.api_request('GET', '/api/cache/stats')
    
    assert response['success'] is True
    stats = response['stats']
    
    # All values should be non-negative
    assert stats['disk_entries'] >= 0
    assert stats['memory_entries'] >= 0
    assert stats['memory_bytes'] >= 0
    assert stats['memory_limit_bytes'] > 0  # Should be positive (not just non-negative)


def test_cache_stats_memory_usage_invariant(generic_server):
    """Test that memory usage is within expected bounds"""
    response = generic_server.api_request('GET', '/api/cache/stats')
    
    assert response['success'] is True
    stats = response['stats']
    
    # Memory bytes should not exceed the memory limit
    assert stats['memory_bytes'] <= stats['memory_limit_bytes']
    
    # If there are memory entries, there should be some memory usage (unless entries are empty)
    # This is a soft check since entries could theoretically be empty strings
    if stats['memory_entries'] > 0:
        # We don't enforce memory_bytes > 0 because entries could be empty strings
        pass


def test_cache_stats_response_format(generic_server):
    """Test that the response format matches the expected schema"""
    response = generic_server.api_request('GET', '/api/cache/stats')
    
    # Check top-level structure
    required_fields = ['success', 'stats', 'message']
    for field in required_fields:
        assert field in response, f"Missing required field: {field}"
    
    # Check stats structure
    stats = response['stats']
    required_stats_fields = ['disk_entries', 'memory_entries', 'memory_bytes', 'memory_limit_bytes']
    for field in required_stats_fields:
        assert field in stats, f"Missing required stats field: {field}"
    
    # Check that there are no unexpected extra fields at the top level
    expected_fields = {'success', 'stats', 'message'}
    actual_fields = set(response.keys())
    extra_fields = actual_fields - expected_fields
    assert len(extra_fields) == 0, f"Unexpected extra fields in response: {extra_fields}"
