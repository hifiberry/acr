#!/usr/bin/env python3
"""
Cover Art API integration tests for AudioControl system
"""

import pytest
import json
import time
import base64
from pathlib import Path
from conftest import AudioControlTestServer, TEST_PORTS
import requests

# Test configuration for cover art
TEST_CONFIG_PATH = Path(__file__).parent / "test_config_generic.json"

@pytest.fixture
def coverart_server():
    """Fixture for cover art integration tests"""
    server = AudioControlTestServer("coverart", TEST_PORTS['generic'])
    
    # Override the config path to use our custom config
    original_create_config = server.create_config
    
    def create_custom_config():
        """Create config with cover art providers enabled"""
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
        
        # Load the base config
        with open(TEST_CONFIG_PATH, 'r') as f:
            config = json.load(f)
        
        # Update port
        config["services"]["webserver"]["port"] = server.port
        
        # Update cache paths
        config["services"]["cache"]["attribute_cache_path"] = str(attributes_cache_dir.absolute())
        config["services"]["cache"]["image_cache_path"] = str(images_cache_dir.absolute())
        
        # Ensure cover art providers are enabled
        if "services" not in config:
            config["services"] = {}
        
        # Enable TheAudioDB for cover art
        if "theaudiodb" not in config["services"]:
            config["services"]["theaudiodb"] = {
                "enable": True
            }
        else:
            config["services"]["theaudiodb"]["enable"] = True
        
        # Enable Spotify for cover art (even if no tokens)
        if "spotify" not in config["services"]:
            config["services"]["spotify"] = {
                "enable": True
            }
        else:
            config["services"]["spotify"]["enable"] = True
        
        # Enable FanArt.tv for cover art (uses default API key)
        if "fanarttv" not in config["services"]:
            config["services"]["fanarttv"] = {
                "enable": True,
                "api_key": "",
                "rate_limit_ms": 500
            }
        else:
            config["services"]["fanarttv"]["enable"] = True
            if "api_key" not in config["services"]["fanarttv"]:
                config["services"]["fanarttv"]["api_key"] = ""
            if "rate_limit_ms" not in config["services"]["fanarttv"]:
                config["services"]["fanarttv"]["rate_limit_ms"] = 500
        
        # Enable MusicBrainz (required for FanArt.tv)
        if "musicbrainz" not in config["services"]:
            config["services"]["musicbrainz"] = {
                "enable": True,
                "user_agent": "AudioControl/Test",
                "rate_limit_ms": 1000
            }
        else:
            config["services"]["musicbrainz"]["enable"] = True
            if "user_agent" not in config["services"]["musicbrainz"]:
                config["services"]["musicbrainz"]["user_agent"] = "AudioControl/Test"
            if "rate_limit_ms" not in config["services"]["musicbrainz"]:
                config["services"]["musicbrainz"]["rate_limit_ms"] = 1000
        
        # Create config file
        config_file = tempfile.NamedTemporaryFile(mode='w', suffix='.json', delete=False)
        json.dump(config, config_file, indent=2)
        config_file.close()
        
        server.config_path = Path(config_file.name)
        return server.config_path
    
    server.create_config = create_custom_config
    yield server
    server.stop_server()

