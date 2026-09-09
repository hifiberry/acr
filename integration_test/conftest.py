#!/usr/bin/env python3
"""
Integration tests for AudioControl system
These tests start the AudioControl server and test the API endpoints
"""

import hashlib
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
    'generic': 18080,
    'librespot': 18080,
    'activemonitor': 18080,
    'raat': 18080,
    'theaudiodb': 18080,
    'fanarttv': 18080,
    'volume': 18080,
    'coverart': 18080,
    'cache': 18080,
    'metadata': 18080,
    # The two-daemon arrangement (test_two_daemons.py). Two ports, four apart,
    # mirroring the shipped 1080/1084 so that a config copied from either
    # direction is recognisable. The metadata daemon's readiness is 18084, its
    # own port; it never learns the player daemon's, which reaches it only
    # because `core.url` is written out.
    'two_daemons': 18080,
    'two_daemons_metadata': 18084,
}

# Path configurations for different test types
TEST_CONFIGS = {
    'generic': Path(__file__).parent / "test_config_generic.json",
    'librespot': Path(__file__).parent / "test_config_librespot.json",
    'activemonitor': Path(__file__).parent / "test_config_activemonitor.json",
    'raat': Path(__file__).parent / "test_config_generic.json",
    'theaudiodb': Path(__file__).parent / "test_config_theaudiodb.json",
    'fanarttv': Path(__file__).parent / "test_config_fanarttv.json",
    'volume': Path(__file__).parent / "test_config_volume.json",
    'coverart': Path(__file__).parent / "test_config_generic.json",
    'cache': Path(__file__).parent / "test_config_cache.json",
    'metadata': Path(__file__).parent / "test_config_metadata.json",
    'two_daemons': Path(__file__).parent / "test_config_two_daemons_player.json",
}

# The metadata daemon reads a file of its own shape -- no players, no
# action_plugins, and a `core` section it refuses to start without -- so it is
# not in TEST_CONFIGS, which is indexed by player-daemon test name.
METADATA_DAEMON_CONFIG = Path(__file__).parent / "test_config_two_daemons_metadata.json"

# Which daemon owns which credential, per
# doc/specs/2026-09-08-two-processes.md. Names only: no test here handles a
# credential value.
SPOTIFY_KEYS = [
    "spotify_access_token",
    "spotify_refresh_token",
    "spotify_token_expiry",
    "spotify_user_id",
    "spotify_display_name",
]
LASTFM_KEYS = ["lastfm_session_key", "lastfm_username"]

# Default path to static configuration file
STATIC_CONFIG_PATH = Path(__file__).parent / "test_config_generic.json"

# Global server processes
_server_processes: Dict[str, subprocess.Popen] = {}
_server_configs: Dict[str, Path] = {}

def deep_merge(target: Dict[str, Any], overrides: Dict[str, Any]) -> Dict[str, Any]:
    """Merge `overrides` into `target` in place, recursing into dicts.

    A plain `dict.update` would replace a whole config section, which is the
    wrong thing here: overriding `services.security_store.path` must not throw
    away the rest of `services`.
    """
    for key, value in overrides.items():
        if isinstance(value, dict) and isinstance(target.get(key), dict):
            deep_merge(target[key], value)
        else:
            target[key] = value
    return target


