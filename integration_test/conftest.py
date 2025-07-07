#!/usr/bin/env python3
"""
Integration tests for AudioControl system
These tests start the AudioControl server and test the API endpoints
"""

import json
import os
import signal
import subprocess
import sys
import time
from pathlib import Path
from typing import Dict, List, Optional, Any
import tempfile
import shutil
import copy

import pytest
import requests
import psutil

# Test configuration
TEST_PORTS = {
    'generic': 3001,
    'librespot': 3002,
    'activemonitor': 3003,
    'raat': 3004,
    'mpd': 3005,
    'theaudiodb': 3006,
}

# Path to static configuration file
STATIC_CONFIG_PATH = Path("test_config_generic.json")

# Global server processes
_server_processes: Dict[str, subprocess.Popen] = {}
_server_configs: Dict[str, Path] = {}

class AudioControlTestServer:
    """Helper class to manage AudioControl server instances for testing"""
    
    def __init__(self, test_name: str, port: int):
        self.test_name = test_name
        self.port = port
        self.process: Optional[subprocess.Popen] = None
        self.config_path: Optional[Path] = None
        self.cache_dir: Optional[Path] = None
        self.server_url = f"http://localhost:{port}"
        
    def create_config(self) -> Path:
        """Create a test configuration file based on the static configuration"""
        # Create cache directory paths
        cache_dir = Path(f"test_cache_{self.port}")
        cache_dir.mkdir(exist_ok=True)
        
        attributes_cache_dir = cache_dir / "attributes"
        attributes_cache_dir.mkdir(exist_ok=True)
        
        images_cache_dir = cache_dir / "images"
        images_cache_dir.mkdir(exist_ok=True)
        
        # Load static configuration file
        if not STATIC_CONFIG_PATH.exists():
            raise FileNotFoundError(f"Static configuration file not found at {STATIC_CONFIG_PATH}")
            
        with open(STATIC_CONFIG_PATH, 'r') as f:
            config = json.load(f)
        
        # Update configuration for this test instance
        
        # Update port
        config["services"]["webserver"]["port"] = self.port
        
        # Update pipe paths for different players based on OS
        for player_config in config["players"]:
            # Update librespot pipe
            if "librespot" in player_config:
                player_config["librespot"]["event_pipe"] = (
                    f"test_librespot_event_{self.port}" if os.name == 'nt' 
                    else f"/tmp/test_librespot_event_{self.port}"
                )
            
            # Update RAAT pipes
            if "raat" in player_config:
                player_config["raat"]["metadata_pipe"] = (
                    f"test_raat_metadata_{self.port}" if os.name == 'nt' 
                    else f"/tmp/test_raat_metadata_{self.port}"
                )
                player_config["raat"]["control_pipe"] = (
                    f"test_raat_control_{self.port}" if os.name == 'nt' 
                    else f"/tmp/test_raat_control_{self.port}"
                )
        
        # Update cache paths
        config["services"]["cache"]["attribute_cache_path"] = str(attributes_cache_dir.absolute())
        config["services"]["cache"]["image_cache_path"] = str(images_cache_dir.absolute())
        
        # Create config file
        self.config_path = Path(f"test_config_{self.port}.json")
        with open(self.config_path, 'w') as f:
            json.dump(config, f, indent=2)
        
        return self.config_path
    
    def create_pipes(self):
        """Create test pipes for librespot and raat"""
        if os.name == 'nt':  # Windows
            # On Windows, we use regular files instead of pipes
            # Use the current working directory instead of temp directory
            librespot_pipe = Path(f"test_librespot_event_{self.port}")
            raat_metadata_pipe = Path(f"test_raat_metadata_{self.port}")
            raat_control_pipe = Path(f"test_raat_control_{self.port}")
        else:  # Unix-like
            librespot_pipe = Path(f"/tmp/test_librespot_event_{self.port}")
            raat_metadata_pipe = Path(f"/tmp/test_raat_metadata_{self.port}")
            raat_control_pipe = Path(f"/tmp/test_raat_control_{self.port}")
        
        # Create the files/pipes
        for pipe_path in [librespot_pipe, raat_metadata_pipe, raat_control_pipe]:
            pipe_path.touch()
            print(f"Created pipe: {pipe_path.absolute()}")
    
    def get_binary_path(self) -> Path:
        """Get the path to the audiocontrol binary"""
        # Get the project root (one level up from tests directory)
        project_root = Path(__file__).parent.parent
        target_dir = os.environ.get('CARGO_TARGET_DIR', 'target')
        binary_name = 'audiocontrol.exe' if os.name == 'nt' else 'audiocontrol'
        return project_root / target_dir / 'debug' / binary_name
    
    def start_server(self) -> bool:
        """Start the AudioControl server"""
        try:
            # Kill any existing processes first
            self.kill_existing_processes()
            
            # Create config and pipes
            config_path = self.create_config()
            self.create_pipes()
            
            # Start server
            binary_path = self.get_binary_path()
            if not binary_path.exists():
                raise FileNotFoundError(f"AudioControl binary not found at {binary_path}")
            
            print(f"Starting AudioControl server on port {self.port}")
            self.process = subprocess.Popen(
                [str(binary_path), '-c', str(config_path)],
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
                text=True
            )
            
            # Wait for server to be ready
            if self.wait_for_server(timeout=40):
                print(f"Server started successfully on port {self.port}")
                return True
            else:
                print(f"Server failed to start on port {self.port}")
                self.stop_server()
                return False
                
        except Exception as e:
            print(f"Error starting server: {e}")
            return False
    
    def wait_for_server(self, timeout: int = 40) -> bool:
        """Wait for the server to be ready"""
        start_time = time.time()
        attempt = 0
        
        print(f"Waiting for server to be ready on port {self.port}...")
        
        # Wait a bit initially for server to start up (it takes ~4 seconds)
        time.sleep(3.0)
        
        while time.time() - start_time < timeout:
            # Check if process has exited
            if self.process and self.process.poll() is not None:
                # Process has exited
                stdout, stderr = self.process.communicate()
                print(f"Server process exited with code {self.process.returncode}")
                if stdout:
                    print(f"stdout: {stdout}")
                if stderr:
                    print(f"stderr: {stderr}")
                return False
            
            # Try to connect to the version API endpoint
            attempt += 1
            try:
                response = requests.get(f"{self.server_url}/api/version", timeout=5)
                if response.status_code == 200:
                    print(f"Server is ready and responding on port {self.port}")
                    return True
            except requests.exceptions.RequestException:
                # Connection failed, continue waiting
                elapsed = int(time.time() - start_time)
                print(f"Attempt {attempt} failed after {elapsed}s - server not ready yet")
            
            # Wait 2 seconds before next attempt
            time.sleep(2.0)
        
        # Timeout reached - get final output from server
        print(f"Timeout waiting for server to start on port {self.port}")
        if self.process and self.process.poll() is None:
            print("Server process is still running, getting current output...")
            # Process is still running, get its output
            try:
                self.process.terminate()
                stdout, stderr = self.process.communicate(timeout=5)
                if stdout:
                    print(f"Final stdout: {stdout}")
                if stderr:
                    print(f"Final stderr: {stderr}")
            except:
                pass
        
        return False
    
    def stop_server(self):
        """Stop the AudioControl server"""
        if self.process:
            try:
                self.process.terminate()
                self.process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                self.process.kill()
                self.process.wait()
            except:
                pass
            finally:
                self.process = None
        
        # Clean up config file
        if self.config_path and self.config_path.exists():
            self.config_path.unlink()
        
        # Clean up cache directory
        cache_dir = Path(f"test_cache_{self.port}")
        if cache_dir.exists():
            shutil.rmtree(cache_dir)
    
    @staticmethod
    def kill_existing_processes():
        """Kill any existing audiocontrol processes"""
        for proc in psutil.process_iter(['pid', 'name']):
            try:
                if proc.info['name'] and 'audiocontrol' in proc.info['name'].lower():
                    proc.kill()
            except (psutil.NoSuchProcess, psutil.AccessDenied):
                pass
    
    def api_request(self, method: str, endpoint: str, data: Any = None) -> Any:
        """Make an API request to the server"""
        url = f"{self.server_url}/{endpoint.lstrip('/')}"
        
        if method.upper() == 'GET':
            response = requests.get(url, timeout=10)
            response.raise_for_status()
            return response.json()
        elif method.upper() == 'POST':
            response = requests.post(url, json=data, timeout=10)
            response.raise_for_status()
            return response.json()
        else:
            raise ValueError(f"Unsupported HTTP method: {method}")
    
    def api_request_with_error_handling(self, method: str, endpoint: str, data: Any = None) -> Any:
        """Make an API request to the server with custom error handling"""
        url = f"{self.server_url}/{endpoint.lstrip('/')}"
        
        if method.upper() == 'GET':
            response = requests.get(url, timeout=10)
            # Don't raise for HTTP errors - let the caller handle them
            return response
        elif method.upper() == 'POST':
            response = requests.post(url, json=data, timeout=10)
            # Don't raise for HTTP errors - let the caller handle them
            return response
        else:
            raise ValueError(f"Unsupported HTTP method: {method}")
    
    def get_players(self) -> Dict[str, Any]:
        """Get all players from the API"""
        response = self.api_request('GET', '/api/players')
        # API returns a dict with 'players' key containing an array of player objects
        # Each player has an 'id' field
        
        # Convert the list of players into a dict indexed by player id for backwards compatibility
        if 'players' in response:
            players_dict = {}
            for player in response['players']:
                if 'id' in player:
                    players_dict[player['id']] = player
            return players_dict
        
        return response
    
    def get_now_playing(self) -> Dict[str, Any]:
        """Get now playing information"""
        return self.api_request('GET', '/api/now-playing')
    
    def send_player_event(self, player_name: str, event_data: Dict[str, Any]) -> Dict[str, Any]:
        """Send an event to a player using the acr_send_update tool"""
        import subprocess
        import json
        
        # Build the command to call audiocontrol_send_update
        cmd = ["cargo", "run", "--bin", "audiocontrol_send_update", "--"]
        cmd.extend(["--baseurl", f"http://localhost:{self.port}/api"])
        cmd.append(player_name)
        
        # Convert event_data to acr_send_update arguments based on new subcommand structure
        event_type = event_data.get("type", "unknown")
        
        if event_type == "state_changed":
            state = event_data.get("state", "Stopped")
            # Convert lowercase to PascalCase for enum
            state = state.capitalize()
            cmd.extend(["state", state])
            
        elif event_type == "metadata_changed" or event_type == "song_changed":
            cmd.append("song")
            metadata = event_data.get("metadata", event_data.get("song", {}))
            if metadata.get("title"):
                cmd.extend(["--title", metadata["title"]])
            if metadata.get("artist"):
                cmd.extend(["--artist", metadata["artist"]])
            if metadata.get("album"):
                cmd.extend(["--album", metadata["album"]])
            if metadata.get("duration"):
                cmd.extend(["--length", str(metadata["duration"])])
            if metadata.get("uri"):
                cmd.extend(["--uri", metadata["uri"]])
            # Add state if specified, otherwise defaults to Playing
            if "state" in event_data:
                state = event_data["state"].capitalize()
                cmd.extend(["--state", state])
                
        elif event_type == "position_changed":
            position = event_data.get("position", 0.0)
            cmd.extend(["position", str(position)])
            
        elif event_type == "shuffle_changed":
            shuffle = event_data.get("enabled", event_data.get("shuffle", False))
            cmd.extend(["shuffle", str(shuffle).lower()])
            
        elif event_type == "loop_mode_changed":
            mode = event_data.get("mode", "None")
            # Convert mode names to match Rust enum
            if mode == "all" or mode == "playlist":
                mode = "Playlist"
            elif mode == "one" or mode == "track" or mode == "song":
                mode = "Track"
            else:
                mode = "None"
            cmd.extend(["loop", mode])
        else:
            # For unknown event types, default to state change
            print(f"Unknown event type '{event_type}', defaulting to state change")
            state = event_data.get("state", "Stopped").capitalize()
            cmd.extend(["state", state])
        
        # Debug output
        print(f"Calling audiocontrol_send_update with command: {' '.join(cmd)}")
        
        # Execute the command
        try:
            # Use the parent directory (project root) as working directory
            project_root = Path(__file__).parent.parent
            result = subprocess.run(cmd, cwd=project_root, capture_output=True, text=True, timeout=30)
            
            if result.returncode == 0:
                print(f"audiocontrol_send_update output: {result.stdout}")
                return {"success": True, "message": "Update sent successfully"}
            else:
                print(f"audiocontrol_send_update error: {result.stderr}")
                return {"success": False, "message": f"Tool failed with exit code {result.returncode}: {result.stderr}"}
                
        except subprocess.TimeoutExpired:
            return {"success": False, "message": "Tool execution timed out"}
        except Exception as e:
            return {"success": False, "message": f"Tool execution failed: {str(e)}"}
    
    def reset_player_state(self, player_id: str = "test_player"):
        """Reset a player to a known state"""
        reset_events = [
            {"type": "state_changed", "state": "stopped"},
            {"type": "shuffle_changed", "enabled": False},
            {"type": "loop_mode_changed", "mode": "none"},
            {"type": "position_changed", "position": 0.0},
        ]
        
        for event in reset_events:
            try:
                self.send_player_event(player_id, event)
                time.sleep(0.1)  # Small delay between events for better reliability
            except Exception as e:
                print(f"Warning: Failed to send reset event {event} to player {player_id}: {e}")
        
        time.sleep(0.5)  # Longer wait for reset to complete

