#!/bin/bash

# Script to run the full integration test suite for AudioControl
# This script runs all integration tests in verbose mode with proper cleanup

echo "🧪 Running AudioControl Integration Test Suite"
echo "=============================================="

# Ensure we're in the correct directory
cd "$(dirname "$0")"

# Kill any existing audiocontrol processes before starting
echo "🧹 Cleaning up any existing audiocontrol processes..."
if [[ "$OSTYPE" == "msys" || "$OSTYPE" == "win32" ]]; then
    # Windows
    taskkill //F //IM audiocontrol.exe 2>/dev/null || true
else
    # Linux/Unix
    pkill -KILL -f audiocontrol 2>/dev/null || true
fi

echo "⏳ Waiting for process cleanup..."
sleep 1

# Run the integration tests with verbose output
echo "🚀 Starting integration test suite..."
echo ""

cargo test --test full_integration_tests -- --nocapture

# Capture the exit code
TEST_EXIT_CODE=$?

# Additional cleanup after tests
echo ""
echo "🧹 Post-test cleanup..."
if [[ "$OSTYPE" == "msys" || "$OSTYPE" == "win32" ]]; then
    # Windows
    taskkill //F //IM audiocontrol.exe 2>/dev/null || true
else
    # Linux/Unix
    pkill -KILL -f audiocontrol 2>/dev/null || true
fi

# Clean up test artifacts
rm -f test_config_*.json
rm -rf test_cache_*

echo "🧹 Cleanup complete"
echo ""

# Report results
if [ $TEST_EXIT_CODE -eq 0 ]; then
    echo "✅ All integration tests passed!"
else
    echo "❌ Some integration tests failed (exit code: $TEST_EXIT_CODE)"
fi

echo "=============================================="

exit $TEST_EXIT_CODE