class AudioControlTestServer:
    """Helper class to manage AudioControl server instances for testing"""
    
    def __init__(self, test_name: str, port: int,
                 config_overrides: Optional[Dict[str, Any]] = None):
        self.test_name = test_name
        self.port = port
        # Merged into the loaded configuration by create_config, so a fixture
        # can give this daemon paths of its own without a second config file.
        # The two-daemon fixture needs it for the credential store and the
        # settings database: those are per-daemon files, and a test about two
        # daemons not overwriting each other's is worthless if they share one.
        self.config_overrides = config_overrides or {}
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
        
        # Choose the appropriate config file for this test type
        config_file = TEST_CONFIGS.get(self.test_name, STATIC_CONFIG_PATH)
        
        # Load static configuration file
        if not config_file.exists():
            raise FileNotFoundError(f"Configuration file not found at {config_file}")
            
        with open(config_file, 'r') as f:
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
        if "datastore" in config["services"]:
            if "attribute_cache" in config["services"]["datastore"]:
                config["services"]["datastore"]["attribute_cache"]["dbfile"] = str(attributes_cache_dir / "cache_attributes.db")
            if "image_cache_path" in config["services"]["datastore"]:
                config["services"]["datastore"]["image_cache_path"] = str(images_cache_dir.absolute())
        elif "cache" in config["services"]:
            # Fallback for older config structure
            config["services"]["cache"]["attribute_cache_path"] = str(attributes_cache_dir.absolute())
            config["services"]["cache"]["image_cache_path"] = str(images_cache_dir.absolute())
        
        deep_merge(config, self.config_overrides)

        # Create config file
        self.config_path = Path(f"test_config_{self.port}.json")
        with open(self.config_path, 'w') as f:
            json.dump(config, f, indent=2)
        
        return self.config_path
    
    def create_pipes(self):
        """Create test pipes for librespot and raat"""
        # Load the appropriate configuration to see which players are actually configured
        config_file = TEST_CONFIGS.get(self.test_name, STATIC_CONFIG_PATH)
        with open(config_file, 'r') as f:
            config = json.load(f)
        
        # Only create pipes for players that are actually in the configuration
        for player_config in config["players"]:
            if "librespot" in player_config:
                if os.name == 'nt':  # Windows
                    librespot_pipe = Path(f"test_librespot_event_{self.port}")
                else:  # Unix-like
                    librespot_pipe = Path(f"/tmp/test_librespot_event_{self.port}")
                librespot_pipe.touch()
                print(f"Created pipe: {librespot_pipe.absolute()}")
            
            if "raat" in player_config:
                if os.name == 'nt':  # Windows
                    raat_metadata_pipe = Path(f"test_raat_metadata_{self.port}")
                    raat_control_pipe = Path(f"test_raat_control_{self.port}")
                else:  # Unix-like
                    raat_metadata_pipe = Path(f"/tmp/test_raat_metadata_{self.port}")
                    raat_control_pipe = Path(f"/tmp/test_raat_control_{self.port}")
                
                raat_metadata_pipe.touch()
                raat_control_pipe.touch()
                print(f"Created pipe: {raat_metadata_pipe.absolute()}")
                print(f"Created pipe: {raat_control_pipe.absolute()}")
    
    def get_binary_path(self) -> Path:
        """Get the path to the audiocontrol binary"""
        # Get the project root (one level up from tests directory)
        project_root = Path(__file__).parent.parent
        target_dir = os.environ.get('CARGO_TARGET_DIR', 'target')
        binary_name = 'audiocontrol.exe' if os.name == 'nt' else 'audiocontrol'
        
        # Try release first, then debug
        release_path = project_root / target_dir / 'release' / binary_name
        debug_path = project_root / target_dir / 'debug' / binary_name
        
        if release_path.exists():
            return release_path
        elif debug_path.exists():
            return debug_path
        else:
            # Default to release path for error reporting
            return release_path
    
    def get_tool_binary_path(self, tool_name: str) -> Path:
        """Get the path to a command line tool binary (like audiocontrol_send_update)"""
        # Get the project root (one level up from tests directory)
        project_root = Path(__file__).parent.parent
        target_dir = os.environ.get('CARGO_TARGET_DIR', 'target')
        binary_name = f'{tool_name}.exe' if os.name == 'nt' else tool_name
        
        # Try release first, then debug
        release_path = project_root / target_dir / 'release' / binary_name
        debug_path = project_root / target_dir / 'debug' / binary_name
        
        if release_path.exists():
            return release_path
        elif debug_path.exists():
            return debug_path
        else:
            # Return None to indicate we need to build
            return None

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
    
    def api_request(self, method: str, endpoint: str, data: Any = None, json: Any = None, expect_error: bool = False) -> Any:
        """Make an API request to the server"""
        url = f"{self.server_url}/{endpoint.lstrip('/')}"
        
        # Choose the data format
        request_data = json if json is not None else data
        
        if method.upper() == 'GET':
            response = requests.get(url, timeout=10)
        elif method.upper() == 'POST':
            response = requests.post(url, json=request_data, timeout=10)
        elif method.upper() == 'DELETE':
            response = requests.delete(url, json=request_data, timeout=10)
        else:
            raise ValueError(f"Unsupported HTTP method: {method}")
        
        # Handle error cases
        if expect_error:
            if response.status_code >= 400:
                try:
                    return response.json()
                except ValueError:
                    return {"error": f"HTTP {response.status_code}: {response.text}"}
            else:
                # Expected an error but didn't get one
                try:
                    return response.json()
                except ValueError:
                    return {"unexpected_success": True, "status": response.status_code}
        else:
            response.raise_for_status()
            try:
                return response.json()
            except ValueError as e:
                print(f"Error parsing JSON response from {url}: {e}")
                print(f"Response status: {response.status_code}")
                print(f"Response text: {response.text}")
                return None
    
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
    
    def get_players_raw(self) -> Dict[str, Any]:
        """Get all players from the API in raw format"""
        return self.api_request('GET', '/api/players')
    
    def get_now_playing(self) -> Dict[str, Any]:
        """Get now playing information"""
        return self.api_request('GET', '/api/now-playing')
    
    def send_player_event(self, player_name: str, event_data: Dict[str, Any]) -> Dict[str, Any]:
        """Send an event to a player using the appropriate tool based on player type"""
        
        # For librespot players, use audiocontrol_notify_librespot
        if "librespot" in player_name.lower():
            return self.send_librespot_player_event(player_name, event_data)
        else:
            # For other players, use audiocontrol_send_update
            return self.send_generic_player_event(player_name, event_data)
    
    def send_librespot_player_event(self, player_name: str, event_data: Dict[str, Any]) -> Dict[str, Any]:
        """Send an event to a librespot player using audiocontrol_notify_librespot"""
        import subprocess
        import os
        
        # Check if we have an existing binary to use
        tool_binary_path = self.get_tool_binary_path('audiocontrol_notify_librespot')
        
        if tool_binary_path:
            # Use the existing binary directly
            cmd = [str(tool_binary_path)]
            print(f"Using existing binary: {tool_binary_path}")
        else:
            # Build the command to call audiocontrol_notify_librespot via cargo run
            cmd = ["cargo", "run", "--bin", "audiocontrol_notify_librespot", "--"]
            print("Using cargo run to build and execute audiocontrol_notify_librespot")
        
        cmd.extend(["--baseurl", f"http://localhost:{self.port}/api"])
        cmd.extend(["--player-name", player_name])
        
        # Set up environment variables based on event type
        env = os.environ.copy()
        event_type = event_data.get("type", "unknown")
        env_vars = {}
        
        if event_type == "state_changed":
            state = event_data.get("state", "stopped").lower()
            env["PLAYER_EVENT"] = state  # playing, paused, stopped
            
        elif event_type == "metadata_changed" or event_type == "song_changed":
            env["PLAYER_EVENT"] = "track_changed"
            metadata = event_data.get("metadata", event_data.get("song", {}))
            
            if metadata.get("title"):
                env_vars["NAME"] = metadata["title"]
            if metadata.get("artist"):
                env_vars["ARTISTS"] = metadata["artist"]
            if metadata.get("album"):
                env_vars["ALBUM"] = metadata["album"]
            if metadata.get("duration"):
                # Convert to milliseconds
                duration_ms = int(metadata["duration"] * 1000)
                env_vars["DURATION_MS"] = str(duration_ms)
            if metadata.get("uri"):
                env_vars["URI"] = metadata["uri"]
            if metadata.get("cover_art_url"):
                env_vars["COVERS"] = metadata["cover_art_url"]
                
        elif event_type == "position_changed":
            env["PLAYER_EVENT"] = "seeked"
            position = event_data.get("position", 0.0)
            # Convert to milliseconds
            position_ms = int(position * 1000)
            env_vars["POSITION_MS"] = str(position_ms)
            
        elif event_type == "shuffle_changed":
            env["PLAYER_EVENT"] = "shuffle_changed"
            shuffle = event_data.get("enabled", event_data.get("shuffle", False))
            env_vars["SHUFFLE"] = str(shuffle).lower()
            
        elif event_type == "loop_mode_changed":
            env["PLAYER_EVENT"] = "repeat_changed"
            mode = event_data.get("mode", "none").lower()
            
            # Convert mode to repeat settings
            if mode == "none":
                env_vars["REPEAT"] = "false"
                env_vars["REPEAT_TRACK"] = "false"
            elif mode == "track" or mode == "song":
                env_vars["REPEAT"] = "true"
                env_vars["REPEAT_TRACK"] = "true"
            elif mode == "playlist" or mode == "all":
                env_vars["REPEAT"] = "true"
                env_vars["REPEAT_TRACK"] = "false"
            else:
                env_vars["REPEAT"] = "false"
                env_vars["REPEAT_TRACK"] = "false"
                
        else:
            # For unknown event types, default to ping
            env["PLAYER_EVENT"] = "preloading"
        
        # Add environment variables
        env.update(env_vars)
        
        # Debug output
        print(f"Calling audiocontrol_notify_librespot with command: {' '.join(cmd)}")
        print(f"Environment: PLAYER_EVENT={env.get('PLAYER_EVENT')}")
        for key, value in env_vars.items():
            print(f"Environment: {key}={value}")
        
        # Execute the command
        try:
            # Use the parent directory (project root) as working directory
            project_root = Path(__file__).parent.parent
            result = subprocess.run(cmd, cwd=project_root, capture_output=True, text=True, timeout=30, env=env)
            
            if result.returncode == 0:
                print(f"audiocontrol_notify_librespot output: {result.stdout}")
                return {"success": True, "message": "Update sent successfully"}
            else:
                print(f"audiocontrol_notify_librespot error: {result.stderr}")
                return {"success": False, "message": f"Tool failed with exit code {result.returncode}: {result.stderr}"}
                
        except subprocess.TimeoutExpired:
            return {"success": False, "message": "Tool execution timed out"}
        except Exception as e:
            return {"success": False, "message": f"Tool execution failed: {str(e)}"}
    
    def send_generic_player_event(self, player_name: str, event_data: Dict[str, Any]) -> Dict[str, Any]:
        """Send an event to a generic player using audiocontrol_send_update"""
        import subprocess
        import json
        
        # Check if we have an existing binary to use
        tool_binary_path = self.get_tool_binary_path('audiocontrol_send_update')
        
        if tool_binary_path:
            # Use the existing binary directly
            cmd = [str(tool_binary_path)]
            print(f"Using existing binary: {tool_binary_path}")
        else:
            # Build the command to call audiocontrol_send_update via cargo run
            cmd = ["cargo", "run", "--bin", "audiocontrol_send_update", "--"]
            print("Using cargo run to build and execute audiocontrol_send_update")
        
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

    def send_librespot_event(self, player_name: str, event_type: str, env_vars: Dict[str, str] = None) -> Dict[str, Any]:
        """Send an event to a player using the audiocontrol_notify_librespot tool"""
        import subprocess
        import os
        
        # Check if we have an existing binary to use
        tool_binary_path = self.get_tool_binary_path('audiocontrol_notify_librespot')
        
        if tool_binary_path:
            # Use the existing binary directly
            cmd = [str(tool_binary_path)]
            print(f"Using existing binary: {tool_binary_path}")
        else:
            # Build the command to call audiocontrol_notify_librespot via cargo run
            cmd = ["cargo", "run", "--bin", "audiocontrol_notify_librespot", "--"]
            print("Using cargo run to build and execute audiocontrol_notify_librespot")
        
        cmd.extend(["--baseurl", f"http://localhost:{self.port}/api"])
        cmd.extend(["--player-name", player_name])
        
        # Set up environment variables
        env = os.environ.copy()
        env["PLAYER_EVENT"] = event_type
        
        if env_vars:
            env.update(env_vars)
        
        # Debug output
        print(f"Calling audiocontrol_notify_librespot with command: {' '.join(cmd)}")
        print(f"Environment: PLAYER_EVENT={event_type}")
        if env_vars:
            for key, value in env_vars.items():
                print(f"Environment: {key}={value}")
        
        # Execute the command
        try:
            # Use the parent directory (project root) as working directory
            project_root = Path(__file__).parent.parent
            result = subprocess.run(cmd, cwd=project_root, capture_output=True, text=True, timeout=30, env=env)
            
            if result.returncode == 0:
                print(f"audiocontrol_notify_librespot output: {result.stdout}")
                return {"success": True, "message": "Event sent successfully"}
            else:
                print(f"audiocontrol_notify_librespot error: {result.stderr}")
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

