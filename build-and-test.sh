#!/bin/bash

# build-and-test.sh - Build all cargo binaries and run unit and integration tests
# This script ensures all audiocontrol binaries are bprint_success "Build and test process completed successfully!"
print_status "✓ All binaries built in release mode"
print_status "✓ All unit tests passed"
print_status "✓ All integration tests passed"
print_status "Built binaries are available in target/release/"
print_status "To run specific tests, use: python -m pytest integration_test/test_<name>.py -v" release mode,
# runs unit tests, and then runs the integration tests

set -e  # Exit on any error

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Function to print colored output
print_status() {
    echo -e "${BLUE}[INFO]${NC} $1"
}

print_success() {
    echo -e "${GREEN}[SUCCESS]${NC} $1"
}

print_warning() {
    echo -e "${YELLOW}[WARNING]${NC} $1"
}

print_error() {
    echo -e "${RED}[ERROR]${NC} $1"
}

# Get the directory where this script is located
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

print_status "Starting build and test process for audiocontrol..."

# Clean previous builds (optional - uncomment if you want clean builds)
# print_status "Cleaning previous builds..."
# cargo clean

# Build all binaries in release mode
print_status "Building all audiocontrol binaries in release mode..."
print_status "This includes:"
print_status "  - audiocontrol (main binary)"
print_status "  - audiocontrol_dump_cache"
print_status "  - audiocontrol_dump_settingsdb"  
print_status "  - audiocontrol_lms_client"
print_status "  - audiocontrol_musicbrainz_client"
print_status "  - audiocontrol_send_update"
print_status "  - audiocontrol_dump_store"
print_status "  - audiocontrol_player_event_client"
print_status "  - audiocontrol_notify_librespot"
print_status "  - audiocontrol_list_mpris_players (Unix only)"
print_status "  - audiocontrol_get_mpris_state (Unix only)"
print_status "  - audiocontrol_monitor_mpris_state (Unix only)"
print_status "  - audiocontrol_listen_shairportsync (Unix only)"
print_status "  - audiocontrol_list_genres"

# Build all binaries
if cargo build --release --bins; then
    print_success "All binaries built successfully"
else
    print_error "Failed to build binaries"
    exit 1
fi

# Verify the main audiocontrol binary exists
if [ ! -f "target/release/audiocontrol" ]; then
    print_error "Main audiocontrol binary not found at target/release/audiocontrol"
    exit 1
fi

print_success "Main audiocontrol binary verified at target/release/audiocontrol"

# Run unit tests
print_status "Running unit tests..."
if cargo test --release; then
    print_success "All unit tests passed!"
else
    print_error "Some unit tests failed"
    exit 1
fi

# Check if integration test dependencies are available
print_status "Checking integration test dependencies..."

# Check if Python and pytest are available
if ! command -v python &> /dev/null; then
    print_error "Python is not installed or not in PATH"
    exit 1
fi

if ! python -c "import pytest" 2>/dev/null; then
    print_warning "pytest not found, attempting to install requirements..."
    if [ -f "integration_test/requirements.txt" ]; then
        pip install -r integration_test/requirements.txt
    else
        print_error "requirements.txt not found and pytest not available"
        exit 1
    fi
fi

# Check for test configuration files and abort if any are missing
print_status "Checking test configuration files..."
if [ ! -f "integration_test/test_config_generic.json" ]; then
    print_error "Required test configuration file integration_test/test_config_generic.json not found"
    exit 1
fi

# Check for required configuration files for full test suite
REQUIRED_CONFIGS=(
    "integration_test/test_config_generic.json"
    "integration_test/test_config_activemonitor.json"
    "integration_test/test_config_librespot.json" 
    "integration_test/test_config_theaudiodb.json"
    "integration_test/test_config_volume.json"
)

missing_configs=0
for config in "${REQUIRED_CONFIGS[@]}"; do
    if [ ! -f "$config" ]; then
        print_error "Required configuration file $config not found"
        missing_configs=$((missing_configs + 1))
    else
        print_status "✓ Found configuration file $config"
    fi
done

if [ $missing_configs -gt 0 ]; then
    print_error "Missing $missing_configs required configuration files"
    print_error "All configuration files must be present to run the full test suite"
    exit 1
fi

# Run integration tests
print_status "Starting integration tests..."

# Run all integration tests by default
print_status "Running all integration tests..."

if python -m pytest integration_test/ -v; then
    print_success "All integration tests passed!"
else
    print_error "Some integration tests failed"
    exit 1
fi

print_success "Build and test process completed successfully!"
print_status "✓ All binaries built in release mode"
print_status "✓ All unit tests passed"
print_status "✓ Core integration tests passed"
print_status "Built binaries are available in target/release/"
print_status "To run specific tests, use: python -m pytest integration_test/test_<name>.py -v"
print_status "To run all tests (may have failures): $0 --all"
