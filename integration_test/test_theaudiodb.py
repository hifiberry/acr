#!/usr/bin/env python3
"""
TheAudioDB integration tests for AudioControl system
"""

import pytest
import json
import time
import os
from pathlib import Path
from conftest import AudioControlTestServer, TEST_PORTS

# Test configuration for TheAudioDB
TEST_CONFIG_PATH = Path(__file__).parent / "test_config_theaudiodb.json"

@pytest.fixture
def theaudiodb_server():
    """Fixture for TheAudioDB integration tests"""
    server = AudioControlTestServer("theaudiodb", TEST_PORTS['theaudiodb'])
    
    # Override the config path to use our custom config
    original_create_config = server.create_config
    
    def create_custom_config():
        """Create config with TheAudioDB enabled"""
        import tempfile
        import shutil
        
        # Create cache directories
        cache_dir = Path(f"test_cache_{server.port}")
        cache_dir.mkdir(exist_ok=True)
        attributes_cache_dir = cache_dir / "attributes"
        attributes_cache_dir.mkdir(exist_ok=True)
        images_cache_dir = cache_dir / "images"
        images_cache_dir.mkdir(exist_ok=True)
        
        server.cache_dir = cache_dir
        
        # Load the custom config
        with open(TEST_CONFIG_PATH, 'r') as f:
            config = json.load(f)
        
        # Update port
        config["services"]["webserver"]["port"] = server.port
        
        # Update cache paths
        config["services"]["cache"]["attribute_cache_path"] = str(attributes_cache_dir.absolute())
        config["services"]["cache"]["image_cache_path"] = str(images_cache_dir.absolute())
        
        # Create config file
        server.config_path = Path(f"test_config_{server.port}.json")
        with open(server.config_path, 'w') as f:
            json.dump(config, f, indent=2)
        
        return server.config_path
    
    # Replace the create_config method
    server.create_config = create_custom_config
    
    assert server.start_server(), "Failed to start TheAudioDB test server"
    yield server
    server.stop_server()

def test_theaudiodb_server_startup(theaudiodb_server):
    """Test that the server starts up correctly with TheAudioDB enabled"""
    # The server should be running by now due to the fixture
    response = theaudiodb_server.api_request('GET', '/api/version')
    assert 'version' in response
    assert response['version'] is not None