class MetadataDaemonTestServer:
    """The metadata daemon (`audiocontrol-metadata`) as a process of its own.

    Deliberately not a subclass of `AudioControlTestServer`. It shares almost
    nothing with it: it has no players and no pipes, it reads a configuration
    file of a different shape, and -- the point of the class -- **its readiness
    is its own port**. Waiting for the player daemon's port would say nothing
    about it: the seam is one-way, so this daemon opens every connection and
    the player daemon holds no address for it. It probes `GET /api/version`
    there for up to 30 s itself and then starts anyway, which means "the player
    daemon answers" is neither necessary nor sufficient for this one to serve.

    Every wait here is bounded and reports what it was waiting for. A hung
    fixture in CI is worse than a failing one.
    """

    # Ignition brings up the caches, the providers and the settings database
    # before Rocket binds, and a cold container is slower than a warm laptop.
    READY_TIMEOUT = 40
    STOP_TIMEOUT = 15

    def __init__(self, port: int, core_port: int, work_dir: Path,
                 config_overrides: Optional[Dict[str, Any]] = None,
                 config_path: Optional[Path] = None):
        self.port = port
        self.core_port = core_port
        self.work_dir = Path(work_dir)
        self.config_overrides = config_overrides or {}
        # A configuration file to use exactly as it is, for the tests that are
        # *about* the file: they build one and the daemon must be given that
        # one, not a regenerated version of it.
        self.given_config_path = Path(config_path) if config_path else None
        self.process: Optional[subprocess.Popen] = None
        self.config_path = self.given_config_path or self.work_dir / "metadata.json"
        self.server_url = f"http://127.0.0.1:{port}"

    @staticmethod
    def binary_path() -> Path:
        """Where `cargo build --workspace` puts the metadata daemon.

        `--workspace` matters: the binary is a `[[bin]]` of
        `crates/audiocontrol-metadata`, not of the root package, so a plain
        `cargo build` does not produce it at all.
        """
        project_root = Path(__file__).parent.parent
        target_dir = os.environ.get('CARGO_TARGET_DIR', 'target')
        binary_name = 'audiocontrol-metadata.exe' if os.name == 'nt' else 'audiocontrol-metadata'

        release_path = project_root / target_dir / 'release' / binary_name
        debug_path = project_root / target_dir / 'debug' / binary_name

        if release_path.exists():
            return release_path
        return debug_path

    def create_config(self) -> Path:
        """Write this run's `metadata.json` into the working directory."""
        if self.given_config_path:
            return self.given_config_path

        self.work_dir.mkdir(parents=True, exist_ok=True)

        with open(METADATA_DAEMON_CONFIG, 'r') as f:
            config = json.load(f)

        # The ports have one source of truth, TEST_PORTS, so that a test can
        # move them without editing JSON in two places.
        config["services"]["webserver"]["port"] = self.port
        config["services"]["core"]["url"] = f"http://127.0.0.1:{self.core_port}/api"
        deep_merge(config, self.config_overrides)

        with open(self.config_path, 'w') as f:
            json.dump(config, f, indent=2)

        return self.config_path

    def start(self) -> bool:
        """Start the daemon and wait for *its own* port to answer."""
        binary_path = self.binary_path()
        if not binary_path.exists():
            raise FileNotFoundError(
                f"metadata daemon binary not found at {binary_path}. It is a "
                "[[bin]] of crates/audiocontrol-metadata, so build with "
                "`cargo build --workspace`"
            )

        self.work_dir.mkdir(parents=True, exist_ok=True)
        self.create_config()

        print(f"Starting the metadata daemon on port {self.port}")
        # Relative paths in the configuration -- the caches, the settings
        # database -- resolve against this directory, so a run writes nothing
        # outside the test's own temporary tree.
        self.process = subprocess.Popen(
            [str(binary_path), '-c', str(self.config_path)],
            cwd=str(self.work_dir),
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )

        if self.wait_for_server(timeout=self.READY_TIMEOUT):
            print(f"The metadata daemon is serving on port {self.port}")
            return True

        print(f"The metadata daemon failed to start on port {self.port}")
        self.stop()
        return False

    @property
    def readiness_url(self) -> str:
        """`/api/metadata/capabilities`, served only by this daemon.

        `/api/version` would be the obvious choice and is the wrong one: it is
        the *player* daemon's route, the one this daemon polls, and it is not
        mounted here. `/api/metadata/capabilities` comes from
        `api::standalone_routes()`, which only this daemon mounts.
        """
        return f"{self.server_url}/api/metadata/capabilities"

    def wait_for_server(self, timeout: int = READY_TIMEOUT) -> bool:
        deadline = time.time() + timeout
        while time.time() < deadline:
            if self.process and self.process.poll() is not None:
                print(f"The metadata daemon exited with code {self.process.returncode}")
                return False
            try:
                if requests.get(self.readiness_url, timeout=5).status_code == 200:
                    return True
            except requests.exceptions.RequestException:
                pass
            time.sleep(0.5)
        return False

    def stop(self):
        """Terminate the daemon, killing it if it will not go.

        This waits for the *process*. A test that goes on to assert something
        about the daemon being down wants `wait_until_unreachable` as well:
        the process ending and the port refusing are not the same instant.
        """
        if self.process:
            try:
                self.process.terminate()
                self.process.wait(timeout=self.STOP_TIMEOUT)
            except subprocess.TimeoutExpired:
                self.process.kill()
                self.process.wait(timeout=self.STOP_TIMEOUT)
            except Exception:
                pass
            finally:
                self.process = None

    def wait_until_unreachable(self, timeout: int = 15):
        """Block until this daemon's port refuses connections.

        The evidence a test needs before asserting anything about the daemon
        being down: a graceful shutdown takes a moment, and a request that
        arrived during it would be answered.
        """
        deadline = time.time() + timeout
        last_status = None
        while time.time() < deadline:
            try:
                last_status = requests.get(self.readiness_url, timeout=2).status_code
            except requests.exceptions.RequestException:
                return
            time.sleep(0.25)

        raise AssertionError(
            f"the metadata daemon is still answering on port {self.port} "
            f"{timeout}s after being stopped (last status {last_status})"
        )


