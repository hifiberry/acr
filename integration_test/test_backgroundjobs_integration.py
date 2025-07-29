"""
Simple integration test for the background jobs API.

This module tests the basic functionality of the background jobs API endpoints.
"""

import pytest


def test_background_jobs_endpoint_basic(generic_server):
    """Test that the background jobs endpoint works and returns valid JSON structure."""
    response = generic_server.api_request('GET', '/api/background/jobs')
    
    # Verify response is a dictionary
    assert isinstance(response, dict)
    
    # Check required fields are present
    assert "success" in response
    assert "jobs" in response
    
    # Verify success is a boolean
    assert isinstance(response["success"], bool)
    
    # Verify jobs is a list (may be empty)
    assert isinstance(response["jobs"], list)
    
    # Check optional message field exists (can be None)
    assert "message" in response

def test_background_job_by_id_not_found(generic_server):
    """Test that requesting a non-existent background job returns appropriate error."""
    fake_job_id = "non_existent_job_123"
    response = generic_server.api_request('GET', f'/api/background/jobs/{fake_job_id}')
    
    # Verify response structure
    assert isinstance(response, dict)
    assert "success" in response
    assert "jobs" in response
    assert "message" in response
    
    # Should indicate failure for non-existent job
    assert response["success"] is False
    assert response["jobs"] is None
    assert "not found" in response["message"].lower()

def test_background_jobs_api_response_performance(generic_server):
    """Test that background jobs API responds quickly."""
    import time
    
    start_time = time.time()
    response = generic_server.api_request('GET', '/api/background/jobs')
    response_time = time.time() - start_time
    
    # Verify the API works
    assert isinstance(response, dict)
    assert response["success"] is True
    
    # Verify it responds quickly (under 1 second)
    assert response_time < 1.0, f"API response took too long: {response_time:.2f}s"