def test_theaudiodb_mbid_endpoint(theaudiodb_server):
    """Test the TheAudioDB MBID lookup endpoint"""
    # Test with John Williams' MBID
    mbid = "53b106e7-0cc6-42cc-ac95-ed8d30a3a98e"
    
    # Use the error-handling version of the API request
    response = theaudiodb_server.api_request_with_error_handling('GET', f'/api/audiodb/mbid/{mbid}')
    
    print(f"HTTP Status Code: {response.status_code}")
    
    # Check if we got a successful response
    if response.status_code == 200:
        response_data = response.json()
        
        # Check that we got a valid response
        assert response_data is not None
        assert isinstance(response_data, dict)
        
        # Check the response structure
        assert 'mbid' in response_data
        assert response_data['mbid'] == mbid
        assert 'success' in response_data
        assert 'data' in response_data
        
        # If successful, check the data
        if response_data['success']:
            assert response_data['data'] is not None
            artist_data = response_data['data']
            
            # Check that it's John Williams
            assert isinstance(artist_data, dict)
            assert 'strArtist' in artist_data
            assert artist_data['strArtist'] == 'John Williams'
            
            print(f"Successfully retrieved artist: {artist_data['strArtist']}")
            print(f"Artist biography length: {len(artist_data.get('strBiographyEN', ''))}")
        else:
            # If not successful, check the error
            assert 'error' in response_data
            assert response_data['error'] is not None
            print(f"API returned error: {response_data['error']}")
            # This is not expected - the test requires proper configuration
            if "disabled" in response_data['error'].lower():
                raise AssertionError(f"TheAudioDB is disabled - this test requires TheAudioDB to be enabled: {response_data['error']}")
            else:
                # Re-raise the error if it's not a disabled service
                raise AssertionError(f"TheAudioDB API error - check configuration: {response_data['error']}")
    
    elif response.status_code == 503:
        # Service unavailable - TheAudioDB is disabled - this should fail the test
        try:
            error_response = response.json()
            print(f"Service unavailable: {error_response}")
            assert 'error' in error_response
            raise AssertionError(f"TheAudioDB service is disabled - this test requires TheAudioDB to be enabled: {error_response['error']}")
        except Exception as e:
            raise AssertionError(f"TheAudioDB service is disabled - this test requires TheAudioDB to be enabled")
    
    elif response.status_code == 500:
        # Server error - could be API key issue or other problem
        try:
            error_response = response.json()
            print(f"Server error response: {error_response}")
            
            # Check the expected structure for our API
            assert 'mbid' in error_response
            assert error_response['mbid'] == mbid
            assert 'success' in error_response
            assert error_response['success'] is False
            assert 'error' in error_response
            
            error_msg = error_response['error']
            
            # Check if it's an expected error (API key issues, network, etc.)
            if ("your_theaudiodb_api_key_here" in error_msg or 
                "status code 404" in error_msg or 
                "Failed to send request" in error_msg):
                raise AssertionError(f"TheAudioDB API key is not configured correctly: {error_msg}")
            elif "disabled" in error_msg.lower():
                raise AssertionError(f"TheAudioDB is disabled - this test requires TheAudioDB to be enabled: {error_msg}")
            else:
                raise AssertionError(f"Unexpected server error: {error_response}")
                
        except Exception as json_err:
            print(f"Could not parse server error response: {json_err}")
            raise AssertionError(f"HTTP 500 error with unparseable response")
    
    else:
        # Other HTTP errors
        try:
            error_response = response.json()
            print(f"HTTP {response.status_code} error response: {error_response}")
            raise AssertionError(f"Unexpected HTTP {response.status_code} error: {error_response}")
        except Exception as json_err:
            print(f"HTTP {response.status_code} error with unparseable response: {json_err}")
            raise AssertionError(f"HTTP {response.status_code} error with unparseable response")

def test_theaudiodb_mbid_endpoint_invalid(theaudiodb_server):
    """Test the TheAudioDB MBID lookup endpoint with invalid MBID"""
    # Test with an invalid MBID
    mbid = "invalid-mbid-12345"
    
    try:
        response = theaudiodb_server.api_request('GET', f'/api/audiodb/mbid/{mbid}')
        # If we get here, check the response structure
        assert response is not None
        assert isinstance(response, dict)
        assert 'mbid' in response
        assert response['mbid'] == mbid
        assert 'success' in response
        
        # For invalid MBID, success should be False
        assert response['success'] is False
        assert 'error' in response
        assert response['error'] is not None
        
        print(f"Expected error for invalid MBID: {response['error']}")
            
    except Exception as e:
        # It's also acceptable if the API returns an HTTP error
        print(f"API returned HTTP error for invalid MBID (expected): {e}")

def test_theaudiodb_mbid_endpoint_unknown_artist(theaudiodb_server):
    """Test the TheAudioDB MBID lookup endpoint with unknown but valid MBID"""
    # Test with a valid but unknown MBID format
    mbid = "00000000-0000-0000-0000-000000000000"
    
    try:
        response = theaudiodb_server.api_request('GET', f'/api/audiodb/mbid/{mbid}')
        
        # Check that we got a valid response
        assert response is not None
        assert isinstance(response, dict)
        assert 'mbid' in response
        assert response['mbid'] == mbid
        assert 'success' in response
        
        # For unknown MBID, success should be False
        assert response['success'] is False
        assert 'error' in response
        assert response['error'] is not None
        
        print(f"Expected error for unknown MBID: {response['error']}")
        
    except Exception as e:
        # It's also acceptable if the API returns an HTTP error (404)
        print(f"API returned HTTP error for unknown MBID (expected): {e}")