def run_metadata_daemon_to_completion(config_path: Path, work_dir: Path,
                                      timeout: int = 30):
    """Run the metadata daemon until it exits, and report how.

    For the configurations that must not start. Returns `(returncode,
    stderr)`; `returncode` is `None` if the daemon was still running when
    `timeout` expired, which is a *result* rather than a hang -- the caller
    asserts on it, so "it started after all" fails a test instead of blocking
    CI.
    """
    process = subprocess.Popen(
        [str(MetadataDaemonTestServer.binary_path()), '-c', str(config_path)],
        cwd=str(work_dir),
        stdout=subprocess.DEVNULL,
        stderr=subprocess.PIPE,
        text=True,
    )
    try:
        _, stderr = process.communicate(timeout=timeout)
        return process.returncode, stderr
    except subprocess.TimeoutExpired:
        process.kill()
        _, stderr = process.communicate(timeout=timeout)
        return None, stderr


def seed_security_store(path: Path, keys: List[str]):
    """Write a security store holding `keys`, with placeholder values.

    The store's on-disk shape is `{"values": {key: ciphertext}, "modified":
    {...}}` -- the keys are plaintext, only the values are encrypted -- so a
    test can seed and inspect which credentials a file holds without ever
    handling one. The values below are not credentials and are not valid
    ciphertext: the daemon logs a decryption failure when it reads them, which
    is the point at which a test that needed a real credential would have to
    stop. None of these tests needs one.
    """
    now = int(time.time())
    path.parent.mkdir(parents=True, exist_ok=True)
    with open(path, 'w') as f:
        json.dump({
            "values": {key: "placeholder-not-a-credential" for key in keys},
            "modified": {key: now for key in keys},
            "version": 1,
            "last_updated": now,
        }, f, indent=2)