# Global cleanup function
def cleanup_all_servers():
    """Clean up all test servers and temporary files"""
    AudioControlTestServer.kill_existing_processes()
    
    # Clean up config files and cache directories
    for port in TEST_PORTS.values():
        config_path = Path(f"test_config_{port}.json")
        if config_path.exists():
            try:
                config_path.unlink()
                print(f"Removed config file: {config_path}")
            except Exception as e:
                print(f"Warning: Failed to remove {config_path}: {e}")
        
        cache_dir = Path(f"test_cache_{port}")
        if cache_dir.exists():
            try:
                shutil.rmtree(cache_dir)
                print(f"Removed cache directory: {cache_dir}")
            except Exception as e:
                print(f"Warning: Failed to remove {cache_dir}: {e}")
    
    # Clean up pipe files for librespot, raat, etc.
    pipe_patterns = [
        "test_librespot_event_*",
        "test_raat_metadata_*",
        "test_raat_control_*"
    ]
    
    # Clean up in both the current directory and /tmp (for Unix systems)
    search_dirs = [Path(".")]
    if os.name != 'nt':  # Add /tmp for Unix-like systems
        search_dirs.append(Path("/tmp"))
    
    for directory in search_dirs:
        for pattern in pipe_patterns:
            for pipe_file in directory.glob(pattern):
                try:
                    pipe_file.unlink()
                    print(f"Removed pipe file: {pipe_file}")
                except Exception as e:
                    print(f"Warning: Failed to remove {pipe_file}: {e}")
    
    # Clean up Python cache files
    try:
        pycache_dir = Path("__pycache__")
        if pycache_dir.exists():
            shutil.rmtree(pycache_dir)
            print(f"Removed Python cache directory: {pycache_dir}")
    except Exception as e:
        print(f"Warning: Failed to remove Python cache: {e}")
    
    # Clean up any leftover output files
    other_temp_files = [
        "output.txt"
    ]
    
    for temp_file in other_temp_files:
        file_path = Path(temp_file)
        if file_path.exists():
            try:
                file_path.unlink()
                print(f"Removed temp file: {file_path}")
            except Exception as e:
                print(f"Warning: Failed to remove {file_path}: {e}")