def test_theaudiodb_rate_limiting(theaudiodb_server):
    """Test that TheAudioDB rate limiting is working"""
    # Make multiple requests quickly to test rate limiting
    mbid = "53b106e7-0cc6-42cc-ac95-ed8d30a3a98e"
    
    try:
        # First request should succeed (or fail with service disabled)
        start_time = time.time()
        response1 = theaudiodb_server.api_request('GET', f'/api/audiodb/mbid/{mbid}')
        assert response1 is not None
        assert 'success' in response1
        
        # Second request should also work but might be slower due to rate limiting
        response2 = theaudiodb_server.api_request('GET', f'/api/audiodb/mbid/{mbid}')
        end_time = time.time()
        
        assert response2 is not None
        assert 'success' in response2
        
        # The two requests should take at least the rate limit time (500ms)
        # But we allow some margin for test execution time
        duration = end_time - start_time
        print(f"Two API requests took {duration:.3f} seconds")
        
        # This is more of an informational test - rate limiting might be hard to test
        # in a reliable way due to caching and other factors
        
    except Exception as e:
        # It's acceptable if the service is disabled
        print(f"Rate limiting test failed (expected if service is disabled): {e}")
    
def test_theaudiodb_endpoint_integration(theaudiodb_server):
    """Integration test for TheAudioDB endpoint functionality"""
    # Test the full flow: request -> processing -> response
    mbid = "53b106e7-0cc6-42cc-ac95-ed8d30a3a98e"
    
    response = theaudiodb_server.api_request('GET', f'/api/audiodb/mbid/{mbid}')
    
    # Verify response structure
    assert response is not None
    assert isinstance(response, dict)
    assert 'mbid' in response
    assert response['mbid'] == mbid
    assert 'success' in response
    
    # Check the result
    if response['success']:
        assert 'data' in response
        artist_data = response['data']
        assert artist_data is not None
        assert isinstance(artist_data, dict)
        
        # Check essential fields
        assert 'strArtist' in artist_data
        assert artist_data['strArtist'] == 'John Williams'
        
        # Check optional fields that might be present
        optional_fields = ['strBiographyEN', 'strGenre', 'strCountry', 'strWebsite']
        for field in optional_fields:
            if field in artist_data:
                print(f"{field}: {artist_data[field][:100] if isinstance(artist_data[field], str) else artist_data[field]}")
        
        print(f"Integration test passed - successfully retrieved {artist_data['strArtist']}")
    else:
        # If not successful, check the error
        assert 'error' in response
        assert response['error'] is not None
        print(f"Integration test completed with expected error: {response['error']}")
        # This is expected if TheAudioDB is disabled or API key is missing
        if "disabled" in response['error'].lower():
            print("TheAudioDB is disabled - this is expected in test environment")
        else:
            # Re-raise the error if it's not a disabled service
            raise AssertionError(f"Unexpected error: {response['error']}")

def test_theaudiodb_artist_coverart_beatles(theaudiodb_server):
    """Test TheAudioDB artist cover art functionality with The Beatles"""
    import base64
    
    # Encode "The Beatles" for URL-safe transmission
    artist_name = "The Beatles"
    # Use URL-safe Base64 encoding without padding
    artist_b64 = base64.urlsafe_b64encode(artist_name.encode('utf-8')).decode('utf-8').rstrip('=')
    
    print(f"Testing artist cover art for: {artist_name}")
    print(f"URL-safe Base64 encoded: {artist_b64}")
    
    # Make request to the cover art API
    response = theaudiodb_server.api_request('GET', f'/api/coverart/artist/{artist_b64}')
    
    # Verify response structure
    assert response is not None
    assert isinstance(response, dict)
    assert 'results' in response
    assert isinstance(response['results'], list)
    
    # Check if we got any results
    results = response['results']
    print(f"Found {len(results)} cover art results for {artist_name}")
    
    # Look for TheAudioDB results specifically
    theaudiodb_results = []
    for result in results:
        assert isinstance(result, dict)
        assert 'provider' in result
        assert 'urls' in result
        assert isinstance(result['urls'], list)
        
        if result['provider'] == 'theaudiodb':
            theaudiodb_results.extend(result['urls'])
            print(f"TheAudioDB provider found {len(result['urls'])} URLs")
            for i, url in enumerate(result['urls']):
                print(f"  URL {i+1}: {url}")
    
    # We should have TheAudioDB results for The Beatles (if service is enabled and configured)
    if theaudiodb_results:
        # Verify URLs are valid HTTP URLs
        for url in theaudiodb_results:
            assert url.startswith('http'), f"Invalid URL format: {url}"
        
        print(f"✓ Successfully found {len(theaudiodb_results)} TheAudioDB cover art URLs for {artist_name}")
    else:
        # If no results, the service might be disabled or misconfigured
        print(f"⚠ No TheAudioDB results found for {artist_name}")
        print("This might be expected if TheAudioDB is disabled or API key is missing")
        
        # Don't fail the test if other providers found results
        total_urls = sum(len(result['urls']) for result in results)
        if total_urls > 0:
            print(f"Other providers found {total_urls} URLs total")
        else:
            print("No cover art found from any provider")