def security_store_keys(path: Path) -> List[str]:
    """The names of the credentials a store file holds, never their values."""
    with open(path, 'r') as f:
        return sorted(json.load(f)["values"].keys())


def digest(path: Path) -> str:
    """A digest of a file, so a test can say "unchanged" without quoting it.

    The credential stores hold encrypted values. Comparing digests keeps them
    out of assertion messages and out of CI logs.
    """
    return hashlib.sha256(path.read_bytes()).hexdigest()


class TwoDaemons:
    """The pair, as one test sees them."""

    def __init__(self, player: AudioControlTestServer,
                 metadata: MetadataDaemonTestServer,
                 player_store: Path, metadata_store: Path):
        self.player = player
        self.metadata = metadata
        self.player_store = player_store
        self.metadata_store = metadata_store


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
def cache_server():
    """Fixture for cache integration tests"""
    server = AudioControlTestServer("cache", TEST_PORTS['cache'])
    assert server.start_server(), "Failed to start cache test server"
    yield server
    server.stop_server()

@pytest.fixture
def metadata_server():
    """Fixture for the player/metadata seam integration tests"""
    server = AudioControlTestServer("metadata", TEST_PORTS['metadata'])
    assert server.start_server(), "Failed to start metadata test server"
    yield server
    server.stop_server()