class TestCoverArtAPI:
    """Test class for Cover Art API functionality"""
    
    def test_coverart_providers_available(self, coverart_server):
        """Test that cover art providers are available and registered"""
        # Start the server
        success = coverart_server.start_server()
        assert success, "Failed to start audiocontrol server"
        
        # Check if there's a methods endpoint to see what providers are available
        url = f"{coverart_server.server_url}/api/coverart/methods"
        print(f"Checking providers at: {url}")
        response = requests.get(url, timeout=30)
        
        print(f"Methods endpoint status: {response.status_code}")
        print(f"Methods response: {response.text}")
        
        if response.status_code == 200:
            data = response.json()
            print(f"Available methods: {data}")
        else:
            print("Methods endpoint not available - testing basic functionality")
    
    def test_coverart_artist_metallica(self, coverart_server):
        """Test retrieving cover art for Metallica"""
        # Start the server
        success = coverart_server.start_server()
        assert success, "Failed to start audiocontrol server"
        
        # Encode "Metallica" using URL-safe base64
        artist_name = "Metallica"
        artist_b64 = base64.urlsafe_b64encode(artist_name.encode()).decode().rstrip('=')
        
        # Make API request
        url = f"{coverart_server.server_url}/api/coverart/artist/{artist_b64}"
        print(f"Making request to: {url}")
        response = requests.get(url, timeout=30)
        
        # Check response
        print(f"Response status: {response.status_code}")
        print(f"Response text: {response.text}")
        assert response.status_code == 200, f"Expected 200, got {response.status_code}: {response.text}"
        
        data = response.json()
        print(f"Response data: {data}")
        assert "results" in data, f"Response missing 'results' field: {data}"
        
        # Require that we find cover art for Metallica - this is a widely available artist
        assert len(data["results"]) > 0, f"No cover art found for {artist_name}. This test requires that at least one provider returns results for this popular artist."
        
        # Ensure we have at least one provider with actual images
        total_images = sum(len(result["images"]) for result in data["results"])
        assert total_images > 0, f"Found {len(data['results'])} provider(s) but no actual images for {artist_name}. At least one provider should return images."

        # If we do have results, validate them
        print(f"Found {len(data['results'])} provider(s) with results")        # Check the structure of results
        for result in data["results"]:
            assert "provider" in result, "Result missing 'provider' field"
            assert "images" in result, "Result missing 'images' field"
            
            # Check provider structure
            provider = result["provider"]
            assert "name" in provider, "Provider missing 'name' field"
            assert "display_name" in provider, "Provider missing 'display_name' field"
            assert isinstance(provider["name"], str), "Provider name should be string"
            assert isinstance(provider["display_name"], str), "Provider display_name should be string"
            
            # Check images structure
            images = result["images"]
            assert isinstance(images, list), "Images should be a list"
            assert len(images) > 0, f"Provider {provider['name']} returned empty images list"
            
            # Check each image structure
            for image in images:
                assert "url" in image, "Image missing 'url' field"
                assert isinstance(image["url"], str), "Image URL should be string"
                assert len(image["url"]) > 0, "Image URL should not be empty"
                
                # Check that URL is valid (starts with http/https or file://)
                assert (image["url"].startswith("http://") or 
                       image["url"].startswith("https://") or 
                       image["url"].startswith("file://") or
                       image["url"].startswith("data:")), f"Invalid URL format: {image['url']}"
                
                # Check optional metadata fields (should be present if image analysis worked)
                if "width" in image:
                    assert isinstance(image["width"], int), "Image width should be integer"
                    assert image["width"] > 0, "Image width should be positive"
                
                if "height" in image:
                    assert isinstance(image["height"], int), "Image height should be integer"  
                    assert image["height"] > 0, "Image height should be positive"
                
                if "size_bytes" in image:
                    assert isinstance(image["size_bytes"], int), "Image size_bytes should be integer"
                    assert image["size_bytes"] > 0, "Image size_bytes should be positive"
                
                if "format" in image:
                    assert isinstance(image["format"], str), "Image format should be string"
                    assert image["format"] in ["JPEG", "PNG", "GIF", "WebP", "BMP"], f"Unknown image format: {image['format']}"
        
        print(f"✓ Successfully retrieved cover art for {artist_name}")
        total_images = sum(len(result["images"]) for result in data["results"])
        print(f"  Total images: {total_images}")
        
        # Print provider details
        for result in data["results"]:
            provider_name = result["provider"]["display_name"]
            image_count = len(result["images"])
            print(f"  - {provider_name}: {image_count} image(s)")
            
            # Print image details for first few images
            for i, image in enumerate(result["images"][:2]):  # Show first 2 images per provider
                metadata_parts = []
                if "width" in image and "height" in image:
                    metadata_parts.append(f"{image['width']}x{image['height']}")
                if "size_bytes" in image:
                    size_kb = image["size_bytes"] / 1024
                    metadata_parts.append(f"{size_kb:.1f}KB")
                if "format" in image:
                    metadata_parts.append(image["format"])
                
                metadata_str = f" ({', '.join(metadata_parts)})" if metadata_parts else ""
                print(f"    {i+1}. {image['url'][:80]}...{metadata_str}")
    
    def test_coverart_empty_results(self, coverart_server):
        """Test cover art API with artist that likely has no results"""
        # Start the server
        success = coverart_server.start_server()
        assert success, "Failed to start audiocontrol server"
        
        # Encode a non-existent artist name
        artist_name = "NonExistentArtistXYZ123"
        artist_b64 = base64.urlsafe_b64encode(artist_name.encode()).decode().rstrip('=')
        
        # Make API request
        url = f"{coverart_server.server_url}/api/coverart/artist/{artist_b64}"
        response = requests.get(url, timeout=30)
        
        # Check response
        assert response.status_code == 200, f"Expected 200, got {response.status_code}: {response.text}"
        
        data = response.json()
        assert "results" in data, f"Response missing 'results' field: {data}"
        
        # Results should be empty or contain empty image lists
        for result in data["results"]:
            assert len(result["images"]) == 0, f"Expected no images for non-existent artist, got {len(result['images'])}"
    
    def test_coverart_invalid_base64(self, coverart_server):
        """Test cover art API with invalid base64 encoding"""
        # Start the server
        success = coverart_server.start_server()
        assert success, "Failed to start audiocontrol server"
        
        # Use invalid base64 string
        invalid_b64 = "invalid_base64_string!"
        
        # Make API request
        url = f"{coverart_server.server_url}/api/coverart/artist/{invalid_b64}"
        response = requests.get(url, timeout=30)
        
        # Should handle gracefully and return empty results
        assert response.status_code == 200, f"Expected 200, got {response.status_code}: {response.text}"
        
        data = response.json()
        assert "results" in data, f"Response missing 'results' field: {data}"
        assert len(data["results"]) == 0, "Expected empty results for invalid base64"

if __name__ == "__main__":
    pytest.main([__file__])