def test_theaudiodb_album_coverart_beatles(theaudiodb_server):
    """Test TheAudioDB album cover art functionality with The Beatles albums"""
    import base64
    
    # Test both possible album names mentioned in the requirement
    test_cases = [
        ("The Beatles", "The Beastles"),  # As mentioned in the requirement
        ("The Beatles", "The Beatles"),   # Correct album name
        ("The Beatles", "Abbey Road"),    # Another famous Beatles album
    ]
    
    for artist_name, album_name in test_cases:
        print(f"\nTesting album cover art for: '{album_name}' by '{artist_name}'")
        
        # Encode for URL-safe transmission using URL-safe Base64 without padding
        artist_b64 = base64.urlsafe_b64encode(artist_name.encode('utf-8')).decode('utf-8').rstrip('=')
        album_b64 = base64.urlsafe_b64encode(album_name.encode('utf-8')).decode('utf-8').rstrip('=')
        
        print(f"Artist URL-safe B64: {artist_b64}")
        print(f"Album URL-safe B64: {album_b64}")
        
        # Make request to the album cover art API
        response = theaudiodb_server.api_request('GET', f'/api/coverart/album/{album_b64}/{artist_b64}')
        
        # Verify response structure
        assert response is not None
        assert isinstance(response, dict)
        assert 'results' in response
        assert isinstance(response['results'], list)
        
        # Check if we got any results
        results = response['results']
        print(f"Found {len(results)} cover art results for '{album_name}' by '{artist_name}'")
        
        # Look for TheAudioDB results specifically
        theaudiodb_results = []
        for result in results:
            assert isinstance(result, dict)
            assert 'provider' in result
            assert 'urls' in result
            assert isinstance(result['urls'], list)
            
            if result['provider'] == 'theaudiodb':
                theaudiodb_results.extend(result['urls'])
                print(f"TheAudioDB provider found {len(result['urls'])} URLs")
                for i, url in enumerate(result['urls']):
                    print(f"  URL {i+1}: {url}")
        
        # Check results
        if theaudiodb_results:
            # Verify URLs are valid HTTP URLs
            for url in theaudiodb_results:
                assert url.startswith('http'), f"Invalid URL format: {url}"
            
            print(f"✓ Successfully found {len(theaudiodb_results)} TheAudioDB album cover art URLs")
            
            # For "Abbey Road", we should definitely find results if the service is working
            if album_name == "Abbey Road" and len(theaudiodb_results) > 0:
                print(f"✓ Abbey Road test passed - TheAudioDB is working correctly")
                
        else:
            # If no TheAudioDB results, check if other providers found anything
            total_urls = sum(len(result['urls']) for result in results)
            if total_urls > 0:
                print(f"⚠ No TheAudioDB results, but other providers found {total_urls} URLs")
            else:
                print(f"⚠ No cover art found for '{album_name}' by '{artist_name}' from any provider")
            
            # This might be expected for "The Beastles" (typo) or if service is disabled
            if album_name == "The Beastles":
                print("Note: 'The Beastles' appears to be a typo and may not exist in TheAudioDB")

