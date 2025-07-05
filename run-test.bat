@echo off
setlocal

REM Script to run the full integration test suite for AudioControl
REM This script runs all integration tests in verbose mode with proper cleanup

echo 🧪 Running AudioControl Integration Test Suite
echo ==============================================

REM Ensure we're in the correct directory
cd /d "%~dp0"

REM Kill any existing audiocontrol processes before starting
echo 🧹 Cleaning up any existing audiocontrol processes...
taskkill /F /IM audiocontrol.exe 2>nul || echo No existing audiocontrol processes found

echo ⏳ Waiting for process cleanup...
timeout /t 1 /nobreak >nul

REM Run the integration tests with verbose output
echo 🚀 Starting integration test suite...
echo.

cargo test --test full_integration_tests -- --nocapture

REM Capture the exit code
set TEST_EXIT_CODE=%ERRORLEVEL%

REM Additional cleanup after tests
echo.
echo 🧹 Post-test cleanup...
taskkill /F /IM audiocontrol.exe 2>nul || echo No audiocontrol processes to clean up

REM Clean up test artifacts
del /q test_config_*.json 2>nul || echo No config files to clean up
rmdir /s /q test_cache_* 2>nul || echo No cache directories to clean up

echo 🧹 Cleanup complete
echo.

REM Report results
if %TEST_EXIT_CODE% equ 0 (
    echo ✅ All integration tests passed!
) else (
    echo ❌ Some integration tests failed (exit code: %TEST_EXIT_CODE%^)
)

echo ==============================================

exit /b %TEST_EXIT_CODE%