@pytest.fixture
def two_daemons(tmp_path):
    """Both daemons, each with its own port, its own stores and its own
    credential file.

    The player daemon starts first, and not because the metadata daemon needs
    it to: the shipped unit has `After=audiocontrol.service` and no
    `Requires=`, so this order is the ordinary case rather than a requirement,
    and the metadata daemon would come up either way after its 30 s probe. It
    is first here because starting it first is what `kill_existing_processes`
    allows -- that call, inside `start_server`, kills every process whose name
    contains "audiocontrol", the metadata daemon included.

    Both credential stores are seeded before either daemon starts, because
    each daemon loads its store once at start-up: a file written afterwards
    would not be in the memory that a later write serialises back out, which
    is the very hazard under test.
    """
    player_store = tmp_path / "player" / "security_store.json"
    metadata_store = tmp_path / "metadata" / "security_store.json"

    # The five keys the player daemon owns and the two the metadata daemon
    # owns. Placeholders, not credentials.
    seed_security_store(player_store, SPOTIFY_KEYS)
    seed_security_store(metadata_store, LASTFM_KEYS)

    player = AudioControlTestServer(
        "two_daemons", TEST_PORTS['two_daemons'],
        config_overrides={"services": {
            "security_store": {"path": str(player_store)},
            "settingsdb": {"path": str(tmp_path / "player")},
        }},
    )
    assert player.start_server(), "Failed to start the player daemon"

    metadata = MetadataDaemonTestServer(
        port=TEST_PORTS['two_daemons_metadata'],
        core_port=TEST_PORTS['two_daemons'],
        work_dir=tmp_path / "metadata",
        config_overrides={"services": {
            "security_store": {"path": str(metadata_store)},
        }},
    )
    assert metadata.start(), "Failed to start the metadata daemon"

    try:
        yield TwoDaemons(player, metadata, player_store, metadata_store)
    finally:
        metadata.stop()
        player.stop_server()


if __name__ == "__main__":
    # Run cleanup if executed directly
    cleanup_all_servers()