# Pytest fixtures
@pytest.fixture(scope="session", autouse=True)
def setup_and_cleanup():
    """Setup and cleanup for the entire test session"""
    cleanup_all_servers()
    yield
    cleanup_all_servers()

@pytest.fixture
def generic_server():
    """Fixture for generic integration tests"""
    server = AudioControlTestServer("generic", TEST_PORTS['generic'])
    assert server.start_server(), "Failed to start generic test server"
    yield server
    server.stop_server()

@pytest.fixture
def librespot_server():
    """Fixture for librespot integration tests"""
    server = AudioControlTestServer("librespot", TEST_PORTS['librespot'])
    assert server.start_server(), "Failed to start librespot test server"
    yield server
    server.stop_server()

@pytest.fixture
def activemonitor_server():
    """Fixture for active monitor integration tests"""
    server = AudioControlTestServer("activemonitor", TEST_PORTS['activemonitor'])
    assert server.start_server(), "Failed to start activemonitor test server"
    yield server
    server.stop_server()

@pytest.fixture
def raat_server():
    """Fixture for RAAT integration tests"""
    server = AudioControlTestServer("raat", TEST_PORTS['raat'])
    assert server.start_server(), "Failed to start RAAT test server"
    yield server
    server.stop_server()

@pytest.fixture
def mpd_server():
    """Fixture for MPD integration tests"""
    server = AudioControlTestServer("mpd", TEST_PORTS['mpd'])
    assert server.start_server(), "Failed to start MPD test server"
    yield server
    server.stop_server()

if __name__ == "__main__":
    # Run cleanup if executed directly
    cleanup_all_servers()
