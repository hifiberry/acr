#!/bin/bash

# Script to run the AudioControl integration test suite
# Usage: 
#   ./run-test.sh                    - Run all tests
#   ./run-test.sh test_name          - Run specific test
#   ./run-test.sh test1 test2 test3  - Run multiple specific tests
#
# Examples:
#   ./run-test.sh test_librespot_api_events
#   ./run-test.sh test_librespot_api_events test_generic_player_becomes_active_on_playing

if [ $# -eq 0 ]; then
    echo "[TEST] Running AudioControl Integration Test Suite (All Tests)"
    echo "========================================================="
    TEST_ARGS=""
else
    echo "[TEST] Running AudioControl Integration Test Suite (Specific Tests)"
    echo "=============================================================="
    echo "Tests to run: $*"
    echo ""
    # For multiple tests, we need to run them individually or use a pattern
    # Rust test filter supports space-separated names or regex patterns
    TEST_ARGS="$*"
fi

# Ensure we're in the correct directory
cd "$(dirname "$0")"

# Kill any existing audiocontrol processes before starting
echo "[CLEANUP] Cleaning up any existing audiocontrol processes..."
if [[ "$OSTYPE" == "msys" || "$OSTYPE" == "win32" ]]; then
    # Windows
    taskkill //F //IM audiocontrol.exe 2>/dev/null || true
else
    # Linux/Unix
    pkill -KILL -f audiocontrol 2>/dev/null || true
fi

echo "[WAIT] Waiting for process cleanup..."
sleep 1

# Run the integration tests with verbose output
echo "[START] Starting integration test suite..."
echo ""

if [ -z "$TEST_ARGS" ]; then
    # Run all tests
    cargo test --test full_integration_tests -- --nocapture
else
    # Run specific tests - for multiple tests, we need to run them individually
    # But we can use a pattern that matches all of them
    for test_name in $TEST_ARGS; do
        echo "Running test: $test_name"
        cargo test --test full_integration_tests "$test_name" -- --nocapture
        if [ $? -ne 0 ]; then
            echo "[FAIL] Test $test_name failed"
            exit 1
        fi
        echo "[PASS] Test $test_name passed"
        echo ""
    done
fi

# Capture the exit code
TEST_EXIT_CODE=$?

# Additional cleanup after tests
echo ""
echo "[CLEANUP] Post-test cleanup..."
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

echo "[CLEANUP] Cleanup complete"
echo ""

# Report results
if [ $TEST_EXIT_CODE -eq 0 ]; then
    if [ -z "$TEST_ARGS" ]; then
        echo "[PASS] All integration tests passed!"
    else
        echo "[PASS] Selected integration tests passed!"
    fi
else
    if [ -z "$TEST_ARGS" ]; then
        echo "[FAIL] Some integration tests failed (exit code: $TEST_EXIT_CODE)"
    else
        echo "[FAIL] Some selected integration tests failed (exit code: $TEST_EXIT_CODE)"
    fi
fi

echo "=============================================="

exit $TEST_EXIT_CODE
