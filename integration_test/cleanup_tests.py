#!/usr/bin/env python3
"""
Utility script to clean up all temporary files created by tests
"""

import os
import shutil
import glob
from pathlib import Path

def cleanup_test_files():
    """Clean up all temporary files created during tests"""
    print("Cleaning up test artifacts...")
    
    # Find and remove test config files
    for config_file in glob.glob("test_config_*.json"):
        try:
            os.remove(config_file)
            print(f"Removed: {config_file}")
        except Exception as e:
            print(f"Error removing {config_file}: {e}")
    
    # Find and remove test cache directories
    for cache_dir in glob.glob("test_cache_*"):
        try:
            shutil.rmtree(cache_dir)
            print(f"Removed: {cache_dir}")
        except Exception as e:
            print(f"Error removing {cache_dir}: {e}")
    
    # Find and remove pipe files
    pipe_patterns = [
        "test_librespot_event_*",
        "test_raat_metadata_*",
        "test_raat_control_*"
    ]
    
    # Check both current directory and /tmp (for Unix systems)
    search_dirs = ["."]
    if os.name != 'nt':
        search_dirs.append("/tmp")
    
    for directory in search_dirs:
        for pattern in pipe_patterns:
            for pipe_file in glob.glob(os.path.join(directory, pattern)):
                try:
                    os.remove(pipe_file)
                    print(f"Removed: {pipe_file}")
                except Exception as e:
                    print(f"Error removing {pipe_file}: {e}")
    
    # Clean up Python cache
    pycache_dir = "__pycache__"
    if os.path.exists(pycache_dir):
        try:
            shutil.rmtree(pycache_dir)
            print(f"Removed: {pycache_dir}")
        except Exception as e:
            print(f"Error removing {pycache_dir}: {e}")
    
    # Clean up any other test artifacts
    other_files = ["output.txt"]
    for file in other_files:
        if os.path.exists(file):
            try:
                os.remove(file)
                print(f"Removed: {file}")
            except Exception as e:
                print(f"Error removing {file}: {e}")
    
    print("Cleanup complete!")

if __name__ == "__main__":
    cleanup_test_files()
