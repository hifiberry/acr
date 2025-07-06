#!/usr/bin/env python3
"""
Test runner for AudioControl integration tests
"""

import os
import subprocess
import sys
from pathlib import Path

def ensure_dependencies():
    """Ensure Python dependencies are installed"""
    requirements_file = Path(__file__).parent / "requirements.txt"
    
    print("Installing Python dependencies...")
    result = subprocess.run([
        sys.executable, "-m", "pip", "install", "-r", str(requirements_file)
    ], capture_output=True, text=True)
    
    if result.returncode != 0:
        print(f"Failed to install dependencies: {result.stderr}")
        return False
    
    print("Dependencies installed successfully")
    return True

def build_audiocontrol():
    """Build the AudioControl binary"""
    print("Building AudioControl binary...")
    
    # Change to project root directory (one level up from tests)
    project_root = Path(__file__).parent.parent
    
    result = subprocess.run([
        "cargo", "build"
    ], cwd=str(project_root), capture_output=True, text=True)
    
    if result.returncode != 0:
        print(f"Failed to build AudioControl: {result.stderr}")
        return False
    
    print("AudioControl built successfully")
    return True

def run_tests():
    """Run the integration tests"""
    test_dir = Path(__file__).parent
    
    print("Running integration tests...")
    
    # Run all test files
    test_files = [
        "test_generic_integration.py",
        "test_librespot_integration.py",
        "test_activemonitor_integration.py",
        "test_raat_integration.py",
        "test_mpd_integration.py",
        "test_websocket.py"
    ]
    
    all_passed = True
    
    for test_file in test_files:
        test_path = test_dir / test_file
        if not test_path.exists():
            print(f"Warning: Test file {test_file} not found")
            continue
            
        print(f"\\n{'='*50}")
        print(f"Running {test_file}")
        print(f"{'='*50}")
        
        result = subprocess.run([
            sys.executable, "-m", "pytest", str(test_path), "-v", "--tb=short"
        ])
        
        if result.returncode != 0:
            all_passed = False
            print(f"FAILED: {test_file}")
        else:
            print(f"PASSED: {test_file}")
    
    return all_passed

def main():
    """Main function"""
    print("AudioControl Integration Test Runner")
    print("=" * 40)
    
    # Change to the tests directory
    os.chdir(Path(__file__).parent)
    
    # Step 1: Install dependencies
    if not ensure_dependencies():
        return 1
    
    # Step 2: Build AudioControl
    if not build_audiocontrol():
        return 1
    
    # Step 3: Run tests
    if run_tests():
        print("\\n" + "=" * 40)
        print("All tests passed!")
        return 0
    else:
        print("\\n" + "=" * 40)
        print("Some tests failed!")
        return 1

if __name__ == "__main__":
    sys.exit(main())