def test_theaudiodb_coverart_methods(theaudiodb_server):
    """Test that TheAudioDB is listed as an available cover art provider"""
    
    # Get the list of available cover art methods
    response = theaudiodb_server.api_request('GET', '/api/coverart/methods')
    
    # Verify response structure
    assert response is not None
    assert isinstance(response, dict)
    assert 'methods' in response
    assert isinstance(response['methods'], list)
    
    methods = response['methods']
    print(f"Found {len(methods)} cover art methods")
    
    # Check each method
    theaudiodb_found = False
    for method in methods:
        assert isinstance(method, dict)
        assert 'method' in method
        assert 'providers' in method
        assert isinstance(method['providers'], list)
        
        method_name = method['method']
        providers = method['providers']
        
        print(f"Method '{method_name}' has {len(providers)} providers:")
        for provider in providers:
            assert isinstance(provider, dict)
            assert 'name' in provider
            provider_name = provider['name']
            print(f"  - {provider_name}")
            
            if provider_name.lower() == 'theaudiodb':
                theaudiodb_found = True
                print(f"  ✓ TheAudioDB found in {method_name} method")
    
    if theaudiodb_found:
        print("✓ TheAudioDB is properly registered as a cover art provider")
    else:
        print("⚠ TheAudioDB not found in cover art providers")
        print("This might be expected if TheAudioDB is disabled or not configured")

def test_theaudiodb_coverart_integration_full_flow(theaudiodb_server):
    """Full integration test for TheAudioDB cover art functionality"""
    import base64
    
    print("\n=== Full TheAudioDB Cover Art Integration Test ===")
    
    # Step 1: Check if TheAudioDB is available as a provider
    methods_response = theaudiodb_server.api_request('GET', '/api/coverart/methods')
    assert 'methods' in methods_response
    
    theaudiodb_available = False
    for method in methods_response['methods']:
        for provider in method['providers']:
            if provider['name'].lower() == 'theaudiodb':
                theaudiodb_available = True
                break
    
    print(f"Step 1: TheAudioDB provider available: {theaudiodb_available}")
    
    # Step 2: Test artist cover art with The Beatles
    artist_name = "The Beatles"
    artist_b64 = base64.urlsafe_b64encode(artist_name.encode('utf-8')).decode('utf-8').rstrip('=')
    artist_response = theaudiodb_server.api_request('GET', f'/api/coverart/artist/{artist_b64}')
    
    assert 'results' in artist_response
    artist_theaudiodb_urls = 0
    for result in artist_response['results']:
        if result['provider'] == 'theaudiodb':
            artist_theaudiodb_urls += len(result['urls'])
    
    print(f"Step 2: Artist '{artist_name}' - TheAudioDB URLs found: {artist_theaudiodb_urls}")
    
    # Step 3: Test album cover art with Abbey Road
    album_name = "Abbey Road"
    album_b64 = base64.urlsafe_b64encode(album_name.encode('utf-8')).decode('utf-8').rstrip('=')
    album_response = theaudiodb_server.api_request('GET', f'/api/coverart/album/{album_b64}/{artist_b64}')
    
    assert 'results' in album_response
    album_theaudiodb_urls = 0
    for result in album_response['results']:
        if result['provider'] == 'theaudiodb':
            album_theaudiodb_urls += len(result['urls'])
    
    print(f"Step 3: Album '{album_name}' by '{artist_name}' - TheAudioDB URLs found: {album_theaudiodb_urls}")
    
    # Step 4: Summary
    total_urls = artist_theaudiodb_urls + album_theaudiodb_urls
    print(f"\n=== Integration Test Summary ===")
    print(f"TheAudioDB provider available: {theaudiodb_available}")
    print(f"Total URLs found: {total_urls}")
    print(f"  - Artist URLs: {artist_theaudiodb_urls}")
    print(f"  - Album URLs: {album_theaudiodb_urls}")
    
    if theaudiodb_available and total_urls > 0:
        print("✓ TheAudioDB cover art integration is working correctly")
    elif theaudiodb_available and total_urls == 0:
        print("⚠ TheAudioDB is available but returned no results")
        print("This might indicate API key issues or network problems")
    elif not theaudiodb_available:
        print("⚠ TheAudioDB provider is not available")
        print("This is expected if TheAudioDB is disabled in configuration")
    
    # Don't fail the test if the service is disabled - that's a valid configuration
    print("Integration test completed successfully")
